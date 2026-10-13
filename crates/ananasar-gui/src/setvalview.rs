//! Set-value view (`TFRMSETVAL`): edit one register by value or by its 16
//! individual bits, with a 32-bit Uint/Float interpretation. Operates directly
//! on the app's shared register file in-process (the original edits the shared
//! register model, and "server is fully operational during editing", §2.5).

use crate::embed::SharedRegs;
use ananasar_core::registers::Bank;
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    B0123,
    B1032,
    B2301,
    B3210,
}

impl ByteOrder {
    fn label(self) -> &'static str {
        match self {
            ByteOrder::B0123 => "0-1-2-3",
            ByteOrder::B1032 => "1-0-3-2",
            ByteOrder::B2301 => "2-3-0-1",
            ByteOrder::B3210 => "3-2-1-0",
        }
    }
    fn u32_of(self, r0: u16, r1: u16) -> u32 {
        let b = [(r0 >> 8) as u8, r0 as u8, (r1 >> 8) as u8, r1 as u8];
        let o = match self {
            ByteOrder::B0123 => [b[0], b[1], b[2], b[3]],
            ByteOrder::B1032 => [b[1], b[0], b[3], b[2]],
            ByteOrder::B2301 => [b[2], b[3], b[0], b[1]],
            ByteOrder::B3210 => [b[3], b[2], b[1], b[0]],
        };
        u32::from_be_bytes(o)
    }
}

pub struct SetValView {
    addr_text: String,
    bank: Bank,
    hex: bool,
    pending: u16,
    value_text: String,
    byte_order: ByteOrder,
}

impl Default for SetValView {
    fn default() -> Self {
        Self {
            addr_text: "0".into(),
            bank: Bank::Holding,
            hex: true,
            pending: 0,
            value_text: "0000".into(),
            byte_order: ByteOrder::B0123,
        }
    }
}

impl SetValView {
    fn fmt(&self, v: u16) -> String {
        if self.hex { format!("{v:04X}") } else { v.to_string() }
    }
    fn parse_value(&self, s: &str) -> Option<u16> {
        let s = s.trim();
        if self.hex {
            u16::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
        } else {
            s.parse().ok()
        }
    }
    fn set_pending(&mut self, v: u16) {
        self.pending = v;
        self.value_text = self.fmt(v);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, regs: &SharedRegs) {
        let addr = self.addr_text.trim().parse::<u16>().unwrap_or(0);
        let (current, next) = {
            let r = regs.lock().unwrap();
            (r.get(self.bank, addr), r.get(self.bank, addr.saturating_add(1)))
        };

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("Register:");
                ui.add(egui::TextEdit::singleline(&mut self.addr_text).desired_width(70.0));
                ui.radio_value(&mut self.bank, Bank::Holding, "Holding");
                ui.radio_value(&mut self.bank, Bank::Input, "Input");
            });
        });
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong("Current value:");
                ui.monospace(format!("dec {current}"));
                ui.monospace(format!("hex {current:04X}"));
                ui.monospace(format!("bin {current:016b}"));
            });
        });
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong("Edit value:");
                let resp = ui.add(egui::TextEdit::singleline(&mut self.value_text).desired_width(80.0));
                if resp.changed()
                    && let Some(v) = self.parse_value(&self.value_text.clone()) {
                        self.pending = v;
                    }
                ui.checkbox(&mut self.hex, "hex");
                if ui.button("⟵ load current").clicked() {
                    self.set_pending(current);
                }
            });
        });
        ui.group(|ui| {
            ui.label("Bitmask (MSB left)");
            egui::Grid::new("bits").spacing([2.0, 2.0]).show(ui, |ui| {
                for bit in (0..16).rev() {
                    let mut set = (self.pending >> bit) & 1 == 1;
                    if ui.checkbox(&mut set, "").changed() {
                        if set { self.pending |= 1 << bit; } else { self.pending &= !(1 << bit); }
                        self.value_text = self.fmt(self.pending);
                    }
                    if bit % 4 == 0 {
                        ui.end_row();
                    }
                }
            });
            ui.horizontal(|ui| {
                for bit in (0..16).rev() {
                    ui.add_sized([17.0, 12.0], egui::Label::new(egui::RichText::new(format!("{bit:>2}")).small().monospace()));
                }
            });
        });
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("32-bit:");
                egui::ComboBox::from_id_salt("bo")
                    .selected_text(self.byte_order.label())
                    .show_ui(ui, |ui| {
                        for bo in [ByteOrder::B0123, ByteOrder::B1032, ByteOrder::B2301, ByteOrder::B3210] {
                            ui.selectable_value(&mut self.byte_order, bo, bo.label());
                        }
                    });
                let u = self.byte_order.u32_of(current, next);
                ui.monospace(format!("Uint {u}"));
                ui.monospace(format!("Float {}", f32::from_bits(u)));
            });
        });
        ui.horizontal(|ui| {
            let can_set = self.bank == Bank::Holding;
            if ui.add_enabled(can_set, egui::Button::new("Set")).clicked() {
                regs.lock().unwrap().set_holding(addr, self.pending);
            }
            if self.bank == Bank::Input {
                ui.weak("Input registers are read-only.");
            }
            ui.label(format!("pending: {}", self.fmt(self.pending)));
        });
    }
}
