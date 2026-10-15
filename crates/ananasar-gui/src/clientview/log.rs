//! Event log: a sunken white list with a hint while empty, a Copy context
//! menu and double-click to clear.

use super::widgets::Canvas;
use super::ClientView;
use crate::theme;
use ananasar_core::sync::lock;
use eframe::egui;

const HINT: &str = "Connection events and errors detected during data exchange are listed here.\n\nDouble-click to clear";

/// Joins log lines with newlines, for the clipboard.
fn join_lines<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> String {
    let mut out = String::new();
    for (i, line) in lines.into_iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.as_ref());
    }
    out
}

impl ClientView {
    pub(super) fn log_ui(&self, ui: &mut egui::Ui, c: &Canvas, area: (f32, f32, f32, f32)) {
        let outer = c.rect(area);
        ui.painter().rect_filled(outer, 0.0, egui::Color32::WHITE);
        theme::sunken_border(ui.painter(), outer);
        let status = lock(&self.shared.status).clone();
        let inner = outer.shrink(3.0);
        let scroll = ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                let log = lock(&self.shared.log);
                if log.is_empty() && status.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(HINT).color(egui::Color32::BLACK)).wrap().selectable(false));
                }
                if !status.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(status).color(egui::Color32::BLACK)).selectable(false));
                }
                for line in log.iter() {
                    ui.add(egui::Label::new(egui::RichText::new(line).monospace().color(egui::Color32::BLACK)).selectable(false));
                }
            })
        });
        let area = ui.interact(scroll.inner.inner_rect, scroll.inner.id.with("log_menu"), egui::Sense::click());
        if area.double_clicked() {
            lock(&self.shared.log).clear();
            self.shared.set_status(String::new());
        }
        area.context_menu(|ui| {
            if ui.button("Copy").clicked() {
                let text = join_lines(lock(&self.shared.log).iter());
                ui.ctx().copy_text(text);
                ui.close();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_lines_separates_with_newlines() {
        assert_eq!(join_lines(["a", "b", "c"]), "a\nb\nc");
        assert_eq!(join_lines(["only"]), "only");
        assert_eq!(join_lines(Vec::<String>::new()), "");
    }
}
