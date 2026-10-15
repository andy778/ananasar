//! Client poll engine: query dispatch, the read builder / optimizer, the
//! write-multiple builder, and one-off manual commands.
//!
//! Each register category is a sorted address list with a cursor. On every
//! poll the engine round-robins read-holding → read-input → write, and for the
//! current category emits one query covering the longest run of *consecutive*
//! addresses starting at the cursor, capped by the user's max-per-query limit
//! (the N50/100/123/125 menu). The transaction id increments per poll and wraps
//! `0xFFFF -> 0`.

use crate::protocol::{build, Function};
use crate::registers::{Bank, RegisterFile};

/// Address numbering offset applied on the wire.
/// `One` subtracts 1 so a user-facing 1-based address maps to a 0-based bus address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressBase {
    /// Addresses are used as given (0-based).
    Zero,
    /// Addresses are 1-based; one is subtracted on the wire.
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
    /// Build a list from `addrs`, sorted and de-duplicated.
    pub fn new(mut addrs: Vec<u16>) -> Self {
        addrs.sort_unstable();
        addrs.dedup();
        Self { addrs, cursor: 0 }
    }

    /// True when the list holds no addresses.
    pub fn is_empty(&self) -> bool {
        self.addrs.is_empty()
    }

    /// Number of addresses in the list.
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
        let rest = self.addrs.get(self.cursor..)?;
        let start = *rest.first()?;
        let mut run: u16 = 1;
        for pair in rest.windows(2) {
            let [prev, next] = *pair else { break };
            if run >= max || prev.checked_add(1) != Some(next) {
                break;
            }
            run = run.saturating_add(1);
        }
        self.cursor = self.cursor.saturating_add(usize::from(run));
        if self.cursor >= self.addrs.len() {
            self.cursor = 0;
        }
        Some((start, run))
    }
}

/// A one-off manual command (FC6, FC16 and FC22, plus FC7).
#[derive(Clone, Copy, Debug)]
pub enum ManualCommand {
    /// FC6: write `value` to holding register `addr`.
    WriteSingle {
        /// Register address.
        addr: u16,
        /// Value to write.
        value: u16,
    },
    /// FC16 with a register count of 1: write `value` to holding register
    /// `addr` as a write-multiple request.
    WriteSingleMultiple {
        /// Register address.
        addr: u16,
        /// Value to write.
        value: u16,
    },
    /// FC22: `(current & and_mask) | (or_mask & !and_mask)` into `addr`.
    MaskWrite {
        /// Register address.
        addr: u16,
        /// Bits kept from the current value.
        and_mask: u16,
        /// Bits set where `and_mask` is clear.
        or_mask: u16,
    },
    /// FC7: read the exception status byte.
    ReadExceptionStatus,
}

/// The client poll engine state.
#[derive(Clone, Debug)]
pub struct PollEngine {
    /// Unit id placed in every query.
    pub unit_id: u8,
    /// Next transaction id; wraps from 0xFFFF to 0.
    pub txn: u16,
    /// Send transaction id 0 in every request instead of incrementing.
    pub force_zero_txn: bool,
    /// Most registers merged into one read query.
    pub max_read: u16,
    /// Most registers merged into one write query.
    pub max_write: u16,
    /// Address numbering applied on the wire.
    pub base: AddressBase,
    /// Holding registers polled with FC3.
    pub read_holding: PollList,
    /// Input registers polled with FC4.
    pub read_input: PollList,
    /// Holding registers written with FC16.
    pub write: PollList,
    state: u8,
}

impl PollEngine {
    /// A new engine for `unit_id` with empty lists and the Modbus limits.
    pub fn new(unit_id: u8) -> Self {
        Self {
            unit_id,
            txn: 0,
            force_zero_txn: false,
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
        if self.force_zero_txn {
            return 0;
        }
        let t = self.txn;
        self.txn = self.txn.wrapping_add(1);
        t
    }

    /// True when there is nothing to poll.
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
            self.state = self.state.wrapping_add(1) % 3;
            if let Some(frame) = built {
                return Some(frame);
            }
        }
        None
    }

    /// FC3/FC4 read query.
    fn build_read(&mut self, bank: Bank) -> Vec<u8> {
        let (func, list) = match bank {
            Bank::Holding => (Function::ReadHolding, &mut self.read_holding),
            Bank::Input => (Function::ReadInput, &mut self.read_input),
        };
        let (start, count) = list.next_run(self.max_read).unwrap_or((0, 0));
        let wire = start.wrapping_sub(self.base.offset());
        let txn = self.next_txn();
        let mut data = Vec::with_capacity(4);
        data.extend_from_slice(&wire.to_be_bytes());
        data.extend_from_slice(&count.to_be_bytes());
        build(txn, self.unit_id, func as u8, &data)
    }

    /// FC16 write-multiple query. Values come from the shared
    /// holding bank at the listed addresses.
    fn build_write(&mut self, regs: &RegisterFile) -> Vec<u8> {
        let (start, count) = self.write.next_run(self.max_write).unwrap_or((0, 0));
        let wire = start.wrapping_sub(self.base.offset());
        let txn = self.next_txn();
        let mut data = Vec::with_capacity(usize::from(count).saturating_mul(2).saturating_add(5));
        data.extend_from_slice(&wire.to_be_bytes());
        data.extend_from_slice(&count.to_be_bytes());
        data.push(u8::try_from(usize::from(count).saturating_mul(2)).unwrap_or(u8::MAX));
        for i in 0..count {
            let v = regs.get(Bank::Holding, start.wrapping_add(i));
            data.extend_from_slice(&v.to_be_bytes());
        }
        build(txn, self.unit_id, Function::WriteMultiple as u8, &data)
    }

    /// Build a one-off manual command frame.
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
            ManualCommand::WriteSingleMultiple { addr, value } => {
                let wire = addr.wrapping_sub(self.base.offset());
                let mut data = Vec::with_capacity(7);
                data.extend_from_slice(&wire.to_be_bytes());
                data.extend_from_slice(&1u16.to_be_bytes());
                data.push(2);
                data.extend_from_slice(&value.to_be_bytes());
                build(txn, self.unit_id, Function::WriteMultiple as u8, &data)
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
