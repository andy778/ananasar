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
    for scenario in scenarios::for_profile(profile, unit)
        .into_iter()
        .filter(|s| only.is_empty() || only.iter().any(|n| n == s.name))
    {
        rec.sections.push(run_scenario(addr, &scenario, timeout)?);
    }
    Ok(rec)
}

/// Run `profile` again on a fresh server at `addr` and return the sections
/// called `names`, in catalog order.
///
/// Scenarios depend on the registers earlier ones left behind (the first one
/// seeds them), so a section can only be reproduced by replaying everything
/// before it. The scenarios in `names` wait `timeout` for each reply, the rest
/// only `replay_timeout`: a reply that is really missing is still missing after
/// a long wait, but one that only arrived late on a slow machine is then
/// recorded. The caller must pass a server whose registers are still zero.
pub fn rerun_sections(
    addr: impl ToSocketAddrs + Copy,
    profile: &str,
    names: &[String],
    unit: u8,
    replay_timeout: Duration,
    timeout: Duration,
) -> io::Result<Vec<Section>> {
    let mut out = Vec::new();
    for scenario in scenarios::for_profile(profile, unit) {
        if names.iter().any(|n| n == scenario.name) {
            out.push(run_scenario(addr, &scenario, timeout)?);
        } else {
            run_scenario(addr, &scenario, replay_timeout)?;
        }
    }
    Ok(out)
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
        if let Err(e) = stream.set_read_timeout(Some(wait)) {
            // macOS refuses setsockopt (EINVAL) on a socket the peer has reset, where
            // Linux reports the reset on the read below. std's own InvalidInput (a zero
            // timeout) has no OS error code and stays an error.
            if e.kind() == io::ErrorKind::InvalidInput && e.raw_os_error().is_some() {
                closed = true;
                break;
            }
            return Err(e);
        }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "test code")]
    use super::*;
    use std::net::TcpListener;

    /// A server that answers every request after 400 ms with a fixed frame.
    fn slow_server() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                std::thread::spawn(move || {
                    let mut buf = [0u8; 256];
                    while matches!(stream.read(&mut buf), Ok(n) if n > 0) {
                        std::thread::sleep(Duration::from_millis(400));
                        if stream.write_all(&[0, 1, 0, 0, 0, 3, 1, 0x83, 1]).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        addr
    }

    #[test]
    fn a_long_timeout_records_a_late_reply_a_short_one_misses() {
        let addr = slow_server();
        let name = scenarios::for_profile(scenarios::DEFAULT_PROFILE, 1)[0].name.to_owned();
        let names = [name];
        let short = rerun_sections(
            addr,
            scenarios::DEFAULT_PROFILE,
            &names,
            1,
            Duration::from_millis(1),
            Duration::from_millis(250),
        )
        .unwrap();
        assert_eq!(short[0].entries[1], Entry::NoReply);
        let long = rerun_sections(
            addr,
            scenarios::DEFAULT_PROFILE,
            &names,
            1,
            Duration::from_millis(1),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(matches!(long[0].entries[1], Entry::Received(_)));
    }

    #[test]
    fn unknown_scenario_names_give_nothing() {
        let addr = slow_server();
        let names = ["nope".to_owned()];
        let none = rerun_sections(
            addr,
            scenarios::DEFAULT_PROFILE,
            &names,
            1,
            Duration::from_millis(1),
            Duration::from_millis(10),
        );
        assert!(none.unwrap().is_empty());
    }

    /// Regression: a section that reads registers an earlier scenario wrote must
    /// come out the same when re-run, not as if the server had just started.
    #[test]
    fn a_rerun_section_sees_the_registers_earlier_scenarios_wrote() {
        let name = "split_two_requests_mid_frame".to_owned();
        let first = crate::ours::start(Default::default()).unwrap();
        let full = run(first.addr, 1, Duration::from_millis(500)).unwrap();
        let expected = full.sections.iter().find(|s| s.name == name).unwrap();
        let second = crate::ours::start(Default::default()).unwrap();
        let again = rerun_sections(
            second.addr,
            scenarios::DEFAULT_PROFILE,
            std::slice::from_ref(&name),
            1,
            Duration::from_millis(500),
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(&again[0], expected);
    }
}
