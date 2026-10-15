//! The embedded Modbus/TCP server, serving the app's shared register file to
//! external clients (default port 502, at most two clients).

use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_core::sync::lock;
use ananasar_net::{LogSink, Server, Stats};
use eframe::egui;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub type SharedRegs = Arc<Mutex<RegisterFile>>;

/// Live state of the embedded server, shown in the UI.
pub struct ServerHandle {
    pub status: Mutex<String>,
    pub listening: AtomicBool,
    pub clients: Arc<AtomicUsize>,
    pub port: AtomicUsize,
    /// Bumped on every start, so a server that was replaced leaves its successor's state alone.
    generation: AtomicUsize,
    /// Data log shared with the running server; idle until the user starts it.
    pub log: Arc<LogSink>,
    /// Live statistics for the status panel.
    pub stats: Arc<Mutex<Stats>>,
    /// Server settings the UI can change while it runs (the accepted UID).
    pub cfg: Arc<Mutex<ServerConfig>>,
}

impl ServerHandle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new("stopped".into()),
            listening: AtomicBool::new(false),
            clients: Arc::new(AtomicUsize::new(0)),
            port: AtomicUsize::new(0),
            generation: AtomicUsize::new(0),
            log: Arc::new(LogSink::idle()),
            stats: Arc::default(),
            cfg: Arc::default(),
        })
    }
}

/// Start listening on `port`. The port is bound before this returns, so a
/// caller can report a bind error and keep the previous server running. A
/// `stop` flag lets the UI take the port down (the original's "Monitor port"
/// option); the accept loop exits when it is set. The UI is repainted whenever
/// a client connects, disconnects or sends a request, so it can stay idle in
/// between.
pub fn start(
    ctx: egui::Context,
    regs: SharedRegs,
    handle: Arc<ServerHandle>,
    port: u16,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let generation = handle.generation.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
    handle.listening.store(true, Ordering::SeqCst);
    handle.port.store(usize::from(port), Ordering::SeqCst);
    *lock(&handle.status) = "listening".into();
    ctx.request_repaint();

    std::thread::spawn(move || {
        let (repaint, log) = (ctx.clone(), handle.log.clone());
        Server {
            regs,
            cfg: handle.cfg.clone(),
            clients: handle.clients.clone(),
            stop,
            notify: Arc::new(move |_| repaint.request_repaint()),
            log: Some(log),
            stats: handle.stats.clone(),
        }
        .run(&listener);

        // A replaced server must not mark its successor as stopped.
        if handle.generation.load(Ordering::SeqCst) == generation {
            handle.listening.store(false, Ordering::SeqCst);
            *lock(&handle.status) = "stopped".into();
        }
        ctx.request_repaint();
    });
    Ok(())
}
