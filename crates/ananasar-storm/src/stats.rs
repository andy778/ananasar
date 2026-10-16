//! Counters: the per-worker [`Stats`] merged into the final report, and the
//! shared [`Live`] atomics the progress line reads once a second.

use crate::fault::FaultKind;
use crate::hist::{AtomicHistogram, Histogram};
use crate::request::MismatchKind;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

fn bump(map_value: &mut u64, by: u64) {
    *map_value = map_value.saturating_add(by);
}

/// Everything counted for one run (or one worker of it).
///
/// Every valid request sent ends in exactly one of: an ok reply, an
/// exception, a mismatch, a timeout, a reset or an I/O error, so
/// [`Stats::sent`] always equals [`Stats::accounted`] after a run.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Valid requests written (injected malformed frames are not included).
    pub sent: u64,
    /// Correct normal replies.
    pub ok: u64,
    /// Well-formed exception replies by code.
    pub exceptions: BTreeMap<u8, u64>,
    /// Replies that did not match their request, by reason.
    pub mismatches: BTreeMap<MismatchKind, u64>,
    /// Requests that got no reply within the timeout.
    pub timeouts: u64,
    /// Requests that ended in a closed or reset connection.
    pub resets: u64,
    /// Requests that ended in another I/O error.
    pub io_errors: u64,
    /// Connection attempts that failed.
    pub connect_errors: u64,
    /// Connections opened.
    pub connects: u64,
    /// Connections reopened after a timeout, error or injected fault.
    pub reconnects: u64,
    /// Completed connect/disconnect cycles of the churn mode.
    pub churn_cycles: u64,
    /// Idle connections held open for the run.
    pub half_open_held: u64,
    /// Idle connections that could not be opened.
    pub half_open_failed: u64,
    /// Injected faults by kind.
    pub faults: BTreeMap<FaultKind, u64>,
    /// Malformed frames the target answered with a reply.
    pub fault_replied: u64,
    /// Malformed frames after which the target closed the connection.
    pub fault_closed: u64,
    /// Malformed frames the target ignored for the whole fault wait.
    pub fault_silent: u64,
    /// Workers that panicked (should be 0).
    pub worker_panics: u64,
    /// Request-to-reply time of every well-formed reply, in microseconds.
    pub latency: Histogram,
    /// Time to establish a connection, in microseconds.
    pub connect_latency: Histogram,
}

impl Stats {
    /// Total exception replies.
    pub fn exception_total(&self) -> u64 {
        self.exceptions.values().fold(0u64, |a, &n| a.saturating_add(n))
    }

    /// Total mismatches.
    pub fn mismatch_total(&self) -> u64 {
        self.mismatches.values().fold(0u64, |a, &n| a.saturating_add(n))
    }

    /// Total injected faults.
    pub fn fault_total(&self) -> u64 {
        self.faults.values().fold(0u64, |a, &n| a.saturating_add(n))
    }

    /// Requests with a known outcome; equals `sent` after a completed run.
    pub fn accounted(&self) -> u64 {
        [self.ok, self.exception_total(), self.mismatch_total(), self.timeouts, self.resets, self.io_errors]
            .iter()
            .fold(0u64, |a, &n| a.saturating_add(n))
    }

    /// Add another worker's counters.
    pub fn merge(&mut self, o: &Self) {
        for (dst, src) in [
            (&mut self.sent, o.sent),
            (&mut self.ok, o.ok),
            (&mut self.timeouts, o.timeouts),
            (&mut self.resets, o.resets),
            (&mut self.io_errors, o.io_errors),
            (&mut self.connect_errors, o.connect_errors),
            (&mut self.connects, o.connects),
            (&mut self.reconnects, o.reconnects),
            (&mut self.churn_cycles, o.churn_cycles),
            (&mut self.half_open_held, o.half_open_held),
            (&mut self.half_open_failed, o.half_open_failed),
            (&mut self.fault_replied, o.fault_replied),
            (&mut self.fault_closed, o.fault_closed),
            (&mut self.fault_silent, o.fault_silent),
            (&mut self.worker_panics, o.worker_panics),
        ] {
            bump(dst, src);
        }
        for (&k, &n) in &o.exceptions {
            bump(self.exceptions.entry(k).or_default(), n);
        }
        for (&k, &n) in &o.mismatches {
            bump(self.mismatches.entry(k).or_default(), n);
        }
        for (&k, &n) in &o.faults {
            bump(self.faults.entry(k).or_default(), n);
        }
        self.latency.merge(&o.latency);
        self.connect_latency.merge(&o.connect_latency);
    }
}

/// Counters shared with the progress printer.
#[derive(Debug, Default)]
pub struct Live {
    /// Valid requests written.
    pub sent: AtomicU64,
    /// Replies (ok or exception).
    pub replies: AtomicU64,
    /// Exception replies.
    pub exceptions: AtomicU64,
    /// Timeouts.
    pub timeouts: AtomicU64,
    /// Resets, I/O errors and failed connects.
    pub errors: AtomicU64,
    /// Mismatched replies.
    pub mismatches: AtomicU64,
    /// Injected faults.
    pub faults: AtomicU64,
    /// Churn cycles completed.
    pub cycles: AtomicU64,
    /// Sum of reply latencies in microseconds.
    pub latency_sum_us: AtomicU64,
    /// Reply latencies so far, for live percentiles.
    pub latency: AtomicHistogram,
}

fn add(c: &AtomicU64, n: u64) {
    c.fetch_add(n, Ordering::Relaxed);
}

/// Updates a worker's [`Stats`] and the [`Live`] counters together.
pub struct Recorder<'a> {
    /// The worker's own counters.
    pub stats: Stats,
    live: &'a Live,
}

impl<'a> Recorder<'a> {
    /// A recorder feeding `live`.
    pub fn new(live: &'a Live) -> Self {
        Self { stats: Stats::default(), live }
    }

    /// `n` valid requests were written.
    pub fn sent(&mut self, n: u64) {
        bump(&mut self.stats.sent, n);
        add(&self.live.sent, n);
    }

    fn latency(&mut self, micros: u64) {
        self.stats.latency.record(micros);
        add(&self.live.replies, 1);
        add(&self.live.latency_sum_us, micros);
        self.live.latency.record(micros);
    }

    /// A correct normal reply after `micros`.
    pub fn ok(&mut self, micros: u64) {
        bump(&mut self.stats.ok, 1);
        self.latency(micros);
    }

    /// An exception reply with `code` after `micros`.
    pub fn exception(&mut self, code: u8, micros: u64) {
        bump(self.stats.exceptions.entry(code).or_default(), 1);
        add(&self.live.exceptions, 1);
        self.latency(micros);
    }

    /// `n` replies that did not match their request.
    pub fn mismatch(&mut self, kind: MismatchKind, n: u64) {
        bump(self.stats.mismatches.entry(kind).or_default(), n);
        add(&self.live.mismatches, n);
    }

    /// `n` requests without a reply in time.
    pub fn timeout(&mut self, n: u64) {
        bump(&mut self.stats.timeouts, n);
        add(&self.live.timeouts, n);
    }

    /// `n` requests lost to a closed or reset connection.
    pub fn reset(&mut self, n: u64) {
        bump(&mut self.stats.resets, n);
        add(&self.live.errors, n);
    }

    /// `n` requests lost to another I/O error.
    pub fn io_error(&mut self, n: u64) {
        bump(&mut self.stats.io_errors, n);
        add(&self.live.errors, n);
    }

    /// A connection attempt failed.
    pub fn connect_error(&mut self) {
        bump(&mut self.stats.connect_errors, 1);
        add(&self.live.errors, 1);
    }

    /// A connection was established after `micros`; `again` if it replaces one.
    pub fn connected(&mut self, micros: u64, again: bool) {
        bump(&mut self.stats.connects, 1);
        if again {
            bump(&mut self.stats.reconnects, 1);
        }
        self.stats.connect_latency.record(micros);
    }

    /// A fault of `kind` was injected.
    pub fn fault(&mut self, kind: FaultKind) {
        bump(self.stats.faults.entry(kind).or_default(), 1);
        add(&self.live.faults, 1);
    }

    /// A churn cycle completed.
    pub fn cycle(&mut self) {
        bump(&mut self.stats.churn_cycles, 1);
        add(&self.live.cycles, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_keeps_stats_and_live_in_step_and_merges() {
        let live = Live::default();
        let mut a = Recorder::new(&live);
        a.sent(5);
        a.ok(100);
        a.ok(300);
        a.exception(2, 200);
        a.mismatch(MismatchKind::TransactionId, 1);
        a.timeout(1);
        assert_eq!(a.stats.sent, a.stats.accounted());
        let mut b = Recorder::new(&live);
        b.sent(2);
        b.reset(2);
        b.fault(FaultKind::Split);
        let mut total = a.stats.clone();
        total.merge(&b.stats);
        assert_eq!((total.sent, total.accounted()), (7, 7));
        assert_eq!(total.exceptions.get(&2), Some(&1));
        assert_eq!(total.latency.count(), 3);
        assert_eq!(live.sent.load(Ordering::Relaxed), 7);
        assert_eq!(live.replies.load(Ordering::Relaxed), 3);
        assert_eq!(live.latency_sum_us.load(Ordering::Relaxed), 600);
        assert_eq!(total.fault_total(), 1);
    }
}
