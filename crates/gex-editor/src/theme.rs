//! The editor's look: a dark, Steam-like slate palette with a blue accent.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

pub const BG_DEEP: Color32 = Color32::from_rgb(0x12, 0x15, 0x1b);
pub const BG: Color32 = Color32::from_rgb(0x17, 0x1d, 0x25);
pub const PANEL: Color32 = Color32::from_rgb(0x1b, 0x28, 0x38);
pub const WIDGET: Color32 = Color32::from_rgb(0x24, 0x34, 0x47);
pub const WIDGET_HOVER: Color32 = Color32::from_rgb(0x2a, 0x47, 0x5e);
pub const ACCENT: Color32 = Color32::from_rgb(0x66, 0xc0, 0xf4);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1a, 0x9f, 0xff);
pub const TEXT: Color32 = Color32::from_rgb(0xc7, 0xd5, 0xe0);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8f, 0x98, 0xa0);
pub const GOOD: Color32 = Color32::from_rgb(0xa4, 0xd0, 0x07);
pub const WARN: Color32 = Color32::from_rgb(0xe5, 0xb1, 0x3a);
pub const BAD: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x4f);

// Code colors.
pub const C_KEYWORD: Color32 = Color32::from_rgb(0x66, 0xc0, 0xf4);
pub const C_BUILTIN: Color32 = Color32::from_rgb(0xe5, 0xc0, 0x7b);
pub const C_ENGINE: Color32 = Color32::from_rgb(0xa4, 0xd0, 0x07);
pub const C_EVENT: Color32 = Color32::from_rgb(0xff, 0x9e, 0x64);
pub const C_STRING: Color32 = Color32::from_rgb(0x98, 0xc3, 0x79);
pub const C_NUMBER: Color32 = Color32::from_rgb(0xd1, 0x9a, 0x66);
pub const C_COMMENT: Color32 = Color32::from_rgb(0x5c, 0x6a, 0x7a);

pub fn apply(ctx: &egui::Context) {
    // Always dark, whatever the system's light/dark setting (Windows in light mode would
    // otherwise switch to egui's light look under ours).
    ctx.set_theme(egui::Theme::Dark);
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = BG;
    v.extreme_bg_color = BG_DEEP;
    v.faint_bg_color = Color32::from_rgb(0x1f, 0x2d, 0x3e);
    v.code_bg_color = BG_DEEP;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(0x1e, 0x4f, 0x74);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x2a, 0x3a, 0x4d));
    v.window_corner_radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x24, 0x33, 0x45));
    w.inactive.bg_fill = WIDGET;
    w.inactive.weak_bg_fill = WIDGET;
    w.hovered.bg_fill = WIDGET_HOVER;
    w.hovered.weak_bg_fill = WIDGET_HOVER;
    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::same(3);
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
    });
}
