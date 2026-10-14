//! Headless Ananasar: a Modbus/TCP server and client driven from the command
//! line, built on the shared `ananasar-core` engine and `ananasar-net` server.

use ananasar_core::client::{PollEngine, PollList};
use ananasar_core::protocol::{self, DEFAULT_PORT};
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_net::{Event, MAX_CLIENTS, Server, read_frame};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

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
    /// Print a shell completion script to stdout
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
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
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "ananasar", &mut std::io::stdout());
            0
        }
    };
    std::process::exit(code);
}

fn run_server(port: u16, uid: u8) -> i32 {
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            // Manual §2.1: port 502 in use elsewhere surfaces a socket error.
            eprintln!("Socket initialisation failed on port {port}: {e}");
            return 1;
        }
    };
    println!("Ananasar server listening on 0.0.0.0:{port} (uid {uid}, max {MAX_CLIENTS} clients)");

    let server = Server {
        regs: Arc::new(Mutex::new(RegisterFile::new())),
        cfg: Arc::new(ServerConfig { unit_id: uid, ..Default::default() }),
        clients: Arc::new(AtomicUsize::new(0)),
        stop: Arc::new(AtomicBool::new(false)),
        notify: Arc::new(|event| match event {
            Event::Connected(peer) => println!("client connected: {peer}"),
            Event::Disconnected(peer) => println!("client disconnected: {peer}"),
            Event::Activity => {}
        }),
    };
    server.run(&listener);
    0
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
        match read_frame(&mut stream) {
            Ok(resp) => print_response(&resp),
            Err(e) => {
                eprintln!("no/invalid response: {e}");
                return 1;
            }
        }
        std::thread::sleep(interval);
    }
    0
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
