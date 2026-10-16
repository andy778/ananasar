//! A log-linear latency histogram (microseconds). Each power of two is split
//! into 32 buckets, so percentiles are within about 3 % of the true value;
//! min, max, count and mean are exact.

const SUB_BITS: u32 = 5;
const SUB: u64 = 1 << SUB_BITS;
/// Values below this are stored exactly, one bucket each.
const EXACT: u64 = SUB * 2;
const BUCKETS: usize = 1920;

/// Latency histogram.
#[derive(Clone, Debug)]
pub struct Histogram {
    buckets: Vec<u64>,
    count: u64,
    sum: u128,
    min: u64,
    max: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self { buckets: vec![0; BUCKETS], count: 0, sum: 0, min: u64::MAX, max: 0 }
    }
}

fn index(v: u64) -> usize {
    if v < EXACT {
        return usize::try_from(v).unwrap_or(0);
    }
    let msb = 63u32.saturating_sub(v.leading_zeros());
    let e = msb.saturating_sub(SUB_BITS);
    let mantissa = (v >> e).saturating_sub(SUB);
    let idx = u64::from(e).saturating_add(1).saturating_mul(SUB).saturating_add(mantissa);
    usize::try_from(idx).unwrap_or(BUCKETS.saturating_sub(1)).min(BUCKETS.saturating_sub(1))
}

/// Largest value that falls in bucket `idx`.
fn upper(idx: usize) -> u64 {
    let idx = u64::try_from(idx).unwrap_or(0);
    if idx < EXACT {
        return idx;
    }
    let e = u32::try_from((idx >> SUB_BITS).saturating_sub(1)).unwrap_or(0);
    let m = (idx & (SUB - 1)) | SUB;
    let low = m.wrapping_shl(e);
    low.saturating_add(1u64.wrapping_shl(e).saturating_sub(1))
}

impl Histogram {
    /// Record one value in microseconds.
    pub fn record(&mut self, micros: u64) {
        if let Some(b) = self.buckets.get_mut(index(micros)) {
            *b = b.saturating_add(1);
        }
        self.count = self.count.saturating_add(1);
        self.sum = self.sum.saturating_add(u128::from(micros));
        self.min = self.min.min(micros);
        self.max = self.max.max(micros);
    }

    /// Add another histogram's samples.
    pub fn merge(&mut self, other: &Self) {
        for (a, b) in self.buckets.iter_mut().zip(&other.buckets) {
            *a = a.saturating_add(*b);
        }
        self.count = self.count.saturating_add(other.count);
        self.sum = self.sum.saturating_add(other.sum);
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }

    /// Number of samples.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Smallest sample, if any.
    pub fn min(&self) -> Option<u64> {
        (self.count > 0).then_some(self.min)
    }

    /// Largest sample, if any.
    pub fn max(&self) -> Option<u64> {
        (self.count > 0).then_some(self.max)
    }

    /// Mean in microseconds (rounded), if any samples.
    pub fn mean(&self) -> Option<u64> {
        let n = u128::from(self.count);
        let mean = self.sum.checked_add(n / 2)?.checked_div(n)?;
        u64::try_from(mean).ok()
    }

    /// The value at percentile `p` (0-100): the upper edge of the bucket that
    /// holds the sample of that rank, kept within the exact min and max.
    pub fn percentile(&self, p: f64) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        let p = if p.is_finite() { p.clamp(0.0, 100.0) } else { 100.0 };
        let rank = ((p / 100.0 * self.count as f64).ceil() as u64).clamp(1, self.count);
        let mut seen = 0u64;
        for (i, &n) in self.buckets.iter().enumerate() {
            seen = seen.saturating_add(n);
            if seen >= rank {
                return Some(upper(i).clamp(self.min, self.max));
            }
        }
        Some(self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_has_no_statistics() {
        let h = Histogram::default();
        assert_eq!((h.count(), h.min(), h.max(), h.mean(), h.percentile(50.0)), (0, None, None, None, None));
    }

    #[test]
    fn exact_for_small_values() {
        let mut h = Histogram::default();
        for v in 1..=50 {
            h.record(v);
        }
        assert_eq!((h.min(), h.max(), h.count()), (Some(1), Some(50), 50));
        assert_eq!(h.mean(), Some(26)); // 25.5 rounds up
        assert_eq!(h.percentile(50.0), Some(25));
        assert_eq!(h.percentile(95.0), Some(48));
        assert_eq!(h.percentile(99.0), Some(50));
        assert_eq!(h.percentile(100.0), Some(50));
        assert_eq!(h.percentile(0.0), Some(1));
    }

    #[test]
    fn large_values_are_within_three_percent() {
        let mut h = Histogram::default();
        for v in 1..=10_000u64 {
            h.record(v * 100); // 100 us .. 1 s
        }
        for (p, want) in [(50.0, 500_000.0), (95.0, 950_000.0), (99.0, 990_000.0)] {
            let got = h.percentile(p).unwrap() as f64;
            assert!((got - want).abs() / want < 0.035, "p{p}: {got} vs {want}");
        }
        assert_eq!(h.max(), Some(1_000_000));
        assert_eq!(h.min(), Some(100));
    }

    #[test]
    fn bucket_upper_edge_covers_its_values() {
        for v in [0u64, 1, 63, 64, 65, 127, 128, 1000, 123_456, 1 << 40] {
            let i = index(v);
            assert!(upper(i) >= v, "upper({i}) < {v}");
            if i > 0 {
                assert!(upper(i - 1) < v, "bucket {} overlaps {v}", i - 1);
            }
        }
        assert!(index(u64::MAX) < BUCKETS);
    }

    #[test]
    fn merge_adds_up() {
        let (mut a, mut b) = (Histogram::default(), Histogram::default());
        a.record(10);
        b.record(1000);
        a.merge(&b);
        assert_eq!((a.count(), a.min(), a.max()), (2, Some(10), Some(1000)));
        assert_eq!(a.percentile(100.0), Some(1000));
    }

    #[test]
    fn single_sample_percentiles_equal_the_sample() {
        let mut h = Histogram::default();
        h.record(12_345);
        assert_eq!(h.percentile(1.0), Some(12_345));
        assert_eq!(h.percentile(99.0), Some(12_345));
    }
}
