//! Building requests from the configured mix, and checking that a reply
//! matches the request it answers.

use crate::config::Mix;
use crate::rng::Rng;
use ananasar_core::protocol::{self, FrameError};
use ananasar_core::server::{MAX_READ, MAX_WRITE};

/// Most registers the write half of FC23 may carry.
const MAX_RW_WRITE: u16 = 121;

/// What a successful reply must contain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expect {
    /// A register read: a byte count of twice this many registers, then data.
    Registers(u16),
    /// The PDU data (after the function byte) must echo these bytes.
    Echo(Vec<u8>),
}

/// One generated request and what to expect back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Transaction id.
    pub txn: u16,
    /// Unit id.
    pub unit: u8,
    /// Function code.
    pub fc: u8,
    /// The complete frame.
    pub frame: Vec<u8>,
    /// The reply check.
    pub expect: Expect,
}

/// Why a reply does not match its request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MismatchKind {
    /// Not a decodable Modbus/TCP frame (too short, or bad length framing).
    Malformed,
    /// The protocol id is not 0.
    ProtocolId,
    /// The transaction id is not the request's.
    TransactionId,
    /// The unit id is not the request's.
    UnitId,
    /// The function code is not the request's (or its exception form).
    Function,
    /// The byte count does not fit the quantity requested or the data sent.
    ByteCount,
    /// A write reply does not echo the request.
    EchoData,
}

impl MismatchKind {
    /// Every kind, in report order.
    pub const ALL: [Self; 7] =
        [Self::Malformed, Self::ProtocolId, Self::TransactionId, Self::UnitId, Self::Function, Self::ByteCount, Self::EchoData];

    /// The name used in reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::ProtocolId => "protocol-id",
            Self::TransactionId => "transaction-id",
            Self::UnitId => "unit-id",
            Self::Function => "function-code",
            Self::ByteCount => "byte-count",
            Self::EchoData => "echo-data",
        }
    }
}

/// The result of checking one reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A correct normal reply.
    Ok,
    /// A well-formed Modbus exception reply with this code.
    Exception(u8),
    /// The reply does not match the request.
    Mismatch(MismatchKind),
}

/// Check `reply` against `req`: transaction id, unit id and function code
/// echoed, and a byte count or echo that is consistent with the request.
pub fn validate(req: &Request, reply: &[u8]) -> Verdict {
    let (hdr, func, pdu) = match protocol::frame(reply) {
        Ok(parts) => parts,
        Err(FrameError::BadHeader) => return Verdict::Mismatch(MismatchKind::ProtocolId),
        Err(_) => return Verdict::Mismatch(MismatchKind::Malformed),
    };
    if hdr.transaction_id != req.txn {
        return Verdict::Mismatch(MismatchKind::TransactionId);
    }
    if hdr.unit_id != req.unit {
        return Verdict::Mismatch(MismatchKind::UnitId);
    }
    if func == (req.fc | 0x80) {
        return match pdu {
            [code] => Verdict::Exception(*code),
            _ => Verdict::Mismatch(MismatchKind::Malformed),
        };
    }
    if func != req.fc {
        return Verdict::Mismatch(MismatchKind::Function);
    }
    match &req.expect {
        Expect::Registers(n) => {
            let want = usize::from(*n).saturating_mul(2);
            match pdu.split_first() {
                Some((&count, data)) if usize::from(count) == want && data.len() == want => Verdict::Ok,
                _ => Verdict::Mismatch(MismatchKind::ByteCount),
            }
        }
        Expect::Echo(bytes) if pdu == bytes.as_slice() => Verdict::Ok,
        Expect::Echo(_) => Verdict::Mismatch(MismatchKind::EchoData),
    }
}

/// Pick a start address and quantity inside the mix's address range, with the
/// quantity limited to `limit` (the protocol maximum for the function).
fn span(mix: &Mix, rng: &mut Rng, limit: u16) -> (u16, u16) {
    let len = u32::from(mix.addr_end).saturating_sub(u32::from(mix.addr_start)).saturating_add(1);
    let hi = u32::from(mix.qty_max).min(u32::from(limit)).min(len).max(1);
    let lo = u32::from(mix.qty_min).clamp(1, hi);
    let qty = rng.range_u16(u16::try_from(lo).unwrap_or(1), u16::try_from(hi).unwrap_or(1));
    let room = len.saturating_sub(u32::from(qty)).saturating_add(1);
    let off = u16::try_from(rng.below(u64::from(room))).unwrap_or(0);
    (mix.addr_start.saturating_add(off), qty)
}

fn words(rng: &mut Rng, qty: u16) -> Vec<u8> {
    (0..usize::from(qty).saturating_mul(2)).map(|_| rng.byte()).collect()
}

fn put(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// Generate the next request of the mix. The same generator state yields the
/// same request.
pub fn generate(mix: &Mix, rng: &mut Rng, txn: u16, unit: u8) -> Request {
    let fc = mix.pick(rng);
    let mut data = Vec::new();
    let expect = match fc {
        0x03 | 0x04 => {
            let (addr, qty) = span(mix, rng, MAX_READ);
            put(&mut data, addr);
            put(&mut data, qty);
            Expect::Registers(qty)
        }
        0x06 => {
            let addr = mix.addr_start.saturating_add(rng.range_u16(0, mix.addr_end.saturating_sub(mix.addr_start)));
            let value = rng.range_u16(0, u16::MAX);
            put(&mut data, addr);
            put(&mut data, value);
            Expect::Echo(data.clone())
        }
        0x10 => {
            let (addr, qty) = span(mix, rng, MAX_WRITE);
            put(&mut data, addr);
            put(&mut data, qty);
            let echo = data.clone();
            data.push(u8::try_from(usize::from(qty).saturating_mul(2)).unwrap_or(0));
            data.extend(words(rng, qty));
            Expect::Echo(echo)
        }
        0x16 => {
            let addr = mix.addr_start.saturating_add(rng.range_u16(0, mix.addr_end.saturating_sub(mix.addr_start)));
            put(&mut data, addr);
            put(&mut data, rng.range_u16(0, u16::MAX));
            put(&mut data, rng.range_u16(0, u16::MAX));
            Expect::Echo(data.clone())
        }
        _ => {
            // FC23: read span, then write span.
            let (r_addr, r_qty) = span(mix, rng, MAX_READ);
            let (w_addr, w_qty) = span(mix, rng, MAX_RW_WRITE);
            put(&mut data, r_addr);
            put(&mut data, r_qty);
            put(&mut data, w_addr);
            put(&mut data, w_qty);
            data.push(u8::try_from(usize::from(w_qty).saturating_mul(2)).unwrap_or(0));
            data.extend(words(rng, w_qty));
            Expect::Registers(r_qty)
        }
    };
    Request { txn, unit, fc, frame: protocol::build(txn, unit, fc, &data), expect }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ananasar_core::registers::RegisterFile;
    use ananasar_core::server::{Outcome, ServerConfig, process};

    fn all_fc_mix() -> Mix {
        Mix { weights: [1, 1, 1, 1, 1, 1], addr_start: 10, addr_end: 40, qty_min: 2, qty_max: 8 }
    }

    #[test]
    fn same_seed_gives_the_same_request_sequence() {
        let run = |seed| {
            let mut rng = Rng::derive(seed, 3);
            (0..200u16).map(|i| generate(&all_fc_mix(), &mut rng, i, 1)).collect::<Vec<_>>()
        };
        assert_eq!(run(5), run(5));
        assert_ne!(run(5), run(6));
    }

    #[test]
    fn mix_follows_weights_and_stays_in_range() {
        let mix = Mix { weights: [3, 1, 0, 0, 0, 0], addr_start: 100, addr_end: 120, qty_min: 1, qty_max: 5 };
        let mut rng = Rng::derive(1, 0);
        let (mut fc3, mut fc4) = (0u32, 0u32);
        for i in 0..4000u16 {
            let r = generate(&mix, &mut rng, i, 1);
            match r.fc {
                3 => fc3 += 1,
                4 => fc4 += 1,
                other => panic!("unexpected FC {other}"),
            }
            let (_, _, pdu) = protocol::frame(&r.frame).unwrap();
            let addr = u16::from_be_bytes([pdu[0], pdu[1]]);
            let qty = u16::from_be_bytes([pdu[2], pdu[3]]);
            assert!((100..=120).contains(&addr) && (1..=5).contains(&qty) && addr + qty - 1 <= 120);
        }
        let ratio = f64::from(fc3) / f64::from(fc4);
        assert!((2.5..3.6).contains(&ratio), "FC3:FC4 = {ratio}");
    }

    #[test]
    fn narrow_ranges_clamp_the_quantity() {
        let mix = Mix { weights: [1, 0, 0, 0, 0, 0], addr_start: 5, addr_end: 6, qty_min: 10, qty_max: 100 };
        let mut rng = Rng::derive(2, 0);
        for i in 0..100u16 {
            let r = generate(&mix, &mut rng, i, 1);
            assert_eq!(r.expect, Expect::Registers(2));
        }
    }

    #[test]
    fn real_server_replies_to_every_function_validate_ok() {
        let cfg = ServerConfig::default();
        let mut regs = RegisterFile::new();
        let mut rng = Rng::derive(11, 0);
        for i in 0..2000u16 {
            let req = generate(&all_fc_mix(), &mut rng, i, 1);
            let Outcome::Reply(reply) = process(&cfg, &mut regs, &req.frame) else { panic!("no reply to FC {}", req.fc) };
            assert_eq!(validate(&req, &reply), Verdict::Ok, "FC {}", req.fc);
        }
    }

    fn read_req() -> Request {
        let frame = protocol::build(9, 1, 3, &[0, 0, 0, 2]);
        Request { txn: 9, unit: 1, fc: 3, frame, expect: Expect::Registers(2) }
    }

    #[test]
    fn detects_each_kind_of_mismatch() {
        let req = read_req();
        let good = protocol::build(9, 1, 3, &[4, 0, 1, 0, 2]);
        assert_eq!(validate(&req, &good), Verdict::Ok);

        let m = |reply: &[u8]| validate(&req, reply);
        assert_eq!(m(&protocol::build(10, 1, 3, &[4, 0, 1, 0, 2])), Verdict::Mismatch(MismatchKind::TransactionId));
        assert_eq!(m(&protocol::build(9, 2, 3, &[4, 0, 1, 0, 2])), Verdict::Mismatch(MismatchKind::UnitId));
        assert_eq!(m(&protocol::build(9, 1, 4, &[4, 0, 1, 0, 2])), Verdict::Mismatch(MismatchKind::Function));
        assert_eq!(m(&protocol::build(9, 1, 3, &[2, 0, 1])), Verdict::Mismatch(MismatchKind::ByteCount)); // wrong quantity
        assert_eq!(m(&protocol::build(9, 1, 3, &[4, 0, 1, 0])), Verdict::Mismatch(MismatchKind::ByteCount)); // byte count lies
        assert_eq!(m(&protocol::build(9, 1, 3, &[4, 0, 1, 0, 2, 0])), Verdict::Mismatch(MismatchKind::ByteCount));
        assert_eq!(m(&[0, 9, 0, 0]), Verdict::Mismatch(MismatchKind::Malformed));
        let mut bad_proto = good.clone();
        bad_proto[3] = 1;
        assert_eq!(m(&bad_proto), Verdict::Mismatch(MismatchKind::ProtocolId));
        let mut bad_len = good;
        bad_len[5] = 9;
        assert_eq!(m(&bad_len), Verdict::Mismatch(MismatchKind::Malformed));
    }

    #[test]
    fn exceptions_are_recognised_and_kept_apart_from_mismatches() {
        let req = read_req();
        assert_eq!(validate(&req, &protocol::build(9, 1, 0x83, &[2])), Verdict::Exception(2));
        // An exception for another function, or with a wrong transaction id.
        assert_eq!(validate(&req, &protocol::build(9, 1, 0x84, &[2])), Verdict::Mismatch(MismatchKind::Function));
        assert_eq!(validate(&req, &protocol::build(8, 1, 0x83, &[2])), Verdict::Mismatch(MismatchKind::TransactionId));
        assert_eq!(validate(&req, &protocol::build(9, 1, 0x83, &[2, 3])), Verdict::Mismatch(MismatchKind::Malformed));
    }

    #[test]
    fn write_replies_must_echo() {
        let frame = protocol::build(1, 1, 6, &[0, 5, 0, 9]);
        let req = Request { txn: 1, unit: 1, fc: 6, frame, expect: Expect::Echo(vec![0, 5, 0, 9]) };
        assert_eq!(validate(&req, &protocol::build(1, 1, 6, &[0, 5, 0, 9])), Verdict::Ok);
        assert_eq!(validate(&req, &protocol::build(1, 1, 6, &[0, 5, 0, 8])), Verdict::Mismatch(MismatchKind::EchoData));
    }
}
