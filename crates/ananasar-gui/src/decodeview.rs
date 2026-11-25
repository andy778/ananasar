//! View > Decode Registers…, an Ananasar addition: the registers at one
//! address decoded as 32- and 64-bit values in all four byte orders side by
//! side, so the order a device uses can be read off at a glance.

use crate::embed::SharedRegs;
use ananasar_core::registers::Bank;
use ananasar_core::sync::lock;
use ananasar_core::typed::WordOrder;
use eframe::egui::{self, RichText};
use std::fmt::{Display, LowerExp};

const SIZE: [f32; 2] = [820.0, 370.0];
/// Gap between grid columns: wider than egui's default so that numbers in
/// neighbouring columns (u32 next to i32) do not read as one.
const COLUMN_GAP: [f32; 2] = [20.0, 4.0];

pub struct DecodeView {
    addr_text: String,
    bank: Bank,
}

impl Default for DecodeView {
    fn default() -> Self {
        Self { addr_text: "0".into(), bank: Bank::Holding }
    }
}

/// The digit form of an order's `letters` (as the Set Register Value window
/// writes it for 32-bit values): which wire byte (0 = high byte of the first
/// register) lands in each position of the value, most significant first.
fn digits(letters: &str) -> String {
    ('A'..='H')
        .take(letters.len())
        .filter_map(|c| letters.find(c))
        .map(|wire| wire.to_string())
        .collect::<Vec<_>>()
        .join("-")
}

/// A float in plain notation, or in exponent notation when plain would be
/// very long; NaN and infinities are shown as such.
fn fmt_float<T: Display + LowerExp + Into<f64> + Copy>(v: T) -> String {
    let m = v.into().abs();
    if m.is_finite() && m != 0.0 && !(1e-4..1e9).contains(&m) { format!("{v:e}") } else { format!("{v}") }
}

/// The registers at `addr`, `addr + 1`, …; `None` past the end of the bank.
fn read<const N: usize>(regs: &SharedRegs, bank: Bank, addr: u16) -> Option<[u16; N]> {
    let r = lock(regs);
    let mut out = [0u16; N];
    let mut a = Some(addr);
    for slot in &mut out {
        let here = a?;
        *slot = r.get(bank, here);
        a = here.checked_add(1);
    }
    Some(out)
}

/// The two cells naming an order: its letters, and the endian name tools such
/// as Modbus Poll use. Both are shown because "little-endian" alone is read by
/// many device manuals as the word swap (CDAB), not the full reverse (DCBA).
/// `letters` is the order's 4- or 8-letter form, matching the grid's width.
fn order_label(ui: &mut egui::Ui, order: WordOrder, letters: &str) {
    let hover = format!("Wire bytes {} as the value, most significant first", digits(letters));
    ui.label(RichText::new(letters).monospace().strong()).on_hover_text(&hover);
    ui.label(order.endian_name()).on_hover_text(hover);
}

/// The value grid: the order's letters and name, then three value columns.
fn grid(id: &str) -> egui::Grid {
    egui::Grid::new(id).striped(true).num_columns(5).min_col_width(70.0).spacing(COLUMN_GAP)
}

fn cell(ui: &mut egui::Ui, text: String) {
    ui.label(RichText::new(text).monospace());
}

impl DecodeView {
    /// Show the window; `open` is cleared when it is closed.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, regs: &SharedRegs) {
        if !*open {
            return;
        }
        let closed = crate::dialog::show(ctx, "decode-registers", "Decode Registers", SIZE, 8, &mut |ui| {
            self.ui(ui, regs);
        });
        if closed {
            *open = false;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, regs: &SharedRegs) {
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.bank, Bank::Input, "Input");
            ui.radio_value(&mut self.bank, Bank::Holding, "Holding");
            ui.add_space(12.0);
            let label = ui.label("Start address");
            ui.add(egui::TextEdit::singleline(&mut self.addr_text).desired_width(60.0)).labelled_by(label.id);
        });
        let Ok(addr) = self.addr_text.trim().parse::<u16>() else {
            ui.label("Enter an address from 0 to 65535.");
            return;
        };

        ui.add_space(6.0);
        if let Some([r0]) = read::<1>(regs, self.bank, addr) {
            let signed = i16::from_be_bytes(r0.to_be_bytes());
            cell(ui, format!("Register {addr}: u16 {r0}   i16 {signed}   hex 0x{r0:04X}"));
        }

        ui.add_space(8.0);
        ui.strong("32-bit (2 registers)");
        let pair = read::<2>(regs, self.bank, addr);
        grid("decode32").show(ui, |ui| {
            for h in ["Order", "", "u32", "i32", "f32"] {
                ui.strong(h);
            }
            ui.end_row();
            for order in WordOrder::ALL {
                order_label(ui, order, order.letters());
                match pair {
                    Some(p) => {
                        let v = order.decode32(p);
                        cell(ui, v.u.to_string());
                        cell(ui, v.i.to_string());
                        cell(ui, fmt_float(v.f));
                    }
                    None => {
                        for _ in 0..3 {
                            cell(ui, "—".into());
                        }
                    }
                }
                ui.end_row();
            }
        });

        ui.add_space(8.0);
        ui.strong("64-bit (4 registers)");
        let quad = read::<4>(regs, self.bank, addr);
        grid("decode64").show(ui, |ui| {
            for h in ["Order", "", "u64", "i64", "f64"] {
                ui.strong(h);
            }
            ui.end_row();
            for order in WordOrder::ALL {
                order_label(ui, order, order.letters64());
                match quad {
                    Some(q) => {
                        let v = order.decode64(q);
                        cell(ui, v.u.to_string());
                        cell(ui, v.i.to_string());
                        cell(ui, fmt_float(v.f));
                    }
                    None => {
                        for _ in 0..3 {
                            cell(ui, "—".into());
                        }
                    }
                }
                ui.end_row();
            }
        });
        // The registers can change under the window (the server keeps serving).
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ananasar_core::registers::RegisterFile;
    use std::sync::{Arc, Mutex};

    #[test]
    fn floats_switch_to_exponent_only_when_long() {
        assert_eq!(fmt_float(1.5f32), "1.5");
        assert_eq!(fmt_float(0.0f64), "0");
        assert_eq!(fmt_float(f32::NAN), "NaN");
        assert_eq!(fmt_float(f64::NEG_INFINITY), "-inf");
        assert_eq!(fmt_float(1.0e20f64), "1e20");
        assert_eq!(fmt_float(-2.5e-7f32), "-2.5e-7");
    }

    #[test]
    fn digit_forms_match_the_set_register_value_window() {
        // Bytes 01 02 03 04 on the wire, decoded in each order, spell out the digit form.
        for order in WordOrder::ALL {
            let v = order.decode32([0x0001, 0x0203]).u.to_be_bytes();
            let spelled = v.iter().map(u8::to_string).collect::<Vec<_>>().join("-");
            assert_eq!(spelled, digits(order.letters()), "{}", order.letters());
        }
        assert_eq!(digits("CDAB"), "2-3-0-1");
    }

    #[test]
    fn digit_forms_match_the_64_bit_decoding() {
        // Bytes 00 01 … 07 on the wire, decoded in each order, spell out the digit form.
        for order in WordOrder::ALL {
            let v = order.decode64([0x0001, 0x0203, 0x0405, 0x0607]).u.to_be_bytes();
            let spelled = v.iter().map(u8::to_string).collect::<Vec<_>>().join("-");
            assert_eq!(spelled, digits(order.letters64()), "{}", order.letters64());
        }
        assert_eq!(digits("GHEFCDAB"), "6-7-4-5-2-3-0-1");
    }

    #[test]
    fn reads_stop_at_the_end_of_the_bank() {
        let regs: SharedRegs = Arc::new(Mutex::new(RegisterFile::new()));
        lock(&regs).set_holding(0xFFFE, 7);
        assert_eq!(read::<2>(&regs, Bank::Holding, 0xFFFE), Some([7, 0]));
        assert_eq!(read::<2>(&regs, Bank::Holding, 0xFFFF), None);
        assert_eq!(read::<4>(&regs, Bank::Holding, 0xFFFD), None);
    }
}
