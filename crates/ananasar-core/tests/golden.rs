//! Replays every recording in `tests/golden/` against the engine.
//!
//! `spec.rec` holds frames from the Modbus specification. Recordings made
//! with the original Ananas (see `docs/parity-testing.md`) are dropped into
//! the same folder and checked the same way, so CI needs no executable.
//!
//! Each file is one session: the register file starts all zero and keeps its
//! state across the file's sections.

use ananasar_core::registers::RegisterFile;
use ananasar_core::server::{Outcome, ServerConfig, process};
use std::path::Path;

#[path = "../../ananasar-parity/src/recording.rs"]
#[allow(dead_code)]
mod recording;

use recording::{Entry, Recording};

/// Feed one recorded block (possibly several pipelined frames) to the engine
/// and return the reply frames it would send.
fn replies_for(cfg: &ServerConfig, regs: &mut RegisterFile, block: &[u8]) -> Vec<Vec<u8>> {
    recording::split_frames(block)
        .iter()
        .filter_map(|frame| match process(cfg, regs, frame) {
            Outcome::Reply(r) => Some(r),
            Outcome::Drop => None,
        })
        .collect()
}

fn replay(name: &str, rec: &Recording) {
    let cfg = ServerConfig::default();
    let mut regs = RegisterFile::new();
    for section in &rec.sections {
        let mut i = 0;
        while i < section.entries.len() {
            let Entry::Sent(request) = &section.entries[i] else {
                panic!("{name} [{}]: entry {} is a reply without a request", section.name, i + 1);
            };
            let mut expected = Vec::new();
            i += 1;
            while let Some(e) = section.entries.get(i) {
                match e {
                    Entry::Sent(_) => break,
                    Entry::Received(r) => expected.push(r.clone()),
                    Entry::NoReply | Entry::Closed => {}
                }
                i += 1;
            }
            let actual = replies_for(&cfg, &mut regs, request);
            assert_eq!(
                actual.iter().map(|b| recording::hex(b)).collect::<Vec<_>>(),
                expected.iter().map(|b| recording::hex(b)).collect::<Vec<_>>(),
                "{name} [{}]: request {}",
                section.name,
                recording::hex(request),
            );
        }
    }
}

#[test]
fn golden_recordings_replay() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rec") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        let rec = Recording::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        replay(&name, &rec);
        checked += 1;
    }
    assert!(checked > 0, "no recordings in {}", dir.display());
}
