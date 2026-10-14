//! The embedded Modbus/TCP server, serving the app's shared register file to
//! external clients (manual §2.1: port 502 default, max two clients).

use ananasar_core::registers::RegisterFile;
use ananasar_core::server::ServerConfig;
use ananasar_core::sync::lock;
use ananasar_net::Server;
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
}

impl ServerHandle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new("stopped".into()),
            listening: AtomicBool::new(false),
            clients: Arc::new(AtomicUsize::new(0)),
            port: AtomicUsize::new(0),
        })
    }
}

/// Start listening on `port`. A `stop` flag lets the UI take the port down
/// (manual §2.7.2 "Monitor port"); the accept loop exits when it is set.
/// The UI is repainted whenever a client connects, disconnects or sends a
/// request, so it can stay idle in between.
pub fn start(
    ctx: egui::Context,
    regs: SharedRegs,
    handle: Arc<ServerHandle>,
    cfg: ServerConfig,
    port: u16,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => l,
            Err(e) => {
                *lock(&handle.status) = format!("bind :{port} failed: {e}");
                ctx.request_repaint();
                return;
            }
        };
        handle.listening.store(true, Ordering::SeqCst);
        handle.port.store(usize::from(port), Ordering::SeqCst);
        *lock(&handle.status) = "listening".into();
        ctx.request_repaint();

        let repaint = ctx.clone();
        Server {
            regs,
            cfg: Arc::new(cfg),
            clients: handle.clients.clone(),
            stop,
            notify: Arc::new(move |_| repaint.request_repaint()),
            log: None,
        }
        .run(&listener);

        handle.listening.store(false, Ordering::SeqCst);
        *lock(&handle.status) = "stopped".into();
        ctx.request_repaint();
    });
}
