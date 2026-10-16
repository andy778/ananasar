//! The server loop over loopback: what it does with bad headers and with the
//! Reply Out of Synch option.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_core::protocol::build;
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_net::Server;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Running {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn start(cfg: ServerConfig) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server {
        regs: Arc::new(Mutex::new(RegisterFile::new())),
        cfg: Arc::new(Mutex::new(cfg)),
        clients: Arc::default(),
        stop: stop.clone(),
        notify: Arc::new(|_| {}),
        log: None,
        stats: Arc::default(),
    };
    std::thread::spawn(move || server.run(&listener));
    Running { addr, stop }
}

fn connect(server: &Running) -> TcpStream {
    let s = TcpStream::connect(server.addr).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    s
}

/// What comes next on the stream: bytes, or `None` once the peer has closed.
/// Panics on a timeout, which means the server neither answered nor closed.
fn next(s: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buf = [0u8; 600];
    match s.read(&mut buf) {
        Ok(0) => None,
        Ok(n) => Some(buf[..n].to_vec()),
        Err(e) if matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted) => None,
        Err(e) => panic!("{e}"),
    }
}

fn read_request(txn: u16) -> Vec<u8> {
    build(txn, 1, 0x03, &[0x0B, 0xB8, 0, 1])
}

#[test]
fn bad_protocol_id_or_length_high_byte_closes_the_connection() {
    let server = start(ServerConfig::default());
    let patches: [fn(&mut Vec<u8>); 3] = [|f| f[3] = 1, |f| f[2] = 1, |f| f[4] = 1];
    for patch in patches {
        let mut s = connect(&server);
        let mut frame = read_request(1);
        patch(&mut frame);
        s.write_all(&frame).unwrap();
        assert_eq!(next(&mut s), None);
    }
}

#[test]
fn a_refused_request_with_a_bad_header_gets_its_exception_before_the_close() {
    let mut cfg = ServerConfig::default();
    cfg.supported.remove(&0x03);
    let server = start(cfg);
    let mut s = connect(&server);
    let mut frame = read_request(1);
    frame[3] = 1;
    s.write_all(&frame).unwrap();
    assert_eq!(next(&mut s), Some(vec![0, 1, 0, 1, 0, 3, 1, 0x83, 1]));
    assert_eq!(next(&mut s), None);
}

#[test]
fn a_good_frame_after_the_bad_one_is_not_served() {
    let server = start(ServerConfig::default());
    let mut s = connect(&server);
    let mut both = read_request(1);
    both[3] = 1;
    both.extend(read_request(2));
    s.write_all(&both).unwrap();
    assert_eq!(next(&mut s), None);
}
