//! `ananas-core` — the Modbus/TCP protocol engine for the Ananas reimplementation.
//!
//! A from-scratch reimplementation of the Ananas Modbus/TCP tool. The crate
//! is `std`-only and platform-agnostic so the same core drives the Linux and
//! Windows builds; OS-specific features (DDE, adapter IP config) live in the
//! binary crates, not here.

#![warn(missing_docs)]

pub mod ananas_file;
pub mod client;
pub mod protocol;
pub mod registers;
pub mod scanner;
pub mod server;
pub mod sync;

#[cfg(test)]
mod tests {
    use crate::client::{AddressBase, ManualCommand, PollEngine, PollList};
    use crate::protocol::{frame, Function};
    use crate::registers::{Bank, InitMode, RegisterFile};
    use crate::server::{process, Outcome, ServerConfig};

    #[test]
    fn frame_roundtrip_and_header_checks() {
        let f = crate::protocol::build(0x1234, 1, 0x03, &[0, 10, 0, 2]);
        // length = unit id + function + 4 data = 6
        assert_eq!(&f[4..6], &[0x00, 0x06]);
        let (hdr, func, pdu) = frame(&f).unwrap();
        assert_eq!(hdr.transaction_id, 0x1234);
        assert_eq!(func, 0x03);
        assert_eq!(pdu, &[0, 10, 0, 2]);
        // Non-zero protocol id is rejected.
        let mut bad = f.clone();
        bad[2] = 1;
        assert!(frame(&bad).is_err());
    }

    #[test]
    fn read_holding_returns_values() {
        let mut regs = RegisterFile::new();
        regs.initialize(Bank::Holding, InitMode::Ascending);
        let cfg = ServerConfig::default();
        let req = crate::protocol::build(1, 1, 0x03, &[0x00, 0x05, 0x00, 0x03]); // read 3 from addr 5
        match process(&cfg, &mut regs, &req) {
            Outcome::Reply(r) => {
                let (_, func, pdu) = frame(&r).unwrap();
                assert_eq!(func, 0x03);
                assert_eq!(pdu[0], 6); // byte count
                assert_eq!(&pdu[1..], &[0, 5, 0, 6, 0, 7]); // regs 5,6,7
            }
            other => panic!("expected reply, got {other:?}"),
        }
    }

    #[test]
    fn write_single_then_read_back() {
        let mut regs = RegisterFile::new();
        let cfg = ServerConfig::default();
        let w = crate::protocol::build(1, 1, 0x06, &[0x00, 0x2A, 0xBE, 0xEF]);
        assert!(matches!(process(&cfg, &mut regs, &w), Outcome::Reply(_)));
        assert_eq!(regs.get(Bank::Holding, 42), 0xBEEF);
    }

    #[test]
    fn unsupported_function_yields_exception() {
        let mut regs = RegisterFile::new();
        let cfg = ServerConfig::default();
        let req = crate::protocol::build(1, 1, 0x01, &[0, 0, 0, 1]); // FC1 not supported
        match process(&cfg, &mut regs, &req) {
            Outcome::Reply(r) => {
                let (_, func, pdu) = frame(&r).unwrap();
                assert_eq!(func, 0x81); // 0x01 | 0x80
                assert_eq!(pdu[0], 0x01); // IllegalFunction
            }
            other => panic!("expected exception, got {other:?}"),
        }
    }

    #[test]
    fn over_limit_quantity_is_rejected_not_overflowed() {
        // The original overflowed its 513-byte reply buffer here; we reject it.
        let mut regs = RegisterFile::new();
        let cfg = ServerConfig::default();
        let req = crate::protocol::build(1, 1, 0x03, &[0x00, 0x00, 0x00, 0xFF]); // 255 regs
        match process(&cfg, &mut regs, &req) {
            Outcome::Reply(r) => {
                let (_, func, pdu) = frame(&r).unwrap();
                assert_eq!(func, 0x83);
                assert_eq!(pdu[0], 0x03); // IllegalDataValue
            }
            other => panic!("expected exception, got {other:?}"),
        }
    }

    #[test]
    fn uid_mismatch_is_dropped() {
        let mut regs = RegisterFile::new();
        let cfg = ServerConfig { unit_id: 7, ..Default::default() };
        let req = crate::protocol::build(1, 9, 0x03, &[0, 0, 0, 1]);
        assert_eq!(process(&cfg, &mut regs, &req), Outcome::Drop);
    }

    #[test]
    fn optimizer_coalesces_consecutive_runs() {
        // 10,11,12 are consecutive; 20 is isolated.
        let mut list = PollList::new(vec![12, 10, 11, 20]);
        assert_eq!(list.next_run(125), Some((10, 3)));
        assert_eq!(list.next_run(125), Some((20, 1)));
        assert_eq!(list.next_run(125), Some((10, 3))); // wrapped
    }

    #[test]
    fn optimizer_respects_max_per_query() {
        let mut list = PollList::new(vec![0, 1, 2, 3, 4]);
        assert_eq!(list.next_run(2), Some((0, 2)));
        assert_eq!(list.next_run(2), Some((2, 2)));
        assert_eq!(list.next_run(2), Some((4, 1)));
    }

    #[test]
    fn engine_round_robins_and_builds_valid_reads() {
        let regs = RegisterFile::new();
        let mut eng = PollEngine::new(1);
        eng.read_holding = PollList::new(vec![100, 101]);
        eng.read_input = PollList::new(vec![200]);
        let q1 = eng.next_query(&regs).unwrap();
        let q2 = eng.next_query(&regs).unwrap();
        let (_, f1, _) = frame(&q1).unwrap();
        let (_, f2, _) = frame(&q2).unwrap();
        assert_eq!(f1, Function::ReadHolding as u8);
        assert_eq!(f2, Function::ReadInput as u8);
        // transaction id incremented between the two queries
        assert_ne!(&q1[0..2], &q2[0..2]);
    }

    #[test]
    fn address_base_one_subtracts_on_wire() {
        let regs = RegisterFile::new();
        let mut eng = PollEngine::new(1);
        eng.base = AddressBase::One;
        eng.read_holding = PollList::new(vec![1]);
        let q = eng.next_query(&regs).unwrap();
        let (_, _, pdu) = frame(&q).unwrap();
        assert_eq!(&pdu[0..2], &[0x00, 0x00]); // user addr 1 -> wire 0
    }

    #[test]
    fn manual_mask_write_frame() {
        let mut eng = PollEngine::new(3);
        let q = eng.build_manual(ManualCommand::MaskWrite { addr: 4, and_mask: 0xF0F0, or_mask: 0x0A0A });
        let (hdr, func, pdu) = frame(&q).unwrap();
        assert_eq!(hdr.unit_id, 3);
        assert_eq!(func, 0x16);
        assert_eq!(pdu, &[0x00, 0x04, 0xF0, 0xF0, 0x0A, 0x0A]);
    }
}
