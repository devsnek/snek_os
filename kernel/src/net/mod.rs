use alloc::sync::Arc;
use conquer_once::spin::OnceCell;
use core::time::Duration;
use rand::{rngs::OsRng, Rng, RngCore};
use spin::Mutex;

pub trait Driver: smoltcp::phy::Device + Sized + Send + Sync {
    fn address(&self) -> smoltcp::wire::HardwareAddress;
}

struct InterfaceInner {
    iface: smoltcp::iface::Interface,
    sockets: smoltcp::iface::SocketSet<'static>,
    dns_servers: Vec<smoltcp::wire::IpAddress>,
}

struct Interface<D: Driver> {
    device: D,
    inner: Arc<Mutex<InterfaceInner>>,
    dhcp_handle: Option<smoltcp::iface::SocketHandle>,
}

impl<D: Driver> Interface<D> {
    fn new(mut device: D) -> Self {
        let mut config = smoltcp::iface::Config::new(device.address());
        config.random_seed = OsRng.next_u64();

        let now = smoltcp::time::Instant::from_micros(crate::arch::now().as_micros() as i64);

        let iface = smoltcp::iface::Interface::new(config, &mut device, now);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);

        let dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
        let dhcp_handle = sockets.add(dhcp_socket);

        let inner = InterfaceInner {
            iface,
            sockets,
            dns_servers: vec![],
        };

        Self {
            device,
            inner: Arc::new(Mutex::new(inner)),
            dhcp_handle: Some(dhcp_handle),
        }
    }

    fn poll_dhcp(&mut self) {
        let Some(dhcp_handle) = self.dhcp_handle else {
            return;
        };
        let inner = &mut *self.inner.lock();
        let socket = inner
            .sockets
            .get_mut::<smoltcp::socket::dhcpv4::Socket>(dhcp_handle);

        let event = socket.poll();

        match event {
            None => {}
            Some(smoltcp::socket::dhcpv4::Event::Configured(config)) => {
                inner.iface.update_ip_addrs(|addrs| {
                    addrs.truncate(0);
                    addrs
                        .push(smoltcp::wire::IpCidr::Ipv4(config.address))
                        .unwrap();
                });

                if let Some(router) = config.router {
                    inner
                        .iface
                        .routes_mut()
                        .add_default_ipv4_route(router)
                        .unwrap();
                } else {
                    inner.iface.routes_mut().remove_default_ipv4_route();
                }

                for s in config.dns_servers.into_iter() {
                    inner.dns_servers.push(s.into());
                }
            }
            Some(smoltcp::socket::dhcpv4::Event::Deconfigured) => {
                inner.iface.update_ip_addrs(|addrs| {
                    addrs.truncate(0);
                    addrs
                        .push(smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                            smoltcp::wire::Ipv4Address::UNSPECIFIED,
                            0,
                        )))
                        .unwrap();
                });
                inner.iface.routes_mut().remove_default_ipv4_route();
                inner.dns_servers.truncate(0);
            }
        }
    }

    async fn run(&mut self) {
        loop {
            let timestamp = {
                let inner = &mut *self.inner.lock();
                let timestamp =
                    smoltcp::time::Instant::from_micros(crate::arch::now().as_micros() as i64);
                inner
                    .iface
                    .poll(timestamp, &mut self.device, &mut inner.sockets);
                timestamp
            };

            self.poll_dhcp();

            let delay = {
                let inner = &mut *self.inner.lock();
                inner
                    .iface
                    .poll_delay(timestamp, &inner.sockets)
                    .map(|d| Duration::from_micros(d.micros()))
            };
            maitake::time::sleep(delay.unwrap_or_default()).await;
        }
    }
}

static DEFAULT_DRIVER: OnceCell<Arc<Mutex<InterfaceInner>>> = OnceCell::uninit();

pub fn register<D>(device: D)
where
    D: Driver + 'static,
{
    let mut interface = Interface::new(device);
    let inner2 = interface.inner.clone();
    DEFAULT_DRIVER.try_init_once(|| inner2).unwrap();

    crate::task::spawn(async move {
        interface.run().await;
    });

    println!("[NET] device registered");
}

pub fn test_task(host: String) {
    crate::task::spawn(async {
        let ip = get_ips(&host).await[0];
        println!("got ip {:?}", ip);
        http_get(host, ip).await;
    });
}

async fn http_get(host: String, ip: smoltcp::wire::IpAddress) {
    use smoltcp::*;

    let tcp_rx_buffer = socket::tcp::SocketBuffer::new(vec![0; 1500]);
    let tcp_tx_buffer = socket::tcp::SocketBuffer::new(vec![0; 1500]);

    let tcp_handle = {
        let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
        let tcp_socket = socket::tcp::Socket::new(tcp_rx_buffer, tcp_tx_buffer);
        inner.sockets.add(tcp_socket)
    };

    // println!("connecting");
    {
        let inner = &mut *DEFAULT_DRIVER.get().unwrap().lock();
        let socket = inner.sockets.get_mut::<socket::tcp::Socket>(tcp_handle);
        let local_port = 49152 + OsRng.gen::<u16>() % 16384;
        socket
            .connect(inner.iface.context(), (ip, 80), local_port)
            .unwrap();
    }
    core::future::poll_fn(|cx| {
        use core::task::Poll;
        let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
        let socket = inner.sockets.get_mut::<socket::tcp::Socket>(tcp_handle);
        if socket.is_open() {
            Poll::Ready(())
        } else {
            socket.register_send_waker(cx.waker());
            Poll::Pending
        }
    })
    .await;

    // SEND

    // TODO: convert other states to poll_fn
    enum State {
        Request,
        Response,
    }

    let mut state = State::Request;

    loop {
        {
            let inner = &mut *DEFAULT_DRIVER.get().unwrap().lock();
            let socket = inner.sockets.get_mut::<socket::tcp::Socket>(tcp_handle);

            state = match state {
                State::Request if socket.may_send() => {
                    println!("sending request");

                    socket
                        .send_slice(b"GET / HTTP/1.1\r\n")
                        .expect("cannot send");
                    socket
                        .send_slice(format!("Host: {}\r\n", host).as_bytes())
                        .expect("cannot send");
                    socket
                        .send_slice(
                            b"User-Agent: snek_os (https://github.com/devsnek/snek_os, 0.1.0)\r\n",
                        )
                        .expect("cannot send");
                    socket
                        .send_slice(b"Connection: close\r\n")
                        .expect("cannot send");
                    socket.send_slice(b"\r\n").expect("cannot send");
                    State::Response
                }
                State::Response if socket.can_recv() => {
                    socket
                        .recv(|data| {
                            println!("{}", core::str::from_utf8(data).unwrap_or("(invalid utf8)"));
                            (data.len(), ())
                        })
                        .unwrap();
                    State::Response
                }
                State::Response if !socket.may_recv() => {
                    println!("received complete response");
                    break;
                }
                _ => state,
            };
        }

        maitake::future::yield_now().await;
    }

    DEFAULT_DRIVER
        .get()
        .unwrap()
        .lock()
        .sockets
        .remove(tcp_handle);
}

async fn get_ips(name: &str) -> Vec<smoltcp::wire::IpAddress> {
    use smoltcp::*;

    loop {
        let inner = DEFAULT_DRIVER.get().unwrap().lock();
        if inner.dns_servers.is_empty() {
            drop(inner);
            maitake::time::sleep(Duration::from_millis(50)).await;
        } else {
            break;
        }
    }

    let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
    let mut dns_socket = socket::dns::Socket::new(&inner.dns_servers, vec![]);

    let query_handle = dns_socket
        .start_query(inner.iface.context(), name, wire::DnsQueryType::A)
        .unwrap();

    let dns_handle = inner.sockets.add(dns_socket);
    drop(inner);

    let ips = core::future::poll_fn(|cx| {
        use core::task::Poll;
        let mut inner = DEFAULT_DRIVER.get().unwrap().lock();
        let socket = inner.sockets.get_mut::<socket::dns::Socket>(dns_handle);
        match socket.get_query_result(query_handle) {
            Err(socket::dns::GetQueryResultError::Pending) => {
                socket.register_query_waker(query_handle, cx.waker());
                Poll::Pending
            }
            Ok(results) => Poll::Ready(results.into_iter().collect::<Vec<_>>()),
            Err(socket::dns::GetQueryResultError::Failed) => {
                panic!("query failed");
            }
        }
    })
    .await;

    DEFAULT_DRIVER
        .get()
        .unwrap()
        .lock()
        .sockets
        .remove(dns_handle);

    ips
}
