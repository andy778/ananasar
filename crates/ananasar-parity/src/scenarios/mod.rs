//! The scenario catalog. Each scenario is a list of byte blocks to write to
//! one fresh connection; the runner records whatever comes back.
//!
//! The catalog is data: one file per area, each returning a list of
//! [`Scenario`] values built with [`Builder`]. A scenario belongs to a
//! [`PROFILES`] entry, which names the server options it was designed for
//! (the default options, one function code switched off, a forced exception,
//! a deliberate-fault option). Run one profile's scenarios only against a
//! server set up that way, with all registers zero.
//!
//! Within a profile the scenarios share one server, so register state carries
//! from one to the next in catalog order; a run is only repeatable on a
//! freshly started (or freshly zeroed) server. Scenarios that leave the
//! connection out of sync (bad lengths) have a single step.
//!
//! Not in the catalog on purpose: quantities of 253 or more registers and
//! other requests the original does not bound. They are unsafe to send to it.

use ananasar_core::protocol::build;

mod diagnostics;
mod exceptions;
mod faults;
mod malformed;
mod read;
mod readwrite;
mod write;

/// The default profile: every function code enabled, no fault option, unit id
/// filter off (accepts any).
pub const DEFAULT_PROFILE: &str = "default";

/// One block of bytes written to the connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The bytes to send (may hold several frames).
    pub bytes: Vec<u8>,
    /// Offsets at which the write is cut into separate TCP segments, with a
    /// short pause between them. Empty means one write.
    pub split_at: Vec<usize>,
}

/// A named list of blocks written one after another on one connection.
pub struct Scenario {
    /// Short identifier used as the recording section name.
    pub name: &'static str,
    /// The server options this scenario is meant for, see [`PROFILES`].
    pub profile: &'static str,
    /// One line on what the scenario checks.
    pub about: &'static str,
    /// The scenario may disturb register state unpredictably: a write past
    /// the end of the holding bank (the original spills it into the input
    /// bank) or a write PDU too short to hold its fields (the original fills
    /// the gap from earlier traffic). These run last so the state for the
    /// other scenarios stays clean.
    pub pollutes: bool,
    /// Blocks, written in order; the reply is collected after each.
    pub steps: Vec<Step>,
}

/// A server setup the scenarios are grouped by.
pub struct ProfileInfo {
    /// Identifier used on the command line and as the golden file name.
    pub name: &'static str,
    /// What to set on a server before running the profile.
    pub setup: &'static str,
}

/// Every profile, in run order.
pub const PROFILES: &[ProfileInfo] = &[
    ProfileInfo { name: "default", setup: "all function codes on, no fault option, no forced exception" },
    ProfileInfo { name: "disable-fc3", setup: "Options > Modbus > Active MB Functions: only FC3 off" },
    ProfileInfo { name: "disable-fc4", setup: "only FC4 off" },
    ProfileInfo { name: "disable-fc6", setup: "only FC6 off" },
    ProfileInfo { name: "disable-fc7", setup: "only FC7 off" },
    ProfileInfo { name: "disable-fc8", setup: "only FC8 off" },
    ProfileInfo { name: "disable-fc16", setup: "only FC16 off" },
    ProfileInfo { name: "disable-fc22", setup: "only FC22 off" },
    ProfileInfo { name: "disable-fc23", setup: "only FC23 off" },
    ProfileInfo { name: "exception-01", setup: "Return Exception: Illegal Function" },
    ProfileInfo { name: "exception-02", setup: "Return Exception: Illegal Data Address" },
    ProfileInfo { name: "exception-03", setup: "Return Exception: Illegal Data Value" },
    ProfileInfo { name: "exception-04", setup: "Return Exception: Slave Device Failure" },
    ProfileInfo { name: "exception-05", setup: "Return Exception: Acknowledge" },
    ProfileInfo { name: "exception-06", setup: "Return Exception: Slave Device Busy" },
    ProfileInfo { name: "exception-07", setup: "Return Exception: Negative Acknowledge" },
    ProfileInfo { name: "exception-08", setup: "Return Exception: Memory Parity Error" },
    ProfileInfo { name: "exception-0a", setup: "Return Exception: Gateway Path Unavailable" },
    ProfileInfo { name: "exception-0b", setup: "Return Exception: Gateway Target Device Failed to Respond" },
    ProfileInfo { name: "fault-ti", setup: "Return Incorrect TI on" },
    ProfileInfo { name: "fault-protocol-id", setup: "Return Incorrect Protocol ID on" },
    ProfileInfo { name: "fault-length-high", setup: "Return Incorrect Length High Byte on" },
    ProfileInfo { name: "fault-out-of-synch", setup: "Reply Out of Synch on" },
    ProfileInfo { name: "address-offset", setup: "Use Address Offset on" },
];

/// Incremental scenario construction. Transaction ids count up from 1 per
/// scenario.
pub(crate) struct Builder {
    name: &'static str,
    profile: &'static str,
    about: &'static str,
    pollutes: bool,
    unit: u8,
    txn: u16,
    steps: Vec<Step>,
}

impl Builder {
    pub(crate) fn new(name: &'static str, about: &'static str, unit: u8) -> Self {
        Self { name, profile: DEFAULT_PROFILE, about, pollutes: false, unit, txn: 0, steps: Vec::new() }
    }

    /// Mark the scenario as one that can write past the end of the bank.
    pub(crate) fn pollutes(mut self) -> Self {
        self.pollutes = true;
        self
    }

    pub(crate) fn profile(mut self, profile: &'static str) -> Self {
        self.profile = profile;
        self
    }

    fn push(mut self, bytes: Vec<u8>, split_at: &[usize]) -> Self {
        self.steps.push(Step { bytes, split_at: split_at.to_vec() });
        self
    }

    pub(crate) fn pdu(mut self, function: u8, data: &[u8]) -> Self {
        self.txn += 1;
        let bytes = build(self.txn, self.unit, function, data);
        self.push(bytes, &[])
    }

    pub(crate) fn read(self, function: u8, addr: u16, count: u16) -> Self {
        self.pdu(function, &[addr.to_be_bytes(), count.to_be_bytes()].concat())
    }

    pub(crate) fn write_multiple(self, addr: u16, values: &[u16]) -> Self {
        let mut d = Vec::new();
        d.extend_from_slice(&addr.to_be_bytes());
        d.extend_from_slice(&(values.len() as u16).to_be_bytes());
        d.push((values.len() * 2) as u8);
        for v in values {
            d.extend_from_slice(&v.to_be_bytes());
        }
        self.pdu(0x10, &d)
    }

    /// Bytes exactly as given, no framing.
    pub(crate) fn raw(self, bytes: Vec<u8>) -> Self {
        self.push(bytes, &[])
    }

    /// Bytes written as several TCP segments cut at `split_at`.
    pub(crate) fn raw_split(self, bytes: Vec<u8>, split_at: &[usize]) -> Self {
        self.push(bytes, split_at)
    }

    /// A framed request for a different unit id.
    pub(crate) fn pdu_for_unit(mut self, unit: u8, function: u8, data: &[u8]) -> Self {
        self.txn += 1;
        let bytes = build(self.txn, unit, function, data);
        self.push(bytes, &[])
    }

    pub(crate) fn done(self) -> Scenario {
        Scenario { name: self.name, profile: self.profile, about: self.about, pollutes: self.pollutes, steps: self.steps }
    }
}

pub(crate) fn ramp(n: u16) -> Vec<u16> {
    (0..n).map(|i| 0x1000 + i).collect()
}

fn everything(unit: u8) -> Vec<Scenario> {
    let mut list = Vec::new();
    list.extend(read::scenarios(unit));
    list.extend(write::scenarios(unit));
    list.extend(readwrite::scenarios(unit));
    list.extend(diagnostics::scenarios(unit));
    list.extend(exceptions::scenarios(unit));
    list.extend(malformed::scenarios(unit));
    list.extend(faults::scenarios(unit));
    list.sort_by_key(|s| s.pollutes);
    list
}

/// The scenarios of the default profile, for requests addressed to `unit`.
pub fn all(unit: u8) -> Vec<Scenario> {
    for_profile(DEFAULT_PROFILE, unit)
}

/// The scenarios of `profile` (empty for an unknown name).
pub fn for_profile(profile: &str, unit: u8) -> Vec<Scenario> {
    let mut list = everything(unit);
    list.retain(|s| s.profile == profile);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_per_profile_and_steps_non_empty() {
        for p in PROFILES {
            let list = for_profile(p.name, 1);
            assert!(!list.is_empty(), "profile {} has no scenarios", p.name);
            let mut names: Vec<_> = list.iter().map(|s| s.name).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), list.len(), "duplicate names in {}", p.name);
            assert!(list.iter().all(|s| !s.steps.is_empty() && !s.about.is_empty()));
        }
    }

    #[test]
    fn every_scenario_belongs_to_a_listed_profile() {
        let total: usize = PROFILES.iter().map(|p| for_profile(p.name, 1).len()).sum();
        assert_eq!(total, everything(1).len());
    }
}
