//! Scenarios for the non-default profiles: functions switched off, a forced
//! exception, and the deliberate-fault options. Every profile gets the same
//! probe set (so differences between profiles show what an option changes);
//! the address offset profile adds boundary reads and writes.
//!
//! The probes touch only registers 3000..3010, which no default scenario uses.

use super::{Builder, PROFILES, Scenario};

/// The profiles this file covers: everything but the default.
fn option_profiles() -> impl Iterator<Item = &'static str> {
    PROFILES.iter().map(|p| p.name).filter(|n| *n != super::DEFAULT_PROFILE)
}

pub(super) fn scenarios(unit: u8) -> Vec<Scenario> {
    let mut list = Vec::new();
    for profile in option_profiles() {
        let b = |name, about| Builder::new(name, about, unit).profile(profile);
        list.push(
            b("probe_every_function", "One valid request per supported function code, then an unknown one.")
                .write_multiple(3000, &[0x0102, 0x0304])
                .read(3, 3000, 2)
                .read(4, 3000, 2)
                .pdu(6, &[0x0B, 0xBA, 0x12, 0x34])
                .pdu(7, &[])
                .pdu(8, &[0, 0, 0xA5, 0x37])
                .pdu(0x16, &[0x0B, 0xB8, 0, 0xF0, 0, 0x0F])
                .pdu(0x17, &[0x0B, 0xB8, 0, 2, 0x0B, 0xBA, 0, 1, 2, 0xAB, 0xCD])
                .pdu(0x2B, &[0x0E, 1, 0])
                .done(),
        );
        list.push(
            b("probe_error_paths", "Requests the default server answers with exceptions: bad quantity and bad address.")
                .read(3, 3000, 0)
                .read(3, 3000, 126)
                .read(3, 65535, 2)
                .pdu(0x10, &[0x0B, 0xB8, 0, 2, 3, 0, 1, 0, 2])
                .done(),
        );
        list.push(
            b("probe_bad_header_then_valid", "A frame with protocol id 1, then a valid request.")
                .raw(vec![0, 1, 0, 1, 0, 6, unit, 3, 0x0B, 0xB8, 0, 1])
                .read(3, 3000, 1)
                .done(),
        );
        list.push(
            b("probe_pipelined", "Two requests in one TCP segment.")
                .raw(
                    [
                        ananasar_core::protocol::build(1, unit, 3, &[0x0B, 0xB8, 0, 1]),
                        ananasar_core::protocol::build(2, unit, 3, &[0x0B, 0xB9, 0, 1]),
                    ]
                    .concat(),
                )
                .done(),
        );
    }
    // Use Address Offset: the effect on the first and last registers, per function.
    let b = |name, about| Builder::new(name, about, unit).profile("address-offset");
    list.push(
        b("offset_edges", "Reads and writes at address 0, 1 and 65535 to show how the offset moves them.")
            .pdu(6, &[0, 0, 0x11, 0x11])
            .pdu(6, &[0, 1, 0x22, 0x22])
            .read(3, 0, 1)
            .read(3, 1, 1)
            .read(3, 0, 3)
            .read(4, 0, 1)
            .read(3, 65535, 1)
            .pdu(6, &[0xFF, 0xFF, 0x33, 0x33])
            .read(3, 65534, 2)
            .done(),
    );
    list
}
