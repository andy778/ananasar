//! Headless Ananas: a Modbus/TCP server and client driven from the command line.
//!
//! This drives the `ananasar-core` protocol engine end to end without a GUI. The
//! server uses blocking `std::net` sockets with a thread per connection and
//! accepts at most two clients, matching the original's "limited multiport
//! support" (manual §2.1). At that scale threads are simpler than an async
//! runtime; if the client cap is ever lifted, or connections need timeouts and
//! graceful shutdown, moving this binary to tokio would be the natural step
//! (the core crate is I/O-free, so it would not change).

use ananasar_core::sync::lock;
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
/// A connection that sends nothing for this long is dropped, so idle clients
/// cannot hold both slots forever.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

use clap::{Args, Parser, Subcommand};

/// Modbus/TCP client and server (headless).
#[derive(Parser)]
#[command(name = "ananasar", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a Modbus/TCP server backed by an in-memory register file
    Server(ServerArgs),
    /// Poll a Modbus/TCP server and print the responses
    Client(ClientArgs),
}

#[derive(Args)]
struct ServerArgs {
    /// TCP port to listen on (port 502 usually needs root)
    #[arg(short, long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// Accepted unit id; 0 accepts any
    #[arg(short, long, default_value_t = 0)]
    uid: u8,
}

#[derive(Args)]
struct ClientArgs {
    /// Server address as host:port
    target: String,
    /// Unit id to address
    #[arg(short, long, default_value_t = 1)]
    uid: u8,
    /// Holding registers to read (FC3), comma-separated
    #[arg(long, value_delimiter = ',', value_name = "ADDR,...")]
    read_holding: Vec<u16>,
    /// Input registers to read (FC4), comma-separated
    #[arg(long, value_delimiter = ',', value_name = "ADDR,...")]
    read_input: Vec<u16>,
    /// Holding registers to write (FC16), comma-separated
    #[arg(long, value_delimiter = ',', value_name = "ADDR,...")]
    write: Vec<u16>,
    /// Number of polls to send
    #[arg(short = 'n', long, default_value_t = 5)]
    count: usize,
    /// Delay between polls in milliseconds
    #[arg(long, default_value_t = 1000)]
    interval_ms: u64,
}

fn main() {
    let code = match Cli::parse().command {
        Command::Server(a) => run_server(a.port, a.uid),
        Command::Client(a) => run_client(a),
    };
    std::process::exit(code);
}

fn run_server(port: u16, uid: u8) -> i32 {
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
    stream.set_read_timeout(Some(IDLE_TIMEOUT)).ok();
    stream.set_write_timeout(Some(IDLE_TIMEOUT)).ok();
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
            let mut regs = lock(&regs);
            process(cfg, &mut regs, &buf[..total])
        };
        if let Outcome::Reply(r) = outcome
            && stream.write_all(&r).is_err() {
                return;
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

fn run_client(args: ClientArgs) -> i32 {
    let ClientArgs { target, uid, read_holding, read_input, write, count: polls, interval_ms } = args;
    let interval = Duration::from_millis(interval_ms);
    let regs = RegisterFile::new();
    let mut eng = PollEngine::new(uid);
    eng.read_holding = PollList::new(read_holding);
    eng.read_input = PollList::new(read_input);
    eng.write = PollList::new(write);
    if eng.is_idle() {
        eprintln!("nothing to poll: give --read-holding / --read-input / --write");
        return 2;
    }

    let mut stream = match TcpStream::connect(&target) {
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
        Err(e) => println!("unframeable response: {e}"),
    }
}
