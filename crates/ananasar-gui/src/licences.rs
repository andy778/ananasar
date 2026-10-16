//! The "Licences" window opened from the About dialog: the dependencies, the
//! advisories known when this build was made and the licence texts, from
//! `ananasar_core::notices`.

use ananasar_core::notices::{ADVISORIES, DEPENDENCIES, LICENSES};
use eframe::egui;

const TABS: [(&str, &str); 3] = [("Dependencies", DEPENDENCIES), ("Advisories", ADVISORIES), ("Licence texts", LICENSES)];

/// State of the window: whether it is open and which tab is shown.
#[derive(Default)]
pub struct Licences {
    pub open: bool,
    tab: usize,
    /// The lines of each tab, split once when first shown.
    lines: [Option<Vec<&'static str>>; 3],
}

impl Licences {
    /// Draw the window while it is open.
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let builder = egui::ViewportBuilder::default().with_title("Third-party licences").with_inner_size([720.0, 520.0]).with_min_inner_size([360.0, 240.0]);
        let mut close = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("licences"), builder, |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, (name, _)) in TABS.iter().enumerate() {
                        ui.selectable_value(&mut self.tab, i, *name);
                    }
                });
                ui.separator();
                let tab = self.tab.min(TABS.len().saturating_sub(1));
                let text = TABS.get(tab).map_or("", |(_, text)| *text);
                let lines = self.lines.get_mut(tab).map(|slot| slot.get_or_insert_with(|| text.lines().collect()));
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let row_height = ui.fonts_mut(|f| f.row_height(&font));
                let count = lines.as_ref().map_or(0, |l| l.len());
                egui::ScrollArea::both().auto_shrink([false, false]).show_rows(ui, row_height, count, |ui, range| {
                    for line in lines.iter().flat_map(|l| l.iter().skip(range.start).take(range.len())) {
                        ui.add(egui::Label::new(egui::RichText::new(*line).monospace()).extend().selectable(true));
                    }
                });
            });
            close = ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape));
        });
        if close {
            self.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tab_has_lines() {
        for (name, text) in TABS {
            assert!(text.lines().next().is_some(), "{name} is empty");
        }
    }
}
