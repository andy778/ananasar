//! Runs the tools against the in-process Ananasar server on loopback.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_net::Server;
use ananasar_parity::recording::{Entry, Recording};
use ananasar_parity::{diff, proxy, runner};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A running server that stops when dropped.
struct Running {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn start_server() -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server {
        regs: Arc::new(Mutex::new(RegisterFile::new())),
        cfg: Arc::new(Mutex::new(ServerConfig::default())),
        clients: Arc::default(),
        stop: stop.clone(),
        notify: Arc::new(|_| {}),
        log: None,
        stats: Arc::default(),
    };
    std::thread::spawn(move || server.run(&listener));
    Running { addr, stop }
}

fn section<'a>(rec: &'a Recording, name: &str) -> &'a [Entry] {
    &rec.sections.iter().find(|s| s.name == name).unwrap().entries
}

#[test]
fn scenarios_record_expected_replies_and_repeat_identically() {
    let timeout = Duration::from_millis(200);
    let first = start_server();
    let a = runner::run(first.addr, 1, timeout).unwrap();
    let second = start_server();
    let b = runner::run(second.addr, 1, timeout).unwrap();
    assert!(diff::diff(&a, &b).is_empty());

    // 125 registers: byte count 250, MBAP length 253.
    let Entry::Received(limit) = &section(&a, "read_holding_limit")[1] else { panic!("no reply") };
    assert_eq!(limit.len(), 6 + 253);
    // One over the limit: exception 3 on FC3.
    let Entry::Received(over) = &section(&a, "read_holding_over_limit")[1] else { panic!("no reply") };
    assert_eq!(&over[7..], &[0x83, 0x03]);
    // Modbus spec FC22 example.
    let Entry::Received(mask) = &section(&a, "mask_write")[5] else { panic!("no reply") };
    assert_eq!(&mask[8..], &[2, 0x00, 0x17]);
    // An incomplete frame gets no reply.
    assert_eq!(section(&a, "bad_length_too_long")[1], Entry::NoReply);
    // A bad protocol id is dropped but the connection still serves.
    let bad = section(&a, "bad_protocol_id");
    assert_eq!(bad[1], Entry::NoReply);
    assert!(matches!(bad[3], Entry::Received(_)));
}

#[test]
fn proxy_records_both_directions() {
    let server = start_server();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let target = server.addr;
    let client = std::thread::spawn(move || {
        let stream = listener.accept().unwrap().0;
        proxy::proxy_connection(stream, target, "conn 1").unwrap()
    });

    let request = ananasar_core::protocol::build(9, 1, 3, &[0, 0, 0, 2]);
    let mut c = TcpStream::connect(proxy_addr).unwrap();
    c.write_all(&request).unwrap();
    let mut reply = [0u8; 13];
    c.read_exact(&mut reply).unwrap();
    drop(c);

    let section = client.join().unwrap();
    assert_eq!(section.entries, vec![Entry::Sent(request), Entry::Received(reply.to_vec())]);
}
