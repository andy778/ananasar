//! Typed views of consecutive 16-bit registers.
//!
//! The Modbus specification sends each register big-endian (high byte first)
//! but says nothing about how a value wider than 16 bits spreads over several
//! registers, so devices differ. This module decodes two or four registers as
//! 32- or 64-bit integers and IEEE 754 floats in each of the four common orders.

/// How the bytes of a multi-register value are arranged.
///
/// The letters name the bytes of a 32-bit value from most significant (`A`)
/// to least significant (`D`), in the order they arrive on the wire: `ABCD`
/// means the first register holds the high word, high byte first. The order is
/// two independent choices: whether the registers come in reverse order
/// (least significant word first) and whether the two bytes inside each
/// register are swapped. For 64-bit values the same two choices apply to all
/// four registers, so a reversed order reverses all four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordOrder {
    /// Big-endian: high word first, high byte first. The Modbus default.
    Abcd,
    /// Words reversed, bytes in each word as sent.
    Cdab,
    /// Words as sent, bytes in each word swapped.
    Badc,
    /// Words reversed and bytes swapped: fully little-endian.
    Dcba,
}

impl WordOrder {
    /// The four orders, the Modbus default first.
    pub const ALL: [WordOrder; 4] = [WordOrder::Abcd, WordOrder::Cdab, WordOrder::Badc, WordOrder::Dcba];

    /// The byte letters, such as `"CDAB"`.
    pub fn letters(self) -> &'static str {
        match self {
            WordOrder::Abcd => "ABCD",
            WordOrder::Cdab => "CDAB",
            WordOrder::Badc => "BADC",
            WordOrder::Dcba => "DCBA",
        }
    }

    /// The endian name many Modbus tools use for this order.
    pub fn endian_name(self) -> &'static str {
        match self {
            WordOrder::Abcd => "Big-endian",
            WordOrder::Cdab => "Little-endian byte swap",
            WordOrder::Badc => "Big-endian byte swap",
            WordOrder::Dcba => "Little-endian",
        }
    }

    fn reverses_words(self) -> bool {
        matches!(self, WordOrder::Cdab | WordOrder::Dcba)
    }

    fn swaps_bytes(self) -> bool {
        matches!(self, WordOrder::Badc | WordOrder::Dcba)
    }

    /// The registers rearranged so the most significant word comes first and
    /// each word is in its natural byte order.
    fn normalise<const N: usize>(self, mut regs: [u16; N]) -> [u16; N] {
        if self.reverses_words() {
            regs.reverse();
        }
        if self.swaps_bytes() {
            for r in &mut regs {
                *r = r.swap_bytes();
            }
        }
        regs
    }

    /// The four bytes of `regs`, most significant first.
    fn bytes32(self, regs: [u16; 2]) -> [u8; 4] {
        let [hi, lo] = self.normalise(regs);
        let ([a, b], [c, d]) = (hi.to_be_bytes(), lo.to_be_bytes());
        [a, b, c, d]
    }

    /// The eight bytes of `regs`, most significant first.
    fn bytes64(self, regs: [u16; 4]) -> [u8; 8] {
        let [w0, w1, w2, w3] = self.normalise(regs);
        let ([a, b], [c, d]) = (w0.to_be_bytes(), w1.to_be_bytes());
        let ([e, f], [g, h]) = (w2.to_be_bytes(), w3.to_be_bytes());
        [a, b, c, d, e, f, g, h]
    }

    /// Two registers, in the order they are addressed, read as 32-bit values.
    pub fn decode32(self, regs: [u16; 2]) -> Value32 {
        let b = self.bytes32(regs);
        Value32 { u: u32::from_be_bytes(b), i: i32::from_be_bytes(b), f: f32::from_be_bytes(b) }
    }

    /// Four registers, in the order they are addressed, read as 64-bit values.
    pub fn decode64(self, regs: [u16; 4]) -> Value64 {
        let b = self.bytes64(regs);
        Value64 { u: u64::from_be_bytes(b), i: i64::from_be_bytes(b), f: f64::from_be_bytes(b) }
    }
}

/// Two registers read as an unsigned, a signed and a floating-point 32-bit value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value32 {
    /// Unsigned integer.
    pub u: u32,
    /// Two's-complement signed integer.
    pub i: i32,
    /// IEEE 754 single precision.
    pub f: f32,
}

/// Four registers read as an unsigned, a signed and a floating-point 64-bit value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value64 {
    /// Unsigned integer.
    pub u: u64,
    /// Two's-complement signed integer.
    pub i: i64,
    /// IEEE 754 double precision.
    pub f: f64,
}

#[cfg(test)]
mod tests {
    use super::WordOrder::{self, Abcd, Badc, Cdab, Dcba};

    /// `bytes` (most significant first) as registers in `order`.
    fn regs32(order: WordOrder, bytes: [u8; 4]) -> [u16; 2] {
        let [a, b, c, d] = bytes;
        match order {
            Abcd => [u16::from_be_bytes([a, b]), u16::from_be_bytes([c, d])],
            Cdab => [u16::from_be_bytes([c, d]), u16::from_be_bytes([a, b])],
            Badc => [u16::from_be_bytes([b, a]), u16::from_be_bytes([d, c])],
            Dcba => [u16::from_be_bytes([d, c]), u16::from_be_bytes([b, a])],
        }
    }

    #[test]
    fn known_pair_in_each_order() {
        let regs = [0x0102, 0x0304];
        assert_eq!(Abcd.decode32(regs).u, 0x0102_0304);
        assert_eq!(Cdab.decode32(regs).u, 0x0304_0102);
        assert_eq!(Badc.decode32(regs).u, 0x0201_0403);
        assert_eq!(Dcba.decode32(regs).u, 0x0403_0201);
    }

    #[test]
    fn known_quad_in_each_order() {
        let regs = [0x0102, 0x0304, 0x0506, 0x0708];
        assert_eq!(Abcd.decode64(regs).u, 0x0102_0304_0506_0708);
        assert_eq!(Cdab.decode64(regs).u, 0x0708_0506_0304_0102);
        assert_eq!(Badc.decode64(regs).u, 0x0201_0403_0605_0807);
        assert_eq!(Dcba.decode64(regs).u, 0x0807_0605_0403_0201);
    }

    #[test]
    fn floats_round_trip_through_every_order() {
        for order in WordOrder::ALL {
            let regs = regs32(order, 1.5f32.to_be_bytes());
            assert_eq!(order.decode32(regs).f, 1.5, "{}", order.letters());
        }
        // 1.5f64 is 0x3FF8_0000_0000_0000.
        assert_eq!(Abcd.decode64([0x3FF8, 0, 0, 0]).f, 1.5);
        assert_eq!(Cdab.decode64([0, 0, 0, 0x3FF8]).f, 1.5);
        assert_eq!(Badc.decode64([0xF83F, 0, 0, 0]).f, 1.5);
        assert_eq!(Dcba.decode64([0, 0, 0, 0xF83F]).f, 1.5);
    }

    #[test]
    fn the_float_examples_from_the_issue() {
        assert_eq!(Abcd.decode32([0x3FC0, 0x0000]).f, 1.5);
        assert_eq!(Cdab.decode32([0x0000, 0x3FC0]).f, 1.5);
    }

    #[test]
    fn signed_values_use_twos_complement() {
        let v = Abcd.decode32([0xFFFF, 0xFFFE]);
        assert_eq!((v.u, v.i), (0xFFFF_FFFE, -2));
        assert_eq!(Cdab.decode32([0xFFFE, 0xFFFF]).i, -2);
        assert_eq!(Abcd.decode64([0x8000, 0, 0, 0]).i, i64::MIN);
        assert_eq!(Dcba.decode64([0xFFFF; 4]).i, -1);
    }

    #[test]
    fn nan_and_infinity_survive() {
        assert!(Abcd.decode32([0x7FC0, 0]).f.is_nan());
        assert_eq!(Abcd.decode32([0xFF80, 0]).f, f32::NEG_INFINITY);
        assert!(Abcd.decode64([0x7FF8, 0, 0, 0]).f.is_nan());
    }

    #[test]
    fn labels() {
        let letters: Vec<_> = WordOrder::ALL.iter().map(|o| o.letters()).collect();
        assert_eq!(letters, ["ABCD", "CDAB", "BADC", "DCBA"]);
        assert_eq!(Cdab.endian_name(), "Little-endian byte swap");
        assert_eq!(Badc.endian_name(), "Big-endian byte swap");
    }
}
