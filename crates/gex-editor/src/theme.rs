//! The editor's look: Monokai Black colors (black, neutral grays, Monokai's pink, green,
//! orange, yellow, purple and cyan), square corners everywhere, and gradients kept to three
//! places: the backdrop (seen faintly through the panels), header strips, and primary buttons.

use crate::docs::Tip as _;
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Shape, Stroke, Visuals};

// Monokai's colors.
pub const MK_PINK: Color32 = Color32::from_rgb(0xf9, 0x26, 0x72);
pub const MK_GREEN: Color32 = Color32::from_rgb(0xa6, 0xe2, 0x2e);
pub const MK_ORANGE: Color32 = Color32::from_rgb(0xfd, 0x97, 0x1f);
pub const MK_YELLOW: Color32 = Color32::from_rgb(0xe6, 0xdb, 0x74);
pub const MK_PURPLE: Color32 = Color32::from_rgb(0xae, 0x81, 0xff);
pub const MK_CYAN: Color32 = Color32::from_rgb(0x66, 0xd9, 0xef);

/// Neutral grays for controls.
pub const GRAY: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
pub const GRAY_LIGHT: Color32 = Color32::from_rgb(0x4a, 0x4a, 0x4a);
pub const BG_DEEP: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const ACCENT: Color32 = MK_CYAN;
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2b, 0x8a, 0x9c);
pub const TEXT: Color32 = Color32::from_rgb(0xf8, 0xf8, 0xf2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8c, 0x8c, 0x8c);
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
    alpha(Color32::from_rgb(0x11, 0x11, 0x11), 215)
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

/// The backdrop behind the whole window: neutral dark gray at the top fading to black.
pub fn backdrop(ctx: &egui::Context) {
    let r = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::background()).add(vgradient(r, Color32::from_rgb(0x1e, 0x1e, 0x1e), Color32::from_rgb(0x04, 0x04, 0x04)));
}

/// A header strip (menu bar, status bar, window titles): a slight shade and a hairline.
pub fn header(rect: Rect, top_edge: bool) -> Shape {
    let y = if top_edge { rect.top() + 0.5 } else { rect.bottom() - 0.5 };
    Shape::Vec(vec![
        vgradient(rect, Color32::from_rgb(0x26, 0x26, 0x26), Color32::from_rgb(0x17, 0x17, 0x17)),
        Shape::line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 14))),
    ])
}

/// A primary button (Play, Stop, Create, Open): the one place color fills a control.
pub fn primary_button(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let galley = egui::WidgetText::from(egui::RichText::new(text).strong()).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let size = galley.size() + egui::vec2(24.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let mut c = color;
        if resp.is_pointer_button_down_on() {
            c = mix(color, Color32::BLACK, 0.2);
        } else if resp.hovered() {
            c = mix(color, Color32::WHITE, 0.12);
        }
        let p = ui.painter();
        p.add(vgradient(rect, mix(c, Color32::WHITE, 0.08), mix(c, Color32::BLACK, 0.22)));
        let lum = 0.299 * color.r() as f32 + 0.587 * color.g() as f32 + 0.114 * color.b() as f32;
        p.galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, if lum > 150.0 { Color32::from_rgb(0x10, 0x10, 0x10) } else { Color32::WHITE });
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A panel or section title: small dim capitals, the same everywhere.
pub fn title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text.to_uppercase()).small().strong().color(TEXT_DIM))
}

/// A menu entry with its keyboard shortcut shown at the right.
pub fn menu_item(ui: &mut egui::Ui, text: &str, shortcut: &str) -> egui::Response {
    ui.add(egui::Button::new(text).shortcut_text(egui::RichText::new(shortcut).color(TEXT_DIM)))
}

pub fn apply(ctx: &egui::Context) {
    // Always dark, whatever the system's light/dark setting (Windows in light mode would
    // otherwise switch to egui's light look under ours).
    ctx.set_theme(egui::Theme::Dark);
    let mut v = Visuals::dark();
    // See-through panels over the gradient backdrop.
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::from_rgb(0x1a, 0x1a, 0x1a);
    v.extreme_bg_color = alpha(Color32::from_rgb(0x00, 0x00, 0x00), 200);
    v.faint_bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 6);
    v.code_bg_color = alpha(BG_DEEP, 200);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = alpha(GRAY_LIGHT, 230);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 22));
    v.window_corner_radius = CornerRadius::ZERO;
    v.window_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(100) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(100) };
    v.menu_corner_radius = CornerRadius::ZERO;
    v.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = Color32::TRANSPARENT;
    w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 14));
    // Buttons and fields: neutral gray, lighter when hovered, with a cyan edge.
    w.inactive.bg_fill = alpha(GRAY, 170);
    w.inactive.weak_bg_fill = alpha(GRAY, 130);
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 12));
    w.hovered.bg_fill = alpha(GRAY_LIGHT, 220);
    w.hovered.weak_bg_fill = alpha(GRAY_LIGHT, 190);
    w.hovered.bg_stroke = Stroke::new(1.0, alpha(MK_CYAN, 110));
    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.open.weak_bg_fill = alpha(GRAY_LIGHT, 190);
    for s in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::ZERO;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
    });
}

/// The window's own minimize, maximize and close buttons (the system title bar is off to save
/// height). Call inside a right-to-left layout.
pub fn window_buttons(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    ui.spacing_mut().item_spacing.x = 0.0;
    for (i, tip) in ["Close", if maximized { "Restore" } else { "Maximize" }, "Minimize"].into_iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(34.0, 22.0), egui::Sense::click());
        let hot = resp.hovered();
        let p = ui.painter();
        if hot {
            p.rect_filled(rect, 0.0, if i == 0 { Color32::from_rgb(0xc8, 0x1e, 0x3c) } else { GRAY_LIGHT });
        }
        let st = Stroke::new(1.0, if hot { Color32::WHITE } else { TEXT_DIM });
        let c = rect.center();
        match i {
            0 => {
                p.line_segment([c + egui::vec2(-4.0, -4.0), c + egui::vec2(4.5, 4.5)], st);
                p.line_segment([c + egui::vec2(-4.0, 4.0), c + egui::vec2(4.5, -4.5)], st);
            }
            1 => {
                p.rect_stroke(Rect::from_center_size(c, egui::vec2(9.0, 9.0)), 0.0, st, egui::StrokeKind::Middle);
            }
            _ => {
                p.line_segment([c + egui::vec2(-4.5, 0.5), c + egui::vec2(4.5, 0.5)], st);
            }
        }
        if resp.tip(tip).clicked() {
            ctx.send_viewport_cmd(match i {
                0 => egui::ViewportCommand::Close,
                1 => egui::ViewportCommand::Maximized(!maximized),
                _ => egui::ViewportCommand::Minimized(true),
            });
        }
    }
    ui.add_space(4.0);
}

/// The empty part of the top bar moves the window (double-click: maximize). Call before
/// the bar's own widgets so they stay clickable.
pub fn window_drag(ui: &mut egui::Ui, rect: Rect) {
    let r = ui.interact(rect, egui::Id::new("window drag"), egui::Sense::click_and_drag());
    if r.double_clicked() {
        let m = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
    } else if r.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// Without the system frame the window's edges are ours to make resizable.
pub fn window_resize(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection as D;
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let r = ctx.content_rect();
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let m = 5.0;
    let (l, rt, t, b) = (p.x < r.left() + m, p.x > r.right() - m, p.y < r.top() + m, p.y > r.bottom() - m);
    let (dir, cur) = match (l, rt, t, b) {
        (true, _, true, _) => (D::NorthWest, egui::CursorIcon::ResizeNwSe),
        (_, true, true, _) => (D::NorthEast, egui::CursorIcon::ResizeNeSw),
        (true, _, _, true) => (D::SouthWest, egui::CursorIcon::ResizeNeSw),
        (_, true, _, true) => (D::SouthEast, egui::CursorIcon::ResizeNwSe),
        (true, ..) => (D::West, egui::CursorIcon::ResizeHorizontal),
        (_, true, ..) => (D::East, egui::CursorIcon::ResizeHorizontal),
        (_, _, true, _) => (D::North, egui::CursorIcon::ResizeVertical),
        (_, _, _, true) => (D::South, egui::CursorIcon::ResizeVertical),
        _ => return,
    };
    ctx.set_cursor_icon(cur);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
}

/// Fades in whatever is drawn in this ui from the moment it first shows (menus, dialogs,
/// windows): call at the top of its contents. Gives how far in it is (0..1).
pub fn fade_in(ui: &mut egui::Ui) -> f32 {
    let id = ui.id().with("fade in");
    let (now, frame) = (ui.input(|i| i.time), ui.ctx().cumulative_frame_nr());
    // (when it appeared, the last frame it was drawn)
    let (start, last) = ui.data(|d| d.get_temp::<(f64, u64)>(id)).unwrap_or((now, 0));
    let start = if frame > last + 1 { now } else { start };
    ui.data_mut(|d| d.insert_temp(id, (start, frame)));
    let t = (((now - start) / 0.11) as f32).clamp(0.0, 1.0);
    if t < 1.0 {
        ui.set_opacity(0.15 + 0.85 * t);
        ui.ctx().request_repaint();
    }
    t
}

/// What a tab did this frame.
pub struct TabOut {
    pub resp: egui::Response,
    /// Its close cross was clicked.
    pub close: bool,
}

/// One tab, the same everywhere: a trapezoid, lit when it's the open one; optionally a cross
/// to close it and a dot for unsaved changes.
pub fn tab(ui: &mut egui::Ui, label: &str, active: bool, closeable: bool, dirty: bool) -> TabOut {
    let h = 24.0;
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), TEXT);
    let w = (galley.size().x + 28.0 + if closeable || dirty { 18.0 } else { 0.0 }).max(78.0);
    let (rect, r) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click_and_drag());
    let lit = ui.ctx().animate_bool_with_time(r.id, active, 0.12);
    let hover = ui.ctx().animate_bool_with_time(r.id.with("h"), r.hovered(), 0.1);
    let p = ui.painter();
    let base = mix(Color32::from_rgb(0x17, 0x17, 0x17), Color32::from_rgb(0x20, 0x20, 0x20), hover);
    // Wider at the bottom, where it joins the page.
    let slant = 9.0;
    let (top, bottom) = (rect.top() + if active { 0.0 } else { 2.0 }, rect.bottom());
    let shape = vec![Pos2::new(rect.left() - slant / 2.0, bottom), Pos2::new(rect.left() + slant / 2.0, top), Pos2::new(rect.right() - slant / 2.0, top), Pos2::new(rect.right() + slant / 2.0, bottom)];
    p.add(Shape::convex_polygon(shape.clone(), mix(base, Color32::from_rgb(0x2a, 0x2a, 0x2a), lit), Stroke::NONE));
    p.add(Shape::line(shape, Stroke::new(1.0, Color32::from_rgb(0x34, 0x34, 0x34))));
    p.galley(Pos2::new(rect.left() + 12.0, (top + bottom) / 2.0 - galley.size().y / 2.0), galley, if active { TEXT } else { TEXT_DIM });
    let x = Rect::from_center_size(Pos2::new(rect.right() - 16.0, rect.center().y + 1.0), egui::vec2(16.0, 16.0));
    let over_x = closeable && ui.rect_contains_pointer(x);
    if closeable && (r.hovered() || !dirty) && (r.hovered() || active) {
        let c = if over_x { TEXT } else { TEXT_DIM };
        if over_x {
            p.rect_filled(x, 0.0, GRAY_LIGHT);
        }
        let (c0, d) = (x.center(), 3.5);
        p.line_segment([c0 + egui::vec2(-d, -d), c0 + egui::vec2(d, d)], Stroke::new(1.2, c));
        p.line_segment([c0 + egui::vec2(-d, d), c0 + egui::vec2(d, -d)], Stroke::new(1.2, c));
    } else if dirty {
        p.circle_filled(x.center(), 3.5, WARN);
    }
    let close = over_x && r.clicked();
    TabOut { resp: r, close }
}

/// A row of tabs that only switch between pages (no closing): gives the page picked.
pub fn tab_row(ui: &mut egui::Ui, labels: &[&str], current: &mut usize) {
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.add_space(6.0);
        for (i, l) in labels.iter().enumerate() {
            if tab(ui, l, *current == i, false, false).resp.clicked() {
                *current = i;
            }
        }
    });
    // The line the tabs sit on.
    let y = row.response.rect.bottom();
    ui.painter().line_segment([Pos2::new(ui.max_rect().left(), y), Pos2::new(ui.max_rect().right(), y)], Stroke::new(1.0, Color32::from_rgb(0x34, 0x34, 0x34)));
    ui.add_space(4.0);
}

/// The colors a row can be highlighted with (right-click > Color).
pub const HIGHLIGHTS: [(&str, &str); 12] = [
    ("Red", "#f92672"), ("Orange", "#fd971f"), ("Yellow", "#e6db74"), ("Green", "#a6e22e"), ("Teal", "#35c9a5"), ("Blue", "#66d9ef"),
    ("Indigo", "#5c7cfa"), ("Purple", "#ae81ff"), ("Pink", "#ff79c6"), ("Brown", "#a5714e"), ("Gray", "#8f908a"), ("White", "#f8f8f2"),
];

/// "#rrggbb" as a color.
pub fn hex(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok().filter(|_| h.len() == 6)?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// The "Color" entry of a right-click menu: a grid of swatches and "None". Gives the pick
/// (Some(None) = take the color away).
pub fn color_menu(ui: &mut egui::Ui) -> Option<Option<String>> {
    let mut pick = None;
    ui.menu_button("Color", |ui| {
        fade_in(ui);
        egui::Grid::new("highlights").min_col_width(20.0).spacing(egui::vec2(3.0, 3.0)).show(ui, |ui| {
            for (i, (name, h)) in HIGHLIGHTS.iter().enumerate() {
                let (rect, r) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                ui.painter().rect_filled(rect, 0.0, hex(h).unwrap());
                if r.hovered() {
                    ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
                }
                if r.on_hover_text(*name).clicked() {
                    pick = Some(Some(h.to_string()));
                }
                if i % 6 == 5 {
                    ui.end_row();
                }
            }
        });
        if ui.button("None").clicked() {
            pick = Some(None);
        }
        if pick.is_some() {
            ui.close();
        }
    });
    pick
}

/// Draws a row with a highlight color behind it, across the panel.
pub fn tinted(ui: &mut egui::Ui, color: Option<Color32>, row: impl FnOnce(&mut egui::Ui) -> egui::Response) -> egui::Response {
    let Some(c) = color else { return row(ui) };
    let slot = ui.painter().add(egui::Shape::Noop);
    let r = row(ui);
    let rect = Rect::from_min_max(egui::pos2(r.rect.left(), r.rect.top()), egui::pos2(ui.clip_rect().right(), r.rect.bottom()));
    ui.painter().set(slot, egui::Shape::rect_filled(rect, 0.0, c.gamma_multiply(0.20)));
    r
}
