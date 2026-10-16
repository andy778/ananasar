//! A tiny deterministic generator (SplitMix64), so a seed reproduces a run's
//! request sequence without a dependency.

/// SplitMix64 state.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator for one worker: `stream` separates workers sharing a seed.
    pub fn derive(seed: u64, stream: u64) -> Self {
        let mut r = Self(seed ^ stream.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        r.next_u64();
        r
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (0 when `n` is 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64().checked_rem(n).unwrap_or(0)
    }

    /// A value in `lo..=hi`; `hi` below `lo` yields `lo`.
    pub fn range_u16(&mut self, lo: u16, hi: u16) -> u16 {
        let span = u64::from(hi.saturating_sub(lo)).saturating_add(1);
        let off = u16::try_from(self.below(span)).unwrap_or(0);
        lo.saturating_add(off)
    }

    /// A random byte.
    pub fn byte(&mut self) -> u8 {
        u8::try_from(self.next_u64() & 0xFF).unwrap_or(0)
    }

    /// A uniform value in `[0, 100)`.
    pub fn percent(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(seed: u64, stream: u64) -> Vec<u64> {
        let mut r = Rng::derive(seed, stream);
        (0..8).map(|_| r.next_u64()).collect()
    }

    #[test]
    fn same_seed_same_sequence_and_streams_differ() {
        assert_eq!(take(42, 0), take(42, 0));
        assert_ne!(take(42, 0), take(42, 1));
        assert_ne!(take(42, 0), take(43, 0));
    }

    #[test]
    fn ranges_stay_inside() {
        let mut r = Rng::derive(1, 1);
        for _ in 0..1000 {
            let v = r.range_u16(5, 9);
            assert!((5..=9).contains(&v));
            assert!((0.0..100.0).contains(&r.percent()));
        }
        assert_eq!(r.range_u16(7, 3), 7);
        assert_eq!(r.below(0), 0);
    }
}
