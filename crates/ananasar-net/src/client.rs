//! Client-side connection helpers: endpoint parsing and a connect function
//! that can turn on TCP keepalive.

use socket2::{SockRef, TcpKeepalive};
use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Idle time before the first keepalive probe, and the gap between probes.
const KEEPALIVE_TIME: Duration = Duration::from_secs(30);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
/// How long a connect attempt may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Validate a host and a port typed by the user. Returns the trimmed host and
/// the port. The port must be 1-65535; the host must be non-empty and free of
/// whitespace.
pub fn parse_endpoint(host: &str, port: &str) -> Result<(String, u16), String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("host is empty".into());
    }
    if host.contains(char::is_whitespace) {
        return Err("host contains spaces".into());
    }
    let port = match port.trim().parse::<u16>() {
        Ok(p) if p != 0 => p,
        _ => return Err("port must be 1-65535".into()),
    };
    Ok((host.to_string(), port))
}

/// Split `host:port` (also `[v6]:port`) into its parts. A string without a
/// port, or a bare IPv6 address, yields `None` for the port.
pub fn split_host_port(s: &str) -> (&str, Option<&str>) {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('[')
        && let Some((host, tail)) = rest.split_once(']')
    {
        return (host, tail.strip_prefix(':'));
    }
    match s.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, Some(port)),
        _ => (s, None),
    }
}

/// Connect to `host:port`, optionally enabling TCP keepalive on the socket.
pub fn connect(host: &str, port: u16, keepalive: bool) -> io::Result<TcpStream> {
    let mut last = None;
    for addr in (host, port).to_socket_addrs()? {
        match connect_addr(addr, keepalive) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address found")))
}

fn connect_addr(addr: SocketAddr, keepalive: bool) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    if keepalive {
        let ka = TcpKeepalive::new().with_time(KEEPALIVE_TIME).with_interval(KEEPALIVE_INTERVAL);
        SockRef::from(&stream).set_tcp_keepalive(&ka)?;
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parses_valid_endpoint() {
        assert_eq!(parse_endpoint(" 10.0.0.5 ", " 502 "), Ok(("10.0.0.5".into(), 502)));
    }

    #[test]
    fn rejects_bad_endpoints() {
        assert!(parse_endpoint("", "502").is_err());
        assert!(parse_endpoint("a b", "502").is_err());
        assert!(parse_endpoint("h", "0").is_err());
        assert!(parse_endpoint("h", "70000").is_err());
        assert!(parse_endpoint("h", "x").is_err());
    }

    #[test]
    fn splits_host_port() {
        assert_eq!(split_host_port("127.0.0.1:502"), ("127.0.0.1", Some("502")));
        assert_eq!(split_host_port("plc.local"), ("plc.local", None));
        assert_eq!(split_host_port("[::1]:1502"), ("::1", Some("1502")));
        assert_eq!(split_host_port("::1"), ("::1", None));
    }

    #[test]
    fn connects_with_and_without_keepalive() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        for ka in [false, true] {
            let s = connect("127.0.0.1", port, ka).unwrap();
            assert_eq!(SockRef::from(&s).keepalive().unwrap(), ka);
        }
    }
}
