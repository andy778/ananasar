//! Polling controls: trigger mode, per-transaction limits, interval and addressing options.

use super::worker::{Cmd, Mode};
use super::ClientView;
use ananasar_core::client::AddressBase;
use eframe::egui;

/// Protocol maximums for registers per read and per write transaction.
pub(super) const MAX_READ: u16 = 125;
pub(super) const MAX_WRITE: u16 = 123;
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

/// A custom value box plus a preset menu for one transaction limit. Returns true if it changed.
fn limit_control(ui: &mut egui::Ui, id: &str, value: &mut u16, presets: &[u16], max: u16) -> bool {
    let mut changed = ui.add(egui::DragValue::new(value).range(1..=max)).changed();
    egui::ComboBox::from_id_salt(id).selected_text("").width(14.0).show_ui(ui, |ui| {
        for p in valid_presets(presets, max) {
            if ui.selectable_label(*value == p, p.to_string()).clicked() {
                *value = p;
                changed = true;
            }
        }
    });
    *value = clamp_limit(*value, max);
    changed
}

impl ClientView {
    pub(super) fn polling_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.group(|ui| {
                ui.label("Trigger:");
                let before = self.mode;
                ui.radio_value(&mut self.mode, Mode::Cycle, "Cycle");
                ui.radio_value(&mut self.mode, Mode::WaitAfter, "Wait");
                ui.radio_value(&mut self.mode, Mode::Immediate, "Imm");
                ui.radio_value(&mut self.mode, Mode::Pause, "Pause");
                if self.mode != before {
                    let _ = self.tx.send(Cmd::SetMode(self.mode));
                }
            });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Read/q:");
                    let mut limits_changed = limit_control(ui, "max_read", &mut self.max_read, READ_PRESETS, MAX_READ);
                    ui.label("Write/q:");
                    limits_changed |= limit_control(ui, "max_write", &mut self.max_write, WRITE_PRESETS, MAX_WRITE);
                    if limits_changed {
                        let _ = self.tx.send(Cmd::SetLimits { read: self.max_read, write: self.max_write });
                    }
                    ui.label("ms:");
                    if ui.add(egui::DragValue::new(&mut self.interval_ms).range(0..=60000).speed(10.0)).changed() {
                        let _ = self.tx.send(Cmd::SetInterval(self.interval_ms));
                    }
                });
                if ui.checkbox(&mut self.base_one, "1-based").changed() {
                    let _ = self.tx.send(Cmd::SetBase(if self.base_one { AddressBase::One } else { AddressBase::Zero }));
                }
                if ui.checkbox(&mut self.zero_ti, "TI always 00 00").changed() {
                    let _ = self.tx.send(Cmd::SetZeroTi(self.zero_ti));
                }
            });
        });
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
