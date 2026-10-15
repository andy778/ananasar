//! Run the scenarios against a server and record what it sends back.

use crate::recording::{Entry, Recording, Section, split_frames};
use crate::scenarios::{self, Scenario};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Settle time after a reply starts arriving, to catch further frames.
const SETTLE: Duration = Duration::from_millis(50);

/// Run every scenario against `addr` and return the recording.
///
/// Each scenario gets its own connection. `timeout` is how long to wait for a
/// reply before recording [`Entry::NoReply`].
pub fn run(addr: impl ToSocketAddrs + Copy, unit: u8, timeout: Duration) -> io::Result<Recording> {
    let mut rec = Recording::default();
    for scenario in scenarios::all(unit) {
        rec.sections.push(run_scenario(addr, &scenario, timeout)?);
    }
    Ok(rec)
}

fn run_scenario(addr: impl ToSocketAddrs, scenario: &Scenario, timeout: Duration) -> io::Result<Section> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    let mut entries = Vec::new();
    for step in &scenario.steps {
        entries.push(Entry::Sent(step.clone()));
        if stream.write_all(step).is_err() {
            entries.push(Entry::Closed);
            break;
        }
        let replies = collect_reply(&mut stream, timeout)?;
        let closed = replies.last() == Some(&Entry::Closed);
        entries.extend(replies);
        if closed {
            break;
        }
    }
    Ok(Section { name: scenario.name.to_owned(), entries })
}

/// Wait for the first bytes, then keep reading until the line goes quiet.
fn collect_reply(stream: &mut TcpStream, timeout: Duration) -> io::Result<Vec<Entry>> {
    let mut data = Vec::new();
    let mut closed = false;
    let mut wait = timeout;
    loop {
        stream.set_read_timeout(Some(wait))?;
        let mut buf = [0u8; 1024];
        match stream.read(&mut buf) {
            Ok(0) => {
                closed = true;
                break;
            }
            Ok(n) => {
                data.extend_from_slice(&buf[..n]);
                wait = SETTLE;
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => break,
            Err(e) if matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted) => {
                closed = true;
                break;
            }
            Err(e) => return Err(e),
        }
    }
    let mut entries: Vec<Entry> = split_frames(&data).into_iter().map(Entry::Received).collect();
    if closed {
        entries.push(Entry::Closed);
    } else if entries.is_empty() {
        entries.push(Entry::NoReply);
    }
    Ok(entries)
}
