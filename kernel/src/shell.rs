use crate::{
    drivers::keyboard::{next_key, DecodedKey, KeyCode},
    framebuffer::DISPLAY,
};
use core::{fmt::Write, pin::Pin, str::FromStr, time::Duration};
use futures::FutureExt;
use hashbrown::HashMap;
use spin::Mutex;

#[derive(Debug)]
struct Args {
    args: Vec<String>,
}

impl Args {
    fn write_str(&self, s: &str) {
        DISPLAY.lock().write_str(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments) {
        let _ = DISPLAY.lock().write_fmt(args);
    }
}

type CmdRet = Result<(), Box<dyn core::error::Error + Send + Sync>>;
type Command =
    Box<dyn (Fn(Args) -> Pin<Box<dyn core::future::Future<Output = CmdRet> + Send>>) + Send>;

lazy_static::lazy_static! {
    static ref DEBUG: Mutex<Vec<u8>> = Mutex::new(Vec::new());
}

fn print(args: core::fmt::Arguments) {
    struct W<'a>(&'a mut Vec<u8>);
    impl<'a> core::fmt::Write for W<'a> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            self.0.extend_from_slice(s.as_bytes());
            Ok(())
        }
    }

    let mut debug = DEBUG.lock();
    let _ = W(&mut debug).write_fmt(args);
}

async fn shell() {
    crate::framebuffer::logo();

    let mut commands = HashMap::<&'static str, Command>::new();
    macro_rules! reg {
        ($name:ident) => {
            commands.insert(
                stringify!($name),
                Box::new(|a| Box::pin(commands::$name(a))),
            );
        };
    }

    reg!(panic);
    reg!(timing);
    reg!(ifconfig);
    reg!(dig);
    reg!(http);
    reg!(ping);
    reg!(shutdown);
    reg!(reboot);
    reg!(logs);
    reg!(logo);
    reg!(lspci);
    // reg!(wasm);

    let command_names = commands.keys().map(|s| s.to_owned()).collect::<Vec<_>>();
    commands.insert(
        "help",
        Box::new(move |args| {
            let command_names = command_names.clone();
            Box::pin(async move {
                args.write_fmt(format_args!(
                    "Available commands:\n{}\n",
                    command_names.join("\n")
                ));
                Ok(())
            })
        }),
    );

    let mut line = String::new();
    DISPLAY.lock().write_str("> ");
    while let Some(key) = next_key().await {
        match key {
            DecodedKey::Unicode('\r') | DecodedKey::Unicode('\n') => {
                DISPLAY.lock().write_char('\n');

                let mut args = line.split(" ");
                if let Some(cmd) = args.next() {
                    match commands.get(cmd) {
                        Some(f) => {
                            let args = args.map(|s| s.to_owned()).collect();
                            let args = Args { args };

                            let mut cmd_fut = Fuse {
                                inner: Some(crate::task::spawn(f(args))),
                            };
                            let mut key_fut = Box::pin(
                                async {
                                    while let Some(key) = next_key().await {
                                        if key == DecodedKey::Unicode('\u{0003}') {
                                            break;
                                        }
                                    }
                                }
                                .fuse(),
                            );

                            futures::select_biased! {
                                r = cmd_fut => {
                                    if let Err(e) = r {
                                        let _ = DISPLAY.lock().write_fmt(format_args!("{e:?}\n"));
                                    }
                                }
                                _ = key_fut => {
                                    if let Some(handle) = cmd_fut.inner {
                                        handle.cancel();
                                    }
                                    DISPLAY.lock().write_str("Cancelled task\n");
                                }
                            };
                        }
                        _ => {
                            DISPLAY.lock().write_str("unknown command\n");
                        }
                    }
                };

                line.clear();
                DISPLAY.lock().write_str("> ");
            }
            DecodedKey::RawKey(KeyCode::Backspace)
            | DecodedKey::RawKey(KeyCode::Delete)
            | DecodedKey::Unicode('\u{0008}') => {
                line.pop();
                DISPLAY.lock().write_char('\u{0008}');
            }
            DecodedKey::Unicode(c) => {
                line.push(c);
                DISPLAY.lock().write_char(c);
            }
            DecodedKey::RawKey(_key) => {}
        }
    }
}

pub fn start() {
    crate::debug::set_print(print);
    crate::task::spawn(shell());
}

mod commands {
    use super::*;

    pub async fn panic(_: Args) -> CmdRet {
        panic!("a panic");
    }

    pub async fn timing(args: Args) -> CmdRet {
        let n = args.args[0].parse::<u32>().unwrap();
        for _ in 0..n {
            let uptime = crate::arch::now();
            let unix = crate::arch::timestamp();
            args.write_fmt(format_args!("{uptime:?} {unix:?}\n"));
            maitake::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(())
    }

    pub async fn ifconfig(args: Args) -> CmdRet {
        let Some(conf) = crate::net::config() else {
            args.write_str("No interface\n");
            return Ok(());
        };
        if let Some(mac) = conf.mac {
            args.write_fmt(format_args!(
                "ether {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            ));
        }
        for addr in &conf.ip_addrs {
            args.write_fmt(format_args!("inet  {addr}\n"));
        }
        for addr in &conf.dns_servers {
            args.write_fmt(format_args!("dns   {addr}\n"));
        }
        for route in &conf.routes {
            args.write_fmt(format_args!(
                "route {} via {}\n",
                route.cidr, route.via_router
            ));
        }
        Ok(())
    }

    pub async fn dig(args: Args) -> CmdRet {
        use crate::net::{DnsQueryType, DnsSocket};

        let host = args.args[1].clone();
        let typ = match args.args[0].to_lowercase().as_str() {
            "a" => DnsQueryType::A,
            "aaaa" => DnsQueryType::Aaaa,
            "cname" => DnsQueryType::Cname,
            "ns" => DnsQueryType::Ns,
            "soa" => DnsQueryType::Soa,
            _ => return Ok(()),
        };
        let dns = DnsSocket::new()?;
        let results =
            match maitake::time::timeout(Duration::from_millis(500), dns.query(&host, typ)).await {
                Ok(v) => v?,
                Err(_) => {
                    args.write_str("DNS query timed out\n");
                    return Ok(());
                }
            };
        for item in results {
            args.write_fmt(format_args!("{item}\n"));
        }

        Ok(())
    }

    async fn get(url: &str) -> Result<http::Response<Vec<u8>>, anyhow::Error> {
        use crate::net::{DnsQueryType, DnsSocket, TcpSocket};

        let url = url::Url::parse(url)?;

        let ip = match url.host() {
            Some(url::Host::Domain(name)) => {
                let dns = DnsSocket::new()?;
                let results = match maitake::time::timeout(
                    Duration::from_millis(3000),
                    dns.query(name, DnsQueryType::A),
                )
                .await
                {
                    Ok(v) => v?,
                    Err(_) => {
                        return Err(anyhow::anyhow!("DNS lookup timed out"));
                    }
                };
                results[0]
            }
            Some(url::Host::Ipv4(ip)) => core::net::IpAddr::V4(ip),
            Some(url::Host::Ipv6(ip)) => core::net::IpAddr::V6(ip),
            _ => panic!(),
        };

        let sock = TcpSocket::new();
        sock.set_timeout(Some(core::time::Duration::from_millis(3000)));
        sock.connect((ip, 80)).await?;

        let payload = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            url.path(),
            url.host_str().unwrap()
        );
        sock.write(payload.as_bytes()).await?;

        let mut body = vec![];
        let mut data = [0; 512];
        loop {
            let n = sock.read(&mut data).await?;
            if n == 0 {
                break;
            }
            body.extend(&data[..n]);
        }

        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut res = httparse::Response::new(&mut headers);
        let n = res.parse(&body).unwrap().unwrap();
        let body = body[n..].to_vec();

        let mut res = http::Response::builder()
            .status(res.code.unwrap())
            .body(body)
            .unwrap();
        for h in headers {
            let Ok(name) = http::HeaderName::from_str(h.name) else {
                continue;
            };
            let Ok(value) = http::HeaderValue::from_bytes(h.value) else {
                continue;
            };
            res.headers_mut().insert(name, value);
        }

        Ok(res)
    }

    pub async fn http(args: Args) -> CmdRet {
        match args.args[0].as_str() {
            "serve" => {
                //crate::net::serve_task();
            }
            "get" => match get(&args.args[1]).await {
                Ok(res) => {
                    let (_, body) = res.into_parts();
                    let s = String::from_utf8_lossy(&body);
                    args.write_fmt(format_args!("{s}"));
                }
                Err(e) => {
                    args.write_fmt(format_args!("Error: {e}"));
                }
            },
            _ => {}
        }

        Ok(())
    }

    /*
    pub async fn wasm(args: Args) -> CmdRet {
        let res = get(&args.args[1]).await?;
        let (_, body) = res.into_parts();
        crate::wasm::run(body);
        Ok(())
    }
    */

    pub async fn ping(args: Args) -> CmdRet {
        use crate::net::{DnsQueryType, DnsSocket};
        let ip = if let Ok(ip) = args.args[0].parse() {
            ip
        } else {
            let dns = DnsSocket::new()?;
            let results = match maitake::time::timeout(
                Duration::from_millis(3000),
                dns.query(&args.args[0], DnsQueryType::A),
            )
            .await
            {
                Ok(v) => v?,
                Err(_) => {
                    args.write_str("DNS lookup timed out\n");
                    return Ok(());
                }
            };
            results[0]
        };
        let rx = crate::net::ping(ip).await?;
        while let Ok((src, bytes, seq, time)) = rx.recv().await {
            let time = time.as_millis_f64();
            args.write_fmt(format_args!(
                "{bytes} bytes from {src} icmp_seq={seq} time={time}ms\n"
            ));
        }
        Ok(())
    }

    pub async fn shutdown(_: Args) -> CmdRet {
        crate::arch::shutdown();

        Ok(())
    }

    pub async fn reboot(_: Args) -> CmdRet {
        crate::arch::reboot();

        Ok(())
    }

    pub async fn logs(args: Args) -> CmdRet {
        let debug = DEBUG.lock();
        let debug = if let Some(mut lines) = args.args.first().and_then(|s| s.parse::<u32>().ok()) {
            lines += 1;
            let index = debug.iter().rposition(|v| {
                if *v == b'\n' {
                    lines -= 1;
                    lines == 0
                } else {
                    false
                }
            });
            if let Some(index) = index {
                &debug[index..debug.len()]
            } else {
                &debug[..]
            }
        } else {
            &debug[..]
        };
        let debug = unsafe { core::str::from_utf8_unchecked(debug) };
        args.write_fmt(format_args!("{debug}\n"));

        Ok(())
    }

    pub async fn logo(_: Args) -> CmdRet {
        crate::framebuffer::logo();
        Ok(())
    }

    pub async fn lspci(args: Args) -> CmdRet {
        for (address, device) in &*crate::arch::get_pci_devices() {
            args.write_fmt(format_args!(
                "PCI {address} {:04x}:{:04x} {}\n",
                device.vendor_id,
                device.device_id,
                device.name()
            ));
        }
        Ok(())
    }
}

#[pin_project::pin_project]
#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Fuse<Fut> {
    #[pin]
    inner: Option<Fut>,
}

impl<Fut: core::future::Future> futures::future::FusedFuture for Fuse<Fut> {
    fn is_terminated(&self) -> bool {
        self.inner.is_none()
    }
}

impl<Fut: core::future::Future> core::future::Future for Fuse<Fut> {
    type Output = Fut::Output;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Fut::Output> {
        match self.as_mut().project().inner.as_pin_mut() {
            Some(fut) => fut.poll(cx).map(|output| {
                self.project().inner.set(None);
                output
            }),
            None => core::task::Poll::Pending,
        }
    }
}
