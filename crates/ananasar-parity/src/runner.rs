//! Run the scenarios against a server and record what it sends back.

use crate::recording::{Entry, Recording, Section, split_frames};
use crate::scenarios::{self, Scenario, Step};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Settle time after a reply starts arriving, to catch further frames.
const SETTLE: Duration = Duration::from_millis(25);

/// Run every scenario against `addr` and return the recording.
///
/// Each scenario gets its own connection. `timeout` is how long to wait for a
/// reply before recording [`Entry::NoReply`].
pub fn run(addr: impl ToSocketAddrs + Copy, unit: u8, timeout: Duration) -> io::Result<Recording> {
    run_profile(addr, scenarios::DEFAULT_PROFILE, &[], unit, timeout)
}

/// Like [`run`] for the scenarios of one profile (see [`scenarios::PROFILES`]).
/// A non-empty `only` keeps just the scenarios with those names.
pub fn run_profile(
    addr: impl ToSocketAddrs + Copy,
    profile: &str,
    only: &[String],
    unit: u8,
    timeout: Duration,
) -> io::Result<Recording> {
    let mut rec = Recording::default();
    for scenario in scenarios::for_profile(profile, unit).into_iter().filter(|s| only.is_empty() || only.iter().any(|n| n == s.name)) {
        rec.sections.push(run_scenario(addr, &scenario, timeout)?);
    }
    Ok(rec)
}

/// Pause between the TCP segments of a split step.
const SEGMENT_GAP: Duration = Duration::from_millis(40);

/// Write a step, cut into segments where it asks.
fn send(stream: &mut TcpStream, step: &Step) -> io::Result<()> {
    let mut start = 0;
    for &cut in step.split_at.iter().chain(std::iter::once(&step.bytes.len())) {
        let cut = cut.min(step.bytes.len());
        if cut > start {
            stream.write_all(&step.bytes[start..cut])?;
            if cut < step.bytes.len() {
                std::thread::sleep(SEGMENT_GAP);
            }
            start = cut;
        }
    }
    Ok(())
}

fn run_scenario(addr: impl ToSocketAddrs, scenario: &Scenario, timeout: Duration) -> io::Result<Section> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    let mut entries = Vec::new();
    for step in &scenario.steps {
        entries.push(Entry::Sent(step.bytes.clone()));
        if send(&mut stream, step).is_err() {
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
