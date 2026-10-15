//! Event log: scrolling view with a Copy context menu.

use super::ClientView;
use ananasar_core::sync::lock;
use eframe::egui;

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
    pub(super) fn log_ui(&self, ui: &mut egui::Ui) {
        ui.strong("Event log");
        let scroll = egui::ScrollArea::vertical().stick_to_bottom(true).max_height(240.0).auto_shrink([false, false]).show(ui, |ui| {
            let log = lock(&self.shared.log);
            for line in log.iter() {
                ui.monospace(line);
            }
        });
        let log_area = ui.interact(scroll.inner_rect, scroll.id.with("log_menu"), egui::Sense::click());
        log_area.context_menu(|ui| {
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
