//! Options → DDE Server…: choose which registers the DDE server offers and
//! start it. Windows only; the server itself is the `ananasar-dde` crate.

use crate::embed::SharedRegs;
use crate::{dialog, setvalview, theme};
use ananasar_dde::item::{Activation, Scope};
use ananasar_dde::{SERVICE, Server};
use eframe::egui;

/// The six choices of the dialog, top to bottom.
const CHOICES: [(Scope, &str); 6] = [
    (Scope::AllBoth, "All Input and Holding registers"),
    (Scope::Input, "Input registers"),
    (Scope::InputRange, "Range of Input registers"),
    (Scope::Holding, "Holding registers"),
    (Scope::HoldingRange, "Range of Holding registers"),
    (Scope::BothRange, "Range of Input and Holding registers"),
];

/// The DDE dialog and the server it starts.
pub struct DdeView {
    open: bool,
    info: bool,
    scope: Scope,
    from: String,
    to: String,
    /// Why the last OK was refused, shown in the dialog.
    error: String,
    server: Option<Server>,
}

impl Default for DdeView {
    fn default() -> Self {
        // The original opens on a range of holding registers every time.
        Self { open: false, info: false, scope: Scope::HoldingRange, from: String::new(), to: String::new(), error: String::new(), server: None }
    }
}

/// An "i" in a blue disc, drawn rather than loaded.
fn info_icon(painter: &egui::Painter, rect: egui::Rect, hot: bool) {
    let blue = if hot { egui::Color32::from_rgb(0x2A, 0x8A, 0xE0) } else { egui::Color32::from_rgb(0x00, 0x78, 0xD7) };
    painter.circle_filled(rect.center(), rect.width() / 2.0 - 3.0, blue);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(20.0), egui::Color32::WHITE);
}

impl DdeView {
    /// Opens the dialog, again on its defaults.
    pub fn open(&mut self) {
        *self = Self { server: self.server.take(), open: true, ..Self::default() };
    }

    /// Draws the dialog (and its help window) while open. Errors go to `message`.
    pub fn show(&mut self, ctx: &egui::Context, regs: &SharedRegs, message: &mut Option<String>) {
        if self.info {
            self.info_ui(ctx);
        }
        if !self.open {
            return;
        }
        let (mut ok, mut cancel, mut info) = (false, false, false);
        let closed = dialog::show(ctx, "dde", "DDE", [328.0, 230.0], 0, &mut |ui| {
            let (area, _) = ui.allocate_exact_size(egui::vec2(328.0, 230.0), egui::Sense::hover());
            let at = |x: f32, y: f32, w: f32, h: f32| setvalview::client_rect(area, x, y, w, h);
            setvalview::group_box(ui, at(9.0, 9.0, 311.0, 171.0), "Server activation");
            for (i, (scope, label)) in CHOICES.into_iter().enumerate() {
                let y = 29.0 + 25.0 * f32::from(u8::try_from(i).unwrap_or(0));
                let clicked = ui
                    .scope_builder(egui::UiBuilder::new().max_rect(at(14.0, y, 220.0, 20.0)).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                        ui.radio(self.scope == scope, label).clicked()
                    })
                    .inner;
                if clicked {
                    self.scope = scope;
                }
            }
            if self.scope.has_range() {
                let black = egui::Color32::BLACK;
                let font = egui::FontId::proportional(12.0);
                for (text, y, value) in [("From:", 40.0, &mut self.from), ("To:", 72.0, &mut self.to)] {
                    ui.painter().text(at(214.0, y, 0.0, 0.0).min, egui::Align2::LEFT_CENTER, text, font.clone(), black);
                    ui.put(at(247.0, y - 7.0, 64.0, 20.0), egui::TextEdit::singleline(value));
                }
            }
            let rect = at(273.0, 131.0, 38.0, 38.0);
            let resp = ui.interact(rect, ui.scope_id().with("dde_info"), egui::Sense::click());
            info_icon(ui.painter(), rect, resp.hovered());
            info = resp.clicked();
            if !self.error.is_empty() {
                let text = egui::RichText::new(self.error.as_str()).size(11.0).color(ui.visuals().error_fg_color);
                ui.put(at(9.0, 181.0, 311.0, 16.0), egui::Label::new(text));
            }
            ok = theme::bevel_button(ui, at(9.0, 199.0, 80.0, 22.0), "dde_ok", "OK", true, None).clicked();
            cancel = theme::bevel_button(ui, at(112.0, 199.0, 80.0, 22.0), "dde_cancel", "Cancel", true, None).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
        self.info |= info;
        if cancel || closed {
            self.open = false;
        } else if ok {
            self.apply(regs, message);
        }
    }

    /// Starts the server for the chosen registers, replacing a running one.
    fn apply(&mut self, regs: &SharedRegs, message: &mut Option<String>) {
        match Activation::new(self.scope, &self.from, &self.to) {
            Err(e) => self.error = e.to_string(),
            Ok(activation) => {
                self.error.clear();
                // The service name can only be registered once, so the old server goes first.
                self.server = None;
                match Server::start(regs.clone(), activation) {
                    Ok(server) => {
                        self.server = Some(server);
                        self.open = false;
                    }
                    Err(e) => *message = Some(format!("Cannot start the DDE server: {e}")),
                }
            }
        }
    }

    /// The (i) button: the names a DDE client uses.
    fn info_ui(&mut self, ctx: &egui::Context) {
        let mut ok = false;
        let example = format!("\"{SERVICE}|holding!r8862\"");
        let closed = dialog::show(ctx, "dde-info", "DDE", [410.0, 190.0], 0, &mut |ui| {
            let (area, _) = ui.allocate_exact_size(egui::vec2(410.0, 190.0), egui::Sense::hover());
            let at = |x: f32, y: f32, w: f32, h: f32| setvalview::client_rect(area, x, y, w, h);
            let text = format!(
                "The service is \"{SERVICE}\". The topics are:\n\n\"input\" for input registers\n\"holding\" for holding registers\n\n\
                 An item is the letter r (lower case) and the register number.\n\n\
                 For example {example} is holding register 8862."
            );
            ui.put(at(16.0, 14.0, 380.0, 120.0), egui::Label::new(egui::RichText::new(text).size(12.0)).wrap());
            ok = theme::bevel_button(ui, at(166.0, 150.0, 80.0, 24.0), "dde_info_ok", "OK", true, None).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
        if closed || ok {
            self.info = false;
        }
    }
}
