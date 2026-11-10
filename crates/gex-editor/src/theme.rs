//! The editor's look: Monokai Black colors (black, warm grays, Monokai's pink, green,
//! orange, yellow, purple and cyan), a soft gradient backdrop behind everything, and panels
//! of very slightly see-through "glass" over it.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Shape, Stroke, Visuals};

// Monokai's colors.
pub const MK_PINK: Color32 = Color32::from_rgb(0xf9, 0x26, 0x72);
pub const MK_GREEN: Color32 = Color32::from_rgb(0xa6, 0xe2, 0x2e);
pub const MK_ORANGE: Color32 = Color32::from_rgb(0xfd, 0x97, 0x1f);
pub const MK_YELLOW: Color32 = Color32::from_rgb(0xe6, 0xdb, 0x74);
pub const MK_PURPLE: Color32 = Color32::from_rgb(0xae, 0x81, 0xff);
pub const MK_CYAN: Color32 = Color32::from_rgb(0x66, 0xd9, 0xef);
/// Selection, and the current line.
pub const MK_SELECTION: Color32 = Color32::from_rgb(0x49, 0x48, 0x3e);
pub const MK_LINE: Color32 = Color32::from_rgb(0x3e, 0x3d, 0x32);

pub const BG_DEEP: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const ACCENT: Color32 = MK_CYAN;
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2b, 0x8a, 0x9c);
pub const TEXT: Color32 = Color32::from_rgb(0xf8, 0xf8, 0xf2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x90, 0x90, 0x84);
pub const GOOD: Color32 = MK_GREEN;
pub const WARN: Color32 = MK_ORANGE;
pub const BAD: Color32 = MK_PINK;

// Code colors (as Monokai uses them).
pub const C_KEYWORD: Color32 = MK_PINK;
pub const C_BUILTIN: Color32 = MK_CYAN;
pub const C_ENGINE: Color32 = MK_GREEN;
pub const C_EVENT: Color32 = MK_ORANGE;
pub const C_STRING: Color32 = MK_YELLOW;
pub const C_NUMBER: Color32 = MK_PURPLE;
pub const C_COMMENT: Color32 = Color32::from_rgb(0x75, 0x71, 0x5e);

/// A color with a new alpha (0-255).
pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Mixes two colors (t = 0 gives a, 1 gives b).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// The see-through fill of panels.
pub fn glass_fill() -> Color32 {
    alpha(Color32::from_rgb(0x14, 0x14, 0x11), 196)
}

/// The faint light edge glass has.
pub fn glass_edge() -> Stroke {
    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 14))
}

/// A top-to-bottom gradient filling a rectangle.
pub fn vgradient(rect: Rect, top: Color32, bottom: Color32) -> Shape {
    let mut m = egui::Mesh::default();
    m.colored_vertex(rect.left_top(), top);
    m.colored_vertex(rect.right_top(), top);
    m.colored_vertex(rect.right_bottom(), bottom);
    m.colored_vertex(rect.left_bottom(), bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    Shape::mesh(m)
}

/// A soft round glow: `color` in the middle fading to nothing at `radius` (an ellipse when the
/// two radii differ).
pub fn glow(center: Pos2, radius: egui::Vec2, color: Color32) -> Shape {
    let mut m = egui::Mesh::default();
    m.colored_vertex(center, color);
    let n = 48;
    for i in 0..=n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        m.colored_vertex(center + egui::vec2(a.cos() * radius.x, a.sin() * radius.y), Color32::TRANSPARENT);
        if i > 0 {
            m.add_triangle(0, i, i + 1);
        }
    }
    Shape::mesh(m)
}

/// The backdrop behind the whole window: black with a faint warm-gray gradient and a couple
/// of very faint glows. Call first each frame; panels drawn after it let it show through.
pub fn backdrop(ctx: &egui::Context) {
    let r = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::background());
    p.add(vgradient(r, Color32::from_rgb(0x1b, 0x1b, 0x17), Color32::from_rgb(0x00, 0x00, 0x00)));
    p.add(glow(r.left_top() + egui::vec2(r.width() * 0.18, r.height() * 0.05), egui::vec2(r.width() * 0.6, r.height() * 0.55), alpha(Color32::from_rgb(0x3e, 0x3d, 0x32), 150)));
    p.add(glow(r.right_bottom() - egui::vec2(r.width() * 0.1, r.height() * 0.05), egui::vec2(r.width() * 0.5, r.height() * 0.45), alpha(MK_PURPLE, 18)));
}

/// A glass header strip (menu bar, status bar): a slight top-lit gradient and a hairline.
pub fn header(rect: Rect, top_edge: bool) -> Shape {
    let g = vgradient(rect, alpha(Color32::from_rgb(0x22, 0x22, 0x1e), 240), alpha(Color32::from_rgb(0x0e, 0x0e, 0x0c), 240));
    let y = if top_edge { rect.top() + 0.5 } else { rect.bottom() - 0.5 };
    Shape::Vec(vec![g, Shape::line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, if top_edge { 18 } else { 10 })))])
}

/// A big gradient button, like Steam's PLAY: `top`/`bottom` are its colors.
pub fn gradient_button(ui: &mut egui::Ui, text: &str, top: Color32, bottom: Color32) -> egui::Response {
    let galley = egui::WidgetText::from(egui::RichText::new(text).strong().size(14.0)).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let size = galley.size() + egui::vec2(28.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (mut t, mut b) = (top, bottom);
        if resp.is_pointer_button_down_on() {
            (t, b) = (mix(bottom, Color32::BLACK, 0.1), mix(top, Color32::BLACK, 0.1));
        } else if resp.hovered() {
            (t, b) = (mix(top, Color32::WHITE, 0.15), mix(bottom, Color32::WHITE, 0.15));
        }
        let p = ui.painter();
        p.add(vgradient(rect, t, b));
        // Glass: a light line along the top edge.
        p.line_segment([rect.left_top() + egui::vec2(1.0, 0.5), rect.right_top() + egui::vec2(-1.0, 0.5)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 60)));
        // Dark text on light buttons, light text on dark ones.
        let lum = 0.299 * top.r() as f32 + 0.587 * top.g() as f32 + 0.114 * top.b() as f32;
        p.galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, if lum > 150.0 { Color32::from_rgb(0x10, 0x10, 0x0d) } else { Color32::WHITE });
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn apply(ctx: &egui::Context) {
    // Always dark, whatever the system's light/dark setting (Windows in light mode would
    // otherwise switch to egui's light look under ours).
    ctx.set_theme(egui::Theme::Dark);
    let mut v = Visuals::dark();
    // See-through panels over the gradient backdrop.
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = alpha(Color32::from_rgb(0x1a, 0x1a, 0x17), 245);
    v.extreme_bg_color = alpha(Color32::from_rgb(0x00, 0x00, 0x00), 200);
    v.faint_bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 6);
    v.code_bg_color = alpha(BG_DEEP, 200);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = alpha(MK_SELECTION, 230);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 22));
    v.window_corner_radius = CornerRadius::same(6);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(100) };
    v.menu_corner_radius = CornerRadius::same(5);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = Color32::TRANSPARENT;
    w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 14));
    // Buttons and fields: warm gray glass, lighter when hovered, with a cyan edge.
    w.inactive.bg_fill = alpha(MK_LINE, 170);
    w.inactive.weak_bg_fill = alpha(MK_LINE, 130);
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 12));
    w.hovered.bg_fill = alpha(MK_SELECTION, 220);
    w.hovered.weak_bg_fill = alpha(MK_SELECTION, 190);
    w.hovered.bg_stroke = Stroke::new(1.0, alpha(MK_CYAN, 110));
    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.open.weak_bg_fill = alpha(MK_SELECTION, 190);
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::same(3);
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
    });
}
