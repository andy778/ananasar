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

use ananasar_core::log::LogMode;
use ananasar_core::protocol::build;
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_net::{LogSink, Server, Stats};
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
    start_with_log(cfg, None)
}

/// A server whose traffic log (raw mode) is collected in the returned buffer.
fn start_logged(cfg: ServerConfig) -> (Running, LogBuf) {
    let buf = LogBuf::default();
    let sink = LogSink::from_writer(Box::new(buf.clone()), LogMode::Raw);
    (start_with_log(cfg, Some(Arc::new(sink))), buf)
}

#[derive(Clone, Default)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuf {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

fn start_with_log(cfg: ServerConfig, log: Option<Arc<LogSink>>) -> Running {
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
        log,
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
fn client_slots_are_released_so_connections_in_sequence_are_never_refused() {
    let server = start(ServerConfig::default());
    // Well past MAX_CLIENTS, each closed before the next opens. A slot that is
    // never released would make the later ones be refused (closed unanswered).
    for txn in 1..=(ananasar_net::MAX_CLIENTS as u16 * 3) {
        let mut s = connect(&server);
        s.write_all(&read_request(txn)).unwrap();
        assert_eq!(next(&mut s), Some(build(txn, 1, 0x03, &[2, 0, 0])));
        drop(s);
        // Wait for the server to notice the close and free the slot.
        for _ in 0..100 {
            if server.stats.lock().unwrap().slots.iter().all(Option::is_none) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // The connection thread drops its client slot just after freeing the
        // stats slot; give it that moment so a free slot is not a race.
        std::thread::sleep(Duration::from_millis(50));
    }
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

#[test]
fn an_invalid_frame_length_closes_the_connection_and_is_logged() {
    // High byte zero, so the head is well formed, but the length field cannot
    // cover the unit id and function byte already read. (A length over the
    // server's limit cannot be sent this way: it needs a non-zero high byte,
    // which is the bad-header case below.)
    for length in [0u8, 1] {
        let (server, log) = start_logged(ServerConfig::default());
        let mut s = connect(&server);
        s.write_all(&[0, 1, 0, 0, 0, length, 1, 3]).unwrap();
        assert_eq!(next(&mut s), None);
        let text = log.text();
        assert!(text.contains("Error: invalid frame length, connection closed"), "{text}");
        assert!(text.contains("(no data)"), "{text}");
        assert!(server.stats.lock().unwrap().out_of_sync);
    }
}

#[test]
fn a_bad_protocol_id_is_logged_with_the_request_and_closes_the_connection() {
    let (server, log) = start_logged(ServerConfig::default());
    let mut s = connect(&server);
    let mut frame = read_request(1);
    frame[3] = 1;
    s.write_all(&frame).unwrap();
    assert_eq!(next(&mut s), None);
    let text = log.text();
    assert!(text.contains("00 01 00 01 00 06 01 03 0B B8 00 01"), "{text}");
    assert!(text.contains("Error: bad protocol id or length high byte, connection closed"), "{text}");
}

#[test]
fn a_refused_request_with_a_bad_protocol_id_logs_the_reply_then_the_close() {
    let mut cfg = ServerConfig::default();
    cfg.supported.remove(&0x03);
    let (server, log) = start_logged(cfg);
    let mut s = connect(&server);
    let mut frame = read_request(1);
    frame[3] = 1;
    s.write_all(&frame).unwrap();
    assert_eq!(next(&mut s), Some(vec![0, 1, 0, 1, 0, 3, 1, 0x83, 1]));
    assert_eq!(next(&mut s), None);
    let text = log.text();
    let reply = text.find("00 01 00 01 00 03 01 83 01").expect(&text);
    let close = text.find("Error: bad protocol id or length high byte, connection closed").expect(&text);
    assert!(reply < close, "{text}");
}

#[test]
fn a_request_for_another_unit_gets_no_reply_stays_open_and_is_logged() {
    let (server, log) = start_logged(ServerConfig { unit_id: 5, ..Default::default() });
    let mut s = connect(&server);
    s.write_all(&build(1, 6, 0x03, &[0x0B, 0xB8, 0, 1])).unwrap();
    // Requests are handled in order, so the answer to the next one shows the
    // first was dropped and the connection is still served.
    s.write_all(&build(2, 5, 0x03, &[0x0B, 0xB8, 0, 1])).unwrap();
    assert_eq!(next(&mut s), Some(build(2, 5, 0x03, &[2, 0, 0])));
    let text = log.text();
    assert!(text.contains("Error: request dropped: unit id mismatch"), "{text}");
    assert!(text.contains("00 01 00 00 00 06 06 03 0B B8 00 01"), "{text}");
}
