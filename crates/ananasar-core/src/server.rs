//! Server-side request handling.
//!
//! Validation order follows the original: header (protocol id / length, done in
//! [`crate::protocol::frame`]), then UID, then function, then data address.
//! A UID mismatch is dropped silently (the original sets `local_110 = -1`, which
//! suppresses the reply); function and address faults produce an exception
//! response with the function code's MSB set.
//!
//! Deviation from the original: we reject out-of-spec quantities
//! (`> 125` read / `> 123` write) with `IllegalDataValue`. The original checked
//! only `start + count <= 65536` and assembled the reply in a fixed 513-byte
//! global, so quantities of 253-255 overflowed it. Enforcing the Modbus limits
//! closes that hole.

use crate::protocol::{be_u16, build, Exception, Function};
use crate::registers::{Bank, RegisterFile};
use std::collections::BTreeSet;

/// Most registers one read request may ask for.
pub const MAX_READ: u16 = 125;
/// Most registers one write request may carry.
pub const MAX_WRITE: u16 = 123;

/// What the server decided to do with a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Send these bytes back to the client.
    Reply(Vec<u8>),
    /// Drop the request with no reply (UID mismatch, as the original does).
    Drop,
}

/// Server configuration (the original's Options menu).
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Accepted unit id. `0` means "accept any UID".
    pub unit_id: u8,
    /// Which function codes are enabled. If a request uses one not in here,
    /// the server replies with `IllegalFunction`.
    pub supported: BTreeSet<u8>,
    /// When set, every request that passes the unit id filter is answered with
    /// this exception instead of being processed.
    pub forced_exception: Option<Exception>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        // By default the server supports FC 3, 4, 6, 7, 8, 16, 22 and 23.
        let supported = [0x03u8, 0x04, 0x06, 0x07, 0x08, 0x10, 0x16, 0x17].into_iter().collect();
        Self { unit_id: 0, supported, forced_exception: None }
    }
}

impl ServerConfig {
    fn uid_ok(&self, uid: u8) -> bool {
        self.unit_id == 0 || self.unit_id == uid
    }
}

/// Process one framed request against the shared register file.
pub fn process(cfg: &ServerConfig, regs: &mut RegisterFile, buf: &[u8]) -> Outcome {
    // Malformed header/length: the original logs and does not reply.
    let Ok((hdr, func_byte, pdu)) = crate::protocol::frame(buf) else {
        return Outcome::Drop;
    };

    if !cfg.uid_ok(hdr.unit_id) {
        return Outcome::Drop;
    }

    let exc = |e: Exception| {
        Outcome::Reply(build(
            hdr.transaction_id,
            hdr.unit_id,
            func_byte | 0x80,
            &[e as u8],
        ))
    };

    if let Some(forced) = cfg.forced_exception {
        return exc(forced);
    }
    if !cfg.supported.contains(&func_byte) {
        return exc(Exception::IllegalFunction);
    }
    let Some(func) = Function::from_u8(func_byte) else {
        return exc(Exception::IllegalFunction);
    };

    let reply = |data: Vec<u8>| Outcome::Reply(build(hdr.transaction_id, hdr.unit_id, func_byte, &data));

    match func {
        Function::ReadHolding | Function::ReadInput => {
            let bank = if func == Function::ReadHolding { Bank::Holding } else { Bank::Input };
            let Some((addr, count)) = read_addr_count(pdu) else {
                return exc(Exception::IllegalDataValue);
            };
            if count == 0 || count > MAX_READ {
                return exc(Exception::IllegalDataValue);
            }
            match regs.read_span(bank, addr, count) {
                Some(words) => reply(words_reply(words)),
                None => exc(Exception::IllegalDataAddress),
            }
        }

        Function::WriteSingle => {
            let Some((addr, value)) = read_addr_count(pdu) else {
                return exc(Exception::IllegalDataValue);
            };
            regs.set_holding(addr, value);
            reply(pdu.get(..4).unwrap_or_default().to_vec()) // echo address + value
        }

        Function::WriteMultiple => {
            let (Some(addr), Some(count), Some(&byte_count)) =
                (be_u16(pdu, 0), be_u16(pdu, 2), pdu.get(4))
            else {
                return exc(Exception::IllegalDataValue);
            };
            let byte_count = usize::from(byte_count);
            let Some(data) = pdu.get(5..).and_then(|d| d.get(..byte_count)) else {
                return exc(Exception::IllegalDataValue);
            };
            if count == 0 || count > MAX_WRITE || byte_count != usize::from(count).saturating_mul(2) {
                return exc(Exception::IllegalDataValue);
            }
            if !write_words(regs, addr, data) {
                return exc(Exception::IllegalDataAddress);
            }
            reply(pdu.get(..4).unwrap_or_default().to_vec()) // echo address + quantity
        }

        Function::MaskWrite => {
            let (Some(addr), Some(and_mask), Some(or_mask)) =
                (be_u16(pdu, 0), be_u16(pdu, 2), be_u16(pdu, 4))
            else {
                return exc(Exception::IllegalDataValue);
            };
            let cur = regs.get(Bank::Holding, addr);
            // result = (current AND and_mask) OR (or_mask AND NOT and_mask)
            regs.set_holding(addr, (cur & and_mask) | (or_mask & !and_mask));
            reply(pdu.get(..6).unwrap_or_default().to_vec()) // echo
        }

        Function::ReadWriteMultiple => {
            // Write is performed before the read (FC23).
            let (Some(r_addr), Some(r_count), Some(w_addr), Some(w_count), Some(&w_bytes)) = (
                be_u16(pdu, 0),
                be_u16(pdu, 2),
                be_u16(pdu, 4),
                be_u16(pdu, 6),
                pdu.get(8),
            ) else {
                return exc(Exception::IllegalDataValue);
            };
            let w_bytes = usize::from(w_bytes);
            let Some(data) = pdu.get(9..).and_then(|d| d.get(..w_bytes)) else {
                return exc(Exception::IllegalDataValue);
            };
            if r_count == 0 || r_count > MAX_READ || w_count == 0 || w_count > 121
                || w_bytes != usize::from(w_count).saturating_mul(2)
            {
                return exc(Exception::IllegalDataValue);
            }
            // Validate the read span before writing, so a bad read leaves no side effect.
            if regs.read_span(Bank::Holding, r_addr, r_count).is_none()
                || !span_fits(w_addr, w_count)
            {
                return exc(Exception::IllegalDataAddress);
            }
            write_words(regs, w_addr, data);
            match regs.read_span(Bank::Holding, r_addr, r_count) {
                Some(words) => reply(words_reply(words)),
                None => exc(Exception::IllegalDataAddress),
            }
        }

        // Serial-line functions the TCP server accepts but has no real data for.
        Function::ReadExceptionStatus => reply(vec![0x00]),
        Function::Diagnostics => reply(pdu.to_vec()),
    }
}

fn read_addr_count(pdu: &[u8]) -> Option<(u16, u16)> {
    Some((be_u16(pdu, 0)?, be_u16(pdu, 2)?))
}

/// Whether `count` registers starting at `addr` fit inside a bank.
fn span_fits(addr: u16, count: u16) -> bool {
    usize::from(addr).saturating_add(usize::from(count)) <= crate::registers::BANK_LEN
}

/// Write big-endian words from `data` to consecutive holding registers from
/// `addr`. Returns `false` (writing nothing) if the span runs past the bank.
fn write_words(regs: &mut RegisterFile, addr: u16, data: &[u8]) -> bool {
    let Ok(count) = u16::try_from(data.len() / 2) else {
        return false;
    };
    if !span_fits(addr, count) {
        return false;
    }
    for (a, &pair) in (addr..=u16::MAX).zip(data.as_chunks::<2>().0) {
        regs.set_holding(a, u16::from_be_bytes(pair));
    }
    true
}

/// Byte count followed by the words, big-endian: the body of a read reply.
fn words_reply(words: &[u16]) -> Vec<u8> {
    let bytes = words.len().saturating_mul(2);
    let mut data = Vec::with_capacity(bytes.saturating_add(1));
    data.push(u8::try_from(bytes).unwrap_or(u8::MAX));
    for w in words {
        data.extend_from_slice(&w.to_be_bytes());
    }
    data
}
