//! File → Open… / Save As…: load and save the register values and the Values
//! list position as a `*.ananas` file, through the system's own file dialog.

use ananasar_core::ananas_file::{self, view_from};
use ananasar_core::registers::{Bank, RegisterFile};
use ananasar_core::sync::lock;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Open,
    Save,
}

/// A pending Open or Save request.
#[derive(Default)]
pub struct FileWindow {
    mode: Option<Mode>,
}

impl FileWindow {
    pub fn open(&mut self) {
        self.mode = Some(Mode::Open);
    }

    pub fn save(&mut self) {
        self.mode = Some(Mode::Save);
    }

    /// Run the requested file dialog, if any. Loading replaces all register
    /// values and moves `start_address` to the first address of the shown bank's
    /// list. Returns an error message for the caller to show.
    pub fn ui(&mut self, regs: &Arc<Mutex<RegisterFile>>, bank: Bank, start_address: &mut u16) -> Option<String> {
        let mode = self.mode.take()?;
        let dialog = rfd::FileDialog::new().add_filter("Ananasar variable list [*.ananas]", &["ananas"]);
        let picked = match mode {
            Mode::Open => dialog.set_title("Open Ananasar variable list").pick_file(),
            Mode::Save => dialog.set_title("Save Ananasar variable list").set_file_name("registers.ananas").save_file(),
        }?;
        Self::run(mode, &picked.to_string_lossy(), regs, bank, start_address).err()
    }

    fn run(mode: Mode, path: &str, regs: &Arc<Mutex<RegisterFile>>, bank: Bank, start_address: &mut u16) -> Result<(), String> {
        match mode {
            Mode::Save => {
                let view = view_from(*start_address);
                let text = ananas_file::save(&lock(regs), &view, &view, &utc_stamp());
                std::fs::write(path, text).map_err(|e| format!("cannot save: {e}"))
            }
            Mode::Open => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("cannot open: {e}"))?;
                let mut loaded_regs = RegisterFile::new();
                let loaded = ananas_file::load(&text, &mut loaded_regs).map_err(|e| format!("{path}: {e}"))?;
                *lock(regs) = loaded_regs;
                let view = if bank == Bank::Input { &loaded.input_view } else { &loaded.holding_view };
                if let Some(&first) = view.first() {
                    *start_address = first;
                }
                Ok(())
            }
        }
    }
}

/// Current UTC time as `YYYY-MM-DD HH:MM:SS UTC` (std has no calendar).
// `secs / 86_400` is below 2^48, so the date maths cannot overflow an i64.
#[allow(clippy::arithmetic_side_effects, reason = "bounded date arithmetic, see above")]
fn utc_stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (i64::try_from(secs / 86_400).unwrap_or(0), secs % 86_400);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn stamp_has_the_expected_shape() {
        let s = super::utc_stamp();
        assert_eq!(s.len(), "2026-10-14 10:15:00 UTC".len());
        assert!(s.starts_with("20"));
    }
}
