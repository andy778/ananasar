//! Manual commands: exception status and the Write/Mask single register tool window.

use super::widgets::{fixed_window, Canvas};
use super::worker::Cmd;
use super::ClientView;
use ananasar_core::client::ManualCommand;
use eframe::egui;

/// A plain value: decimal, or hex with a `0x` prefix.
fn parse_value(text: &str) -> Option<u16> {
    let t = text.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => t.parse().ok(),
    }
}

/// A mask pattern: hex, with an optional `0x` prefix.
fn parse_hex(text: &str) -> Option<u16> {
    let t = text.trim();
    u16::from_str_radix(t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t), 16).ok()
}

/// Builds the command the Apply button sends, or `None` while a field is not valid.
pub(super) fn build_command(addr: &str, value: &str, and: &str, or: &str, mask: bool, fc16: bool) -> Option<ManualCommand> {
    let addr = addr.trim().parse::<u16>().ok()?;
    if mask {
        return Some(ManualCommand::MaskWrite { addr, and_mask: parse_hex(and)?, or_mask: parse_hex(or)? });
    }
    let value = parse_value(value)?;
    Some(if fc16 { ManualCommand::WriteSingleMultiple { addr, value } } else { ManualCommand::WriteSingle { addr, value } })
}

impl ClientView {
    pub(super) fn manual_ui(&mut self, ui: &mut egui::Ui, connected: bool) {
        ui.horizontal(|ui| {
            if ui.button("Write register…").on_hover_text("Write or mask a single register").clicked() {
                self.man_open = true;
            }
            if ui.add_enabled(connected, egui::Button::new("FC7")).clicked() {
                let _ = self.tx.send(Cmd::Manual(ManualCommand::ReadExceptionStatus));
            }
        });
        self.write_dialog(ui.ctx(), connected);
    }

    fn write_dialog(&mut self, ctx: &egui::Context, connected: bool) {
        if !self.man_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        fixed_window(if self.man_mask { "Mask single register" } else { "Write single register" }).id(egui::Id::new("write-register")).open(&mut open).show(ctx, |ui| {
            let c = Canvas::new(ui, 382.0, 128.0);
            let mask = self.man_mask;
            let command = build_command(&self.man_addr, &self.man_value, &self.man_and, &self.man_or, mask, self.man_use_fc16);
            c.group(ui, (8.0, 8.0, 113.0, 49.0), "Address", |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.man_addr).desired_width(97.0)).on_hover_text("Register address");
            });
            c.group(ui, (160.0, 8.0, 113.0, 49.0), "Value", |ui| {
                let (text, hint) = if mask {
                    (&mut self.man_and, "AND pattern of masking (hex)")
                } else {
                    (&mut self.man_value, "Value to write (decimal, or hex with 0x)")
                };
                ui.add(egui::TextEdit::singleline(text).desired_width(97.0)).on_hover_text(hint);
            });
            c.group(ui, (160.0, 64.0, 113.0, 49.0), "OR (Hex)", |ui| {
                ui.add_enabled(mask, egui::TextEdit::singleline(&mut self.man_or).desired_width(97.0)).on_hover_text("OR pattern of masking");
            });
            c.at(ui, (8.0, 72.0, 140.0, 20.0), |ui| {
                ui.checkbox(&mut self.man_mask, "Mask AND + OR").on_hover_text("Mask Write Register (function 22) instead of a plain write");
            });
            c.at(ui, (8.0, 96.0, 140.0, 20.0), |ui| {
                ui.add_enabled(!mask, egui::Checkbox::new(&mut self.man_use_fc16, "Use Function 16")).on_hover_text("Use FC16 instead of FC6");
            });
            c.at(ui, (293.0, 15.0, 81.0, 49.0), |ui| {
                let apply = ui.add_enabled(connected && command.is_some(), egui::Button::new("Apply").min_size(egui::vec2(81.0, 49.0)));
                if apply.on_hover_text("Write value to connection partner").clicked()
                    && let Some(cmd) = command
                {
                    let _ = self.tx.send(Cmd::Manual(cmd));
                }
            });
            c.at(ui, (293.0, 70.0, 81.0, 50.0), |ui| {
                if ui.add_sized([81.0, 50.0], egui::Button::new("Close")).on_hover_text("Close window").clicked() {
                    close = true;
                }
            });
        });
        self.man_open = open && !close;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_write_picks_fc6_or_fc16() {
        assert!(matches!(build_command("5", "100", "", "", false, false), Some(ManualCommand::WriteSingle { addr: 5, value: 100 })));
        assert!(matches!(build_command("5", "0x10", "", "", false, true), Some(ManualCommand::WriteSingleMultiple { addr: 5, value: 16 })));
        assert!(build_command("5", "70000", "", "", false, false).is_none());
        assert!(build_command("x", "1", "", "", false, false).is_none());
    }

    #[test]
    fn mask_write_reads_hex_patterns() {
        assert!(matches!(
            build_command("4", "", "F0F0", "0a0a", true, true),
            Some(ManualCommand::MaskWrite { addr: 4, and_mask: 0xF0F0, or_mask: 0x0A0A })
        ));
        assert!(build_command("4", "", "G", "0", true, false).is_none());
    }
}
