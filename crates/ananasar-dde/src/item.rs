//! The pure parts of the DDE server: which items exist, how a poke is read and
//! how a value is formatted. No I/O and no unsafe, so it is tested everywhere.

use ananasar_core::registers::Bank;

/// What the DDE dialog offers: which registers are served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// All input and all holding registers.
    AllBoth,
    /// All input registers.
    Input,
    /// A range of input registers.
    InputRange,
    /// All holding registers.
    Holding,
    /// A range of holding registers.
    HoldingRange,
    /// The same range in both banks.
    BothRange,
}

impl Scope {
    /// Whether the scope needs the From/To range.
    pub fn has_range(self) -> bool {
        matches!(
            self,
            Self::InputRange | Self::HoldingRange | Self::BothRange
        )
    }
}

/// The registers a running DDE server answers for, per bank.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activation {
    input: Option<(u16, u16)>,
    holding: Option<(u16, u16)>,
}

/// The From/To range was empty, not a number in 0..=65535, or backwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeError;

impl std::fmt::Display for RangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Range error (0 - 65535) - check values")
    }
}

impl std::error::Error for RangeError {}

/// Parses the From/To boxes of the dialog.
pub fn parse_range(from: &str, to: &str) -> Result<(u16, u16), RangeError> {
    let from: u16 = from.trim().parse().map_err(|_| RangeError)?;
    let to: u16 = to.trim().parse().map_err(|_| RangeError)?;
    if from > to {
        return Err(RangeError);
    }
    Ok((from, to))
}

impl Activation {
    /// Builds the activation for a dialog choice. `from` and `to` are only
    /// read for the range scopes.
    pub fn new(scope: Scope, from: &str, to: &str) -> Result<Self, RangeError> {
        let all = (0, u16::MAX);
        let range = if scope.has_range() {
            parse_range(from, to)?
        } else {
            all
        };
        Ok(match scope {
            Scope::AllBoth => Self {
                input: Some(all),
                holding: Some(all),
            },
            Scope::Input => Self {
                input: Some(all),
                holding: None,
            },
            Scope::InputRange => Self {
                input: Some(range),
                holding: None,
            },
            Scope::Holding => Self {
                input: None,
                holding: Some(all),
            },
            Scope::HoldingRange => Self {
                input: None,
                holding: Some(range),
            },
            Scope::BothRange => Self {
                input: Some(range),
                holding: Some(range),
            },
        })
    }

    /// The served range of `bank`, if any.
    pub fn range(&self, bank: Bank) -> Option<(u16, u16)> {
        match bank {
            Bank::Input => self.input,
            Bank::Holding => self.holding,
        }
    }

    /// Whether register `addr` of `bank` is served.
    pub fn covers(&self, bank: Bank, addr: u16) -> bool {
        self.range(bank)
            .is_some_and(|(from, to)| (from..=to).contains(&addr))
    }
}

/// The DDE topic of a bank.
pub fn topic_name(bank: Bank) -> &'static str {
    match bank {
        Bank::Input => "input",
        Bank::Holding => "holding",
    }
}

/// Maps a topic to its bank. DDE names are not case-sensitive.
pub fn parse_topic(topic: &str) -> Option<Bank> {
    [Bank::Input, Bank::Holding]
        .into_iter()
        .find(|&b| topic.eq_ignore_ascii_case(topic_name(b)))
}

/// The DDE item name of register `addr`.
pub fn item_name(addr: u16) -> String {
    format!("r{addr}")
}

/// Maps an item name (`r` and the register number in decimal) to its address.
pub fn parse_item(item: &str) -> Option<u16> {
    let digits = item.strip_prefix('r')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A register value as DDE text: decimal and a CRLF, the way spreadsheets expect a cell.
pub fn format_value(value: u16) -> String {
    format!("{value}\r\n")
}

/// Reads the text of a poke. Decimal, `$` or `0x` hexadecimal; surrounding
/// white space and the terminating NUL are ignored. Values above 65535 wrap to
/// 16 bits; anything that is not a non-negative integer is not a value.
pub fn parse_poke(text: &str) -> Option<u16> {
    let t = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    let (digits, radix) = match t
        .strip_prefix('$')
        .or_else(|| t.strip_prefix("0x"))
        .or_else(|| t.strip_prefix("0X"))
    {
        Some(hex) => (hex, 16),
        None => (t, 10),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let wide = u64::from_str_radix(digits, radix).ok()?;
    u16::try_from(wide & 0xFFFF).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_lower_case_r_and_a_decimal_number() {
        assert_eq!(parse_item("r0"), Some(0));
        assert_eq!(parse_item("r933"), Some(933));
        assert_eq!(parse_item("r65535"), Some(65535));
        for bad in [
            "r65536", "R3", "3", "r", "r-1", "r 3", "r3 ", "r0x10", "reg3", "",
        ] {
            assert_eq!(parse_item(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn item_names_round_trip() {
        for addr in [0, 7, 8862, u16::MAX] {
            assert_eq!(parse_item(&item_name(addr)), Some(addr));
        }
    }

    #[test]
    fn topics_ignore_case() {
        assert_eq!(parse_topic("input"), Some(Bank::Input));
        assert_eq!(parse_topic("Holding"), Some(Bank::Holding));
        assert_eq!(parse_topic("coil"), None);
        assert_eq!(parse_topic(""), None);
    }

    #[test]
    fn values_end_in_crlf() {
        assert_eq!(format_value(0), "0\r\n");
        assert_eq!(format_value(65535), "65535\r\n");
    }

    #[test]
    fn pokes_accept_decimal_and_hex() {
        assert_eq!(parse_poke("1234"), Some(1234));
        assert_eq!(parse_poke(" 8\r\n\0"), Some(8));
        assert_eq!(parse_poke("0x10"), Some(16));
        assert_eq!(parse_poke("$1F"), Some(31));
        assert_eq!(parse_poke("65535"), Some(65535));
    }

    #[test]
    fn pokes_wrap_to_sixteen_bits() {
        assert_eq!(parse_poke("70000"), Some(4464));
        assert_eq!(parse_poke("65536"), Some(0));
    }

    #[test]
    fn pokes_reject_non_integers() {
        for bad in [
            "",
            "abc",
            "-5",
            "3.7",
            "1e2",
            "0x",
            "$",
            "12 34",
            "99999999999999999999999",
        ] {
            assert_eq!(parse_poke(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn ranges_must_be_ordered_numbers_in_16_bits() {
        assert_eq!(parse_range("0", "9"), Ok((0, 9)));
        assert_eq!(parse_range(" 5 ", "5"), Ok((5, 5)));
        for (from, to) in [
            ("", ""),
            ("0", ""),
            ("9", "0"),
            ("0", "65536"),
            ("-1", "5"),
            ("a", "b"),
        ] {
            assert_eq!(parse_range(from, to), Err(RangeError), "{from:?}..{to:?}");
        }
    }

    #[test]
    fn activation_follows_the_scope() {
        let a = Activation::new(Scope::HoldingRange, "0", "9").unwrap_or_default();
        assert!(a.covers(Bank::Holding, 9));
        assert!(!a.covers(Bank::Holding, 10));
        assert!(!a.covers(Bank::Input, 0));

        let all = Activation::new(Scope::AllBoth, "", "").unwrap_or_default();
        assert!(all.covers(Bank::Input, 65535));
        assert!(all.covers(Bank::Holding, 0));

        let input = Activation::new(Scope::Input, "x", "y").unwrap_or_default();
        assert!(input.covers(Bank::Input, 1234));
        assert!(!input.covers(Bank::Holding, 1234));

        let both = Activation::new(Scope::BothRange, "10", "20").unwrap_or_default();
        assert_eq!(both.range(Bank::Input), Some((10, 20)));
        assert_eq!(both.range(Bank::Holding), Some((10, 20)));
        assert_eq!(Activation::new(Scope::InputRange, "", ""), Err(RangeError));
    }
}
