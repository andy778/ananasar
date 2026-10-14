//! Register scanner: a sweep of single-register reads over an address range.
//!
//! The original's scanner issues one FC3 or FC4 read per register on its own
//! connection, records what each address answers, and finishes with a summary
//! ("N registers scanned ... succeeded/failed"). This type is the I/O-free
//! half: it produces the queries and classifies the responses; the caller
//! moves the bytes.

use crate::protocol::{self, Function, build};
use crate::registers::Bank;

/// What one scanned address answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanResult {
    /// The register's value.
    Value(u16),
    /// The server replied with this exception code.
    Exception(u8),
    /// No reply, or one that did not match the request.
    NoReply,
}

/// Totals for a finished (or partial) scan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanSummary {
    /// Addresses tried.
    pub scanned: usize,
    /// Addresses that returned a value.
    pub succeeded: usize,
    /// Addresses that returned an exception or nothing.
    pub failed: usize,
}

/// A sequential scan of `start..=end` in one bank.
#[derive(Clone, Debug)]
pub struct Scanner {
    unit_id: u8,
    bank: Bank,
    next: u32,
    end: u32,
    txn: u16,
    results: Vec<(u16, ScanResult)>,
}

impl Scanner {
    /// Scan `start..=end` (inclusive) of `bank` on `unit_id`. If `end < start`
    /// the scan is empty.
    pub fn new(unit_id: u8, bank: Bank, start: u16, end: u16) -> Self {
        Self { unit_id, bank, next: u32::from(start), end: u32::from(end), txn: 0, results: Vec::new() }
    }

    /// The next address and its query, or `None` when the range is exhausted.
    /// Call [`Scanner::record`] with the response before asking again.
    pub fn next_query(&mut self) -> Option<(u16, Vec<u8>)> {
        if self.next > self.end {
            return None;
        }
        let addr = u16::try_from(self.next).ok()?;
        self.next += 1;
        let func = match self.bank {
            Bank::Holding => Function::ReadHolding,
            Bank::Input => Function::ReadInput,
        };
        let txn = self.txn;
        self.txn = self.txn.wrapping_add(1);
        let [hi, lo] = addr.to_be_bytes();
        Some((addr, build(txn, self.unit_id, func as u8, &[hi, lo, 0, 1])))
    }

    /// Classify the response to the query for `addr`. `None` means no reply
    /// (timeout or closed connection).
    pub fn record(&mut self, addr: u16, response: Option<&[u8]>) -> ScanResult {
        let result = response.map_or(ScanResult::NoReply, |r| self.classify(r));
        self.results.push((addr, result));
        result
    }

    fn classify(&self, response: &[u8]) -> ScanResult {
        let expected = match self.bank {
            Bank::Holding => Function::ReadHolding as u8,
            Bank::Input => Function::ReadInput as u8,
        };
        let Ok((hdr, func, pdu)) = protocol::frame(response) else {
            return ScanResult::NoReply;
        };
        // The query for `addr` used transaction id (number of addresses issued
        // before it), which is `results.len()` modulo 2^16 as long as every
        // query is recorded in order.
        let sent_txn = u16::try_from(self.results.len() & 0xFFFF).unwrap_or(0);
        if hdr.unit_id != self.unit_id || hdr.transaction_id != sent_txn {
            return ScanResult::NoReply;
        }
        if func == expected | 0x80 {
            return pdu.first().map_or(ScanResult::NoReply, |&c| ScanResult::Exception(c));
        }
        if func == expected && pdu.len() == 3 && pdu[0] == 2 {
            return ScanResult::Value(u16::from_be_bytes([pdu[1], pdu[2]]));
        }
        ScanResult::NoReply
    }

    /// Every address recorded so far, in scan order.
    pub fn results(&self) -> &[(u16, ScanResult)] {
        &self.results
    }

    /// Totals so far.
    pub fn summary(&self) -> ScanSummary {
        let succeeded = self.results.iter().filter(|(_, r)| matches!(r, ScanResult::Value(_))).count();
        ScanSummary { scanned: self.results.len(), succeeded, failed: self.results.len() - succeeded }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registers::{InitMode, RegisterFile};
    use crate::server::{Outcome, ServerConfig, process};

    #[test]
    fn scans_a_range_against_the_server_engine() {
        let mut regs = RegisterFile::new();
        regs.initialize(Bank::Holding, InitMode::Ascending);
        let cfg = ServerConfig::default();
        let mut scan = Scanner::new(1, Bank::Holding, 10, 13);
        while let Some((addr, query)) = scan.next_query() {
            let Outcome::Reply(resp) = process(&cfg, &mut regs, &query) else { panic!("no reply") };
            scan.record(addr, Some(&resp));
        }
        let values: Vec<_> = scan.results().iter().map(|(a, r)| (*a, *r)).collect();
        assert_eq!(values.len(), 4);
        assert_eq!(values[0], (10, ScanResult::Value(10)));
        assert_eq!(scan.summary(), ScanSummary { scanned: 4, succeeded: 4, failed: 0 });
    }

    #[test]
    fn counts_exceptions_and_silence_as_failures() {
        let mut scan = Scanner::new(1, Bank::Input, 0, 1);
        let (a0, _) = scan.next_query().unwrap();
        // Exception reply for txn 0.
        let exc = build(0, 1, 0x84, &[2]);
        assert_eq!(scan.record(a0, Some(&exc)), ScanResult::Exception(2));
        let (a1, _) = scan.next_query().unwrap();
        assert_eq!(scan.record(a1, None), ScanResult::NoReply);
        assert!(scan.next_query().is_none());
        assert_eq!(scan.summary(), ScanSummary { scanned: 2, succeeded: 0, failed: 2 });
    }

    #[test]
    fn full_range_and_empty_range() {
        let mut s = Scanner::new(1, Bank::Holding, 65534, 65535);
        assert!(s.next_query().is_some() && s.next_query().is_some() && s.next_query().is_none());
        assert!(Scanner::new(1, Bank::Holding, 5, 4).next_query().is_none());
    }
}
