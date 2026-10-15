//! A logging TCP proxy: sits between any client and any server and records the
//! bytes in both directions in the shared recording format.
//!
//! Each connection becomes one section, written when the connection ends.
//! Bytes are recorded as they arrive and split into frames by the MBAP length
//! field, so a frame that TCP delivers in two pieces is still one entry.

use crate::recording::{Entry, Section, split_frames};
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Tagged chunks from both directions in arrival order (`true` = client to server).
type Chunks = Arc<Mutex<Vec<(bool, Vec<u8>)>>>;

/// Accept connections on `listener` and forward each to `target`, appending
/// one section per finished connection to the file at `out`. Runs forever.
pub fn serve(listener: &TcpListener, target: impl ToSocketAddrs + Clone + Send + 'static, out: &Path) -> io::Result<()> {
    let file = Arc::new(Mutex::new(File::create(out)?));
    for (n, client) in listener.incoming().enumerate() {
        let Ok(client) = client else { continue };
        let target = target.clone();
        let file = file.clone();
        thread::spawn(move || {
            let section = proxy_connection(client, target, &format!("conn {}", n + 1));
            if let Ok(section) = section {
                let mut f = file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut text = format!("## {}\n", section.name);
                for e in &section.entries {
                    text.push_str(&e.to_line());
                    text.push('\n');
                }
                text.push('\n');
                let _ = f.write_all(text.as_bytes()).and_then(|()| f.flush());
            }
        });
    }
    Ok(())
}

/// Forward one client connection to the target until either side closes and
/// return what crossed the wire.
pub fn proxy_connection(client: TcpStream, target: impl ToSocketAddrs, name: &str) -> io::Result<Section> {
    let server = TcpStream::connect(target)?;
    client.set_nodelay(true)?;
    server.set_nodelay(true)?;
    // Chunks from both directions, tagged, in arrival order.
    let log: Chunks = Arc::default();
    // Which side ended the connection first: 1 = client, 2 = server.
    let first: Arc<AtomicU8> = Arc::default();
    let up = pump(client.try_clone()?, server.try_clone()?, true, log.clone(), first.clone());
    let down = pump(server, client, false, log.clone(), first.clone());
    let _ = up.join();
    let _ = down.join();
    let server_closed_first = first.load(Ordering::SeqCst) == 2;

    let chunks = std::mem::take(&mut *log.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    let mut entries = Vec::new();
    // Merge consecutive chunks of one direction before framing.
    let mut i = 0;
    while i < chunks.len() {
        let dir = chunks[i].0;
        let mut bytes = Vec::new();
        while i < chunks.len() && chunks[i].0 == dir {
            bytes.extend_from_slice(&chunks[i].1);
            i += 1;
        }
        for frame in split_frames(&bytes) {
            entries.push(if dir { Entry::Sent(frame) } else { Entry::Received(frame) });
        }
    }
    if server_closed_first {
        entries.push(Entry::Closed);
    }
    Ok(Section { name: name.to_owned(), entries })
}

/// Copy `from` to `to`, logging each chunk. `upstream` marks client-to-server.
/// Whichever direction ends first records itself in `first`.
fn pump(
    mut from: TcpStream,
    mut to: TcpStream,
    upstream: bool,
    log: Chunks,
    first: Arc<AtomicU8>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = [0u8; 2048];
        loop {
            match from.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    log.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((upstream, buf[..n].to_vec()));
                    if to.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            }
        }
        let _ = first.compare_exchange(0, if upstream { 1 } else { 2 }, Ordering::SeqCst, Ordering::SeqCst);
        let _ = to.shutdown(Shutdown::Both);
        let _ = from.shutdown(Shutdown::Both);
    })
}
