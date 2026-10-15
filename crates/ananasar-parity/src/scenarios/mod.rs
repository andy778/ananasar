//! The fixed scenario list. Each scenario is a list of byte blocks to write
//! to one fresh connection; the runner records whatever comes back.
//!
//! Scenarios assume a server whose registers are all zero when the run starts
//! and that is not reused for a second run (the first scenario seeds registers
//! 0..125 and a few later ones write elsewhere). Scenarios that leave the
//! connection out of sync (bad lengths) have a single step.

use ananasar_core::protocol::build;

/// A named list of blocks written one after another on one connection.
pub struct Scenario {
    /// Short identifier used as the recording section name.
    pub name: &'static str,
    /// Byte blocks; a block may hold several frames (pipelining).
    pub steps: Vec<Vec<u8>>,
}

struct Builder {
    name: &'static str,
    unit: u8,
    txn: u16,
    steps: Vec<Vec<u8>>,
}

impl Builder {
    fn new(name: &'static str, unit: u8) -> Self {
        Self { name, unit, txn: 0, steps: Vec::new() }
    }

    fn pdu(mut self, function: u8, data: &[u8]) -> Self {
        self.txn += 1;
        self.steps.push(build(self.txn, self.unit, function, data));
        self
    }

    fn read(self, function: u8, addr: u16, count: u16) -> Self {
        self.pdu(function, &[addr.to_be_bytes(), count.to_be_bytes()].concat())
    }

    fn write_multiple(self, addr: u16, values: &[u16]) -> Self {
        let mut d = Vec::new();
        d.extend_from_slice(&addr.to_be_bytes());
        d.extend_from_slice(&(values.len() as u16).to_be_bytes());
        d.push((values.len() * 2) as u8);
        for v in values {
            d.extend_from_slice(&v.to_be_bytes());
        }
        self.pdu(0x10, &d)
    }

    fn raw(mut self, bytes: Vec<u8>) -> Self {
        self.steps.push(bytes);
        self
    }

    fn done(self) -> Scenario {
        Scenario { name: self.name, steps: self.steps }
    }
}

fn ramp(n: u16) -> Vec<u16> {
    (0..n).map(|i| 0x1000 + i).collect()
}

/// All scenarios for requests addressed to `unit`.
pub fn all(unit: u8) -> Vec<Scenario> {
    let b = |name| Builder::new(name, unit);
    // A header with explicit protocol id and length, for malformed frames.
    let header = |protocol: u16, length: u16, rest: &[u8]| {
        let mut v = vec![0, 1];
        v.extend_from_slice(&protocol.to_be_bytes());
        v.extend_from_slice(&length.to_be_bytes());
        v.push(unit);
        v.extend_from_slice(rest);
        v
    };
    let read_one = [3u8, 0, 0, 0, 1];

    vec![
        b("seed").write_multiple(0, &ramp(100)).write_multiple(100, &ramp(25)).done(),
        b("read_holding").read(3, 0, 1).read(3, 0, 10).read(3, 120, 5).done(),
        b("read_input").read(4, 0, 1).read(4, 0, 10).done(),
        b("read_holding_limit").read(3, 0, 125).done(),
        b("read_holding_over_limit").read(3, 0, 126).done(),
        b("read_input_limit").read(4, 0, 125).done(),
        b("read_input_over_limit").read(4, 0, 126).done(),
        b("read_zero_count").read(3, 0, 0).read(4, 0, 0).done(),
        b("read_address_edge")
            .read(3, 65411, 125)
            .read(3, 65412, 125)
            .read(3, 65535, 1)
            .read(3, 65535, 2)
            .read(4, 65411, 125)
            .read(4, 65412, 125)
            .done(),
        b("write_single").pdu(6, &[0, 5, 0x12, 0x34]).read(3, 5, 1).pdu(6, &[0, 6, 0, 0]).read(3, 6, 1).done(),
        b("write_single_short_pdu").pdu(6, &[0, 5]).done(),
        b("write_single_address_edge").pdu(6, &[0xFF, 0xFF, 0xAB, 0xCD]).read(3, 65535, 1).done(),
        b("read_exception_status").pdu(7, &[]).done(),
        b("write_multiple").write_multiple(10, &[0x0A0B, 0x0C0D, 0x0E0F]).read(3, 10, 3).done(),
        b("write_multiple_limit").write_multiple(0, &ramp(123)).read(3, 0, 123).done(),
        b("write_multiple_over_limit").write_multiple(0, &ramp(124)).done(),
        b("write_multiple_zero_count").pdu(0x10, &[0, 0, 0, 0, 0]).done(),
        b("write_multiple_byte_count_mismatch").pdu(0x10, &[0, 0, 0, 2, 3, 0, 1, 0, 2]).done(),
        b("write_multiple_truncated_data").pdu(0x10, &[0, 0, 0, 2, 4, 0, 1]).done(),
        b("write_multiple_address_edge")
            .write_multiple(65535, &[1])
            .write_multiple(65535, &[1, 2])
            .write_multiple(65534, &[1, 2])
            .done(),
        // Modbus spec example: register 0x0012, masks 0x00F2 / 0x0025, result 0x0017.
        b("mask_write").pdu(6, &[0, 200, 0, 0x12]).pdu(0x16, &[0, 200, 0, 0xF2, 0, 0x25]).read(3, 200, 1).done(),
        b("mask_write_short_pdu").pdu(0x16, &[0, 200, 0, 0xF2]).done(),
        b("read_write_multiple")
            .pdu(0x17, &[0, 0, 0, 4, 1, 0x2C, 0, 2, 4, 0xAA, 0xAA, 0xBB, 0xBB])
            .read(3, 300, 2)
            .done(),
        b("read_write_multiple_limits")
            .pdu(0x17, &[&[0, 0, 0, 125, 0, 0, 0, 121, 242][..], &[0x55; 242]].concat())
            .pdu(0x17, &[0, 0, 0, 126, 0, 0, 0, 1, 2, 0, 1])
            .pdu(0x17, &[&[0, 0, 0, 1, 0, 0, 0, 122, 244][..], &[0x66; 244]].concat())
            .done(),
        b("diagnostics").pdu(8, &[0, 0, 0xA5, 0x37]).done(),
        b("unknown_function")
            .pdu(0x00, &[])
            .pdu(0x01, &[0, 0, 0, 1])
            .pdu(0x2B, &[0x0E, 1, 0])
            .pdu(0x63, &[1, 2, 3])
            .done(),
        b("unit_zero").raw(build(1, 0, 3, &read_one[1..])).done(),
        b("unit_one").raw(build(1, 1, 3, &read_one[1..])).done(),
        b("unit_255").raw(build(1, 255, 3, &read_one[1..])).done(),
        b("bad_protocol_id").raw(header(1, 6, &read_one)).raw(header(0, 6, &read_one)).done(),
        b("bad_length_high_byte").raw(header(0, 0x0106, &read_one)).done(),
        b("bad_length_too_long").raw(header(0, 16, &read_one)).done(),
        b("bad_length_too_short").raw(header(0, 5, &read_one)).done(),
        b("length_zero").raw(header(0, 0, &read_one)).done(),
        b("header_only").raw(vec![0, 1, 0, 0, 0, 1, unit]).done(),
        b("pipelined")
            .raw([build(1, unit, 3, &[0, 0, 0, 1]), build(2, unit, 4, &[0, 0, 0, 1])].concat())
            .done(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_steps_non_empty() {
        let list = all(1);
        let mut names: Vec<_> = list.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), list.len());
        assert!(list.iter().all(|s| !s.steps.is_empty()));
    }
}
