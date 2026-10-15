//! Small widgets shared by the client window sections.

use eframe::egui;

/// A titled group of removable address chips. Returns true if one was removed.
pub(super) fn list_chips(ui: &mut egui::Ui, title: &str, list: &mut Vec<u16>) -> bool {
    let mut removed = None;
    ui.group(|ui| {
        ui.vertical(|ui| {
            ui.label(format!("{title} ({})", list.len()));
            ui.horizontal_wrapped(|ui| {
                for (i, a) in list.iter().enumerate() {
                    if ui.small_button(format!("{a} ✕")).clicked() {
                        removed = Some(i);
                    }
                }
            });
        });
    });
    if let Some(i) = removed {
        list.remove(i);
        true
    } else {
        false
    }
}
