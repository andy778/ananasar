//! Headless Ananasar: a Modbus/TCP server and client driven from the command
//! line, built on the shared `ananasar-core` engine and `ananasar-net` server.
//! With the default features the same binary also runs the GUI (`ananasar gui`)
//! and the MCP server (`ananasar mcp`).

mod about;
#[cfg(feature = "gui")]
mod console;
mod storm;

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

/// Modbus/TCP client and server: command line, GUI and MCP server in one program.
#[derive(Parser)]
#[command(name = "ananasar", version, about, arg_required_else_help = true)]
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
    #[command(alias = "stress")]
    Storm(Box<storm::StormArgs>),
    /// Open the graphical client and server (the window starts a server of its own)
    #[cfg(feature = "gui")]
    Gui(GuiArgs),
    /// Run the MCP server on stdin/stdout, for AI assistants
    #[cfg(feature = "mcp")]
    Mcp(McpArgs),
    /// Show the version, dependencies, licences and advisories known at build time
    About(about::AboutArgs),
}

#[cfg(feature = "gui")]
#[derive(Args)]
struct GuiArgs {
    /// Listen on exactly this port (no fallback to the next free one)
    #[arg(short, long, value_parser = clap::value_parser!(u16).range(1..))]
    port: Option<u16>,
}

#[cfg(feature = "mcp")]
#[derive(Args)]
struct McpArgs {
    /// Let the assistant write to remote Modbus devices (FC6, FC16, FC22); without it the server is read-only
    #[arg(long, conflicts_with = "read_only")]
    allow_write: bool,
    /// Refuse remote writes (the default; accepted for clarity)
    #[arg(long)]
    read_only: bool,
    /// Address the simulated server listens on
    #[arg(long, value_name = "IP", default_value = "127.0.0.1")]
    bind: std::net::IpAddr,
}

#[derive(Args)]
struct ServerArgs {
    /// TCP port to listen on (port 502 usually needs root)
    #[arg(short, long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// Accepted unit id; 0 accepts any
    #[arg(short = 'u', long = "uid", default_value_t = 0)]
    unit_id: u8,
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
    #[arg(short = 'u', long = "uid", default_value_t = 1)]
    unit_id: u8,
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
    #[arg(short = 'u', long = "uid", default_value_t = 1)]
    unit_id: u8,
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
    // Started by a double-click (the process owns its console alone) there is
    // no command line to read: open the GUI instead of printing help.
    #[cfg(feature = "gui")]
    if std::env::args_os().len() == 1 && console::owns_console_alone() {
        std::process::exit(run_gui(None));
    }
    let code = match Cli::parse().command {
        Command::Server(a) => run_server(&a),
        Command::Client(a) => run_client(a),
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "ananasar", &mut std::io::stdout());
            0
        }
        Command::Scan(a) => run_scan(&a),
        Command::Storm(a) => storm::run_storm(&a),
        Command::About(a) => about::run_about(&a),
        #[cfg(feature = "gui")]
        Command::Gui(a) => run_gui(a.port),
        #[cfg(feature = "mcp")]
        Command::Mcp(a) => {
            let cfg = ananasar_mcp::Config { allow_write: a.allow_write, bind: a.bind };
            ananasar_mcp::serve(cfg)
        }
    };
    std::process::exit(code);
}

/// Run the GUI. On Windows the console is released first when this process is
/// its only user, so a double-click shows no console window.
#[cfg(feature = "gui")]
fn run_gui(port: Option<u16>) -> i32 {
    console::detach_if_alone();
    match ananasar_gui::run(port) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cannot open the window: {e}");
            1
        }
    }
}

fn run_server(args: &ServerArgs) -> i32 {
    let ServerArgs { port, unit_id, .. } = *args;
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
    println!("Ananasar server listening on 0.0.0.0:{port} (unit id {unit_id}, max {MAX_CLIENTS} clients)");

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
            unit_id,
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
    let ClientArgs { target, unit_id, read_holding, read_input, write, count: polls, interval_ms, zero_ti } = args;
    let interval = Duration::from_millis(interval_ms);
    let regs = RegisterFile::new();
    let mut eng = PollEngine::new(unit_id);
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
            Ok(resp) => print_response(&query, &resp),
            Err(e) => {
                eprintln!("no/invalid response: {e}");
                return 1;
            }
        }
        std::thread::sleep(interval);
    }
    0
}

fn print_response(query: &[u8], resp: &[u8]) {
    for line in format_response(query, resp) {
        println!("{line}");
    }
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    d.get(at..end)?.first_chunk::<2>().map(|b| u16::from_be_bytes(*b))
}

/// The output lines for one reply. `query` is the request frame that was sent:
/// a read reply carries no addresses, so they come from the request's start
/// address. Bad or short replies never panic; they yield what can be decoded
/// plus a warning line.
fn format_response(query: &[u8], resp: &[u8]) -> Vec<String> {
    let (hdr, func, pdu) = match protocol::frame(resp) {
        Ok(f) => f,
        Err(e) => return vec![format!("unframeable response: {e}")],
    };
    let ti = hdr.transaction_id;
    if func & 0x80 != 0 {
        let code = pdu.first().copied().unwrap_or(0);
        return vec![format!("TI {ti}: exception on FC 0x{:02X}, code {code}", func & 0x7F)];
    }
    // The request PDU: start address and count are its first four bytes.
    let qpdu = protocol::frame(query).map_or(&[][..], |(_, _, d)| d);
    let (start, count) = (be16(qpdu, 0), be16(qpdu, 2));
    match (func, start, count) {
        (0x03 | 0x04, Some(start), Some(count)) => {
            let data = pdu.get(1..).unwrap_or(&[]);
            let mut lines: Vec<String> = data
                .as_chunks::<2>()
                .0
                .iter()
                .take(usize::from(count))
                .enumerate()
                .map(|(i, w)| {
                    let addr = u32::from(start).saturating_add(u32::try_from(i).unwrap_or(u32::MAX));
                    format!("TI {ti}: FC 0x{func:02X} register {addr} = {}", u16::from_be_bytes(*w))
                })
                .collect();
            let declared = usize::from(pdu.first().copied().unwrap_or(0));
            let wanted = usize::from(count).saturating_mul(2);
            if declared != wanted || data.len() != declared {
                lines.push(format!(
                    "TI {ti}: warning: requested {count} registers ({} bytes), reply has byte count {declared} and {} data bytes",
                    wanted,
                    data.len()
                ));
            }
            lines
        }
        (0x10, _, _) => match (be16(pdu, 0), be16(pdu, 2)) {
            (Some(addr), Some(n)) => vec![format!("TI {ti}: FC 0x10 ok, wrote {n} registers at {addr}")],
            _ => vec![format!("TI {ti}: FC 0x10 ok, truncated reply ({} data bytes)", pdu.len())],
        },
        _ => vec![format!("TI {ti}: FC 0x{func:02X} ok, {} data bytes", pdu.len())],
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
    let mut scan = Scanner::new(args.unit_id, bank, args.start, args.end);
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "test code: a panic is a failed test")]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("ananasar").chain(line.split_whitespace()))
    }

    fn rejected(line: &str) -> clap::error::ErrorKind {
        parse(line).err().unwrap_or_else(|| panic!("`{line}` should not parse")).kind()
    }

    fn req(func: u8, start: u16, count: u16) -> Vec<u8> {
        let mut v = vec![0, 7, 0, 0, 0, 6, 1, func];
        v.extend(start.to_be_bytes());
        v.extend(count.to_be_bytes());
        v
    }

    fn reply(func: u8, pdu: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 7, 0, 0];
        v.extend(u16::try_from(pdu.len().saturating_add(2)).unwrap().to_be_bytes());
        v.extend([1, func]);
        v.extend(pdu);
        v
    }

    #[test]
    fn read_reply_prints_one_line_per_register() {
        let r = reply(3, &[4, 0xFF, 0xF9, 0x00, 0x01]);
        assert_eq!(
            format_response(&req(3, 6, 2), &r),
            ["TI 7: FC 0x03 register 6 = 65529", "TI 7: FC 0x03 register 7 = 1"]
        );
        let r = reply(4, &[2, 0x12, 0x34]);
        assert_eq!(format_response(&req(4, 100, 1), &r), ["TI 7: FC 0x04 register 100 = 4660"]);
    }

    #[test]
    fn write_reply_stays_one_line() {
        let r = reply(0x10, &[0, 5, 0, 2]);
        assert_eq!(format_response(&req(0x10, 5, 2), &r), ["TI 7: FC 0x10 ok, wrote 2 registers at 5"]);
    }

    #[test]
    fn exception_reply_keeps_its_line() {
        let r = reply(0x83, &[2]);
        assert_eq!(format_response(&req(3, 0, 1), &r), ["TI 7: exception on FC 0x03, code 2"]);
    }

    #[test]
    fn short_or_mismatched_reply_warns_without_panicking() {
        // Two registers requested, one returned.
        let lines = format_response(&req(3, 0, 2), &reply(3, &[2, 0, 9]));
        assert_eq!(lines[0], "TI 7: FC 0x03 register 0 = 9");
        assert!(lines[1].contains("warning"), "{lines:?}");
        // Byte count claims more than is present.
        let lines = format_response(&req(3, 0, 1), &reply(3, &[4, 0, 9]));
        assert!(lines.last().unwrap().contains("warning"), "{lines:?}");
        // Empty PDU and truncated write reply.
        assert!(format_response(&req(3, 0, 1), &reply(3, &[])).last().unwrap().contains("warning"));
        assert!(format_response(&req(0x10, 0, 1), &reply(0x10, &[0])).last().unwrap().contains("truncated"));
        // Unusable request: falls back to the byte count line.
        assert_eq!(format_response(&[], &reply(3, &[2, 0, 9])), ["TI 7: FC 0x03 ok, 3 data bytes"]);
        assert!(format_response(&req(3, 0, 1), &[1, 2])[0].starts_with("unframeable"));
    }

    #[test]
    fn clap_definitions_are_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn server_defaults() {
        let Command::Server(a) = parse("server").unwrap().command else { panic!("not server") };
        assert_eq!((a.port, a.unit_id, a.interpretive), (DEFAULT_PORT, 0, false));
        assert!(a.load.is_none() && a.log_dir.is_none());
        assert!(!(a.wrong_ti || a.wrong_protocol_id || a.wrong_length_high || a.out_of_synch || a.address_offset));
    }

    #[test]
    fn server_options() {
        let line = "server -p 1502 -u 7 --load regs.ananas --log-dir logs --interpretive --wrong-ti \
                    --wrong-protocol-id --wrong-length-high --out-of-synch --address-offset";
        let Command::Server(a) = parse(line).unwrap().command else { panic!("not server") };
        assert_eq!((a.port, a.unit_id, a.interpretive), (1502, 7, true));
        assert_eq!(a.load.as_deref(), Some(std::path::Path::new("regs.ananas")));
        assert_eq!(a.log_dir.as_deref(), Some(std::path::Path::new("logs")));
        assert!(a.wrong_ti && a.wrong_protocol_id && a.wrong_length_high && a.out_of_synch && a.address_offset);
    }

    #[test]
    fn server_rejects_bad_values() {
        use clap::error::ErrorKind::MissingRequiredArgument;
        assert_eq!(rejected("server -p 70000"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("server --port=-1"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("server -u 256"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("server --interpretive"), MissingRequiredArgument);
        assert_eq!(rejected("server --no-such-flag"), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn client_lists_and_defaults() {
        let line = "client 127.0.0.1:502 --read-holding 0,1,2 --read-input 10 --write 5,6 -u 3 -n 2 --interval-ms 50 --zero-ti";
        let Command::Client(a) = parse(line).unwrap().command else { panic!("not client") };
        assert_eq!(a.target, "127.0.0.1:502");
        assert_eq!((a.read_holding, a.read_input, a.write), (vec![0, 1, 2], vec![10], vec![5, 6]));
        assert_eq!((a.unit_id, a.count, a.interval_ms, a.zero_ti), (3, 2, 50, true));

        let Command::Client(d) = parse("client host:502").unwrap().command else { panic!("not client") };
        assert_eq!((d.unit_id, d.count, d.interval_ms, d.zero_ti), (1, 5, 1000, false));
        assert!(d.read_holding.is_empty() && d.read_input.is_empty() && d.write.is_empty());
    }

    #[test]
    fn client_rejects_bad_values() {
        use clap::error::ErrorKind::MissingRequiredArgument;
        assert_eq!(rejected("client"), MissingRequiredArgument);
        assert_eq!(rejected("client h:1 --read-holding 65536"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("client h:1 --read-holding 1,x"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("client h:1 --count=-1"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("client h:1 -u 256"), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn scan_options_and_defaults() {
        let Command::Scan(d) = parse("scan host:502").unwrap().command else { panic!("not scan") };
        assert_eq!((d.unit_id, d.start, d.end, d.timeout_ms, d.only_ok), (1, 0, 99, 1000, false));
        assert!(matches!(d.bank, BankArg::Holding));

        let line = "scan host:502 -b input --start 10 --end 20 --timeout-ms 200 --only-ok -u 9";
        let Command::Scan(a) = parse(line).unwrap().command else { panic!("not scan") };
        assert!(matches!(a.bank, BankArg::Input));
        assert_eq!((a.unit_id, a.start, a.end, a.timeout_ms, a.only_ok), (9, 10, 20, 200, true));
    }

    #[test]
    fn scan_rejects_bad_values() {
        use clap::error::ErrorKind::MissingRequiredArgument;
        assert_eq!(rejected("scan"), MissingRequiredArgument);
        assert_eq!(rejected("scan h:1 --bank coil"), clap::error::ErrorKind::InvalidValue);
        assert_eq!(rejected("scan h:1 --end 65536"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("scan h:1 --start=-1"), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn storm_and_its_stress_alias_dispatch_to_storm() {
        for name in ["storm", "stress"] {
            assert!(matches!(parse(&format!("{name} 127.0.0.1:502")).unwrap().command, Command::Storm(_)), "{name}");
        }
        assert_eq!(rejected("storm"), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn about_flags() {
        assert!(matches!(parse("about").unwrap().command, Command::About(_)));
        assert!(matches!(parse("about --deps --advisories --licenses").unwrap().command, Command::About(_)));
        assert_eq!(rejected("about --bogus"), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn completions_take_a_known_shell() {
        let Command::Completions { shell } = parse("completions bash").unwrap().command else {
            panic!("not completions")
        };
        assert_eq!(shell, Shell::Bash);
        assert_eq!(rejected("completions"), clap::error::ErrorKind::MissingRequiredArgument);
        assert_eq!(rejected("completions tcsh"), clap::error::ErrorKind::InvalidValue);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn gui_port_must_be_a_nonzero_u16() {
        let Command::Gui(a) = parse("gui -p 1502").unwrap().command else { panic!("not gui") };
        assert_eq!(a.port, Some(1502));
        let Command::Gui(a) = parse("gui").unwrap().command else { panic!("not gui") };
        assert_eq!(a.port, None);
        assert_eq!(rejected("gui -p 0"), clap::error::ErrorKind::ValueValidation);
        assert_eq!(rejected("gui -p 65536"), clap::error::ErrorKind::ValueValidation);
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_write_modes_conflict() {
        let Command::Mcp(a) = parse("mcp").unwrap().command else { panic!("not mcp") };
        assert!(!a.allow_write && !a.read_only);
        assert_eq!(a.bind, std::net::IpAddr::from([127, 0, 0, 1]));
        let Command::Mcp(a) = parse("mcp --allow-write --bind ::1").unwrap().command else { panic!("not mcp") };
        assert!(a.allow_write);
        assert_eq!(a.bind, "::1".parse::<std::net::IpAddr>().unwrap());
        assert_eq!(rejected("mcp --allow-write --read-only"), clap::error::ErrorKind::ArgumentConflict);
        assert_eq!(rejected("mcp --bind nowhere"), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn unknown_subcommand_and_bare_invocation_are_rejected() {
        assert_eq!(rejected("frobnicate"), clap::error::ErrorKind::InvalidSubcommand);
        assert_eq!(rejected(""), clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand);
    }
}
