//! Small widgets shared by the client window sections.

use eframe::egui;

/// A fixed-size client area laid out with absolute offsets, like a Delphi form.
pub(super) struct Canvas {
    origin: egui::Pos2,
}

impl Canvas {
    /// Reserves a `w` x `h` client area at the current position of `ui`.
    pub(super) fn new(ui: &mut egui::Ui, w: f32, h: f32) -> Self {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        Self { origin: rect.min }
    }

    /// The point (`l`, `t`) relative to the client area.
    fn pos(&self, l: f32, t: f32) -> egui::Pos2 {
        egui::pos2(self.origin.x + l, self.origin.y + t)
    }

    /// Runs `add` inside the rectangle (`l`, `t`, `w`, `h`) relative to the client area.
    pub(super) fn at<R>(&self, ui: &mut egui::Ui, (l, t, w, h): (f32, f32, f32, f32), add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let rect = egui::Rect::from_min_size(self.pos(l, t), egui::vec2(w, h));
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add).inner
    }

    /// A captioned frame; `add` fills its 22 px content row.
    pub(super) fn group<R>(&self, ui: &mut egui::Ui, (l, t, w, h): (f32, f32, f32, f32), caption: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
        let text_color = ui.visuals().text_color();
        let fill = ui.visuals().window_fill;
        let frame = egui::Rect::from_min_size(self.pos(l, t + 7.0), egui::vec2(w, h - 7.0));
        ui.painter().rect_stroke(frame, 0.0, stroke, egui::StrokeKind::Inside);
        let galley = ui.painter().layout_no_wrap(caption.to_owned(), egui::FontId::proportional(13.0), text_color);
        let caption_rect = egui::Rect::from_min_size(self.pos(l + 6.0, t), egui::vec2(galley.size().x + 4.0, galley.size().y));
        ui.painter().rect_filled(caption_rect, 0.0, fill);
        ui.painter().galley(self.pos(l + 8.0, t), galley, text_color);
        self.at(ui, (l + 8.0, t + 20.0, w - 16.0, 22.0), add)
    }
}

/// A tool window or dialog frame: titled, fixed in size, not collapsible.
pub(super) fn fixed_window(title: &str) -> egui::Window<'_> {
    egui::Window::new(title).collapsible(false).resizable(false)
}

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
