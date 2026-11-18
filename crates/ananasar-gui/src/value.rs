//! How register values are shown and typed in: unsigned or signed decimal,
//! or hex. Only the display changes; the registers always hold the 16 bits.

/// `v` as shown in the register list and the client log: four hex digits
/// when `hex` is on (it takes precedence), otherwise decimal, read as a
/// two's-complement signed number when `signed` is on.
pub fn fmt(v: u16, hex: bool, signed: bool) -> String {
    if hex {
        format!("{v:04X}")
    } else if signed {
        i16::from_be_bytes(v.to_be_bytes()).to_string()
    } else {
        v.to_string()
    }
}

/// A decimal register value: 0 to 65535, or a negative number from -32768
/// to -1, stored as its two's-complement bit pattern (-5 becomes 65531).
pub fn parse_decimal(s: &str) -> Option<u16> {
    let s = s.trim();
    if s.starts_with('-') { s.parse::<i16>().ok().map(|n| u16::from_be_bytes(n.to_be_bytes())) } else { s.parse().ok() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_show_as_unsigned_signed_or_hex() {
        assert_eq!(fmt(65531, false, false), "65531");
        assert_eq!(fmt(65531, false, true), "-5");
        assert_eq!(fmt(32767, false, true), "32767");
        assert_eq!(fmt(32768, false, true), "-32768");
        assert_eq!(fmt(65531, true, false), "FFFB");
        // Hex wins over signed.
        assert_eq!(fmt(65531, true, true), "FFFB");
    }

    #[test]
    fn negative_input_stores_twos_complement() {
        assert_eq!(parse_decimal("-5"), Some(65531));
        assert_eq!(parse_decimal(" -1 "), Some(0xFFFF));
        assert_eq!(parse_decimal("-32768"), Some(0x8000));
        assert_eq!(parse_decimal("65535"), Some(65535));
        assert_eq!(parse_decimal("0"), Some(0));
        for bad in ["-32769", "65536", "", "-", "5-", "1.5", "--5"] {
            assert_eq!(parse_decimal(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_value_round_trips_through_its_signed_text() {
        for v in 0..=u16::MAX {
            assert_eq!(parse_decimal(&fmt(v, false, true)), Some(v));
            assert_eq!(parse_decimal(&fmt(v, false, false)), Some(v));
        }
    }
}
