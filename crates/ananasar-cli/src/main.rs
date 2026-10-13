//! Headless Ananas: a Modbus/TCP server and client driven from the command line.
//!
//! The GUI (the Delphi VCL forms) is a separate layer and not reimplemented here
//! yet; this exercises the reconstructed protocol core end to end. The server
//! uses blocking sockets with a thread per connection and accepts at most two
//! clients, matching the original's "limited multiport support" (manual §2.1).

use ananasar_core::client::{PollEngine, PollList};
use ananasar_core::protocol::{self, DEFAULT_PORT};
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::{process, Outcome, ServerConfig};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_CLIENTS: usize = 2;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        Some("server") => run_server(&args[2..]),
        Some("client") => run_client(&args[2..]),
        _ => {
            usage();
            2
        }
    };
    std::process::exit(code);
}

fn usage() {
    eprintln!(
        "ananas — Modbus/TCP client/server (headless)\n\n\
         USAGE:\n  \
         ananasar server [--port N] [--uid N]\n  \
         ananasar client <host:port> [--uid N] [--read-holding a,b,..] \\\n                \
         [--read-input a,b,..] [--write a,b,..] [--count N] [--interval-ms N]\n"
    );
}

fn parse_opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn parse_list(s: &str) -> Vec<u16> {
    s.split(',').filter_map(|t| t.trim().parse().ok()).collect()
}

fn run_server(args: &[String]) -> i32 {
    let port: u16 = parse_opt(args, "--port").and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_PORT);
    let uid: u8 = parse_opt(args, "--uid").and_then(|s| s.parse().ok()).unwrap_or(0);

    let cfg = Arc::new(ServerConfig { unit_id: uid, ..Default::default() });
    let regs = Arc::new(Mutex::new(RegisterFile::new()));
    let clients = Arc::new(AtomicUsize::new(0));

    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            // Manual §2.1: port 502 in use elsewhere surfaces a socket error.
            eprintln!("Socket initialisation failed on port {port}: {e}");
            return 1;
        }
    };
    println!("Ananasar server listening on 0.0.0.0:{port} (uid {uid}, max {MAX_CLIENTS} clients)");

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if clients.load(Ordering::SeqCst) >= MAX_CLIENTS {
            // Over the two-client limit: refuse by dropping the connection.
            drop(stream);
            continue;
        }
        clients.fetch_add(1, Ordering::SeqCst);
        let (cfg, regs, clients) = (cfg.clone(), regs.clone(), clients.clone());
        std::thread::spawn(move || {
            let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
            println!("client connected: {peer}");
            serve_connection(stream, &cfg, &regs);
            clients.fetch_sub(1, Ordering::SeqCst);
            println!("client disconnected: {peer}");
        });
    }
    0
}

fn serve_connection(mut stream: TcpStream, cfg: &ServerConfig, regs: &Mutex<RegisterFile>) {
    let mut buf = [0u8; 600];
    loop {
        // Read the MBAP prefix + length, then the rest of the frame.
        if read_exact_or_eof(&mut stream, &mut buf[..protocol::MBAP_PREFIX + 1]).is_none() {
            return;
        }
        let declared = u16::from_be_bytes([buf[4], buf[5]]) as usize;
        let total = 6 + declared;
        if total > buf.len() || total < protocol::MBAP_PREFIX + 1 {
            return; // malformed length
        }
        if read_exact_or_eof(&mut stream, &mut buf[protocol::MBAP_PREFIX + 1..total]).is_none() {
            return;
        }
        let outcome = {
            let mut regs = regs.lock().unwrap();
            process(cfg, &mut regs, &buf[..total])
        };
        if let Outcome::Reply(r) = outcome {
            if stream.write_all(&r).is_err() {
                return;
            }
        }
    }
}

fn read_exact_or_eof(stream: &mut TcpStream, buf: &mut [u8]) -> Option<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return None,
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    Some(())
}

fn run_client(args: &[String]) -> i32 {
    let Some(target) = args.first() else {
        usage();
        return 2;
    };
    let uid: u8 = parse_opt(args, "--uid").and_then(|s| s.parse().ok()).unwrap_or(1);
    let interval = Duration::from_millis(
        parse_opt(args, "--interval-ms").and_then(|s| s.parse().ok()).unwrap_or(1000),
    );
    let polls: usize = parse_opt(args, "--count").and_then(|s| s.parse().ok()).unwrap_or(5);

    let regs = RegisterFile::new();
    let mut eng = PollEngine::new(uid);
    if let Some(s) = parse_opt(args, "--read-holding") {
        eng.read_holding = PollList::new(parse_list(s));
    }
    if let Some(s) = parse_opt(args, "--read-input") {
        eng.read_input = PollList::new(parse_list(s));
    }
    if let Some(s) = parse_opt(args, "--write") {
        eng.write = PollList::new(parse_list(s));
    }
    if eng.is_idle() {
        eprintln!("nothing to poll: give --read-holding / --read-input / --write");
        return 2;
    }

    let mut stream = match TcpStream::connect(target) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("connect to {target} failed: {e}");
            return 1;
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    println!("connected to {target}");

    for _ in 0..polls {
        let Some(query) = eng.next_query(&regs) else { break };
        if stream.write_all(&query).is_err() {
            eprintln!("send failed");
            return 1;
        }
        match read_response(&mut stream) {
            Some(resp) => print_response(&resp),
            None => {
                eprintln!("no/invalid response");
                return 1;
            }
        }
        std::thread::sleep(interval);
    }
    0
}

fn read_response(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut head = [0u8; protocol::MBAP_PREFIX + 1];
    read_exact_or_eof(stream, &mut head)?;
    let declared = u16::from_be_bytes([head[4], head[5]]) as usize;
    let total = 6 + declared;
    let mut out = head.to_vec();
    out.resize(total, 0);
    read_exact_or_eof(stream, &mut out[protocol::MBAP_PREFIX + 1..])?;
    Some(out)
}

fn print_response(resp: &[u8]) {
    match protocol::frame(resp) {
        Ok((hdr, func, pdu)) => {
            if func & 0x80 != 0 {
                println!("TI {}: exception on FC 0x{:02X}, code {}", hdr.transaction_id, func & 0x7F, pdu.first().copied().unwrap_or(0));
            } else {
                println!("TI {}: FC 0x{func:02X} ok, {} data bytes", hdr.transaction_id, pdu.len());
            }
        }
        Err(e) => println!("unframeable response: {e:?}"),
    }
}
