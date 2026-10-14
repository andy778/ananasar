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

use crate::protocol::{build, Exception, Function};
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

/// Server configuration (the Options menu, manual §2.7.3).
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Accepted unit id. `0` means "accept any UID" (manual §2.1).
    pub unit_id: u8,
    /// Which function codes are enabled. If a request uses one not in here,
    /// the server replies with `IllegalFunction`.
    pub supported: BTreeSet<u8>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        // Manual §2.1: server supports FC 3, 4, 6, 16, 22, 23 by default.
        let supported = [0x03u8, 0x04, 0x06, 0x10, 0x16, 0x17].into_iter().collect();
        Self { unit_id: 0, supported }
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
                Some(words) => {
                    let mut data = Vec::with_capacity(1 + words.len() * 2);
                    data.push((words.len() * 2) as u8);
                    for w in words {
                        data.extend_from_slice(&w.to_be_bytes());
                    }
                    reply(data)
                }
                None => exc(Exception::IllegalDataAddress),
            }
        }

        Function::WriteSingle => {
            let Some((addr, value)) = read_addr_count(pdu) else {
                return exc(Exception::IllegalDataValue);
            };
            regs.set_holding(addr, value);
            reply(pdu[..4].to_vec()) // echo address + value
        }

        Function::WriteMultiple => {
            if pdu.len() < 5 {
                return exc(Exception::IllegalDataValue);
            }
            let addr = u16::from_be_bytes([pdu[0], pdu[1]]);
            let count = u16::from_be_bytes([pdu[2], pdu[3]]);
            let byte_count = pdu[4] as usize;
            if count == 0 || count > MAX_WRITE || byte_count != count as usize * 2
                || pdu.len() < 5 + byte_count
            {
                return exc(Exception::IllegalDataValue);
            }
            if (addr as usize) + (count as usize) > crate::registers::BANK_LEN {
                return exc(Exception::IllegalDataAddress);
            }
            for i in 0..count as usize {
                let hi = pdu[5 + i * 2];
                let lo = pdu[6 + i * 2];
                regs.set_holding(addr + i as u16, u16::from_be_bytes([hi, lo]));
            }
            reply(pdu[..4].to_vec()) // echo address + quantity
        }

        Function::MaskWrite => {
            if pdu.len() < 6 {
                return exc(Exception::IllegalDataValue);
            }
            let addr = u16::from_be_bytes([pdu[0], pdu[1]]);
            let and_mask = u16::from_be_bytes([pdu[2], pdu[3]]);
            let or_mask = u16::from_be_bytes([pdu[4], pdu[5]]);
            let cur = regs.get(Bank::Holding, addr);
            // result = (current AND and_mask) OR (or_mask AND NOT and_mask)
            regs.set_holding(addr, (cur & and_mask) | (or_mask & !and_mask));
            reply(pdu[..6].to_vec()) // echo
        }

        Function::ReadWriteMultiple => {
            // Write is performed before the read (manual §1.3, FC23).
            if pdu.len() < 9 {
                return exc(Exception::IllegalDataValue);
            }
            let r_addr = u16::from_be_bytes([pdu[0], pdu[1]]);
            let r_count = u16::from_be_bytes([pdu[2], pdu[3]]);
            let w_addr = u16::from_be_bytes([pdu[4], pdu[5]]);
            let w_count = u16::from_be_bytes([pdu[6], pdu[7]]);
            let w_bytes = pdu[8] as usize;
            if r_count == 0 || r_count > MAX_READ || w_count == 0 || w_count > 121
                || w_bytes != w_count as usize * 2 || pdu.len() < 9 + w_bytes
            {
                return exc(Exception::IllegalDataValue);
            }
            if (w_addr as usize) + (w_count as usize) > crate::registers::BANK_LEN
                || (r_addr as usize) + (r_count as usize) > crate::registers::BANK_LEN
            {
                return exc(Exception::IllegalDataAddress);
            }
            for i in 0..w_count as usize {
                let hi = pdu[9 + i * 2];
                let lo = pdu[10 + i * 2];
                regs.set_holding(w_addr + i as u16, u16::from_be_bytes([hi, lo]));
            }
            let words = regs.read_span(Bank::Holding, r_addr, r_count).unwrap();
            let mut data = Vec::with_capacity(1 + words.len() * 2);
            data.push((words.len() * 2) as u8);
            for w in words {
                data.extend_from_slice(&w.to_be_bytes());
            }
            reply(data)
        }

        // Serial-line functions the TCP server accepts but has no real data for.
        Function::ReadExceptionStatus => reply(vec![0x00]),
        Function::Diagnostics => reply(pdu.to_vec()),
    }
}

fn read_addr_count(pdu: &[u8]) -> Option<(u16, u16)> {
    if pdu.len() < 4 {
        return None;
    }
    Some((
        u16::from_be_bytes([pdu[0], pdu[1]]),
        u16::from_be_bytes([pdu[2], pdu[3]]),
    ))
}
