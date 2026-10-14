//! Blocking Modbus/TCP networking shared by the CLI and the GUI: a frame
//! reader and the two-client server loop.
//!
//! At the original's two-client limit, a thread per connection over blocking
//! `std::net` sockets is simpler than an async runtime. If the client cap is
//! ever lifted, moving this crate to tokio would be the natural step; the core
//! crate is I/O-free and would not change.

pub use client::{connect, parse_endpoint, split_host_port};
pub use log::LogSink;
pub use stats::{ConnStats, Stats};

use ananasar_core::log::Response;
use ananasar_core::protocol::{self, MBAP_PREFIX};
use ananasar_core::registers::RegisterFile;
use ananasar_core::server::{Outcome, ServerConfig, process};
use ananasar_core::sync::lock;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod client;
mod log;
mod stats;

/// Maximum simultaneous clients, as in the original.
pub const MAX_CLIENTS: usize = 2;
/// A connection silent for this long is dropped so idle clients cannot hold
/// both slots forever.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Largest request a server accepts (a 253-byte PDU plus header, with margin).
const MAX_REQUEST: usize = 600;
/// Largest frame the 16-bit MBAP length field can describe.
const MAX_FRAME: usize = 6 + u16::MAX as usize;

/// Read one complete frame: the MBAP prefix and function byte, then the rest
/// as declared by the length field.
pub fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    read_frame_max(r, MAX_FRAME)
}

fn read_frame_max(r: &mut impl Read, max: usize) -> io::Result<Vec<u8>> {
    let mut head = [0u8; MBAP_PREFIX + 1];
    r.read_exact(&mut head)?;
    let total = 6 + usize::from(u16::from_be_bytes([head[4], head[5]]));
    if !(head.len()..=max).contains(&total) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad MBAP length"));
    }
    let mut out = head.to_vec();
    out.resize(total, 0);
    r.read_exact(&mut out[head.len()..])?;
    Ok(out)
}

/// Something worth telling the host application about.
#[derive(Clone, Debug)]
pub enum Event {
    /// A client connected (peer address).
    Connected(String),
    /// A client disconnected (peer address).
    Disconnected(String),
    /// A request was processed and the register file may have changed.
    Activity,
}

/// Everything the accept loop shares with its connection threads.
#[derive(Clone)]
pub struct Server {
    /// The register file served to clients.
    pub regs: Arc<Mutex<RegisterFile>>,
    /// Unit-id filtering and limits.
    pub cfg: Arc<Mutex<ServerConfig>>,
    /// Number of connected clients (readable by the host for display).
    pub clients: Arc<AtomicUsize>,
    /// Set to take the port down; the accept loop then returns.
    pub stop: Arc<AtomicBool>,
    /// Called for every [`Event`], from server threads.
    pub notify: Arc<dyn Fn(Event) + Send + Sync>,
    /// Data log; `None` disables logging.
    pub log: Option<Arc<LogSink>>,
    /// Live statistics for a status display.
    pub stats: Arc<Mutex<Stats>>,
}

/// Decrements the client count when a connection thread ends, even by panic.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Server {
    /// Accept clients on `listener` until `stop` is set. Connections beyond
    /// [`MAX_CLIENTS`] are refused by dropping them.
    pub fn run(&self, listener: &TcpListener) {
        listener.set_nonblocking(true).ok();
        while !self.stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => self.spawn_connection(stream),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }

    fn spawn_connection(&self, stream: TcpStream) {
        if self.clients.load(Ordering::SeqCst) >= MAX_CLIENTS {
            return;
        }
        stream.set_nonblocking(false).ok();
        self.clients.fetch_add(1, Ordering::SeqCst);
        let slot = Slot(self.clients.clone());
        let server = self.clone();
        std::thread::spawn(move || {
            let _slot = slot;
            let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
            let nagle = !stream.nodelay().unwrap_or(false);
            let ip = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
            let stats_slot = lock(&server.stats).connect(&ip, nagle);
            (server.notify)(Event::Connected(peer.clone()));
            let conn = server.log.as_ref().map_or(0, |l| l.open_connection());
            server.serve_connection(stream, conn, stats_slot);
            if let Some(l) = &server.log {
                l.close_connection(conn);
            }
            if let Some(slot) = stats_slot {
                lock(&server.stats).disconnect(slot);
            }
            (server.notify)(Event::Disconnected(peer));
        });
    }

    fn serve_connection(&self, mut stream: TcpStream, conn: u8, stats_slot: Option<usize>) {
        stream.set_read_timeout(Some(IDLE_TIMEOUT)).ok();
        stream.set_write_timeout(Some(IDLE_TIMEOUT)).ok();
        let mut last_request: Option<Instant> = None;
        loop {
            let request = match read_frame_max(&mut stream, MAX_REQUEST) {
                Ok(r) => r,
                Err(e) => {
                    // A bad length field is a detected error; a closed or
                    // idle connection is not.
                    if e.kind() == io::ErrorKind::InvalidData {
                        lock(&self.stats).out_of_sync = true;
                        (self.notify)(Event::Activity);
                        if let Some(log) = &self.log {
                            log.record(conn, &[], Response::Error("invalid frame length, connection closed"));
                        }
                    }
                    return;
                }
            };
            let now = Instant::now();
            let interval_ms = last_request.map(|prev| u64::try_from(now.duration_since(prev).as_millis()).unwrap_or(u64::MAX));
            last_request = Some(now);
            if let Some(slot) = stats_slot {
                lock(&self.stats).record_request(slot, &request, interval_ms);
            }
            let outcome = process(&lock(&self.cfg), &mut lock(&self.regs), &request);
            if let (Some(slot), Outcome::Reply(reply)) = (stats_slot, &outcome) {
                lock(&self.stats).record_reply(slot, reply);
            }
            if let Some(log) = &self.log {
                match &outcome {
                    Outcome::Reply(reply) => log.record(conn, &request, Response::Reply(reply)),
                    Outcome::Drop => {
                        let why = match protocol::frame(&request) {
                            Err(e) => e.to_string(),
                            Ok(_) => "request dropped: unit id mismatch".to_owned(),
                        };
                        log.record(conn, &request, Response::Error(&why));
                    }
                }
            }
            if let Outcome::Reply(reply) = outcome
                && stream.write_all(&reply).is_err()
            {
                return;
            }
            (self.notify)(Event::Activity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ananasar_core::protocol;
    use std::io::Cursor;

    #[test]
    fn reads_a_whole_frame() {
        let f = protocol::build(7, 1, 0x03, &[0, 1, 0, 2]);
        let mut input = f.clone();
        input.extend_from_slice(&[0xAA; 4]); // next frame's bytes stay unread
        assert_eq!(read_frame(&mut Cursor::new(input)).unwrap(), f);
    }

    #[test]
    fn rejects_bad_or_truncated_frames() {
        let f = protocol::build(7, 1, 0x03, &[0, 1, 0, 2]);
        assert!(read_frame(&mut Cursor::new(&f[..f.len() - 1])).is_err());
        let mut tiny = f.clone();
        tiny[4..6].copy_from_slice(&[0, 0]); // length below the minimum
        assert!(read_frame(&mut Cursor::new(tiny)).is_err());
        let mut huge = f;
        huge[4..6].copy_from_slice(&[0x10, 0x00]);
        assert!(read_frame_max(&mut Cursor::new(huge), MAX_REQUEST).is_err());
    }
}
