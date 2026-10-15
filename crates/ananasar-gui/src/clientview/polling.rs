//! Polling controls: trigger mode, per-transaction limits, interval and addressing options.

use super::widgets::Canvas;
use super::worker::{Cmd, Mode};
use super::ClientView;
use ananasar_core::client::AddressBase;
use eframe::egui;

/// Protocol maximums for registers per read and per write transaction.
pub(super) const MAX_READ: u16 = 125;
pub(super) const MAX_WRITE: u16 = 123;
/// The original starts with 100 registers per write transaction.
pub(super) const DEFAULT_WRITE: u16 = 100;
const READ_PRESETS: &[u16] = &[5, 50, 100, 125];
const WRITE_PRESETS: &[u16] = &[50, 100, 123];

/// Clamps a transaction limit to `1..=max`.
pub(super) fn clamp_limit(value: u16, max: u16) -> u16 {
    value.clamp(1, max)
}

/// The presets that fit within `max`, so a preset never exceeds the protocol limit.
fn valid_presets(presets: &[u16], max: u16) -> impl Iterator<Item = u16> + '_ {
    presets.iter().copied().filter(move |&p| (1..=max).contains(&p))
}

impl ClientView {
    /// Edit > Transaction limiter: the registers per transaction, with preset values.
    pub(super) fn limits_menu(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        ui.menu_button("Transaction limiter", |ui| {
            ui.set_min_width(150.0);
            for (title, value, presets, max) in [
                ("Registers to Read", &mut self.max_read, READ_PRESETS, MAX_READ),
                ("Registers to Write", &mut self.max_write, WRITE_PRESETS, MAX_WRITE),
            ] {
                ui.menu_button(title, |ui| {
                    ui.set_min_width(60.0);
                    for p in valid_presets(presets, max) {
                        if crate::theme::check_item(ui, *value == p, true, &p.to_string(), None) {
                            *value = clamp_limit(p, max);
                            changed = true;
                            ui.close();
                        }
                    }
                });
            }
        });
        if changed {
            let _ = self.tx.send(Cmd::SetLimits { read: self.max_read, write: self.max_write });
        }
    }

    pub(super) fn polling_ui(&mut self, ui: &mut egui::Ui, c: &Canvas) {
        c.frame(ui, (7.0, 263.0, 286.0, 120.0), "Polling");
        let before = self.mode;
        let rows = [
            (Mode::Cycle, "Cyclic read/write", 284.0),
            (Mode::Immediate, "Immediate query", 308.0),
            (Mode::WaitAfter, "Wait after response", 332.0),
            (Mode::Pause, "Pause", 356.0),
        ];
        for (mode, label, y) in rows {
            if c.at(ui, (21.0, y, 160.0, 20.0), |ui| ui.add(egui::RadioButton::new(self.mode == mode, label))).clicked() {
                self.mode = mode;
            }
        }
        if self.mode != before {
            let _ = self.tx.send(Cmd::SetMode(self.mode));
        }
        // One interval serves both modes: Cycle repeats at it, Wait delays by it.
        for (label, y, active) in [("Cycle", 286.0, self.mode != Mode::WaitAfter), ("Delay", 334.0, self.mode == Mode::WaitAfter)] {
            ui.painter().text(c.rect((190.0, y + 4.0, 0.0, 0.0)).min, egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), if active { egui::Color32::BLACK } else { egui::Color32::GRAY });
            let rect = c.rect((226.0, y - 3.0, 60.0, 22.0));
            if crate::theme::spin_u64(ui, rect, label, &mut self.interval_ms, 60000, 10, active) {
                let _ = self.tx.send(Cmd::SetInterval(self.interval_ms));
            }
        }
        if c.at(ui, (14.0, 388.0, 150.0, 20.0), |ui| ui.add(egui::Checkbox::new(&mut self.base_one, "Use address offset"))).changed() {
            let _ = self.tx.send(Cmd::SetBase(if self.base_one { AddressBase::One } else { AddressBase::Zero }));
        }
        if c.at(ui, (178.0, 388.0, 118.0, 20.0), |ui| ui.add(egui::Checkbox::new(&mut self.zero_ti, "TI always '00 00'"))).changed() {
            let _ = self.tx.send(Cmd::SetZeroTi(self.zero_ti));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_to_protocol_maximums() {
        assert_eq!(clamp_limit(0, MAX_READ), 1);
        assert_eq!(clamp_limit(200, MAX_READ), 125);
        assert_eq!(clamp_limit(125, MAX_WRITE), 123);
        assert_eq!(clamp_limit(50, MAX_WRITE), 50);
    }

    #[test]
    fn presets_fit_their_maximum() {
        assert!(READ_PRESETS.iter().all(|&p| p <= MAX_READ));
        assert!(WRITE_PRESETS.iter().all(|&p| p <= MAX_WRITE));
        assert_eq!(valid_presets(&[5, 125, 200, 0], MAX_WRITE).collect::<Vec<_>>(), [5]);
    }
}
