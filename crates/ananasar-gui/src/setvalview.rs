//! Set Register Value window: edit one register by value or by its 16
//! individual bits, with a 32-bit Uint/Float interpretation. Operates directly
//! on the app's shared register file in-process (the server keeps serving while
//! a value is edited).
//!
//! The window is a fixed-size tool window with a 392x260 client area; every
//! control is placed at explicit client coordinates.
#![allow(clippy::arithmetic_side_effects, reason = "egui geometry on f32 vectors; integer maths below is bounded by u16/u32 widths")]

use crate::embed::SharedRegs;
use ananasar_core::registers::Bank;
use ananasar_core::sync::lock;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

const CLIENT: Vec2 = Vec2::new(392.0, 260.0);
const BORDER: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);

/// How the four bytes of a register pair are arranged in the 32-bit value.
///
/// The two registers are read as four bytes numbered 0..=3 in wire order:
/// byte 0 is the high byte of the first register, byte 1 its low byte, byte 2
/// the high byte of the second register, byte 3 its low byte. The digits of an
/// entry list, from the most significant byte of the 32-bit value down to the
/// least significant one, which of those four bytes goes there. So `0-1-2-3`
/// is plain big-endian (first register is the high word), `3-2-1-0` is full
/// little-endian, `1-0-3-2` swaps the bytes inside each register and `2-3-0-1`
/// swaps the two registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ByteOrder {
    B0123,
    B1032,
    B2301,
    B3210,
}

impl ByteOrder {
    const ALL: [ByteOrder; 4] = [ByteOrder::B0123, ByteOrder::B1032, ByteOrder::B2301, ByteOrder::B3210];

    fn label(self) -> &'static str {
        match self {
            ByteOrder::B0123 => "0-1-2-3",
            ByteOrder::B1032 => "1-0-3-2",
            ByteOrder::B2301 => "2-3-0-1",
            ByteOrder::B3210 => "3-2-1-0",
        }
    }

    /// The 32-bit value of the register pair `r0`, `r1` in this byte order.
    fn u32_of(self, r0: u16, r1: u16) -> u32 {
        let [a, b] = r0.to_be_bytes();
        let [c, d] = r1.to_be_bytes();
        let o = match self {
            ByteOrder::B0123 => [a, b, c, d],
            ByteOrder::B1032 => [b, a, d, c],
            ByteOrder::B2301 => [c, d, a, b],
            ByteOrder::B3210 => [d, c, b, a],
        };
        u32::from_be_bytes(o)
    }
}

/// Hover text of the bitmask check box for `bit` (0 = least significant).
fn bit_hint(bit: u8) -> String {
    match bit {
        0 => "LSB (1st bit)".to_owned(),
        15 => "MSB (16th bit)".to_owned(),
        n => n.to_string(),
    }
}

/// `v` with `bit` set or cleared; bits above 15 leave `v` unchanged.
fn with_bit(v: u16, bit: u8, on: bool) -> u16 {
    let Some(mask) = 1u16.checked_shl(u32::from(bit)) else { return v };
    if on { v | mask } else { v & !mask }
}

fn bit_of(v: u16, bit: u8) -> bool {
    v.checked_shr(u32::from(bit)).is_some_and(|s| s & 1 == 1)
}

/// Parse the Edit value field: decimal, or hexadecimal with a `0x`/`$` prefix.
fn parse_value(s: &str) -> Option<u16> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).or_else(|| s.strip_prefix('$')) {
        u16::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

pub struct SetValView {
    addr_text: String,
    bank: Bank,
    pending: u16,
    value_text: String,
    byte_order: ByteOrder,
}

impl Default for SetValView {
    fn default() -> Self {
        Self { addr_text: "0".into(), bank: Bank::Holding, pending: 0, value_text: "0".into(), byte_order: ByteOrder::B0123 }
    }
}

/// The rectangle at client offset (`x`, `y`) of size `w` x `h` inside `area`.
pub(crate) fn client_rect(area: Rect, x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(area.min + Vec2::new(x, y), Vec2::new(w, h))
}

/// A group box over `rect` with its caption sitting on the top border.
pub(crate) fn group_box(ui: &egui::Ui, rect: Rect, title: &str) {
    let font = FontId::proportional(12.0);
    let galley = ui.painter().layout_no_wrap(title.to_owned(), font, Color32::BLACK);
    let top = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + galley.size().y / 2.0), rect.max);
    ui.painter().rect_stroke(top, 0.0, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
    let pos = Pos2::new(rect.left() + 8.0, rect.top());
    let label = Rect::from_min_size(pos, galley.size()).expand2(Vec2::new(3.0, 0.0));
    ui.painter().rect_filled(label, 0.0, ui.visuals().panel_fill);
    ui.painter().galley(pos, galley, Color32::BLACK);
}

impl SetValView {
    fn set_pending(&mut self, v: u16) {
        self.pending = v;
        self.value_text = v.to_string();
    }

    /// Show the window; `open` is cleared by the Close button.
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, regs: &SharedRegs, menu: egui::containers::menu::MenuConfig) {
        if !*open {
            return;
        }
        let is_open = *open;
        let mut close = false;
        let closed = crate::dialog::show(ctx, "set-register-value", "Set Register Value", [CLIENT.x + 12.0, CLIENT.y + 44.0], 6, &mut |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                egui::MenuBar::new().config(menu.clone()).ui(ui, |ui| {
                    ui.menu_button("Edit", |ui| {
                        ui.menu_button("32-bit Representation Byte Order", |ui| {
                            for bo in ByteOrder::ALL {
                                if ui.radio_value(&mut self.byte_order, bo, bo.label()).clicked() {
                                    ui.close();
                                }
                            }
                        });
                    });
                });
                ui.add_space(4.0);
                close = self.client(ui, regs);
            });
        *open = is_open && !close && !closed;
    }

    /// The client area; returns whether Close was clicked.
    fn client(&mut self, ui: &mut egui::Ui, regs: &SharedRegs) -> bool {
        let (area, _) = ui.allocate_exact_size(CLIENT, Sense::hover());
        let at = |x: f32, y: f32, w: f32, h: f32| client_rect(area, x, y, w, h);

        let addr = self.addr_text.trim().parse::<u16>().unwrap_or(0);
        let (current, next) = {
            let r = lock(regs);
            (r.get(self.bank, addr), r.get(self.bank, addr.saturating_add(1)))
        };

        // Register
        group_box(ui, at(8.0, 8.0, 185.0, 73.0), "Register");
        for (y, bank, name) in [(28.0, Bank::Input, "Input"), (50.0, Bank::Holding, "Holding")] {
            let r = ui.put(at(18.0, y, 80.0, 20.0), egui::RadioButton::new(self.bank == bank, name)).on_hover_text("Register type used");
            if r.clicked() {
                self.bank = bank;
            }
        }
        ui.put(at(101.0, 32.0, 60.0, 24.0), egui::TextEdit::singleline(&mut self.addr_text))
            .on_hover_text("Register to be used");

        // Current value
        group_box(ui, at(200.0, 7.0, 185.0, 74.0), "Current value");
        let cur = at(208.0, 26.0, 169.0, 48.0);
        let resp = ui.interact(cur, ui.id().with("current"), Sense::click());
        ui.painter().text(cur.center(), Align2::CENTER_CENTER, format!("{current:05}"), FontId::monospace(34.0), Color32::from_rgb(0x00, 0x00, 0xC8));
        if resp.on_hover_text("DoubleClick to copy current value to 'Edit value'").double_clicked() {
            self.set_pending(current);
        }

        // Edit value
        group_box(ui, at(8.0, 88.0, 377.0, 97.0), "Edit value");
        let edit = ui
            .put(at(101.0, 104.0, 60.0, 25.0), egui::TextEdit::singleline(&mut self.value_text))
            .on_hover_text("Value to set");
        if edit.changed()
            && let Some(v) = parse_value(&self.value_text)
        {
            self.pending = v;
        }
        let can_set = self.bank == Bank::Holding;
        let set = ui.put(at(248.0, 102.0, 81.0, 28.0), egui::Button::new("Set").min_size(Vec2::new(81.0, 28.0)));
        let set = if can_set { set.on_hover_text("Write value to register") } else { set.on_disabled_hover_text("Input registers are read-only") };
        if can_set && set.clicked() {
            lock(regs).set_holding(addr, self.pending);
        }
        let close = ui.put(at(333.0, 102.0, 48.0, 28.0), egui::Button::new("Close")).clicked();

        // Bitmask
        let mask = at(16.0, 136.0, 361.0, 41.0);
        group_box(ui, mask, "Bitmask");
        ui.interact(mask, ui.id().with("mask"), Sense::hover()).on_hover_text("Check/Uncheck to set individual bits");
        for i in 0..16u8 {
            let bit = 15 - i;
            let x = 24.0 + f32::from(i) * 21.0;
            let mut on = bit_of(self.pending, bit);
            let r = ui.put(at(x, 150.0, 18.0, 20.0), egui::Checkbox::without_text(&mut on)).on_hover_text(bit_hint(bit));
            if r.changed() {
                self.set_pending(with_bit(self.pending, bit, on));
            }
        }

        // 32-bit representation
        group_box(ui, at(8.0, 191.0, 376.0, 58.0), &format!("32-bit Representation [{}]", self.byte_order.label()));
        let u = self.byte_order.u32_of(current, next);
        let text = |x: f32, y: f32, w: f32, s: String, align: Align2| {
            let rect = at(x, y, w, 18.0);
            let pos = if align == Align2::LEFT_CENTER { rect.left_center() } else { rect.right_center() };
            ui.painter().text(pos, align, s, FontId::proportional(13.0), Color32::BLACK);
        };
        text(20.0, 208.0, 40.0, "Uint:".into(), Align2::LEFT_CENTER);
        text(70.0, 208.0, 120.0, format!("{u:010}"), Align2::LEFT_CENTER);
        text(20.0, 226.0, 40.0, "Float:".into(), Align2::LEFT_CENTER);
        text(70.0, 226.0, 120.0, format!("{}", f32::from_bits(u)), Align2::LEFT_CENTER);
        close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_orders_of_a_known_pair() {
        // bytes in wire order: 01 02 03 04
        let (r0, r1) = (0x0102, 0x0304);
        assert_eq!(ByteOrder::B0123.u32_of(r0, r1), 0x0102_0304);
        assert_eq!(ByteOrder::B1032.u32_of(r0, r1), 0x0201_0403);
        assert_eq!(ByteOrder::B2301.u32_of(r0, r1), 0x0304_0102);
        assert_eq!(ByteOrder::B3210.u32_of(r0, r1), 0x0403_0201);
    }

    #[test]
    fn float_through_byte_orders() {
        let bits = 1.5f32.to_bits(); // 0x3FC00000
        assert_eq!(f32::from_bits(ByteOrder::B0123.u32_of(0x3FC0, 0x0000)), 1.5);
        assert_eq!(f32::from_bits(ByteOrder::B2301.u32_of(0x0000, 0x3FC0)), 1.5);
        assert_eq!(f32::from_bits(ByteOrder::B1032.u32_of(0xC03F, 0x0000)), 1.5);
        assert_eq!(f32::from_bits(ByteOrder::B3210.u32_of(0x0000, 0xC03F)), 1.5);
        assert_eq!(bits, 0x3FC0_0000);
    }

    #[test]
    fn bitmask_round_trip() {
        let mut v = 0u16;
        for bit in 0..16 {
            v = with_bit(v, bit, true);
            assert!(bit_of(v, bit));
        }
        assert_eq!(v, 0xFFFF);
        assert_eq!(with_bit(v, 15, false), 0x7FFF);
        assert_eq!(with_bit(v, 0, false), 0xFFFE);
        assert_eq!(with_bit(0x1234, 16, true), 0x1234);
        assert!(!bit_of(0x1234, 16));
    }

    #[test]
    fn value_parsing() {
        assert_eq!(parse_value(" 42 "), Some(42));
        assert_eq!(parse_value("0xFF"), Some(255));
        assert_eq!(parse_value("$10"), Some(16));
        assert_eq!(parse_value("65536"), None);
        assert_eq!(parse_value(""), None);
    }

    #[test]
    fn bit_hints() {
        assert_eq!(bit_hint(0), "LSB (1st bit)");
        assert_eq!(bit_hint(7), "7");
        assert_eq!(bit_hint(15), "MSB (16th bit)");
    }
}
