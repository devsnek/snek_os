pub trait Driver: Sized + Send + Sync {
    fn address(&self) -> smoltcp::wire::HardwareAddress;
}

pub fn register<D>(device: D)
where
    D: smoltcp::phy::Device + Driver + 'static,
{
    use smoltcp::*;

    fn set_ipv4_addr(iface: &mut iface::Interface, cidr: wire::Ipv4Cidr) {
        iface.update_ip_addrs(|addrs| {
            addrs.truncate(0);
            addrs.push(wire::IpCidr::Ipv4(cidr)).unwrap();
        });
    }

    crate::task::spawn(async move {
        let mut device = device;

        let mut config = iface::Config::new(device.address());
        config.random_seed = crate::arch::rand().unwrap();
        let now = time::Instant::from_micros(crate::arch::now() as i64);
        let mut iface = iface::Interface::new(config, &mut device, now);

        let dhcp_socket = socket::dhcpv4::Socket::new();

        let mut sockets = iface::SocketSet::new(vec![]);
        let dhcp_handle = sockets.add(dhcp_socket);

        loop {
            let timestamp = time::Instant::from_micros(crate::arch::now() as i64);
            iface.poll(timestamp, &mut device, &mut sockets);

            let socket = sockets.get_mut::<socket::dhcpv4::Socket>(dhcp_handle);

            let event = socket.poll();

            match event {
                None => {}
                Some(socket::dhcpv4::Event::Configured(config)) => {
                    println!("DHCP config acquired!");

                    println!("IP address:      {}", config.address);
                    set_ipv4_addr(&mut iface, config.address);

                    if let Some(router) = config.router {
                        println!("Default gateway: {}", router);
                        iface.routes_mut().add_default_ipv4_route(router).unwrap();
                    } else {
                        println!("Default gateway: None");
                        iface.routes_mut().remove_default_ipv4_route();
                    }

                    for (i, s) in config.dns_servers.iter().enumerate() {
                        println!("DNS server {}:    {}", i, s);
                    }
                }
                Some(socket::dhcpv4::Event::Deconfigured) => {
                    println!("DHCP lost config!");
                    set_ipv4_addr(
                        &mut iface,
                        wire::Ipv4Cidr::new(wire::Ipv4Address::UNSPECIFIED, 0),
                    );
                    iface.routes_mut().remove_default_ipv4_route();
                }
            }

            maitake::future::yield_now().await;
        }
    });

    /*
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(wire::IpCidr::new(wire::IpAddress::v4(10, 0, 2, 15), 24))
            .unwrap();
    });
    iface
        .routes_mut()
        .add_default_ipv4_route(wire::Ipv4Address::new(10, 0, 2, 2))
        .unwrap();

    let tcp_rx_buffer = socket::tcp::SocketBuffer::new(vec![0; 1500]);
    let tcp_tx_buffer = socket::tcp::SocketBuffer::new(vec![0; 1500]);
    let tcp_socket = socket::tcp::Socket::new(tcp_rx_buffer, tcp_tx_buffer);

    let mut sockets = iface::SocketSet::new(vec![]);
    let tcp_handle = sockets.add(tcp_socket);

    enum State {
        Connect,
        Request,
        Response,
    }

    let mut state = State::Connect;

    loop {
        let timestamp = time::Instant::from_micros(crate::arch::now());
        iface.poll(
            timestamp,
            &mut device,
            &mut sockets,
        );

        let socket = sockets.get_mut::<socket::tcp::Socket>(tcp_handle);
        let cx = iface.context();

        state = match state {
            State::Connect if !socket.is_active() => {
                println!("connecting");
                let local_port = 49152 + (crate::arch::rand().unwrap() as u16) % 16384;
                socket
                    .connect(
                        cx,
                        (wire::Ipv4Address::new(162, 159, 135, 232), 80),
                        local_port,
                    )
                    .unwrap();
                State::Request
            }
            State::Request if socket.may_send() => {
                println!("sending request");

                socket
                    .send_slice(b"GET / HTTP/1.1\r\n")
                    .expect("cannot send");
                socket
                    .send_slice(b"Host: discord.com\r\n")
                    .expect("cannot send");
                socket
                    .send_slice(b"User-Agent: snek_os (https://github.com/devsnek/snek_os, 0.1.0)\r\n")
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
    */
}
