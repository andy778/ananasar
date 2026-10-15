//! Dialogs as windows of their own, as in the original, instead of popups
//! drawn inside (and clipped by) the window that opened them.

use eframe::egui;

/// Shows a fixed-size native window with `size` as its inner size. `margin` is
/// the padding around the content. Returns true when the window's close button
/// was pressed.
pub fn show(ctx: &egui::Context, id: &str, title: &str, size: [f32; 2], margin: i8, content: &mut dyn FnMut(&mut egui::Ui)) -> bool {
    let builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_resizable(false)
        .with_minimize_button(false)
        .with_maximize_button(false);
    let mut close = false;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(id), builder, |ctx, _class| {
        let frame = egui::Frame::new().fill(ctx.global_style().visuals.panel_fill).inner_margin(egui::Margin::same(margin));
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| content(ui));
        close = ctx.input(|i| i.viewport().close_requested());
    });
    close
}
