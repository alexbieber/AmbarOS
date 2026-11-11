//! TCP port scanning (full connect) with service banners.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

/// nmap's 100 most common TCP ports, plus common modern services.
pub const TOP: &[u16] = &[
    7, 9, 13, 21, 22, 23, 25, 26, 37, 53, 79, 80, 81, 88, 106, 110, 111, 113, 119, 135, 139, 143,
    144, 179, 199, 389, 427, 443, 444, 445, 465, 513, 514, 515, 543, 544, 548, 554, 587, 631, 646,
    873, 990, 993, 995, 1025, 1026, 1027, 1028, 1029, 1110, 1433, 1720, 1723, 1755, 1900, 2000,
    2001, 2049, 2121, 2717, 3000, 3128, 3306, 3389, 3986, 4899, 5000, 5009, 5051, 5060, 5101, 5190,
    5357, 5432, 5631, 5666, 5800, 5900, 6000, 6001, 6646, 7070, 8000, 8008, 8009, 8080, 8081, 8443,
    8888, 9100, 9999, 10000, 32768, 49152, 49153, 49154, 49155, 49156, 49157,
    // modern services
    2375, 2379, 5672, 6379, 6443, 9000, 9090, 9200, 11211, 15672, 27017,
];

/// "top", "all", "22,80,443", "1-1024" or a mix like "22,8000-8100".
pub fn parse_ports(spec: &str) -> Result<Vec<u16>> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        match part {
            "top" => out.extend_from_slice(TOP),
            "all" => out.extend(1..=u16::MAX),
            _ => {
                if let Some((a, b)) = part.split_once('-') {
                    let (a, b): (u16, u16) = (a.trim().parse()?, b.trim().parse()?);
                    if a == 0 || a > b {
                        bail!("bad port range {part}");
                    }
                    out.extend(a..=b);
                } else {
                    let p: u16 = part
                        .parse()
                        .map_err(|_| anyhow::anyhow!("bad port {part}"))?;
                    if p == 0 {
                        bail!("port 0 isn't scannable");
                    }
                    out.push(p);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        bail!("no ports to scan");
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OpenPort {
    pub ip: IpAddr,
    pub port: u16,
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub concurrency: usize,
    pub connect_timeout: Duration,
    pub banner_timeout: Duration,
    pub banners: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            concurrency: 256,
            connect_timeout: Duration::from_millis(1500),
            banner_timeout: Duration::from_millis(1200),
            banners: true,
        }
    }
}

pub async fn scan(ips: &[IpAddr], ports: &[u16], opt: &Options) -> Vec<OpenPort> {
    let limit = Arc::new(Semaphore::new(opt.concurrency.max(1)));
    let mut set = JoinSet::new();
    for &ip in ips {
        for &port in ports {
            let limit = limit.clone();
            let opt = opt.clone();
            set.spawn(async move {
                let _permit = limit.acquire_owned().await.ok()?;
                probe(SocketAddr::new(ip, port), &opt).await
            });
        }
    }
    let mut open = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some(p)) = r {
            open.push(p);
        }
    }
    open.sort_by_key(|p| (p.ip, p.port));
    open
}

async fn probe(addr: SocketAddr, opt: &Options) -> Option<OpenPort> {
    let mut stream = timeout(opt.connect_timeout, TcpStream::connect(addr))
        .await
        .ok()?
        .ok()?;
    let banner = if opt.banners {
        grab(&mut stream, opt.banner_timeout).await
    } else {
        None
    };
    Some(OpenPort {
        ip: addr.ip(),
        port: addr.port(),
        service: identify(addr.port(), banner.as_deref()),
        banner,
    })
}

/// Read what the server says first; if it waits for us, send a harmless HTTP request
/// (as nmap's GetRequest probe does), since web servers run on every kind of port.
async fn grab(stream: &mut TcpStream, wait: Duration) -> Option<String> {
    let mut buf = vec![0u8; 512];
    if let Ok(Ok(n)) = timeout(wait, stream.read(&mut buf)).await
        && n > 0
    {
        return Some(clean(&buf[..n]));
    }
    stream
        .write_all(b"HEAD / HTTP/1.0\r\nUser-Agent: ambar-recon\r\n\r\n")
        .await
        .ok()?;
    if let Ok(Ok(n)) = timeout(wait, stream.read(&mut buf)).await
        && n > 0
    {
        return Some(clean(&buf[..n]));
    }
    None
}

fn clean(bytes: &[u8]) -> String {
    let s: String = String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    let mut lines: Vec<&str> = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(3)
        .collect();
    // keep the useful part of an HTTP reply: status line and Server header
    if lines.first().is_some_and(|l| l.starts_with("HTTP/")) {
        let server = s
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("server:"))
            .map(str::trim);
        lines = lines.into_iter().take(1).chain(server).collect();
    }
    let joined = lines.join(" | ");
    joined.chars().take(160).collect()
}

/// Name the service from what it said, falling back to the usual port assignment.
pub fn identify(port: u16, banner: Option<&str>) -> String {
    if let Some(b) = banner {
        let l = b.to_ascii_lowercase();
        let by_banner = [
            ("ssh-", "ssh"),
            ("http/", "http"),
            ("+ok", "pop3"),
            ("* ok", "imap"),
            ("rfb ", "vnc"),
            ("-err", "redis"),
            ("+pong", "redis"),
            ("mysql", "mysql"),
            ("mariadb", "mysql"),
            ("smtp", "smtp"),
            ("esmtp", "smtp"),
            ("ftp", "ftp"),
            ("amqp", "amqp"),
        ];
        for (needle, name) in by_banner {
            if l.starts_with(needle) || (needle.len() > 4 && l.contains(needle)) {
                return name.into();
            }
        }
        if l.starts_with("220") {
            return if port == 25 || port == 587 || port == 465 {
                "smtp".into()
            } else {
                "ftp".into()
            };
        }
    }
    match port {
        21 => "ftp",
        22 => "ssh",
        23 => "telnet",
        25 | 465 | 587 => "smtp",
        53 => "domain",
        80 | 81 | 8000 | 8008 | 8080 | 8081 | 8888 => "http",
        88 => "kerberos",
        110 => "pop3",
        111 => "rpcbind",
        135 => "msrpc",
        139 => "netbios-ssn",
        143 => "imap",
        389 => "ldap",
        443 | 8443 => "https",
        445 => "microsoft-ds",
        631 => "ipp",
        873 => "rsync",
        993 => "imaps",
        995 => "pop3s",
        1433 => "mssql",
        1723 => "pptp",
        2049 => "nfs",
        2375 => "docker",
        2379 => "etcd",
        3000 => "http (dev)",
        3306 => "mysql",
        3389 => "rdp",
        5432 => "postgresql",
        5672 => "amqp",
        5900 => "vnc",
        6379 => "redis",
        6443 => "kubernetes-api",
        9100 => "printer",
        9200 => "elasticsearch",
        11211 => "memcached",
        15672 => "rabbitmq-mgmt",
        27017 => "mongodb",
        _ => "unknown",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_specs() -> Result<()> {
        assert_eq!(parse_ports("22,80,443")?, vec![22, 80, 443]);
        assert_eq!(
            parse_ports("8000-8003,22")?,
            vec![22, 8000, 8001, 8002, 8003]
        );
        assert_eq!(parse_ports("top")?.len(), TOP.len());
        assert_eq!(parse_ports("all")?.len(), 65535);
        assert!(parse_ports("0").is_err());
        assert!(parse_ports("90-80").is_err());
        assert!(parse_ports("abc").is_err());
        Ok(())
    }

    #[test]
    fn services_from_banners() {
        assert_eq!(identify(2222, Some("SSH-2.0-OpenSSH_9.6")), "ssh");
        assert_eq!(identify(21, Some("220 (vsFTPd 3.0.5)")), "ftp");
        assert_eq!(identify(25, Some("220 mail.example ESMTP Postfix")), "smtp");
        assert_eq!(
            identify(9999, Some("HTTP/1.1 200 OK | Server: nginx")),
            "http"
        );
        assert_eq!(identify(6379, None), "redis");
        assert_eq!(identify(40000, None), "unknown");
    }

    #[test]
    fn http_banners_keep_status_and_server() {
        let b = clean(
            b"HTTP/1.1 404 Not Found\r\nDate: x\r\nServer: Caddy\r\nContent-Length: 0\r\n\r\n",
        );
        assert_eq!(b, "HTTP/1.1 404 Not Found | Server: Caddy");
    }
}
