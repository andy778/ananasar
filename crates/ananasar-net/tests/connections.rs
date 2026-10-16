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
use ananasar_net::{Server, Stats};
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Running {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
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
    let stats = Arc::new(Mutex::new(Stats::default()));
    let server = Server {
        regs: Arc::new(Mutex::new(RegisterFile::new())),
        cfg: Arc::new(Mutex::new(cfg)),
        clients: Arc::default(),
        stop: stop.clone(),
        notify: Arc::new(|_| {}),
        log: None,
        stats: stats.clone(),
    };
    std::thread::spawn(move || server.run(&listener));
    Running { addr, stop, stats }
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

#[test]
fn reply_out_of_synch_answers_one_request_late() {
    let server = start(ServerConfig { out_of_synch: true, ..Default::default() });
    let mut s = connect(&server);
    s.write_all(&read_request(1)).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(150))).unwrap();
    let mut buf = [0u8; 16];
    let first = s.read(&mut buf);
    assert!(matches!(&first, Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)), "{first:?}");
    s.write_all(&read_request(2)).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    assert_eq!(next(&mut s), Some(build(1, 1, 0x03, &[2, 0, 0])));
    s.write_all(&read_request(3)).unwrap();
    assert_eq!(next(&mut s), Some(build(2, 1, 0x03, &[2, 0, 0])));
}

#[test]
fn nagle_is_switched_on_a_live_connection_and_only_while_it_exists() {
    let server = start(ServerConfig::default());
    assert!(server.stats.lock().unwrap().set_nagle(0, false).is_err(), "no connection yet");
    let mut s = connect(&server);
    s.write_all(&read_request(1)).unwrap();
    assert!(next(&mut s).is_some());
    {
        let mut stats = server.stats.lock().unwrap();
        assert!(stats.slots[0].as_ref().unwrap().nagle, "enabled by default");
        stats.set_nagle(0, false).unwrap();
        assert!(!stats.slots[0].as_ref().unwrap().nagle);
        assert_eq!(stats.set_nagle(1, false).unwrap_err().kind(), ErrorKind::NotConnected);
        stats.set_nagle(0, true).unwrap();
        assert!(stats.slots[0].as_ref().unwrap().nagle);
    }
    // The connection still serves requests afterwards.
    s.write_all(&read_request(2)).unwrap();
    assert!(next(&mut s).is_some());
    drop(s);
    for _ in 0..50 {
        if server.stats.lock().unwrap().slots[0].is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(server.stats.lock().unwrap().set_nagle(0, false).unwrap_err().kind(), ErrorKind::NotConnected);
}
