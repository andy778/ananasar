//! Manual commands: single/multiple write, exception status and mask write.

use super::worker::Cmd;
use super::ClientView;
use ananasar_core::client::ManualCommand;
use eframe::egui;

impl ClientView {
    pub(super) fn manual_ui(&mut self, ui: &mut egui::Ui, connected: bool) {
        ui.horizontal(|ui| {
            ui.label("Manual @");
            ui.add(egui::TextEdit::singleline(&mut self.man_addr).desired_width(50.0));
            ui.label("val(hex):");
            ui.add(egui::TextEdit::singleline(&mut self.man_value).desired_width(50.0));
            let addr = self.man_addr.trim().parse::<u16>().ok();
            let val = u16::from_str_radix(self.man_value.trim(), 16).ok();
            let write_label = if self.man_use_fc16 { "FC16" } else { "FC6" };
            if ui.add_enabled(connected, egui::Button::new(write_label)).clicked()
                && let (Some(a), Some(v)) = (addr, val) {
                    let cmd = if self.man_use_fc16 {
                        ManualCommand::WriteSingleMultiple { addr: a, value: v }
                    } else {
                        ManualCommand::WriteSingle { addr: a, value: v }
                    };
                    let _ = self.tx.send(Cmd::Manual(cmd));
                }
            ui.checkbox(&mut self.man_use_fc16, "Use Function 16");
            if ui.add_enabled(connected, egui::Button::new("FC7")).clicked() {
                let _ = self.tx.send(Cmd::Manual(ManualCommand::ReadExceptionStatus));
            }
            let and = u16::from_str_radix(self.man_and.trim(), 16).ok();
            let or = u16::from_str_radix(self.man_or.trim(), 16).ok();
            ui.label("AND:");
            ui.add(egui::TextEdit::singleline(&mut self.man_and).desired_width(50.0));
            ui.label("OR:");
            ui.add(egui::TextEdit::singleline(&mut self.man_or).desired_width(50.0));
            if ui.add_enabled(connected, egui::Button::new("FC22")).clicked()
                && let (Some(a), Some(am), Some(om)) = (addr, and, or) {
                    let _ = self.tx.send(Cmd::Manual(ManualCommand::MaskWrite { addr: a, and_mask: am, or_mask: om }));
                }
        });
    }
}
