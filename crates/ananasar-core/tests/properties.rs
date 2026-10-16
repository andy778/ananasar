//! Property tests for the frame codec, the register file and the server
//! engine, checked against small reference models written for each request
//! and response type. A failing case is shrunk and its seed saved by proptest.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code: a panic is a failed test"
)]

use ananasar_core::protocol::{Exception, MBAP_PREFIX, build, frame};
use ananasar_core::registers::{BANK_LEN, Bank, InitMode, RegisterFile};
use ananasar_core::server::{MAX_READ, MAX_WRITE, Outcome, OutOfSynch, ServerConfig, process};
use proptest::prelude::*;

/// Start addresses that favour the interesting ends of the 16-bit space.
fn any_addr() -> impl Strategy<Value = u16> {
    prop_oneof![0u16..300, 65200u16..=65535, any::<u16>()]
}

/// An address and a quantity whose span fits in the bank.
fn fitting_span(max_count: u16) -> impl Strategy<Value = (u16, u16)> {
    (any_addr(), 1..=max_count).prop_map(|(addr, count)| {
        let room = u16::try_from(BANK_LEN - usize::from(addr)).unwrap_or(u16::MAX);
        (addr, count.min(room.max(1)))
    })
}

fn reply(cfg: &ServerConfig, regs: &mut RegisterFile, request: &[u8]) -> Vec<u8> {
    match process(cfg, regs, request) {
        Outcome::Reply(r) => r,
        other => panic!("no reply ({other:?}) to {request:02x?}"),
    }
}

/// Decode a reply: (unit, function byte, data after the function byte).
fn decode(reply: &[u8]) -> (u8, u8, Vec<u8>) {
    let (hdr, function, data) = frame(reply).expect("reply is a well-formed frame");
    (hdr.unit_id, function, data.to_vec())
}

fn words(data: &[u8]) -> Vec<u16> {
    data.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect()
}

fn word_bytes(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn write_multiple_pdu(addr: u16, values: &[u16]) -> Vec<u8> {
    let mut d = [addr.to_be_bytes(), (values.len() as u16).to_be_bytes()].concat();
    d.push((values.len() * 2) as u8);
    d.extend(word_bytes(values));
    d
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::default() })]

    // ---- fault options and address offset ----

    #[test]
    fn header_fault_options_change_only_their_header_bytes(
        function in prop_oneof![Just(3u8), Just(4), Just(6), Just(7), Just(8), Just(0x10), Just(0x16), Just(0x17), any::<u8>()],
        data in proptest::collection::vec(any::<u8>(), 0..40),
        ti in any::<bool>(), pid in any::<bool>(), len in any::<bool>(),
    ) {
        let request = build(0x1234, 1, function, &data);
        let plain = reply(&ServerConfig::default(), &mut RegisterFile::new(), &request);
        let cfg = ServerConfig { wrong_transaction_id: ti, wrong_protocol_id: pid, wrong_length_high: len, ..ServerConfig::default() };
        let faulty = reply(&cfg, &mut RegisterFile::new(), &request);
        prop_assert_eq!(faulty.len(), plain.len());
        let exception = plain[7] & 0x80 != 0;
        let mut expected = plain.clone();
        if ti { expected[..2].copy_from_slice(&[0xDE, 0xAD]); }
        if pid && !exception { expected[2..4].copy_from_slice(&[0xAB, 0xCD]); }
        if len && !exception { expected[4] = 0xEF; }
        prop_assert_eq!(faulty, expected);
    }

    #[test]
    fn address_offset_stores_one_register_up_and_refuses_the_last_wire_address(
        addr in any_addr(),
        values in proptest::collection::vec(any::<u16>(), 1..=20),
    ) {
        let cfg = ServerConfig { address_offset: true, ..ServerConfig::default() };
        let mut regs = RegisterFile::new();
        let count = values.len();
        let fits = usize::from(addr) + 1 + count <= BANK_LEN;
        let (_, f, d) = decode(&reply(&cfg, &mut regs, &build(1, 1, 0x10, &write_multiple_pdu(addr, &values))));
        if fits {
            prop_assert_eq!(f, 0x10);
            for (k, v) in values.iter().enumerate() {
                prop_assert_eq!(regs.get(Bank::Holding, addr + 1 + k as u16), *v);
            }
            let read = [addr.to_be_bytes(), (count as u16).to_be_bytes()].concat();
            let (_, f, d) = decode(&reply(&cfg, &mut regs, &build(2, 1, 0x03, &read)));
            prop_assert_eq!(f, 3);
            prop_assert_eq!(words(&d[1..]), values);
        } else {
            prop_assert_eq!(f, 0x90);
            prop_assert_eq!(d, vec![Exception::IllegalDataAddress as u8]);
            prop_assert!(regs.bank(Bank::Holding).iter().all(|r| *r == 0));
        }
    }

    #[test]
    fn a_bad_header_never_runs_the_request(
        function in any::<u8>(),
        data in proptest::collection::vec(any::<u8>(), 0..40),
        pid in 1u16..=u16::MAX,
        len_high in 0u8..=1,
    ) {
        let mut request = build(9, 1, function, &data);
        request[2..4].copy_from_slice(&pid.to_be_bytes());
        request[4] = len_high;
        let mut regs = RegisterFile::new();
        match process(&ServerConfig::default(), &mut regs, &request) {
            Outcome::Close | Outcome::ReplyAndClose(_) => {}
            other => prop_assert!(false, "unexpected {other:?}"),
        }
        prop_assert!(regs.bank(Bank::Holding).iter().all(|r| *r == 0));
    }

    #[test]
    fn out_of_synch_shifts_the_replies_by_one(replies in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 1..8), 1..20)) {
        let mut hold = OutOfSynch::default();
        let sent: Vec<Vec<u8>> = replies.iter().flat_map(|r| hold.next(true, r.clone())).collect();
        prop_assert_eq!(&sent[..], &replies[..replies.len() - 1]);
    }

    // ---- frame codec ----

    #[test]
    fn frame_round_trips_every_build(
        txn in any::<u16>(),
        unit in any::<u8>(),
        function in any::<u8>(),
        data in proptest::collection::vec(any::<u8>(), 0..=250),
    ) {
        let bytes = build(txn, unit, function, &data);
        prop_assert_eq!(bytes.len(), MBAP_PREFIX + 1 + data.len());
        let (hdr, f, payload) = frame(&bytes).unwrap();
        prop_assert_eq!(hdr.transaction_id, txn);
        prop_assert_eq!(hdr.unit_id, unit);
        prop_assert_eq!(hdr.protocol_id, 0);
        prop_assert_eq!(usize::from(hdr.length), data.len() + 2);
        prop_assert_eq!(f, function);
        prop_assert_eq!(payload, &data[..]);
    }

    #[test]
    fn arbitrary_bytes_never_panic_frame_or_engine(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let _ = frame(&bytes);
        let mut regs = RegisterFile::new();
        let _ = process(&ServerConfig::default(), &mut regs, &bytes);
    }

    #[test]
    fn arbitrary_pdu_gets_a_well_formed_answer_or_exception(
        function in any::<u8>(),
        data in proptest::collection::vec(any::<u8>(), 0..260),
    ) {
        let mut regs = RegisterFile::new();
        let request = build(7, 1, function, &data);
        if let Outcome::Reply(r) = process(&ServerConfig::default(), &mut regs, &request) {
            let (hdr, f, payload) = frame(&r).unwrap();
            prop_assert_eq!(hdr.transaction_id, 7);
            prop_assert_eq!(hdr.unit_id, 1);
            if f & 0x80 != 0 {
                prop_assert_eq!(f & 0x7F, function & 0x7F);
                prop_assert_eq!(payload.len(), 1);
                prop_assert!(matches!(payload[0], 1..=3), "unexpected exception {}", payload[0]);
            } else {
                prop_assert_eq!(f, function);
            }
        }
    }

    // ---- register file ----

    #[test]
    fn register_set_then_get(addr in any::<u16>(), value in any::<u16>()) {
        let mut regs = RegisterFile::new();
        regs.set_holding(addr, value);
        prop_assert_eq!(regs.get(Bank::Holding, addr), value);
        prop_assert_eq!(regs.get(Bank::Input, addr), 0);
        regs.set(Bank::Input, addr, value ^ 0xFFFF);
        prop_assert_eq!(regs.get(Bank::Input, addr), value ^ 0xFFFF);
        prop_assert_eq!(regs.get(Bank::Holding, addr), value);
    }

    #[test]
    fn read_span_matches_get_and_rejects_overruns(addr in any_addr(), count in 0u16..=300) {
        let mut regs = RegisterFile::new();
        regs.initialize(Bank::Holding, InitMode::Ascending);
        let fits = usize::from(addr) + usize::from(count) <= BANK_LEN;
        match regs.read_span(Bank::Holding, addr, count) {
            Some(span) => {
                prop_assert!(fits);
                prop_assert_eq!(span.len(), usize::from(count));
                for (i, v) in span.iter().enumerate() {
                    prop_assert_eq!(*v, regs.get(Bank::Holding, addr + i as u16));
                }
            }
            None => prop_assert!(!fits),
        }
    }

    // ---- reads and writes through the engine ----

    #[test]
    fn fc16_then_fc3_returns_the_written_values(
        (addr, count) in fitting_span(MAX_WRITE),
        seed in any::<u16>(),
    ) {
        let values: Vec<u16> = (0..count).map(|i| seed.wrapping_mul(31).wrapping_add(i)).collect();
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();

        let write = build(1, 1, 0x10, &write_multiple_pdu(addr, &values));
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &write));
        prop_assert_eq!(f, 0x10);
        prop_assert_eq!(data, [addr.to_be_bytes(), count.to_be_bytes()].concat());

        let read = build(2, 1, 0x03, &[addr.to_be_bytes(), count.to_be_bytes()].concat());
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &read));
        prop_assert_eq!(f, 0x03);
        prop_assert_eq!(usize::from(data[0]), usize::from(count) * 2);
        prop_assert_eq!(words(&data[1..]), values);
    }

    #[test]
    fn fc6_then_fc3_returns_the_written_value_and_echoes(addr in any::<u16>(), value in any::<u16>()) {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        let pdu = [addr.to_be_bytes(), value.to_be_bytes()].concat();
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &build(1, 1, 0x06, &pdu)));
        prop_assert_eq!(f, 0x06);
        prop_assert_eq!(data, pdu);
        let read = build(2, 1, 0x03, &[addr.to_be_bytes(), 1u16.to_be_bytes()].concat());
        let (_, _, data) = decode(&reply(&cfg, &mut regs, &read));
        prop_assert_eq!(words(&data[1..]), vec![value]);
    }

    #[test]
    fn fc4_reads_the_input_bank_and_writes_never_touch_it(
        (addr, count) in fitting_span(MAX_READ),
        input_value in any::<u16>(),
        value in any::<u16>(),
    ) {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        for i in 0..count {
            regs.set(Bank::Input, addr + i, input_value.wrapping_add(i));
        }
        let write = build(1, 1, 0x06, &[addr.to_be_bytes(), value.to_be_bytes()].concat());
        let _ = reply(&cfg, &mut regs, &write);
        let read = build(2, 1, 0x04, &[addr.to_be_bytes(), count.to_be_bytes()].concat());
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &read));
        prop_assert_eq!(f, 0x04);
        let expected: Vec<u16> = (0..count).map(|i| input_value.wrapping_add(i)).collect();
        prop_assert_eq!(words(&data[1..]), expected);
    }

    #[test]
    fn fc22_matches_the_reference_formula(
        addr in any::<u16>(),
        current in any::<u16>(),
        and_mask in any::<u16>(),
        or_mask in any::<u16>(),
    ) {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        regs.set_holding(addr, current);
        let pdu = [addr.to_be_bytes(), and_mask.to_be_bytes(), or_mask.to_be_bytes()].concat();
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &build(1, 1, 0x16, &pdu)));
        prop_assert_eq!(f, 0x16);
        prop_assert_eq!(data, pdu);
        // Modbus specification: Result = (Current AND And_Mask) OR (Or_Mask AND (NOT And_Mask)).
        let expected = (current & and_mask) | (or_mask & !and_mask);
        prop_assert_eq!(regs.get(Bank::Holding, addr), expected);
    }

    #[test]
    fn fc22_with_extreme_masks(addr in any::<u16>(), current in any::<u16>(), mask in any::<u16>()) {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        regs.set_holding(addr, current);
        let keep = [addr.to_be_bytes(), 0xFFFFu16.to_be_bytes(), mask.to_be_bytes()].concat();
        let _ = reply(&cfg, &mut regs, &build(1, 1, 0x16, &keep));
        prop_assert_eq!(regs.get(Bank::Holding, addr), current, "AND 0xFFFF keeps the register");
        let set = [addr.to_be_bytes(), 0u16.to_be_bytes(), mask.to_be_bytes()].concat();
        let _ = reply(&cfg, &mut regs, &build(2, 1, 0x16, &set));
        prop_assert_eq!(regs.get(Bank::Holding, addr), mask, "AND 0 stores the OR mask");
    }

    #[test]
    fn fc23_applies_the_write_before_the_read(
        (w_addr, w_count) in fitting_span(121),
        read_offset in 0u16..8,
        read_count in 1u16..=20,
        seed in any::<u16>(),
    ) {
        // A read range that overlaps the written range (when it fits).
        let r_addr = w_addr.saturating_sub(read_offset);
        let r_count = read_count.min(u16::try_from(BANK_LEN - usize::from(r_addr)).unwrap_or(u16::MAX));
        let values: Vec<u16> = (0..w_count).map(|i| seed.wrapping_add(i).wrapping_mul(7)).collect();
        let before: Vec<u16> = (0..r_count).map(|i| 0xA000 + i).collect();

        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        let mut model = vec![0u16; BANK_LEN];
        for (i, v) in before.iter().enumerate() {
            regs.set_holding(r_addr + i as u16, *v);
            model[usize::from(r_addr) + i] = *v;
        }
        for (i, v) in values.iter().enumerate() {
            model[usize::from(w_addr) + i] = *v;
        }

        let mut pdu = [r_addr.to_be_bytes(), r_count.to_be_bytes(), w_addr.to_be_bytes(), w_count.to_be_bytes()].concat();
        pdu.push((values.len() * 2) as u8);
        pdu.extend(word_bytes(&values));
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &build(1, 1, 0x17, &pdu)));
        prop_assert_eq!(f, 0x17);
        let expected: Vec<u16> = (0..r_count).map(|i| model[usize::from(r_addr + i)]).collect();
        prop_assert_eq!(usize::from(data[0]), usize::from(r_count) * 2);
        prop_assert_eq!(words(&data[1..]), expected);
        prop_assert_eq!(regs.get(Bank::Holding, w_addr), values[0], "the write stays applied");
    }

    // ---- exceptions ----

    #[test]
    fn out_of_range_spans_give_illegal_data_address(
        addr in 65411u16..=65535,
        extra in 1u16..=125,
        function in prop_oneof![Just(0x03u8), Just(0x04u8)],
    ) {
        let count = u16::try_from(BANK_LEN - usize::from(addr)).unwrap() + extra - 1;
        prop_assume!((1..=MAX_READ).contains(&count) && usize::from(addr) + usize::from(count) > BANK_LEN);
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        let request = build(1, 1, function, &[addr.to_be_bytes(), count.to_be_bytes()].concat());
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &request));
        prop_assert_eq!(f, function | 0x80);
        prop_assert_eq!(data, vec![Exception::IllegalDataAddress as u8]);
    }

    #[test]
    fn bad_read_quantities_give_illegal_data_value(
        addr in any::<u16>(),
        count in prop_oneof![Just(0u16), (MAX_READ + 1)..=u16::MAX],
        function in prop_oneof![Just(0x03u8), Just(0x04u8)],
    ) {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        let request = build(1, 1, function, &[addr.to_be_bytes(), count.to_be_bytes()].concat());
        let (_, f, data) = decode(&reply(&cfg, &mut regs, &request));
        prop_assert_eq!(f, function | 0x80);
        prop_assert_eq!(data, vec![Exception::IllegalDataValue as u8]);
    }

    #[test]
    fn disabled_functions_answer_illegal_function(
        function in prop_oneof![Just(3u8), Just(4), Just(6), Just(7), Just(8), Just(0x10), Just(0x16), Just(0x17)],
        data in proptest::collection::vec(any::<u8>(), 0..40),
    ) {
        let mut cfg = ServerConfig::default();
        cfg.supported.remove(&function);
        let mut regs = RegisterFile::new();
        let (_, f, d) = decode(&reply(&cfg, &mut regs, &build(1, 1, function, &data)));
        prop_assert_eq!(f, function | 0x80);
        prop_assert_eq!(d, vec![Exception::IllegalFunction as u8]);
    }

    #[test]
    fn forced_exception_answers_supported_functions_with_the_forced_code_or_illegal_address(
        function in prop_oneof![Just(3u8), Just(4), Just(6), Just(7), Just(8), Just(0x10), Just(0x16), Just(0x17)],
        data in proptest::collection::vec(any::<u8>(), 0..40),
        unit in 1u8..=247,
    ) {
        let cfg = ServerConfig { unit_id: unit, forced_exception: Some(Exception::SlaveDeviceBusy), ..ServerConfig::default() };
        let mut regs = RegisterFile::new();
        let (u, f, d) = decode(&reply(&cfg, &mut regs, &build(1, unit, function, &data)));
        prop_assert_eq!(u, unit);
        prop_assert_eq!(f, function | 0x80);
        prop_assert!(
            d == vec![Exception::SlaveDeviceBusy as u8] || d == vec![Exception::IllegalDataAddress as u8],
            "unexpected exception {d:?}"
        );
        let other = unit.wrapping_add(1);
        prop_assert_eq!(process(&cfg, &mut regs, &build(2, other, function, &data)), Outcome::Drop);
    }

    #[test]
    fn forced_exception_keeps_illegal_function_for_unsupported_codes(
        function in any::<u8>().prop_filter("unsupported", |f| ![3u8, 4, 6, 7, 8, 0x10, 0x16, 0x17].contains(f)),
        data in proptest::collection::vec(any::<u8>(), 0..40),
    ) {
        let cfg = ServerConfig { forced_exception: Some(Exception::SlaveDeviceBusy), ..ServerConfig::default() };
        let mut regs = RegisterFile::new();
        let (_, f, d) = decode(&reply(&cfg, &mut regs, &build(1, 1, function, &data)));
        prop_assert_eq!(f, function | 0x80);
        prop_assert_eq!(d, vec![Exception::IllegalFunction as u8]);
    }
}
