use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use core::{future::poll_fn, task::Poll, time::Duration};
use futures::FutureExt;
use maitake::{sync::WaitCell, time::Instant};
use rand::{rngs::OsRng, Rng, RngCore};
use spin::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    TcpConnect(smoltcp::socket::tcp::ConnectError),
    #[error("{0}")]
    TcpListen(smoltcp::socket::tcp::ListenError),
    #[error("{0}")]
    TcpRecv(smoltcp::socket::tcp::RecvError),
    #[error("{0}")]
    TcpSend(smoltcp::socket::tcp::SendError),
    #[error("{0}")]
    Wire(smoltcp::wire::Error),
    #[error("TcpClosed")]
    TcpClosed,

    #[error("{0}")]
    IcmpRecv(smoltcp::socket::icmp::RecvError),
    #[error("{0}")]
    IcmpSend(smoltcp::socket::icmp::SendError),

    #[error("{0}")]
    DnsStartQuery(smoltcp::socket::dns::StartQueryError),
    #[error("{0}")]
    DnsFinishQuery(smoltcp::socket::dns::GetQueryResultError),

    #[error("destination unreachable")]
    DestinationUnreachable,
}

pub trait Driver: smoltcp::phy::Device + Sized + Send + Sync {
    fn address(&self) -> smoltcp::wire::HardwareAddress;
    fn poll(&self, cx: &mut core::task::Context) -> core::task::Poll<()>;
}

lazy_static::lazy_static! {
    static ref SOCKETS: Mutex<smoltcp::iface::SocketSet<'static>> = Mutex::new(smoltcp::iface::SocketSet::new(vec![]));
}

static DEFAULT_DRIVER: OnceCell<Arc<Mutex<InterfaceInner>>> = OnceCell::uninit();

static WAIT_CELL: WaitCell = WaitCell::new();

struct InterfaceInner {
    iface: smoltcp::iface::Interface,
    dns_servers: Vec<smoltcp::wire::IpAddress>,
}

struct Interface<D: Driver> {
    device: D,
    inner: Arc<Mutex<InterfaceInner>>,
}

impl<D: Driver> Interface<D> {
    fn new(mut device: D) -> Self {
        let mut config = smoltcp::iface::Config::new(device.address());
        config.random_seed = OsRng.next_u64();

        let now = smoltcp::time::Instant::from_micros(crate::arch::now().as_micros() as i64);

        let iface = smoltcp::iface::Interface::new(config, &mut device, now);

        let inner = InterfaceInner {
            iface,
            dns_servers: vec![],
        };

        Self {
            device,
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    async fn run(&mut self) {
        loop {
            // TODO: do not poll if sockets empty

            let delay = {
                let mut inner = self.inner.lock();
                let mut sockets = SOCKETS.lock();

                let timestamp =
                    smoltcp::time::Instant::from_micros(crate::arch::now().as_micros() as i64);

                inner.iface.poll(timestamp, &mut self.device, &mut sockets);

                inner
                    .iface
                    .poll_delay(timestamp, &sockets)
                    .map(|d| Duration::from_micros(d.micros()))
            };

            if let Some(delay) = delay {
                // device can wake us up
                let mut f1 = core::future::poll_fn(|cx| self.device.poll(cx)).fuse();
                // other tasks can us up
                let mut f2 = WAIT_CELL.wait().fuse();
                // fallback wakeup
                let mut f3 = core::pin::pin!(maitake::time::sleep(delay).fuse());

                futures::select_biased! {
                    _ = &mut f1 => {}
                    _ = &mut f2 => {}
                    _ = &mut f3 => {}
                }
            }
        }
    }
}

async fn dhcp4() {
    let sock = Dhcp4Socket::new();
    let mut current_config = None;
    loop {
        match sock.event().await {
            Dhcp4Event::Configure(config) => {
                let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
                inner.iface.update_ip_addrs(|addrs| {
                    addrs.push(smoltcp::wire::IpCidr::Ipv4(config.address));
                });

                if let Some(router) = config.router {
                    let _ = inner.iface.routes_mut().add_default_ipv4_route(router);
                } else {
                    inner.iface.routes_mut().remove_default_ipv4_route();
                }

                for server in &config.dns_servers {
                    inner.dns_servers.push((*server).into());
                }

                current_config = Some(config);
            }
            Dhcp4Event::Deconfigure => {
                if let Some(config) = &current_config {
                    let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
                    inner.iface.update_ip_addrs(|addrs| {
                        if let Some(index) = addrs.iter().position(|c| match c {
                            smoltcp::wire::IpCidr::Ipv4(c) => *c == config.address,
                            _ => false,
                        }) {
                            addrs.remove(index);
                        }
                    });
                    inner.iface.routes_mut().remove_default_ipv4_route();
                    while let Some(index) = inner.dns_servers.iter().position(|a| match a {
                        smoltcp::wire::IpAddress::Ipv4(a) => config.dns_servers.contains(a),
                        _ => false,
                    }) {
                        inner.dns_servers.remove(index);
                    }
                }
            }
        }
    }
}

async fn auto6() {
    let sock = IcmpSocket::bind(smoltcp::socket::icmp::Endpoint::Unspecified);

    sock.set_hop_limit(255);

    let (link_ip, mac) = {
        let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
        let mac = match inner.iface.hardware_addr() {
            smoltcp::wire::HardwareAddress::Ethernet(e) => e.0,
            smoltcp::wire::HardwareAddress::Ip => return,
        };
        let a = (((mac[0] & 0b11111101) as u16) << 8) | (mac[1] as u16);
        let b = ((mac[2] as u16) << 8) | 0xff;
        let c = 0xfe00u16 | (mac[3] as u16);
        let d = ((mac[4] as u16) << 8) | (mac[5] as u16);
        let ip = [0xfe80, 0, 0, 0, a, b, c, d].into();

        inner.iface.update_ip_addrs(|addrs| {
            addrs.push(smoltcp::wire::IpCidr::Ipv6(smoltcp::wire::Ipv6Cidr::new(
                ip, 64,
            )));
        });

        (ip, mac)
    };

    {
        let icmp_repr = smoltcp::wire::Icmpv6Repr::Ndisc(smoltcp::wire::NdiscRepr::RouterSolicit {
            lladdr: Some(smoltcp::wire::RawHardwareAddress::from_bytes(&mac)),
        });
        let mut buf = vec![0; icmp_repr.buffer_len()];
        let mut icmp_packet = smoltcp::wire::Icmpv6Packet::new_unchecked(&mut buf);
        let dst_ip = [0xff02, 0, 0, 0, 0, 0, 0, 2].into();
        icmp_repr.emit(
            &link_ip,
            &dst_ip,
            &mut icmp_packet,
            &smoltcp::phy::ChecksumCapabilities::default(),
        );
        if let Err(e) = sock.write(&buf, dst_ip).await {
            error!("icmp send err {e:?}");
        }
    }

    loop {
        let mut buf = [0; 1500];
        match sock.read(&mut buf).await {
            Ok((n, src_ip)) => match src_ip {
                core::net::IpAddr::V4(_) => {}
                core::net::IpAddr::V6(src_ip) => {
                    let packet = smoltcp::wire::Icmpv6Packet::new_checked(&buf[..n]).unwrap();
                    let repr = smoltcp::wire::Icmpv6Repr::parse(
                        &src_ip,
                        &[0xff02, 0, 0, 0, 0, 0, 0, 1].into(),
                        &packet,
                        &smoltcp::phy::ChecksumCapabilities::default(),
                    );
                    match repr {
                        Ok(smoltcp::wire::Icmpv6Repr::Ndisc(
                            smoltcp::wire::NdiscRepr::RouterAdvert {
                                prefix_info,
                                recursive_dns,
                                flags,
                                ..
                            },
                        )) => {
                            let mut inner = DEFAULT_DRIVER.get().unwrap().lock();

                            if flags.contains(smoltcp::wire::NdiscRouterFlags::MANAGED) {
                                // TODO: dhcpv6
                            }

                            if let Some(prefix_info) = prefix_info {
                                let link_segments = link_ip.segments();
                                let mut segments = prefix_info.prefix.segments();
                                segments[4..8].copy_from_slice(&link_segments[4..8]);

                                inner.iface.update_ip_addrs(|addrs| {
                                    addrs.push(smoltcp::wire::IpCidr::Ipv6(
                                        smoltcp::wire::Ipv6Cidr::new(
                                            segments.into(),
                                            prefix_info.prefix_len,
                                        ),
                                    ));
                                });

                                let _ = inner.iface.routes_mut().add_default_ipv6_route(src_ip);
                            }

                            if let Some(recursive_dns) = recursive_dns {
                                for server in recursive_dns.servers {
                                    inner.dns_servers.push((*server).into());
                                }
                            }

                            return;
                        }
                        Err(e) => {
                            info!("icmp recv error {e:?}");
                        }
                        _ => {}
                    }
                }
            },
            Err(e) => {
                error!("icmp recv error {e:?}");
                break;
            }
        }
    }
}

pub fn register<D>(device: D)
where
    D: Driver + 'static,
{
    let mut interface = Interface::new(device);
    DEFAULT_DRIVER
        .try_init_once(|| interface.inner.clone())
        .unwrap();

    crate::task::spawn(dhcp4());

    crate::task::spawn(auto6());

    crate::task::spawn(async move {
        interface.run().await;
    });

    debug!("[NET] device registered");
}

pub struct TcpSocket {
    handle: smoltcp::iface::SocketHandle,
}

impl TcpSocket {
    pub fn new() -> Self {
        let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0; 1500]);
        let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0; 1500]);

        let tcp_socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
        let handle = SOCKETS.lock().add(tcp_socket);

        Self { handle }
    }

    pub fn listen(&self, endpoint: impl Into<core::net::SocketAddr>) -> Result<(), Error> {
        let endpoint: core::net::SocketAddr = endpoint.into();
        let mut sockets = SOCKETS.lock();
        let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
        if let Err(e) = socket.listen(endpoint) {
            return Err(Error::TcpListen(e));
        }
        Ok(())
    }

    pub async fn connect(&self, addr: impl Into<core::net::SocketAddr>) -> Result<(), Error> {
        let addr: core::net::SocketAddr = addr.into();

        {
            let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
            let local_port = 49152 + OsRng.gen::<u16>() % 16384;
            if let Err(e) = socket.connect(inner.iface.context(), addr, local_port) {
                return Err(Error::TcpConnect(e));
            };
        }

        WAIT_CELL.wake();

        poll_fn(|cx| {
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
            if socket.is_open() {
                Poll::Ready(())
            } else {
                socket.register_send_waker(cx.waker());
                Poll::Pending
            }
        })
        .await;

        Ok(())
    }

    pub fn set_timeout(&self, timeout: Option<core::time::Duration>) {
        let mut sockets = SOCKETS.lock();
        let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
        socket
            .set_timeout(timeout.map(|t| smoltcp::time::Duration::from_micros(t.as_micros() as _)));
    }

    pub async fn read(&self, buf: &mut [u8]) -> Result<usize, Error> {
        let f = poll_fn(|cx| {
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
            match socket.recv_slice(buf) {
                Ok(n) => {
                    if n > 0 {
                        return Poll::Ready(Ok(n));
                    }
                }
                Err(e) => {
                    if e == smoltcp::socket::tcp::RecvError::Finished {
                        return Poll::Ready(Ok(0));
                    }
                }
            }

            socket.register_recv_waker(cx.waker());
            Poll::Pending
        });

        f.await.map_err(Error::TcpRecv)
    }

    pub async fn write(&self, buf: &[u8]) -> Result<usize, Error> {
        let f = poll_fn(|cx| {
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(self.handle);
            if socket.state() == smoltcp::socket::tcp::State::Closed {
                return Poll::Ready(Err(Error::TcpClosed));
            }
            if socket.can_send() {
                match socket.send_slice(buf) {
                    Ok(n) => {
                        WAIT_CELL.wake();
                        Poll::Ready(Ok(n))
                    }
                    Err(e) => Poll::Ready(Err(Error::TcpSend(e))),
                }
            } else {
                socket.register_send_waker(cx.waker());
                Poll::Pending
            }
        });

        f.await
    }
}

impl Drop for TcpSocket {
    fn drop(&mut self) {
        SOCKETS.lock().remove(self.handle);
    }
}

pub struct DnsSocket {
    handle: smoltcp::iface::SocketHandle,
}

pub type DnsQueryType = smoltcp::wire::DnsQueryType;

impl DnsSocket {
    pub fn new() -> Result<Self, Error> {
        let inner = DEFAULT_DRIVER.get().unwrap().lock();
        let mut dns_socket = smoltcp::socket::dns::Socket::new(&[], vec![]);
        dns_socket.update_servers(&inner.dns_servers);
        let handle = SOCKETS.lock().add(dns_socket);
        Ok(Self { handle })
    }

    pub fn update_servers(&self, servers: &[core::net::IpAddr]) {
        let mut sockets = SOCKETS.lock();
        let socket = sockets.get_mut::<smoltcp::socket::dns::Socket>(self.handle);
        let servers = servers.iter().map(|v| (*v).into()).collect::<Vec<_>>();
        socket.update_servers(&servers);
    }

    pub async fn query(
        &self,
        name: &str,
        typ: DnsQueryType,
    ) -> Result<Vec<core::net::IpAddr>, Error> {
        let query_handle = {
            let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::dns::Socket>(self.handle);
            match socket.start_query(inner.iface.context(), name, typ) {
                Ok(handle) => {
                    WAIT_CELL.wake();
                    handle
                }
                Err(e) => {
                    return Err(Error::DnsStartQuery(e));
                }
            }
        };

        struct DropQuery(
            smoltcp::iface::SocketHandle,
            smoltcp::socket::dns::QueryHandle,
        );
        impl Drop for DropQuery {
            fn drop(&mut self) {
                let mut sockets = SOCKETS.lock();
                if let Some(socket) = sockets.try_get_mut::<smoltcp::socket::dns::Socket>(self.0) {
                    socket.cancel_query(self.1);
                }
            }
        }
        let drop_query = DropQuery(self.handle, query_handle);

        let r = poll_fn(|cx| {
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::dns::Socket>(self.handle);
            match socket.get_query_result(query_handle) {
                Ok(results) => Poll::Ready(Ok(results)),
                Err(smoltcp::socket::dns::GetQueryResultError::Pending) => {
                    socket.register_query_waker(query_handle, cx.waker());
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        })
        .await;

        core::mem::forget(drop_query);

        let ips = r.map_err(Error::DnsFinishQuery)?;

        Ok(ips.into_iter().map(Into::into).collect())
    }
}

impl Drop for DnsSocket {
    fn drop(&mut self) {
        SOCKETS.lock().remove(self.handle);
    }
}

#[derive(Debug)]
pub struct Dhcp4Config {
    pub server: smoltcp::socket::dhcpv4::ServerInfo,
    pub address: smoltcp::wire::Ipv4Cidr,
    pub router: Option<smoltcp::wire::Ipv4Address>,
    pub dns_servers: Vec<smoltcp::wire::Ipv4Address>,
}

#[derive(Debug)]
pub enum Dhcp4Event {
    Configure(Dhcp4Config),
    Deconfigure,
}

#[derive(Debug)]
pub struct Dhcp4Socket {
    handle: smoltcp::iface::SocketHandle,
}

impl Dhcp4Socket {
    pub fn new() -> Self {
        let socket = smoltcp::socket::dhcpv4::Socket::new();
        let handle = SOCKETS.lock().add(socket);
        Self { handle }
    }

    pub async fn event(&self) -> Dhcp4Event {
        poll_fn(|cx| self.poll(cx)).await
    }

    pub fn poll(&self, cx: &mut core::task::Context) -> Poll<Dhcp4Event> {
        let mut sockets = SOCKETS.lock();
        let socket = sockets.get_mut::<smoltcp::socket::dhcpv4::Socket>(self.handle);
        match socket.poll() {
            None => {
                socket.register_waker(cx.waker());
                Poll::Pending
            }
            Some(v) => Poll::Ready(match v {
                smoltcp::socket::dhcpv4::Event::Configured(c) => {
                    Dhcp4Event::Configure(Dhcp4Config {
                        server: c.server,
                        address: c.address,
                        router: c.router,
                        dns_servers: c.dns_servers.into_iter().collect(),
                    })
                }
                smoltcp::socket::dhcpv4::Event::Deconfigured => Dhcp4Event::Deconfigure,
            }),
        }
    }
}

impl Drop for Dhcp4Socket {
    fn drop(&mut self) {
        SOCKETS.lock().remove(self.handle);
    }
}

#[derive(Debug)]
struct IcmpSocket {
    handle: smoltcp::iface::SocketHandle,
}

impl IcmpSocket {
    fn bind(endpoint: smoltcp::socket::icmp::Endpoint) -> Self {
        let rx_buffer = smoltcp::socket::icmp::PacketBuffer::new(
            vec![smoltcp::socket::icmp::PacketMetadata::EMPTY],
            vec![0; 1500],
        );
        let tx_buffer = smoltcp::socket::icmp::PacketBuffer::new(
            vec![smoltcp::socket::icmp::PacketMetadata::EMPTY],
            vec![0; 1500],
        );

        let mut socket = smoltcp::socket::icmp::Socket::new(rx_buffer, tx_buffer);
        socket.bind(endpoint).unwrap();
        assert!(socket.is_open());

        let handle = SOCKETS.lock().add(socket);

        Self { handle }
    }

    fn set_hop_limit(&self, limit: u8) {
        let mut sockets = SOCKETS.lock();
        let socket = sockets.get_mut::<smoltcp::socket::icmp::Socket>(self.handle);
        socket.set_hop_limit(Some(limit));
    }

    async fn read(&self, buf: &mut [u8]) -> Result<(usize, core::net::IpAddr), Error> {
        let f = poll_fn(|cx| {
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::icmp::Socket>(self.handle);
            match socket.recv_slice(buf) {
                Ok((n, ip)) => Poll::Ready(Ok((n, ip.into()))),
                Err(e) => match e {
                    smoltcp::socket::icmp::RecvError::Exhausted => {
                        socket.register_recv_waker(cx.waker());
                        Poll::Pending
                    }
                    _ => Poll::Ready(Err(Error::IcmpRecv(e))),
                },
            }
        });

        f.await
    }

    async fn write(&self, buf: &[u8], endpoint: impl Into<core::net::IpAddr>) -> Result<(), Error> {
        let endpoint: core::net::IpAddr = endpoint.into();

        let f = poll_fn(|cx| {
            let mut sockets = SOCKETS.lock();
            let socket = sockets.get_mut::<smoltcp::socket::icmp::Socket>(self.handle);

            match socket.send_slice(buf, endpoint.into()) {
                Ok(()) => {
                    WAIT_CELL.wake();
                    Poll::Ready(Ok(()))
                }
                Err(e) => match e {
                    smoltcp::socket::icmp::SendError::BufferFull => {
                        socket.register_send_waker(cx.waker());
                        Poll::Pending
                    }
                    _ => Poll::Ready(Err(Error::IcmpSend(e))),
                },
            }
        });

        f.await
    }
}

impl Drop for IcmpSocket {
    fn drop(&mut self) {
        SOCKETS.lock().remove(self.handle);
    }
}

#[derive(Debug)]
pub struct InterfaceConfig {
    pub mac: Option<[u8; 6]>,
    pub ip_addrs: Vec<smoltcp::wire::IpCidr>,
    pub routes: Vec<smoltcp::iface::Route>,
    pub dns_servers: Vec<core::net::IpAddr>,
}

pub fn config() -> Option<InterfaceConfig> {
    let inner = DEFAULT_DRIVER.get()?.lock();
    let mut routes = Vec::new();
    routes.extend(inner.iface.routes().v4().iter().map(|a| a.2));
    routes.extend(inner.iface.routes().v6().iter().map(|a| a.2));
    Some(InterfaceConfig {
        mac: match inner.iface.hardware_addr() {
            smoltcp::wire::HardwareAddress::Ethernet(e) => Some(e.0),
            smoltcp::wire::HardwareAddress::Ip => None,
        },
        ip_addrs: inner.iface.ip_addrs().to_owned(),
        routes,
        dns_servers: inner.dns_servers.iter().map(|v| (*v).into()).collect(),
    })
}

pub async fn ping(
    dest_ip: core::net::IpAddr,
) -> Result<async_channel::Receiver<(core::net::IpAddr, usize, u16, core::time::Duration)>, Error> {
    let sock = IcmpSocket::bind(smoltcp::socket::icmp::Endpoint::Ident(0x22b));

    let Some(src_ip) = ({
        let inner = DEFAULT_DRIVER.get().unwrap().lock();
        inner.iface.get_source_address(&dest_ip.into())
    }) else {
        return Err(Error::DestinationUnreachable);
    };

    let (tx, rx) = async_channel::bounded(4);

    crate::task::spawn(async move {
        for seq_no in 0.. {
            let now = Instant::now();

            match src_ip {
                smoltcp::wire::IpAddress::Ipv4(_src_ip) => {
                    let icmp_repr = smoltcp::wire::Icmpv4Repr::EchoRequest {
                        ident: 0x22b,
                        seq_no,
                        data: &[0xffu8; 32],
                    };
                    let mut buf = vec![0; icmp_repr.buffer_len()];
                    let mut icmp_packet = smoltcp::wire::Icmpv4Packet::new_unchecked(&mut buf);
                    icmp_repr.emit(
                        &mut icmp_packet,
                        &smoltcp::phy::ChecksumCapabilities::default(),
                    );
                    sock.write(&buf, dest_ip).await.unwrap();
                }
                smoltcp::wire::IpAddress::Ipv6(src_ip) => {
                    let icmp_repr = smoltcp::wire::Icmpv6Repr::EchoRequest {
                        ident: 0x22b,
                        seq_no,
                        data: &[0xffu8; 32],
                    };
                    let mut buf = vec![0; icmp_repr.buffer_len()];
                    let mut icmp_packet = smoltcp::wire::Icmpv6Packet::new_unchecked(&mut buf);
                    icmp_repr.emit(
                        &src_ip,
                        &if let core::net::IpAddr::V6(a) = dest_ip {
                            a
                        } else {
                            unreachable!()
                        },
                        &mut icmp_packet,
                        &smoltcp::phy::ChecksumCapabilities::default(),
                    );
                    sock.write(&buf, dest_ip).await.unwrap();
                }
            }

            let mut buf = [0; 1500];
            match sock.read(&mut buf).await {
                Ok((n, src_addr)) => match src_addr {
                    core::net::IpAddr::V4(_s) => {
                        let time = Instant::now().duration_since(now);
                        match tx.send((src_addr, n, seq_no, time)).await {
                            Ok(()) => {}
                            Err(_) => {
                                break;
                            }
                        }
                    }
                    core::net::IpAddr::V6(_s) => {
                        let time = Instant::now().duration_since(now);
                        match tx.send((src_addr, n, seq_no, time)).await {
                            Ok(()) => {}
                            Err(_) => {
                                break;
                            }
                        }
                    }
                },
                Err(_) => {
                    break;
                }
            }

            maitake::time::sleep(Duration::from_secs(1)).await;
        }
    });

    Ok(rx)
}
