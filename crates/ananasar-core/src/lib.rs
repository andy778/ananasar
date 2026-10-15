//! `ananas-core` — the Modbus/TCP protocol engine for the Ananas reimplementation.
//!
//! A from-scratch reimplementation of the Ananas Modbus/TCP tool. The crate
//! is `std`-only and platform-agnostic so the same core drives the Linux and
//! Windows builds; OS-specific features (DDE, adapter IP config) live in the
//! binary crates, not here.
#![cfg_attr(
    test,
    allow(clippy::arithmetic_side_effects, reason = "test code: a panic is a failed test")
)]

#![warn(missing_docs)]

pub mod ananas_file;
pub mod client;
pub mod log;
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

    fn exception_code(cfg: &ServerConfig, func: u8) -> Option<(u8, u8)> {
        let req = crate::protocol::build(1, 1, func, &[0, 0, 0, 1]);
        match process(cfg, &mut RegisterFile::new(), &req) {
            Outcome::Reply(r) => {
                let (_, f, pdu) = frame(&r).unwrap();
                (f & 0x80 != 0).then(|| (f, pdu[0]))
            }
            Outcome::Drop => None,
        }
    }

    #[test]
    fn disabled_function_code_yields_illegal_function() {
        let mut cfg = ServerConfig::default();
        assert_eq!(exception_code(&cfg, 0x03), None);
        cfg.supported.remove(&0x03);
        assert_eq!(exception_code(&cfg, 0x03), Some((0x83, 0x01)));
        // Other codes stay enabled.
        assert_eq!(exception_code(&cfg, 0x04), None);
    }

    #[test]
    fn fc7_and_fc8_are_enabled_by_default_and_can_be_disabled() {
        let mut cfg = ServerConfig::default();
        assert_eq!(exception_code(&cfg, 0x07), None);
        assert_eq!(exception_code(&cfg, 0x08), None);
        cfg.supported.remove(&0x07);
        cfg.supported.remove(&0x08);
        assert_eq!(exception_code(&cfg, 0x07), Some((0x87, 0x01)));
        assert_eq!(exception_code(&cfg, 0x08), Some((0x88, 0x01)));
    }

    #[test]
    fn forced_exception_answers_every_request() {
        use crate::protocol::Exception;
        for exc in [Exception::SlaveDeviceBusy, Exception::GatewayTargetFailed] {
            let cfg = ServerConfig { forced_exception: Some(exc), ..Default::default() };
            for func in [0x03u8, 0x04, 0x06, 0x07, 0x08, 0x10, 0x16, 0x17] {
                assert_eq!(exception_code(&cfg, func), Some((func | 0x80, exc as u8)));
            }
        }
    }

    #[test]
    fn forced_exception_keeps_illegal_function_and_illegal_address() {
        // As recorded from the original: an unsupported function code and a span past
        // the end of the register file keep their own exception when one is forced.
        use crate::protocol::Exception;
        let mut cfg = ServerConfig { forced_exception: Some(Exception::SlaveDeviceBusy), ..Default::default() };
        assert_eq!(exception_code(&cfg, 0x01), Some((0x81, 0x01)));
        assert_eq!(exception_code(&cfg, 0x2B), Some((0xAB, 0x01)));
        cfg.supported.remove(&0x03);
        assert_eq!(exception_code(&cfg, 0x03), Some((0x83, 0x01)));
        cfg.supported.insert(0x03);
        let mut regs = RegisterFile::new();
        let past_end = crate::protocol::build(1, 1, 0x03, &[0xFF, 0xFF, 0, 2]);
        assert_eq!(
            process(&cfg, &mut regs, &past_end),
            Outcome::Reply(crate::protocol::build(1, 1, 0x83, &[0x02]))
        );
        let write_past_end = crate::protocol::build(1, 1, 0x10, &[0xFF, 0xFF, 0, 2, 4, 0, 1, 0, 2]);
        assert_eq!(
            process(&cfg, &mut regs, &write_past_end),
            Outcome::Reply(crate::protocol::build(1, 1, 0x90, &[0x02]))
        );
        let read_write = crate::protocol::build(1, 1, 0x17, &[0, 0, 0, 1, 0xFF, 0xFF, 0, 2, 4, 0, 1, 0, 2]);
        assert_eq!(
            process(&cfg, &mut regs, &read_write),
            Outcome::Reply(crate::protocol::build(1, 1, 0x97, &[0x02]))
        );
        // A bad quantity that still fits is answered with the forced code.
        let zero = crate::protocol::build(1, 1, 0x03, &[0, 0, 0, 0]);
        assert_eq!(
            process(&cfg, &mut regs, &zero),
            Outcome::Reply(crate::protocol::build(1, 1, 0x83, &[0x06]))
        );
    }

    #[test]
    fn forced_exception_still_drops_unit_id_mismatch() {
        let cfg = ServerConfig {
            unit_id: 7,
            forced_exception: Some(crate::protocol::Exception::SlaveDeviceFailure),
            ..Default::default()
        };
        let req = crate::protocol::build(1, 9, 0x03, &[0, 0, 0, 1]);
        assert_eq!(process(&cfg, &mut RegisterFile::new(), &req), Outcome::Drop);
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
    fn force_zero_txn_keeps_transaction_id_at_zero() {
        let regs = RegisterFile::new();
        let mut eng = PollEngine::new(1);
        eng.force_zero_txn = true;
        eng.read_holding = PollList::new(vec![1]);
        for _ in 0..3 {
            let q = eng.next_query(&regs).unwrap();
            assert_eq!(&q[0..2], &[0, 0]);
        }
        let q = eng.build_manual(ManualCommand::ReadExceptionStatus);
        assert_eq!(&q[0..2], &[0, 0]);
        eng.force_zero_txn = false;
        let q = eng.next_query(&regs).unwrap();
        assert_eq!(&q[0..2], &[0, 0]); // counter never advanced while forced
        let q = eng.next_query(&regs).unwrap();
        assert_eq!(&q[0..2], &[0, 1]);
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

    #[test]
    fn manual_write_single_as_fc16_frame() {
        let mut eng = PollEngine::new(3);
        eng.base = AddressBase::One;
        let q = eng.build_manual(ManualCommand::WriteSingleMultiple { addr: 5, value: 0x1234 });
        let (hdr, func, pdu) = frame(&q).unwrap();
        assert_eq!(hdr.unit_id, 3);
        assert_eq!(func, 0x10);
        // wire address 4, count 1, byte count 2, value
        assert_eq!(pdu, &[0x00, 0x04, 0x00, 0x01, 0x02, 0x12, 0x34]);
    }

    #[test]
    fn manual_write_single_fc6_frame() {
        let mut eng = PollEngine::new(3);
        let q = eng.build_manual(ManualCommand::WriteSingle { addr: 5, value: 0x1234 });
        let (_, func, pdu) = frame(&q).unwrap();
        assert_eq!(func, 0x06);
        assert_eq!(pdu, &[0x00, 0x05, 0x12, 0x34]);
    }

    /// Every truncation of a valid request, for every function, must yield a
    /// reply or a drop and never a panic.
    #[test]
    fn truncated_requests_never_panic() {
        let cfg = ServerConfig { unit_id: 0, supported: (0..=0x20).collect(), forced_exception: None };
        let pdus: [&[u8]; 6] = [
            &[0, 1, 0, 2],
            &[0, 1, 0, 2, 4, 0, 1, 0, 2],
            &[0, 1, 0xFF, 0xFF, 0, 0],
            &[0, 1, 0, 2, 0, 3, 0, 1, 2, 0xAA, 0xBB],
            &[],
            &[0xFF],
        ];
        for func in 0..=0x20u8 {
            for pdu in pdus {
                for cut in 0..=pdu.len() {
                    let mut regs = RegisterFile::new();
                    let req = crate::protocol::build(1, 1, func, &pdu[..cut]);
                    let _ = process(&cfg, &mut regs, &req);
                }
            }
        }
        for len in 0..12 {
            let junk = vec![0u8; len];
            assert!(frame(&junk).is_err() || len >= 8);
            let _ = process(&cfg, &mut RegisterFile::new(), &junk);
        }
    }

    #[test]
    fn writes_at_the_top_of_the_address_space() {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        // FC16 for the last register succeeds, one more is an address error.
        let ok = crate::protocol::build(1, 1, 0x10, &[0xFF, 0xFF, 0, 1, 2, 0x12, 0x34]);
        assert!(matches!(process(&cfg, &mut regs, &ok), Outcome::Reply(r) if r[7] == 0x10));
        assert_eq!(regs.get(Bank::Holding, 0xFFFF), 0x1234);
        let over = crate::protocol::build(1, 1, 0x10, &[0xFF, 0xFF, 0, 2, 4, 0, 1, 0, 2]);
        assert!(matches!(process(&cfg, &mut regs, &over), Outcome::Reply(r) if r[7] == 0x90 && r[8] == 2));
    }

    #[test]
    fn read_write_multiple_with_bad_read_span_writes_nothing() {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        // read 2 at 0xFFFF (out of range), write 1 word at 0.
        let req = crate::protocol::build(1, 1, 0x17, &[0xFF, 0xFF, 0, 2, 0, 0, 0, 1, 2, 0x12, 0x34]);
        assert!(matches!(process(&cfg, &mut regs, &req), Outcome::Reply(r) if r[7] == 0x97 && r[8] == 2));
        assert_eq!(regs.get(Bank::Holding, 0), 0);
    }

    #[test]
    fn be_u16_rejects_short_and_overflowing_offsets() {
        use crate::protocol::be_u16;
        assert_eq!(be_u16(&[1, 2, 3], 1), Some(0x0203));
        assert_eq!(be_u16(&[1, 2, 3], 2), None);
        assert_eq!(be_u16(&[], 0), None);
        assert_eq!(be_u16(&[1, 2], usize::MAX), None);
    }

    #[test]
    fn poll_run_never_overflows_at_the_top_address() {
        let mut list = PollList::new(vec![0xFFFE, 0xFFFF]);
        assert_eq!(list.next_run(10), Some((0xFFFE, 2)));
    }
}
