//! Register lists: single-address add, chips, clear and the From/To range dialog.

use super::widgets::Canvas;
use crate::theme;
use super::ClientView;
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ListKind {
    ReadHolding,
    ReadInput,
    Write,
}

/// State of the From/To dialog that adds or removes a whole address range.
pub(super) struct RangeDialog {
    pub(super) open: bool,
    pub(super) from: String,
    pub(super) to: String,
    pub(super) list: ListKind,
    pub(super) remove: bool,
}

impl RangeDialog {
    pub(super) fn new() -> Self {
        Self { open: false, from: "0".into(), to: "0".into(), list: ListKind::ReadHolding, remove: false }
    }
}

fn add_sorted(v: &mut Vec<u16>, a: u16) {
    if !v.contains(&a) {
        v.push(a);
        v.sort_unstable();
    }
}

/// Parses the dialog's From/To fields; `None` unless both are addresses with `from <= to`.
fn parse_range(from: &str, to: &str) -> Option<(u16, u16)> {
    let (from, to) = (from.trim().parse::<u16>().ok()?, to.trim().parse::<u16>().ok()?);
    (from <= to).then_some((from, to))
}

/// Adds every address in `from..=to`, keeping the list sorted and free of duplicates.
fn add_range(v: &mut Vec<u16>, from: u16, to: u16) {
    v.extend(from..=to);
    v.sort_unstable();
    v.dedup();
}

/// Removes every address in `from..=to`.
fn remove_range(v: &mut Vec<u16>, from: u16, to: u16) {
    v.retain(|a| !(from..=to).contains(a));
}

/// One address drop-down with its Add and Remove buttons at row `y`. Returns true if `list` changed.
fn address_row(ui: &mut egui::Ui, c: &Canvas, y: f32, id: &str, addr: &mut u16, list: &mut Vec<u16>) -> bool {
    let snapshot = list.clone();
    theme::combo_list(ui, c.rect((21.0, y, 98.0, 22.0)), id, addr, 65535, &snapshot);
    let mut changed = false;
    if theme::bevel_button(ui, c.rect((127.0, y - 1.0, 74.0, 24.0)), &format!("{id}_add"), "Add", true, None).on_hover_text("Add the address to the list").clicked() {
        add_sorted(list, *addr);
        changed = true;
    }
    let present = list.contains(addr);
    if theme::bevel_button(ui, c.rect((209.0, y - 1.0, 74.0, 24.0)), &format!("{id}_remove"), "Remove", present, None).on_hover_text("Remove the address from the list").clicked() {
        list.retain(|a| a != addr);
        changed = true;
    }
    changed
}

impl ClientView {
    /// Edit menu, as in the original: Add Serie, Clear, Transaction limiter and
    /// Hide Window. Returns true when Hide Window was chosen.
    pub(super) fn edit_menu(&mut self, ui: &mut egui::Ui) -> bool {
        ui.set_min_width(190.0);
        let mut hide = false;
        ui.menu_button("Add Serie", |ui| {
            ui.set_min_width(150.0);
            ui.menu_button("Holding Registers", |ui| {
                for (label, kind) in [("Write", ListKind::Write), ("Read", ListKind::ReadHolding)] {
                    if ui.button(label).on_hover_text("Add or remove a series of registers").clicked() {
                        self.range.list = kind;
                        self.range.open = true;
                        ui.close();
                    }
                }
            });
            if ui.button("Input Registers").on_hover_text("Add or remove a series of registers").clicked() {
                self.range.list = ListKind::ReadInput;
                self.range.open = true;
                ui.close();
            }
        });
        let mut cleared = false;
        ui.menu_button("Clear", |ui| {
            ui.set_min_width(150.0);
            ui.menu_button("Holding Registers", |ui| {
                for (label, list) in [("Write", &mut self.write), ("Read", &mut self.read_holding)] {
                    if ui.add_enabled(!list.is_empty(), egui::Button::new(label)).clicked() {
                        list.clear();
                        cleared = true;
                        ui.close();
                    }
                }
            });
            if ui.add_enabled(!self.read_input.is_empty(), egui::Button::new("Input Registers")).clicked() {
                self.read_input.clear();
                cleared = true;
                ui.close();
            }
        });
        if cleared {
            self.push_lists();
        }
        self.limits_menu(ui);
        ui.separator();
        if ui.button("Hide Window").clicked() {
            hide = true;
            ui.close();
        }
        hide
    }

    /// The Read Input/Holding register and Write Holding register groups: an
    /// address drop-down (listing the addresses already in the list) with Add and Remove.
    pub(super) fn lists_ui(&mut self, ui: &mut egui::Ui, c: &Canvas) {
        c.frame(ui, (7.0, 111.0, 286.0, 82.0), "Read Input/Holding register");
        for (label, input, x) in [("Input registers", true, 21.0), ("Holding registers", false, 138.0)] {
            if c.at(ui, (x, 130.0, 118.0, 20.0), |ui| ui.add(egui::RadioButton::new(self.read_input_selected == input, label))).clicked() {
                self.read_input_selected = input;
            }
        }
        let read = if self.read_input_selected { &mut self.read_input } else { &mut self.read_holding };
        let mut changed = address_row(ui, c, 155.0, "read_addr", &mut self.read_addr, read);

        c.frame(ui, (7.0, 197.0, 286.0, 56.0), "Write Holding register");
        changed |= address_row(ui, c, 216.0, "write_addr", &mut self.write_addr, &mut self.write);
        if changed {
            self.push_lists();
        }
        self.range_dialog(ui.ctx());
    }

    fn range_dialog(&mut self, ctx: &egui::Context) {
        if !self.range.open {
            return;
        }
        let (mut ok, mut cancel) = (false, false);
        let closed = crate::dialog::show(ctx, "series", "Series", [231.0, 104.0], 0, &mut |ui| {
            let c = Canvas::new(ui, 231.0, 104.0);
            let r = &mut self.range;
            let valid = parse_range(&r.from, &r.to).is_some();
            c.at(ui, (8.0, 10.0, 40.0, 20.0), |ui| {
                ui.label("From:");
            });
            c.at(ui, (48.0, 8.0, 80.0, 22.0), |ui| {
                ui.add(egui::TextEdit::singleline(&mut r.from).desired_width(80.0)).on_hover_text("Starting value of series");
            });
            c.at(ui, (8.0, 50.0, 40.0, 20.0), |ui| {
                ui.label("To:");
            });
            c.at(ui, (48.0, 48.0, 80.0, 22.0), |ui| {
                ui.add(egui::TextEdit::singleline(&mut r.to).desired_width(80.0)).on_hover_text("Ending value of series");
            });
            c.at(ui, (48.0, 80.0, 80.0, 20.0), |ui| {
                ui.checkbox(&mut r.remove, "Remove").on_hover_text("Remove serie");
            });
            c.at(ui, (136.0, 8.0, 75.0, 25.0), |ui| {
                let button = ui.add_enabled(valid, egui::Button::new("OK").min_size(egui::vec2(75.0, 25.0)));
                ok = button.on_hover_text("Enter series to register list").clicked();
            });
            c.at(ui, (136.0, 48.0, 75.0, 25.0), |ui| {
                cancel = ui.add_sized([75.0, 25.0], egui::Button::new("Cancel")).clicked();
            });
        });
        let apply = ok;
        self.range.open = !ok && !cancel && !closed;
        if apply && let Some((from, to)) = parse_range(&self.range.from, &self.range.to) {
            let list = match self.range.list {
                ListKind::ReadHolding => &mut self.read_holding,
                ListKind::ReadInput => &mut self.read_input,
                ListKind::Write => &mut self.write,
            };
            if self.range.remove {
                remove_range(list, from, to);
            } else {
                add_range(list, from, to);
            }
            self.push_lists();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_range_merges_sorted_and_deduped() {
        let mut v = vec![2, 10];
        add_range(&mut v, 1, 4);
        assert_eq!(v, [1, 2, 3, 4, 10]);
        add_range(&mut v, 4, 4);
        assert_eq!(v, [1, 2, 3, 4, 10]);
    }

    #[test]
    fn add_range_reaches_the_top_address() {
        let mut v = vec![];
        add_range(&mut v, 65534, 65535);
        assert_eq!(v, [65534, 65535]);
    }

    #[test]
    fn remove_range_keeps_the_rest() {
        let mut v = vec![1, 2, 3, 4, 10];
        remove_range(&mut v, 2, 4);
        assert_eq!(v, [1, 10]);
        remove_range(&mut v, 20, 30);
        assert_eq!(v, [1, 10]);
    }

    #[test]
    fn parse_range_rejects_bad_input() {
        assert_eq!(parse_range(" 3", "7 "), Some((3, 7)));
        assert_eq!(parse_range("5", "5"), Some((5, 5)));
        assert_eq!(parse_range("7", "3"), None);
        assert_eq!(parse_range("x", "3"), None);
        assert_eq!(parse_range("0", "65536"), None);
    }

    #[test]
    fn add_sorted_ignores_duplicates() {
        let mut v = vec![1, 5];
        add_sorted(&mut v, 3);
        add_sorted(&mut v, 3);
        assert_eq!(v, [1, 3, 5]);
    }
}
