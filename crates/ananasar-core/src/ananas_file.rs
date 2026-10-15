//! The `*.ananas` register file: a plain-text snapshot of the register values
//! and of the addresses shown in the Values list.
//!
//! Layout (one item per line, `//` lines are comments and ignored on load):
//!
//! ```text
//! #Ananas 1.8.2
//! //
//! 1000005|1234        input register 5 = 1234   (1000000 + address)
//! 2000010|55          holding register 10 = 55  (2000000 + address)
//! #EOR                end of registers; only non-zero registers are stored
//! <26 addresses>      the input Values list, one decimal address per line
//! #EOVLI
//! <26 addresses>      the holding Values list
//! #EOVLH
//! //
//! // 3 (1/2)          total (input/holding)
//! // <saved-at text>
//! #EOF
//! ```
//!
//! Loading clears every register first, as the original does.

use crate::registers::{BANK_LEN, Bank, RegisterFile};
use std::fmt;

/// Number of addresses stored for each Values list.
pub const VIEW_ROWS: usize = 26;
const HEADER: &str = "#Ananas 1.8.2";
const INPUT_BASE: u32 = 1_000_000;
const HOLDING_BASE: u32 = 2_000_000;

/// What a loaded file contains besides the register values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// Addresses of the input Values list (may be empty if the file has none).
    pub input_view: Vec<u16>,
    /// Addresses of the holding Values list (may be empty if the file has none).
    pub holding_view: Vec<u16>,
}

/// A problem found while loading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError {
    /// 1-based line number.
    pub line: usize,
    /// What was wrong.
    pub reason: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

impl std::error::Error for LoadError {}

/// Addresses `start..start + VIEW_ROWS`, clamped at the top of the address space.
pub fn view_from(start: u16) -> Vec<u16> {
    (0..VIEW_ROWS).map(|i| u16::try_from(usize::from(start).saturating_add(i).min(BANK_LEN.saturating_sub(1))).unwrap_or(u16::MAX)).collect()
}

/// Serialise `regs` and the two Values lists. `saved_at` is free text written
/// in a trailing comment (the caller supplies the time; core has no clock).
pub fn save(regs: &RegisterFile, input_view: &[u16], holding_view: &[u16], saved_at: &str) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push_str("\n//\n");
    let (mut inputs, mut holdings) = (0usize, 0usize);
    for (bank, base, count) in [(Bank::Input, INPUT_BASE, &mut inputs), (Bank::Holding, HOLDING_BASE, &mut holdings)] {
        for (addr, &v) in regs.bank(bank).iter().enumerate() {
            if v != 0 {
                out.push_str(&format!("{}|{v}\n", base.saturating_add(u32::try_from(addr).unwrap_or(0))));
                *count = count.saturating_add(1);
            }
        }
    }
    out.push_str("#EOR\n");
    for a in input_view.iter().take(VIEW_ROWS) {
        out.push_str(&format!("{a}\n"));
    }
    out.push_str("#EOVLI\n");
    for a in holding_view.iter().take(VIEW_ROWS) {
        out.push_str(&format!("{a}\n"));
    }
    out.push_str("#EOVLH\n//\n");
    out.push_str(&format!("// {} ({inputs}/{holdings})\n// {saved_at}\n#EOF\n", inputs.saturating_add(holdings)));
    out
}

/// Parse a file into `regs` (cleared first) and return the Values lists.
/// On error `regs` is left cleared or partly filled; callers should load into
/// a scratch [`RegisterFile`] and swap it in on success.
pub fn load(text: &str, regs: &mut RegisterFile) -> Result<Loaded, LoadError> {
    *regs = RegisterFile::new();
    let mut section = 0; // 0 registers, 1 input list, 2 holding list
    let mut loaded = Loaded { input_view: Vec::new(), holding_view: Vec::new() };
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let err = |reason: &str| LoadError { line: i.saturating_add(1), reason: reason.into() };
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('#') {
            match line {
                "#EOR" => section = 1,
                "#EOVLI" => section = 2,
                "#EOVLH" => section = 3,
                "#EOF" => break,
                _ => {} // the "#Ananas x.y.z" header and unknown markers
            }
            continue;
        }
        match section {
            0 => {
                let (num, val) = line.split_once('|').ok_or_else(|| err("expected <number>|<value>"))?;
                let num: u32 = num.trim().parse().map_err(|_| err("bad register number"))?;
                let val: u16 = val.trim().parse().map_err(|_| err("bad register value"))?;
                let (bank, base) = if num >= HOLDING_BASE { (Bank::Holding, HOLDING_BASE) } else { (Bank::Input, INPUT_BASE) };
                let addr = num.checked_sub(base).filter(|a| (*a as usize) < BANK_LEN).ok_or_else(|| err("register address out of range"))?;
                regs.set(bank, u16::try_from(addr).map_err(|_| err("register address out of range"))?, val);
            }
            1 | 2 => {
                let addr: u16 = line.parse().map_err(|_| err("bad list address"))?;
                if section == 1 { loaded.input_view.push(addr) } else { loaded.holding_view.push(addr) }
            }
            _ => {}
        }
    }
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_registers_and_lists() {
        let mut regs = RegisterFile::new();
        regs.set(Bank::Input, 5, 1234);
        regs.set(Bank::Holding, 10, 55);
        regs.set(Bank::Holding, 65535, 1);
        let text = save(&regs, &view_from(0), &view_from(100), "test");
        assert!(text.starts_with("#Ananas 1.8.2\n//\n1000005|1234\n2000010|55\n2065535|1\n#EOR\n"));
        assert!(text.contains("// 3 (1/2)\n// test\n#EOF\n"));
        let mut back = RegisterFile::new();
        back.set(Bank::Holding, 7, 9); // must be cleared by load
        let loaded = load(&text, &mut back).unwrap();
        assert_eq!(back.get(Bank::Input, 5), 1234);
        assert_eq!(back.get(Bank::Holding, 10), 55);
        assert_eq!(back.get(Bank::Holding, 65535), 1);
        assert_eq!(back.get(Bank::Holding, 7), 0);
        assert_eq!(loaded.input_view, view_from(0));
        assert_eq!(loaded.holding_view, view_from(100));
        assert_eq!(loaded.holding_view.len(), VIEW_ROWS);
    }

    #[test]
    fn only_nonzero_registers_are_stored() {
        let text = save(&RegisterFile::new(), &[], &[], "x");
        assert!(text.contains("#EOR\n#EOVLI\n#EOVLH\n"));
        assert!(text.contains("// 0 (0/0)"));
    }

    #[test]
    fn rejects_bad_lines_with_a_line_number() {
        let mut regs = RegisterFile::new();
        assert_eq!(load("#Ananas 1.8.2\n1000005-3\n", &mut regs).unwrap_err().line, 2);
        assert!(load("1000005|70000\n", &mut regs).is_err());
        assert!(load("3000000|1\n", &mut regs).is_err()); // above the holding range
        assert!(load("999999|1\n", &mut regs).is_err());
    }

    #[test]
    fn view_is_clamped_at_the_top() {
        let v = view_from(65530);
        assert_eq!(v.len(), VIEW_ROWS);
        assert_eq!(*v.last().unwrap(), 65535);
    }
}
