//! Client poll engine, reconstructed from `FUN_006647a0` (dispatch),
//! `FUN_00665640` (read builder / optimizer), `FUN_006652b0` (write-multi) and
//! `FUN_00665150` (manual commands).
//!
//! Each register category is a sorted address list with a cursor. On every
//! poll the engine round-robins read-holding → read-input → write, and for the
//! current category emits one query covering the longest run of *consecutive*
//! addresses starting at the cursor, capped by the user's max-per-query limit
//! (the N50/100/123/125 menu). The transaction id increments per poll and wraps
//! `0xFFFF -> 0`.

use crate::protocol::{build, Function};
use crate::registers::{Bank, RegisterFile};

/// Address numbering offset applied on the wire (`- _DAT_006d4450`).
/// `One` subtracts 1 so a user-facing 1-based address maps to a 0-based bus address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressBase {
    Zero,
    One,
}

impl AddressBase {
    fn offset(self) -> u16 {
        match self {
            AddressBase::Zero => 0,
            AddressBase::One => 1,
        }
    }
}

/// A sorted list of register addresses with a round-robin cursor.
#[derive(Clone, Debug, Default)]
pub struct PollList {
    addrs: Vec<u16>,
    cursor: usize,
}

impl PollList {
    pub fn new(mut addrs: Vec<u16>) -> Self {
        addrs.sort_unstable();
        addrs.dedup();
        Self { addrs, cursor: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.addrs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.addrs.len()
    }

    /// Take the next run: the longest stretch of consecutive addresses starting
    /// at the cursor, limited to `max` registers. Returns `(start_addr, count)`
    /// and advances the cursor (wrapping to 0 past the end), matching the
    /// original's coalescing loop.
    pub fn next_run(&mut self, max: u16) -> Option<(u16, u16)> {
        if self.addrs.is_empty() {
            return None;
        }
        if self.cursor >= self.addrs.len() {
            self.cursor = 0;
        }
        let start = self.addrs[self.cursor];
        let mut run: u16 = 1;
        while (run as usize) < self.addrs.len() - self.cursor
            && run < max
            && self.addrs[self.cursor + run as usize - 1] + 1 == self.addrs[self.cursor + run as usize]
        {
            run += 1;
        }
        self.cursor += run as usize;
        if self.cursor >= self.addrs.len() {
            self.cursor = 0;
        }
        Some((start, run))
    }
}

/// A one-off manual command (manual §2.2: FC6/FC22 manual, plus FC7).
#[derive(Clone, Copy, Debug)]
pub enum ManualCommand {
    WriteSingle { addr: u16, value: u16 },
    MaskWrite { addr: u16, and_mask: u16, or_mask: u16 },
    ReadExceptionStatus,
}

/// The client poll engine state.
#[derive(Clone, Debug)]
pub struct PollEngine {
    pub unit_id: u8,
    pub txn: u16,
    pub max_read: u16,
    pub max_write: u16,
    pub base: AddressBase,
    pub read_holding: PollList,
    pub read_input: PollList,
    pub write: PollList,
    state: u8,
}

impl PollEngine {
    pub fn new(unit_id: u8) -> Self {
        Self {
            unit_id,
            txn: 0,
            max_read: 125,
            max_write: 123,
            base: AddressBase::Zero,
            read_holding: PollList::default(),
            read_input: PollList::default(),
            write: PollList::default(),
            state: 0,
        }
    }

    fn next_txn(&mut self) -> u16 {
        let t = self.txn;
        self.txn = if self.txn < 0xFFFF { self.txn + 1 } else { 0 };
        t
    }

    pub fn is_idle(&self) -> bool {
        self.read_holding.is_empty() && self.read_input.is_empty() && self.write.is_empty()
    }

    /// Produce the next query frame, round-robining across the non-empty
    /// categories. Writes pull their values from the shared holding bank.
    /// Returns `None` only when every list is empty ("Idle").
    pub fn next_query(&mut self, regs: &RegisterFile) -> Option<Vec<u8>> {
        if self.is_idle() {
            return None;
        }
        // Advance to the next category that actually has work.
        for _ in 0..3 {
            let built = match self.state {
                0 if !self.read_holding.is_empty() => Some(self.build_read(Bank::Holding)),
                1 if !self.read_input.is_empty() => Some(self.build_read(Bank::Input)),
                2 if !self.write.is_empty() => Some(self.build_write(regs)),
                _ => None,
            };
            self.state = (self.state + 1) % 3;
            if let Some(frame) = built {
                return Some(frame);
            }
        }
        None
    }

    /// FC3/FC4 read query (`FUN_00665640`).
    fn build_read(&mut self, bank: Bank) -> Vec<u8> {
        let (func, list) = match bank {
            Bank::Holding => (Function::ReadHolding, &mut self.read_holding),
            Bank::Input => (Function::ReadInput, &mut self.read_input),
        };
        let (start, count) = list.next_run(self.max_read).unwrap();
        let wire = start.wrapping_sub(self.base.offset());
        let txn = self.next_txn();
        let mut data = Vec::with_capacity(4);
        data.extend_from_slice(&wire.to_be_bytes());
        data.extend_from_slice(&count.to_be_bytes());
        build(txn, self.unit_id, func as u8, &data)
    }

    /// FC16 write-multiple query (`FUN_006652b0`). Values come from the shared
    /// holding bank at the listed addresses.
    fn build_write(&mut self, regs: &RegisterFile) -> Vec<u8> {
        let (start, count) = self.write.next_run(self.max_write).unwrap();
        let wire = start.wrapping_sub(self.base.offset());
        let txn = self.next_txn();
        let mut data = Vec::with_capacity(5 + count as usize * 2);
        data.extend_from_slice(&wire.to_be_bytes());
        data.extend_from_slice(&count.to_be_bytes());
        data.push((count * 2) as u8);
        for i in 0..count {
            let v = regs.get(Bank::Holding, start + i);
            data.extend_from_slice(&v.to_be_bytes());
        }
        build(txn, self.unit_id, Function::WriteMultiple as u8, &data)
    }

    /// Build a one-off manual command frame (`FUN_00665150`).
    pub fn build_manual(&mut self, cmd: ManualCommand) -> Vec<u8> {
        let txn = self.next_txn();
        match cmd {
            ManualCommand::WriteSingle { addr, value } => {
                let wire = addr.wrapping_sub(self.base.offset());
                let mut data = Vec::with_capacity(4);
                data.extend_from_slice(&wire.to_be_bytes());
                data.extend_from_slice(&value.to_be_bytes());
                build(txn, self.unit_id, Function::WriteSingle as u8, &data)
            }
            ManualCommand::MaskWrite { addr, and_mask, or_mask } => {
                let wire = addr.wrapping_sub(self.base.offset());
                let mut data = Vec::with_capacity(6);
                data.extend_from_slice(&wire.to_be_bytes());
                data.extend_from_slice(&and_mask.to_be_bytes());
                data.extend_from_slice(&or_mask.to_be_bytes());
                build(txn, self.unit_id, Function::MaskWrite as u8, &data)
            }
            ManualCommand::ReadExceptionStatus => {
                build(txn, self.unit_id, Function::ReadExceptionStatus as u8, &[])
            }
        }
    }
}
