//! The embedded Modbus/TCP server, serving the app's shared register file to
//! external clients (manual §2.1: port 502 default, max two clients).

use crate::net::read_exact;
use ananasar_core::protocol;
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::{process, Outcome, ServerConfig};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MAX_CLIENTS: usize = 2;

pub type SharedRegs = Arc<Mutex<RegisterFile>>;

/// Live state of the embedded server, shown in the UI.
pub struct ServerHandle {
    pub status: Mutex<String>,
    pub listening: AtomicBool,
    pub clients: AtomicUsize,
    pub port: AtomicUsize,
}

impl ServerHandle {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new("stopped".into()),
            listening: AtomicBool::new(false),
            clients: AtomicUsize::new(0),
            port: AtomicUsize::new(0),
        })
    }
}

/// Start listening on `port`. A `stop` flag lets the UI take the port down
/// (manual §2.7.2 "Monitor port"); the accept loop exits when it is set.
pub fn start(regs: SharedRegs, handle: Arc<ServerHandle>, cfg: ServerConfig, port: u16, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => l,
            Err(e) => {
                *handle.status.lock().unwrap() = format!("bind :{port} failed: {e}");
                return;
            }
        };
        listener.set_nonblocking(true).ok();
        handle.listening.store(true, Ordering::SeqCst);
        handle.port.store(port as usize, Ordering::SeqCst);
        *handle.status.lock().unwrap() = "listening".into();
        let cfg = Arc::new(cfg);

        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    if handle.clients.load(Ordering::SeqCst) >= MAX_CLIENTS {
                        continue;
                    }
                    stream.set_nonblocking(false).ok();
                    handle.clients.fetch_add(1, Ordering::SeqCst);
                    let (regs, cfg, handle) = (regs.clone(), cfg.clone(), handle.clone());
                    std::thread::spawn(move || {
                        serve_conn(stream, &cfg, &regs);
                        handle.clients.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
        handle.listening.store(false, Ordering::SeqCst);
        *handle.status.lock().unwrap() = "stopped".into();
    });
}

/// Idle connections are dropped after this long, freeing the client slot.
const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn serve_conn(mut stream: TcpStream, cfg: &ServerConfig, regs: &Mutex<RegisterFile>) {
    stream.set_read_timeout(Some(IDLE_TIMEOUT)).ok();
    stream.set_write_timeout(Some(IDLE_TIMEOUT)).ok();
    let mut buf = [0u8; 600];
    loop {
        if read_exact(&mut stream, &mut buf[..protocol::MBAP_PREFIX + 1]).is_err() {
            return;
        }
        let total = 6 + u16::from_be_bytes([buf[4], buf[5]]) as usize;
        if total > buf.len() || total < protocol::MBAP_PREFIX + 1 {
            return;
        }
        if read_exact(&mut stream, &mut buf[protocol::MBAP_PREFIX + 1..total]).is_err() {
            return;
        }
        let outcome = {
            let mut regs = regs.lock().unwrap();
            process(cfg, &mut regs, &buf[..total])
        };
        if let Outcome::Reply(r) = outcome
            && stream.write_all(&r).is_err() {
                return;
            }
    }
}
