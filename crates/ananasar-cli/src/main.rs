//! Headless Ananasar: a Modbus/TCP server and client driven from the command
//! line, built on the shared `ananasar-core` engine and `ananasar-net` server.

mod about;
mod stress;

use ananasar_core::ananas_file;
use ananasar_core::client::{PollEngine, PollList};
use ananasar_core::log::LogMode;
use ananasar_core::protocol::{self, DEFAULT_PORT};
use ananasar_core::registers::{Bank, RegisterFile};
use ananasar_core::scanner::{ScanResult, Scanner};
use ananasar_core::server::ServerConfig;
use ananasar_net::{Event, LogSink, MAX_CLIENTS, Server, read_frame};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
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
    /// Read a range of registers one by one and report which respond
    Scan(ScanArgs),
    /// Load-test a Modbus/TCP target (only equipment you may test): rate, ramp, faults, churn
    Stress(Box<stress::StressArgs>),
    /// Show the version, dependencies, licences and advisories known at build time
    About(about::AboutArgs),
}

#[derive(Args)]
struct ServerArgs {
    /// TCP port to listen on (port 502 usually needs root)
    #[arg(short, long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// Accepted unit id; 0 accepts any
    #[arg(short, long, default_value_t = 0)]
    uid: u8,
    /// Load register values from a *.ananas file before serving
    #[arg(long, value_name = "FILE")]
    load: Option<PathBuf>,
    /// Log the server's Modbus exchanges to a file in this directory
    #[arg(long, value_name = "DIR")]
    log_dir: Option<PathBuf>,
    /// Add decoded descriptions to the log entries (needs --log-dir)
    #[arg(long, requires = "log_dir")]
    interpretive: bool,
    /// Put transaction id DE AD in every reply
    #[arg(long)]
    wrong_ti: bool,
    /// Put protocol id AB CD in normal replies
    #[arg(long)]
    wrong_protocol_id: bool,
    /// Put EF in the length high byte of normal replies
    #[arg(long)]
    wrong_length_high: bool,
    /// Answer every request one request late
    #[arg(long)]
    out_of_synch: bool,
    /// Wire address n reaches register n + 1
    #[arg(long)]
    address_offset: bool,
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
    /// Always send transaction id 0 in requests
    #[arg(long)]
    zero_ti: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum BankArg {
    Holding,
    Input,
}

#[derive(Args)]
struct ScanArgs {
    /// Server address as host:port
    target: String,
    /// Unit id to address
    #[arg(short, long, default_value_t = 1)]
    uid: u8,
    /// Register bank to scan
    #[arg(short, long, value_enum, default_value_t = BankArg::Holding)]
    bank: BankArg,
    /// First address
    #[arg(long, default_value_t = 0)]
    start: u16,
    /// Last address (inclusive)
    #[arg(long, default_value_t = 99)]
    end: u16,
    /// Per-request reply timeout in milliseconds
    #[arg(long, default_value_t = 1000)]
    timeout_ms: u64,
    /// Only print addresses that answered with a value
    #[arg(long)]
    only_ok: bool,
}

fn main() {
    let code = match Cli::parse().command {
        Command::Server(a) => run_server(&a),
        Command::Client(a) => run_client(a),
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "ananasar", &mut std::io::stdout());
            0
        }
        Command::Scan(a) => run_scan(&a),
        Command::Stress(a) => stress::run_stress(&a),
        Command::About(a) => about::run_about(&a),
    };
    std::process::exit(code);
}

fn run_server(args: &ServerArgs) -> i32 {
    let ServerArgs { port, uid, .. } = *args;
    let mut registers = RegisterFile::new();
    if let Some(path) = args.load.as_deref() {
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| ananas_file::load(&text, &mut registers).map_err(|e| e.to_string()));
        if let Err(e) = loaded {
            eprintln!("cannot load {}: {e}", path.display());
            return 1;
        }
        println!("loaded registers from {}", path.display());
    }
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            // Like the original: a port already in use surfaces a socket error.
            eprintln!("Socket initialisation failed on port {port}: {e}");
            return 1;
        }
    };
    println!("Ananasar server listening on 0.0.0.0:{port} (uid {uid}, max {MAX_CLIENTS} clients)");

    let log = match &args.log_dir {
        Some(dir) => {
            let mode = if args.interpretive { LogMode::Interpretive } else { LogMode::Raw };
            let made = std::fs::create_dir_all(dir).and_then(|()| LogSink::create(dir, &port.to_string(), mode));
            match made {
                Ok(sink) => {
                    if let Some(p) = sink.path() {
                        println!("logging to {}", p.display());
                    }
                    Some(Arc::new(sink))
                }
                Err(e) => {
                    eprintln!("cannot create log file in {}: {e}", dir.display());
                    return 1;
                }
            }
        }
        None => None,
    };

    let server = Server {
        regs: Arc::new(Mutex::new(registers)),
        cfg: Arc::new(Mutex::new(ServerConfig {
            unit_id: uid,
            wrong_transaction_id: args.wrong_ti,
            wrong_protocol_id: args.wrong_protocol_id,
            wrong_length_high: args.wrong_length_high,
            out_of_synch: args.out_of_synch,
            address_offset: args.address_offset,
            ..Default::default()
        })),
        clients: Arc::new(AtomicUsize::new(0)),
        stop: Arc::new(AtomicBool::new(false)),
        notify: Arc::new(|event| match event {
            Event::Connected(peer) => println!("client connected: {peer}"),
            Event::Disconnected(peer) => println!("client disconnected: {peer}"),
            Event::Activity => {}
        }),
        log,
        stats: Arc::default(),
    };
    server.run(&listener);
    0
}

fn run_client(args: ClientArgs) -> i32 {
    let ClientArgs { target, uid, read_holding, read_input, write, count: polls, interval_ms, zero_ti } = args;
    let interval = Duration::from_millis(interval_ms);
    let regs = RegisterFile::new();
    let mut eng = PollEngine::new(uid);
    eng.force_zero_txn = zero_ti;
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

fn run_scan(args: &ScanArgs) -> i32 {
    let bank = match args.bank {
        BankArg::Holding => Bank::Holding,
        BankArg::Input => Bank::Input,
    };
    let mut stream = match TcpStream::connect(&args.target) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("connect to {} failed: {e}", args.target);
            return 1;
        }
    };
    stream.set_read_timeout(Some(Duration::from_millis(args.timeout_ms))).ok();
    let started = std::time::Instant::now();
    let mut scan = Scanner::new(args.uid, bank, args.start, args.end);
    while let Some((addr, query)) = scan.next_query() {
        let response = stream.write_all(&query).ok().and_then(|()| read_frame(&mut stream).ok());
        let broken = response.is_none();
        match scan.record(addr, response.as_deref()) {
            ScanResult::Value(v) => println!("{addr:5}: {v:5} (0x{v:04X})"),
            ScanResult::Exception(c) if !args.only_ok => println!("{addr:5}: exception {c}"),
            ScanResult::NoReply if !args.only_ok => println!("{addr:5}: no reply"),
            _ => {}
        }
        if broken {
            // A timeout leaves the stream out of step with the server; stop.
            eprintln!("stopping: no usable reply for address {addr}");
            break;
        }
    }
    let s = scan.summary();
    println!(
        "{} registers scanned, {} succeeded, {} failed, total duration {:.2?}",
        s.scanned,
        s.succeeded,
        s.failed,
        started.elapsed()
    );
    i32::from(s.succeeded == 0)
}
