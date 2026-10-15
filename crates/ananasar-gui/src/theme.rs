//! The "Classic" look: the Windows system colours and flat square controls of
//! the original (a Delphi VCL app), as an egui light style. Styling only —
//! fonts and layout are left alone.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, Vec2};

const BTN_FACE: Color32 = Color32::from_rgb(0xF0, 0xF0, 0xF0);
const BTN_SHADOW: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
const BTN_BORDER: Color32 = Color32::from_rgb(0xAD, 0xAD, 0xAD);
const HOT_FACE: Color32 = Color32::from_rgb(0xE5, 0xF1, 0xFB);
const HOT_BORDER: Color32 = Color32::from_rgb(0x00, 0x78, 0xD7);
const DOWN_FACE: Color32 = Color32::from_rgb(0xCC, 0xE4, 0xF7);
const DOWN_BORDER: Color32 = Color32::from_rgb(0x00, 0x54, 0x99);
const HIGHLIGHT: Color32 = Color32::from_rgb(0x00, 0x78, 0xD7);

/// Apply (`classic == true`) or remove the Classic style on the light theme.
/// The dark theme is never touched.
pub fn apply(ctx: &egui::Context, classic: bool) {
    ctx.set_style_of(Theme::Light, if classic { classic_style() } else { egui::Style::default() });
}

fn classic_style() -> egui::Style {
    let mut style = egui::Style { animation_time: 0.0, ..egui::Style::default() };

    style.spacing.item_spacing = Vec2::new(6.0, 3.0);
    style.spacing.button_padding = Vec2::new(6.0, 1.0);
    style.spacing.interact_size = Vec2::new(40.0, 20.0);
    style.spacing.window_margin = egui::Margin::same(6);

    let mut v = egui::Visuals::light();
    v.panel_fill = BTN_FACE;
    v.window_fill = BTN_FACE;
    v.extreme_bg_color = Color32::WHITE; // edit boxes, lists
    v.faint_bg_color = Color32::from_rgb(0xF7, 0xF7, 0xF7);
    v.window_stroke = Stroke::new(1.0, BTN_SHADOW);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.selection.bg_fill = HIGHLIGHT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = Color32::from_rgb(0x00, 0x00, 0xFF);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BTN_FACE;
    w.noninteractive.weak_bg_fill = BTN_FACE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BTN_SHADOW);
    w.noninteractive.fg_stroke = Stroke::new(1.0, Color32::BLACK);

    w.inactive.bg_fill = BTN_FACE;
    w.inactive.weak_bg_fill = BTN_FACE;
    w.inactive.bg_stroke = Stroke::new(1.0, BTN_BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, Color32::BLACK);

    w.hovered.bg_fill = HOT_FACE;
    w.hovered.weak_bg_fill = HOT_FACE;
    w.hovered.bg_stroke = Stroke::new(1.0, HOT_BORDER);
    w.hovered.fg_stroke = Stroke::new(1.0, Color32::BLACK);
    w.hovered.expansion = 0.0;

    w.active.bg_fill = DOWN_FACE;
    w.active.weak_bg_fill = DOWN_FACE;
    w.active.bg_stroke = Stroke::new(1.0, DOWN_BORDER);
    w.active.fg_stroke = Stroke::new(1.0, Color32::BLACK);
    w.active.expansion = 0.0;

    w.open.bg_fill = DOWN_FACE;
    w.open.weak_bg_fill = DOWN_FACE;
    w.open.bg_stroke = Stroke::new(1.0, DOWN_BORDER);
    w.open.fg_stroke = Stroke::new(1.0, Color32::BLACK);

    for ws in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        ws.corner_radius = CornerRadius::ZERO;
    }

    style.visuals = v;
    style
}

/// Menu config for the menu bar: flat white popups with a 1 px grey border,
/// square corners and roomier rows in the Classic style, egui's own otherwise.
pub fn menu_config(classic: bool) -> egui::containers::menu::MenuConfig {
    let config = egui::containers::menu::MenuConfig::new();
    if classic { config.style(classic_menu_style) } else { config }
}

fn classic_menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.window_stroke = Stroke::new(1.0, BTN_SHADOW);
    style.visuals.menu_corner_radius = CornerRadius::ZERO;
    style.visuals.popup_shadow = egui::Shadow::NONE;
    style.spacing.menu_margin = egui::Margin::same(1);
    style.spacing.button_padding = Vec2::new(14.0, 3.0);
    style.spacing.item_spacing = Vec2::new(6.0, 0.0);
    style.spacing.interact_size.y = 22.0;
    style.visuals.widgets.hovered.weak_bg_fill = HOT_FACE;
    style.visuals.widgets.hovered.bg_fill = HOT_FACE;
    style.visuals.widgets.active.weak_bg_fill = DOWN_FACE;
    style.visuals.widgets.open.weak_bg_fill = DOWN_FACE;
}
