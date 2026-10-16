//! Fault injection: builders for malformed frames, and the names of the
//! fault kinds (including the two that send valid requests in an unusual
//! way: coalesced and split).

use crate::request::Request;
use crate::rng::Rng;
use ananasar_core::protocol;

/// One kind of injected fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaultKind {
    /// Fewer bytes than an MBAP header, then silence.
    ShortHeader,
    /// A non-zero protocol id.
    BadProtocol,
    /// A length field that disagrees with the bytes sent.
    BadLength,
    /// A correct length field but fewer bytes than it promises.
    TruncatedPdu,
    /// A PDU larger than the 253 bytes Modbus allows.
    OversizedPdu,
    /// Several valid requests written in one segment.
    Coalesced,
    /// One valid request written as two segments with a delay between.
    Split,
}

impl FaultKind {
    /// Every kind, in report order.
    pub const ALL: [Self; 7] = [
        Self::ShortHeader,
        Self::BadProtocol,
        Self::BadLength,
        Self::TruncatedPdu,
        Self::OversizedPdu,
        Self::Coalesced,
        Self::Split,
    ];

    /// The name used in reports and in the CLI flag `--fault-<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Self::ShortHeader => "short-header",
            Self::BadProtocol => "bad-protocol",
            Self::BadLength => "bad-length",
            Self::TruncatedPdu => "truncated-pdu",
            Self::OversizedPdu => "oversized-pdu",
            Self::Coalesced => "coalesced",
            Self::Split => "split",
        }
    }

    /// Whether the bytes sent are not a valid request.
    pub fn is_malformed(self) -> bool {
        !matches!(self, Self::Coalesced | Self::Split)
    }
}

fn set(frame: &mut [u8], offset: usize, bytes: &[u8]) {
    let end = offset.saturating_add(bytes.len());
    if let Some(dst) = frame.get_mut(offset..end) {
        dst.copy_from_slice(bytes);
    }
}

/// The bytes of a malformed frame derived from the valid `req`. Only the
/// five malformed kinds are meaningful; the others return the valid frame.
pub fn malformed(kind: FaultKind, req: &Request, rng: &mut Rng) -> Vec<u8> {
    let mut frame = req.frame.clone();
    match kind {
        FaultKind::ShortHeader => {
            let n = usize::from(rng.range_u16(1, 6));
            frame.truncate(n);
        }
        FaultKind::BadProtocol => {
            let id = rng.range_u16(1, u16::MAX);
            set(&mut frame, 2, &id.to_be_bytes());
        }
        FaultKind::BadLength => {
            let actual = protocol::be_u16(&frame, 4).unwrap_or(0);
            // Stay below 256 so the length's high byte is still zero and the
            // frame fails on the length check itself; flipping bit 0 makes
            // the value differ from the real one.
            let wrong = rng.range_u16(0, 255);
            let wrong = if wrong == actual { actual ^ 1 } else { wrong };
            set(&mut frame, 4, &wrong.to_be_bytes());
        }
        FaultKind::TruncatedPdu => {
            let data = u16::try_from(frame.len().saturating_sub(8)).unwrap_or(0);
            let cut = usize::from(rng.range_u16(1, data.max(1)));
            frame.truncate(frame.len().saturating_sub(cut).max(8));
        }
        FaultKind::OversizedPdu => {
            let n = usize::from(rng.range_u16(254, 400));
            let data: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
            frame = protocol::build(req.txn, req.unit, req.fc, &data);
        }
        FaultKind::Coalesced | FaultKind::Split => {}
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mix;
    use crate::request::generate;
    use ananasar_core::protocol::FrameError;

    fn sample(seed: u64) -> (Request, Rng) {
        let mut rng = Rng::derive(seed, 0);
        let mix = Mix { weights: [1, 1, 1, 1, 1, 1], ..Mix::default() };
        (generate(&mix, &mut rng, 7, 1), rng)
    }

    #[test]
    fn valid_baseline_frames_correctly() {
        for seed in 0..50 {
            let (req, _) = sample(seed);
            assert!(protocol::frame(&req.frame).is_ok());
        }
    }

    #[test]
    fn short_header_is_shorter_than_an_mbap_header() {
        for seed in 0..50 {
            let (req, mut rng) = sample(seed);
            let f = malformed(FaultKind::ShortHeader, &req, &mut rng);
            assert!((1..=6).contains(&f.len()), "{} bytes", f.len());
            assert_eq!(protocol::frame(&f), Err(FrameError::TooShort));
            assert_eq!(f, req.frame[..f.len()]);
        }
    }

    #[test]
    fn wrong_protocol_id_is_rejected_by_the_header_check() {
        for seed in 0..50 {
            let (req, mut rng) = sample(seed);
            let f = malformed(FaultKind::BadProtocol, &req, &mut rng);
            assert_eq!(f.len(), req.frame.len());
            assert_eq!(protocol::frame(&f), Err(FrameError::BadHeader));
        }
    }

    #[test]
    fn wrong_length_disagrees_with_the_bytes_sent() {
        for seed in 0..50 {
            let (req, mut rng) = sample(seed);
            let f = malformed(FaultKind::BadLength, &req, &mut rng);
            assert_eq!(f.len(), req.frame.len());
            assert_eq!(protocol::frame(&f), Err(FrameError::BadLength));
        }
    }

    #[test]
    fn truncated_pdu_keeps_the_header_but_loses_bytes() {
        for seed in 0..50 {
            let (req, mut rng) = sample(seed);
            let f = malformed(FaultKind::TruncatedPdu, &req, &mut rng);
            assert!(f.len() < req.frame.len() && f.len() >= 8);
            assert_eq!(f[..6], req.frame[..6]); // the length still promises the full PDU
            assert_eq!(protocol::frame(&f), Err(FrameError::BadLength));
        }
    }

    #[test]
    fn oversized_pdu_exceeds_253_bytes_with_a_consistent_header() {
        for seed in 0..50 {
            let (req, mut rng) = sample(seed);
            let f = malformed(FaultKind::OversizedPdu, &req, &mut rng);
            // The length field is consistent with the bytes sent, but it needs
            // more than one byte, which the length check of a strict receiver rejects.
            let declared = usize::from(protocol::be_u16(&f, 4).unwrap());
            assert_eq!(declared, f.len() - 6);
            assert_eq!((protocol::be_u16(&f, 0), f[7]), (Some(req.txn), req.fc));
            assert!(declared - 1 > 253, "PDU is {} bytes", declared - 1);
            assert_eq!(protocol::frame(&f), Err(FrameError::BadHeader));
            assert!(f.len() < 600); // small enough for our own server to read
        }
    }

    #[test]
    fn faults_are_deterministic_for_a_seed() {
        let (req, mut a) = sample(9);
        let mut b = a.clone();
        for kind in FaultKind::ALL {
            assert_eq!(malformed(kind, &req, &mut a), malformed(kind, &req, &mut b));
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = FaultKind::ALL.iter().map(|k| k.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), FaultKind::ALL.len());
    }
}
