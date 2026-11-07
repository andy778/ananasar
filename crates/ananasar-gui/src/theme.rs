//! The "Classic" look: the Windows system colours and flat square controls of
//! the original (a Delphi VCL app), as an egui light style. Styling only —
//! fonts and layout are left alone.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, Vec2};

const BTN_FACE: Color32 = Color32::from_rgb(0xF0, 0xF0, 0xF0);
const BTN_SHADOW: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
const BTN_BORDER: Color32 = Color32::from_rgb(0x70, 0x70, 0x70);
const HOT_FACE: Color32 = Color32::from_rgb(0xE5, 0xF1, 0xFB);
const HOT_BORDER: Color32 = Color32::from_rgb(0x00, 0x78, 0xD7);
const DOWN_FACE: Color32 = Color32::from_rgb(0xCC, 0xE4, 0xF7);
const DOWN_BORDER: Color32 = Color32::from_rgb(0x00, 0x54, 0x99);
pub const HIGHLIGHT: Color32 = Color32::from_rgb(0x00, 0x78, 0xD7);

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

    // The small UI face of the original (Tahoma 8 pt, about 11-12 px).
    for ts in [egui::TextStyle::Body, egui::TextStyle::Button] {
        if let Some(f) = style.text_styles.get_mut(&ts) {
            f.size = 12.0;
        }
    }

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

    w.inactive.bg_fill = Color32::WHITE; // check and radio boxes
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

    // A bolder tick, so checked boxes read at a glance.
    for ws in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        ws.fg_stroke.width = 2.0;
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

/// Blue used for the live values of the Server panel.
pub const VALUE_BLUE: Color32 = Color32::from_rgb(0x00, 0x00, 0xC8);

/// The 1 px grey of the register list's grid lines.
pub const GRID_LINE: Color32 = BTN_SHADOW;

/// `p` moved by (`dx`, `dy`).
pub fn shift(p: egui::Pos2, dx: f32, dy: f32) -> egui::Pos2 {
    egui::pos2(p.x + dx, p.y + dy)
}

/// A 2 px sunken (inset) border drawn just inside `rect`, like a Windows list box.
pub fn sunken_border(painter: &egui::Painter, rect: egui::Rect) {
    let dark = Color32::from_rgb(0x69, 0x69, 0x69);
    let light = Color32::from_rgb(0xE3, 0xE3, 0xE3);
    for (inset, tl, br) in [(0.5, BTN_SHADOW, Color32::WHITE), (1.5, dark, light)] {
        let r = rect.shrink(inset);
        let s = |c: Color32| Stroke::new(1.0, c);
        painter.line_segment([r.left_bottom(), r.left_top()], s(tl));
        painter.line_segment([r.left_top(), r.right_top()], s(tl));
        painter.line_segment([r.right_top(), r.right_bottom()], s(br));
        painter.line_segment([r.right_bottom(), r.left_bottom()], s(br));
    }
}

/// A one-pixel sunken border, as on a status-bar panel.
pub fn sunken_panel(painter: &egui::Painter, rect: egui::Rect) {
    let r = rect.shrink(0.5);
    let s = |c: Color32| Stroke::new(1.0, c);
    painter.line_segment([r.left_bottom(), r.left_top()], s(BTN_SHADOW));
    painter.line_segment([r.left_top(), r.right_top()], s(BTN_SHADOW));
    painter.line_segment([r.right_top(), r.right_bottom()], s(Color32::WHITE));
    painter.line_segment([r.right_bottom(), r.left_bottom()], s(Color32::WHITE));
}

/// A small window glyph for the Client button, drawn here rather than bundled.
pub fn icon_window(painter: &egui::Painter, rect: egui::Rect, enabled: bool) {
    let (frame, bar, body) = if enabled {
        (Color32::from_rgb(0x20, 0x20, 0x50), Color32::from_rgb(0x1B, 0x5E, 0xC0), Color32::from_rgb(0xD8, 0xE8, 0xF8))
    } else {
        (Color32::from_rgb(0xB0, 0xB0, 0xB0), Color32::from_rgb(0xC8, 0xC8, 0xC8), Color32::from_rgb(0xF4, 0xF4, 0xF4))
    };
    let r = egui::Rect::from_center_size(rect.center(), Vec2::new(22.0, 18.0));
    painter.rect_filled(r, 0.0, body);
    painter.rect_filled(egui::Rect::from_min_size(r.min, Vec2::new(r.width(), 5.0)), 0.0, bar);
    painter.rect_stroke(r, 0.0, Stroke::new(1.0, frame), egui::StrokeKind::Inside);
}

/// A small play triangle for the Logging button.
pub fn icon_play(painter: &egui::Painter, rect: egui::Rect, enabled: bool) {
    let (fill, edge) = if enabled {
        (Color32::from_rgb(0x2E, 0xA0, 0x43), Color32::from_rgb(0x15, 0x50, 0x20))
    } else {
        (Color32::from_rgb(0xDC, 0xDC, 0xDC), Color32::from_rgb(0xC0, 0xC0, 0xC0))
    };
    let c = rect.center();
    let pts = vec![shift(c, -8.0, -9.0), shift(c, 9.0, 0.0), shift(c, -8.0, 9.0)];
    painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.0, edge)));
}

/// A Classic push button over `rect`: button-face grey with a raised 3D bevel,
/// a hot face under the pointer, an inverted bevel while pressed and greyed
/// text and icon when disabled. `icon` is drawn at the left; `id` keeps the
/// interaction state apart from other buttons.
pub fn bevel_button(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: &str,
    label: &str,
    enabled: bool,
    icon: Option<fn(&egui::Painter, egui::Rect, bool)>,
) -> egui::Response {
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let resp = ui.interact(rect, ui.scope_id().with(("bevel_button", id)), sense);
    let down = enabled && resp.is_pointer_button_down_on();
    let face = if enabled && resp.hovered() { HOT_FACE } else { BTN_FACE };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, face);
    let (white, light, dark, black) = (Color32::WHITE, Color32::from_rgb(0xE3, 0xE3, 0xE3), Color32::from_rgb(0x69, 0x69, 0x69), Color32::from_rgb(0x30, 0x30, 0x30));
    let s = |c: Color32| Stroke::new(1.0, c);
    painter.rect_stroke(rect, 0.0, s(if enabled && resp.hovered() { HOT_BORDER } else { black }), egui::StrokeKind::Inside);
    // (inset, top-left colour, bottom-right colour) of the two bevel rings.
    let rings = if down { [(1.5, dark, white), (2.5, BTN_SHADOW, light)] } else { [(1.5, white, dark), (2.5, light, BTN_SHADOW)] };
    for (inset, tl, br) in rings {
        let r = rect.shrink(inset);
        painter.line_segment([r.left_bottom(), r.left_top()], s(tl));
        painter.line_segment([r.left_top(), r.right_top()], s(tl));
        painter.line_segment([r.right_top(), r.right_bottom()], s(br));
        painter.line_segment([r.right_bottom(), r.left_bottom()], s(br));
    }
    let press = if down { 1.0 } else { 0.0 };
    let mut centre = shift(rect.center(), press, press);
    if let Some(icon) = icon {
        icon(painter, egui::Rect::from_min_size(shift(rect.left_top(), 8.0 + press, 7.0 + press), Vec2::new(24.0, 24.0)), enabled);
        centre = shift(centre, 14.0, 0.0);
    }
    let text = if enabled { Color32::BLACK } else { Color32::from_rgb(0x8A, 0x8A, 0x8A) };
    painter.text(centre, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), text);
    resp
}

/// A drop-down combo box over `rect`: a white sunken box showing `value`
/// (it can be typed into) with an arrow button on its right that opens the
/// list `0, step, 2*step, ...` of `count` entries. Returns whether the value changed.
#[allow(clippy::too_many_arguments, reason = "a drop-down needs its geometry, value, range and list")]
pub fn combo_u16(ui: &mut egui::Ui, rect: egui::Rect, id: &str, value: &mut u16, max: u16, step: u16, count: usize, hex: bool) -> bool {
    combo_with(ui, rect, id, value, max, count, hex, |i| u16::try_from(i).unwrap_or(0).saturating_mul(step).min(max))
}

/// Like [`combo_u16`], but the list holds exactly `items`.
pub fn combo_list(ui: &mut egui::Ui, rect: egui::Rect, id: &str, value: &mut u16, max: u16, items: &[u16]) -> bool {
    combo_with(ui, rect, id, value, max, items.len(), false, |i| items.get(i).copied().unwrap_or(0))
}

#[allow(clippy::too_many_arguments, reason = "a drop-down needs its geometry, value, range and list")]
fn combo_with(ui: &mut egui::Ui, rect: egui::Rect, id: &str, value: &mut u16, max: u16, count: usize, hex: bool, nth: impl Fn(usize) -> u16) -> bool {
    let fmt = |v: u16| if hex { format!("{v:04X}") } else { v.to_string() };
    let mut changed = false;
    let arrow_w = 17.0;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    sunken_border(&painter, rect);
    let arrow = egui::Rect::from_min_max(egui::pos2(rect.right() - arrow_w - 2.0, rect.top() + 2.0), shift(rect.max, -2.0, -2.0));
    let text_rect = egui::Rect::from_min_max(shift(rect.min, 2.0, 2.0), egui::pos2(arrow.left(), rect.bottom() - 2.0));

    // The text field keeps what is being typed until it loses focus.
    let edit_id = ui.scope_id().with(("combo_edit", id));
    let focused = ui.memory(|m| m.has_focus(edit_id));
    let mut buf = if focused { ui.data(|d| d.get_temp::<String>(edit_id)).unwrap_or_else(|| fmt(*value)) } else { fmt(*value) };
    let edit = egui::TextEdit::singleline(&mut buf).id(edit_id).frame(egui::Frame::NONE).margin(egui::Margin::symmetric(3, 0)).align(egui::Align2::LEFT_CENTER);
    let resp = ui.put(text_rect, edit);
    if resp.changed()
        && let Ok(v) = u32::from_str_radix(buf.trim(), if hex { 16 } else { 10 })
    {
        *value = u16::try_from(v.min(u32::from(max))).unwrap_or(max);
        changed = true;
    }
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(edit_id, buf));
    } else {
        ui.data_mut(|d| d.remove_temp::<String>(edit_id));
    }

    let btn = ui.interact(arrow, ui.scope_id().with(("combo_arrow", id)), egui::Sense::click());
    let down = btn.is_pointer_button_down_on();
    painter.rect_filled(arrow, 0.0, if btn.hovered() { HOT_FACE } else { BTN_FACE });
    let (a, b) = if down { (BTN_SHADOW, Color32::WHITE) } else { (Color32::WHITE, Color32::from_rgb(0x69, 0x69, 0x69)) };
    painter.line_segment([arrow.left_bottom(), arrow.left_top()], Stroke::new(1.0, a));
    painter.line_segment([arrow.left_top(), arrow.right_top()], Stroke::new(1.0, a));
    painter.line_segment([arrow.right_top(), arrow.right_bottom()], Stroke::new(1.0, b));
    painter.line_segment([arrow.right_bottom(), arrow.left_bottom()], Stroke::new(1.0, b));
    let c = shift(arrow.center(), 0.0, if down { 1.0 } else { 0.0 });
    painter.add(egui::Shape::convex_polygon(vec![shift(c, -4.0, -2.0), shift(c, 4.0, -2.0), shift(c, 0.0, 2.0)], Color32::BLACK, Stroke::NONE));

    // A tight list as wide as the combo box, like a Windows drop-down: 15 px
    // rows, the current or hovered entry in the selection blue, eight rows visible.
    const ROW_H: f32 = 15.0;
    let frame = egui::Frame::new().fill(Color32::WHITE).stroke(Stroke::new(1.0, Color32::from_rgb(0x30, 0x30, 0x30)));
    egui::Popup::from_toggle_button_response(&btn)
        .anchor(rect)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .width(rect.width())
        .frame(frame)
        .show(|ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.set_min_width(rect.width());
            egui::ScrollArea::vertical().max_height(ROW_H * 8.0).show_rows(ui, ROW_H, count, |ui, range| {
                for i in range {
                    let v = nth(i);
                    let (row, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), egui::Sense::click());
                    let hot = resp.hovered() || *value == v;
                    if hot {
                        ui.painter().rect_filled(row, 0.0, HIGHLIGHT);
                    }
                    let colour = if hot { Color32::WHITE } else { Color32::BLACK };
                    ui.painter().text(shift(row.left_center(), 3.0, 0.0), egui::Align2::LEFT_CENTER, fmt(v), egui::FontId::proportional(12.0), colour);
                    if resp.clicked() {
                        *value = v;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Courier New, the original's monospace face, read from the Windows fonts
/// folder at runtime when present (it is never bundled). Otherwise egui's own
/// monospace font stays in use.
pub fn load_monospace_font(ctx: &egui::Context) {
    let Some(windir) = std::env::var_os("WINDIR") else { return };
    let Ok(bytes) = std::fs::read(std::path::Path::new(&windir).join("Fonts").join("cour.ttf")) else { return };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("courier".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        list.insert(0, "courier".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// A menu entry with a tick drawn in front of it while `on`. The tick is
/// painted, not a text glyph, because egui's bundled fonts lack U+2713 and
/// would show an empty box. Returns true when clicked.
pub fn check_item(ui: &mut egui::Ui, on: bool, enabled: bool, text: &str, shortcut: Option<String>) -> bool {
    let mut button = egui::Button::new(format!("    {text}"));
    if let Some(sc) = shortcut {
        button = button.shortcut_text(sc);
    }
    let resp = ui.add_enabled(enabled, button);
    if on {
        let colour = if enabled { Color32::BLACK } else { Color32::from_rgb(0x8A, 0x8A, 0x8A) };
        let p = shift(resp.rect.left_center(), 8.0, 0.0);
        ui.painter().add(egui::Shape::line(vec![shift(p, 0.0, 0.0), shift(p, 3.0, 3.5), shift(p, 8.0, -4.0)], Stroke::new(1.8, colour)));
    }
    resp.clicked()
}

/// A spin box over `rect`: a white sunken number field (typeable) with a small
/// up and a down arrow button stacked on its right. Returns whether `value` changed.
pub fn spin_u64(ui: &mut egui::Ui, rect: egui::Rect, id: &str, value: &mut u64, max: u64, step: u64, enabled: bool) -> bool {
    let arrow_w = 15.0;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, if enabled { Color32::WHITE } else { BTN_FACE });
    sunken_border(&painter, rect);
    let inner = rect.shrink(2.0);
    let (up, down) = {
        let mid = inner.center().y;
        let left = inner.right() - arrow_w;
        (egui::Rect::from_min_max(egui::pos2(left, inner.top()), egui::pos2(inner.right(), mid)), egui::Rect::from_min_max(egui::pos2(left, mid), inner.max))
    };
    let text_rect = egui::Rect::from_min_max(inner.min, egui::pos2(up.left(), inner.bottom()));
    let mut changed = false;

    // The text field keeps what is being typed until it loses focus.
    let edit_id = ui.scope_id().with(("spin_edit", id));
    let focused = ui.memory(|m| m.has_focus(edit_id));
    let mut buf = if focused { ui.data(|d| d.get_temp::<String>(edit_id)).unwrap_or_else(|| value.to_string()) } else { value.to_string() };
    let edit = egui::TextEdit::singleline(&mut buf).id(edit_id).frame(egui::Frame::NONE).margin(egui::Margin::symmetric(3, 0)).align(egui::Align2::LEFT_CENTER);
    let resp = ui.add_enabled_ui(enabled, |ui| ui.put(text_rect, edit)).inner;
    if resp.changed()
        && let Ok(v) = buf.trim().parse::<u64>()
    {
        *value = v.min(max);
        changed = true;
    }
    if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(edit_id, buf));
    } else {
        ui.data_mut(|d| d.remove_temp::<String>(edit_id));
    }

    for (r, is_up) in [(up, true), (down, false)] {
        let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
        let b = ui.interact(r, ui.scope_id().with(("spin_btn", id, is_up)), sense);
        let pressed = enabled && b.is_pointer_button_down_on();
        painter.rect_filled(r, 0.0, if enabled && b.hovered() { HOT_FACE } else { BTN_FACE });
        let (tl, br) = if pressed { (BTN_SHADOW, Color32::WHITE) } else { (Color32::WHITE, Color32::from_rgb(0x69, 0x69, 0x69)) };
        painter.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0, tl));
        painter.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, tl));
        painter.line_segment([r.right_top(), r.right_bottom()], Stroke::new(1.0, br));
        painter.line_segment([r.right_bottom(), r.left_bottom()], Stroke::new(1.0, br));
        let c = r.center();
        let (a, bb, tip) = if is_up { (shift(c, -3.0, 1.5), shift(c, 3.0, 1.5), shift(c, 0.0, -1.5)) } else { (shift(c, -3.0, -1.5), shift(c, 3.0, -1.5), shift(c, 0.0, 1.5)) };
        let colour = if enabled { Color32::BLACK } else { Color32::from_rgb(0xA0, 0xA0, 0xA0) };
        painter.add(egui::Shape::convex_polygon(vec![a, bb, tip], colour, Stroke::NONE));
        if b.clicked() {
            *value = if is_up { value.saturating_add(step).min(max) } else { value.saturating_sub(step) };
            changed = true;
        }
    }
    changed
}
