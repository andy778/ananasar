//! The simulated Modbus/TCP server behind the `server_*` tools.

use ananasar_core::log::LogMode;
use ananasar_core::protocol::Exception;
use ananasar_core::registers::{Bank, InitMode, RegisterFile};
use ananasar_core::server::ServerConfig;
use ananasar_core::sync::lock;
use ananasar_net::{LogSink, Server, Stats};
use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// The in-memory data log keeps this many of the most recent bytes.
const LOG_CAP: usize = 1024 * 1024;

/// Function codes the server can be told to answer.
pub const FUNCTION_CODES: [u8; 8] = [3, 4, 6, 7, 8, 16, 22, 23];

/// Modbus exception codes that can be forced, with their wire values.
const FORCEABLE: [(u8, Exception); 10] = [
    (1, Exception::IllegalFunction),
    (2, Exception::IllegalDataAddress),
    (3, Exception::IllegalDataValue),
    (4, Exception::SlaveDeviceFailure),
    (5, Exception::Acknowledge),
    (6, Exception::SlaveDeviceBusy),
    (7, Exception::NegativeAcknowledge),
    (8, Exception::MemoryParityError),
    (10, Exception::GatewayPathUnavailable),
    (11, Exception::GatewayTargetFailed),
];

/// The exception for a wire code, if it is one the server can force.
pub fn exception_from_code(code: u8) -> Option<Exception> {
    FORCEABLE.iter().find(|(c, _)| *c == code).map(|(_, e)| *e)
}

/// Append-only byte buffer that drops its oldest bytes past [`LOG_CAP`].
#[derive(Clone, Default)]
struct RingBuf(Arc<Mutex<Vec<u8>>>);

impl Write for RingBuf {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let mut v = lock(&self.0);
        v.extend_from_slice(b);
        let excess = v.len().saturating_sub(LOG_CAP);
        if excess > 0 {
            v.drain(..excess);
        }
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A running simulated server.
pub struct Sim {
    /// Shared server state (registers, options, statistics).
    pub server: Server,
    /// Address the listener is bound to.
    pub addr: SocketAddr,
    log: Arc<LogSink>,
    buf: RingBuf,
    thread: Option<JoinHandle<()>>,
}

impl Sim {
    /// Bind `bind:port` and start serving on a background thread.
    pub fn start(
        bind: IpAddr,
        port: u16,
        unit_id: u8,
        log_mode: LogMode,
        init: (InitMode, InitMode),
    ) -> Result<Self, String> {
        let listener = TcpListener::bind((bind, port)).map_err(|e| format!("cannot listen on {bind}:{port}: {e}"))?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let mut regs = RegisterFile::new();
        regs.initialize(Bank::Holding, init.0);
        regs.initialize(Bank::Input, init.1);
        let buf = RingBuf::default();
        let log = Arc::new(LogSink::from_writer(Box::new(buf.clone()), log_mode));
        let server = Server {
            regs: Arc::new(Mutex::new(regs)),
            cfg: Arc::new(Mutex::new(ServerConfig { unit_id, ..ServerConfig::default() })),
            clients: Arc::new(AtomicUsize::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(|_| {}),
            log: Some(log.clone()),
            stats: Arc::new(Mutex::new(Stats::default())),
        };
        let runner = server.clone();
        let thread = std::thread::Builder::new()
            .name("modbus-sim".into())
            .spawn(move || runner.run(&listener))
            .map_err(|e| format!("cannot start the server thread: {e}"))?;
        Ok(Self { server, addr, log, buf, thread: Some(thread) })
    }

    /// The log's tail (at most `max_chars` characters), optionally emptying it.
    pub fn log_tail(&self, max_chars: usize, clear: bool) -> String {
        let mut bytes = lock(&self.buf.0);
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if clear {
            bytes.clear();
        }
        let total = text.chars().count();
        let skip = total.saturating_sub(max_chars);
        text.chars().skip(skip).collect()
    }

    /// Entries written to the log since the server started.
    pub fn log_entries(&self) -> u64 {
        self.log.recordings()
    }

    /// Stop accepting clients and wait for the listener to close.
    pub fn stop(&mut self) {
        self.server.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        self.stop();
    }
}
