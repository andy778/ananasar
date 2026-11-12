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
use ananasar_parity::{diff, proxy, runner, scenarios};
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
    // Generous, because a close or reply that arrives after the timeout is
    // recorded as no reply: at 200 ms a slow macOS runner sometimes recorded
    // `bad_length_high_byte` as closed in one run and silent in the other.
    // Only the few scenarios that really get no reply pay this wait.
    let timeout = Duration::from_secs(1);
    let first = start_server();
    let mut a = runner::run(first.addr, 1, timeout).unwrap();
    let second = start_server();
    let mut b = runner::run(second.addr, 1, timeout).unwrap();
    // Before reporting a difference, re-run the differing scenarios against fresh servers
    // with a long wait: a reply that is really missing is still missing after 5 s, but one
    // that only arrived late on a slow machine is then recorded.
    let differing: Vec<String> =
        diff::verdicts(&a, &b, diff::Options::default()).into_iter().filter(|v| !v.matches()).map(|v| v.name).collect();
    if !differing.is_empty() {
        let (first, second) = (start_server(), start_server());
        let retry = Duration::from_secs(5);
        for (rec, server) in [(&mut a, &first), (&mut b, &second)] {
            let again =
                runner::rerun_sections(server.addr, scenarios::DEFAULT_PROFILE, &differing, 1, timeout, retry).unwrap();
            for section in again {
                if let Some(slot) = rec.sections.iter_mut().find(|s| s.name == section.name) {
                    *slot = section;
                }
            }
        }
    }
    let differences = diff::diff(&a, &b);
    assert!(differences.is_empty(), "two runs differ:\n{}", differences.join("\n"));

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
    // A bad protocol id closes the connection, as the original does.
    let bad = section(&a, "bad_protocol_id");
    assert_eq!(bad[1], Entry::Closed);
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
