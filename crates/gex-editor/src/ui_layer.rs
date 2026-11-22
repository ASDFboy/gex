//! Draws UI objects (Panel, Button, Label, TextInput, ProgressBar, Image, Checkbox, Slider,
//! ScrollBox) with egui on top of the game view, and turns clicks, drags, scrolling and typing
//! into events for the game. (Rows, Columns and Grids only place things: gex_core::render.)

use std::collections::HashMap;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke};
use gex_core::glam::Vec2;
use gex_core::render::UiItem;
use gex_core::ObjectKind;
use gex_script::UiEvent;

fn c32(c: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (c[3] * 255.0) as u8)
}

/// `to_screen` maps a game-window pixel to a point on screen; `scale` is points per
/// game pixel. With `live` off (editing), nothing reacts to the mouse.
pub fn paint_ui(
    ui: &mut egui::Ui,
    items: &[UiItem],
    design: Vec2,
    to_screen: &dyn Fn(Vec2) -> Pos2,
    scale: f32,
    live: bool,
    images: &mut HashMap<String, egui::TextureHandle>,
    vfs: &gex_assets::Vfs,
) -> Vec<UiEvent> {
    let mut events = Vec::new();
    let base = ui.painter().clone();
    for it in items {
        // Each item's opacity (its own, times its parents' modulate alpha).
        let mut painter = base.clone();
        painter.multiply_opacity(it.num("opacity", 1.0).clamp(0.0, 1.0) as f32);
        let tl = it.top_left(design);
        let rect = Rect::from_min_max(to_screen(tl), to_screen(tl + it.size));
        // Inside a ScrollBox: only the part within it shows (and can be clicked).
        let shown = match it.clip {
            Some((a, b)) => {
                let c = Rect::from_min_max(to_screen(a), to_screen(b));
                if !c.intersects(rect) {
                    continue;
                }
                painter.set_clip_rect(c.intersect(painter.clip_rect()));
                c.intersect(rect)
            }
            None => rect,
        };
        let painter = painter;
        let fam = crate::text::family(ui.ctx(), vfs, it.text("font"));
        let font = |size: f64| egui::FontId::new(((size as f32) * scale).max(4.0), fam.clone());
        match it.kind {
            ObjectKind::Panel if !it.text("texture").is_empty() => {
                let tex = image(ui, images, vfs, it.text("texture"));
                nine_slice(&painter, &tex, rect, it.num("slice", 0.0) as f32, scale, c32(it.color("color")));
            }
            ObjectKind::Panel => {
                let r = CornerRadius::same((it.num("radius", 4.0) as f32 * scale).clamp(0.0, 255.0) as u8);
                painter.rect_filled(rect, r, c32(it.color("color")));
                let b = it.num("border", 1.0) as f32 * scale;
                if b > 0.0 {
                    painter.rect_stroke(rect, r, Stroke::new(b, c32(it.color("border_color"))), egui::StrokeKind::Inside);
                }
            }
            ObjectKind::Label => {
                let align = match it.text("align") {
                    "center" => egui::Align2::CENTER_TOP,
                    "right" => egui::Align2::RIGHT_TOP,
                    _ => egui::Align2::LEFT_TOP,
                };
                let at = match it.text("align") {
                    "center" => to_screen(Vec2::new(tl.x + it.size.x / 2.0, tl.y)),
                    "right" => to_screen(Vec2::new(tl.x + it.size.x, tl.y)),
                    _ => rect.min,
                };
                // A Label given a width wraps its lines to fit it.
                let wrap = it.props.contains_key("width").then(|| it.size.x * scale);
                crate::text::paint_wrapped(&painter, at, align, it.text("text"), font(it.num("size", 18.0)), c32(it.color("color")), scale, wrap);
            }
            ObjectKind::Button => {
                let disabled = it.props.get("disabled").is_some_and(|v| v.as_bool());
                let id = ui.id().with(("ui button", it.object));
                let resp = if live && !disabled { Some(ui.interact(shown, id, egui::Sense::click())) } else { None };
                let mut fill = c32(it.color("color"));
                if let Some(r) = &resp {
                    if r.is_pointer_button_down_on() {
                        fill = fill.gamma_multiply(0.75);
                    } else if r.hovered() {
                        fill = Color32::from_rgb(fill.r().saturating_add(25), fill.g().saturating_add(25), fill.b().saturating_add(25));
                    }
                    if r.clicked() {
                        events.push(UiEvent::Pressed(it.object));
                    }
                }
                if disabled {
                    fill = fill.gamma_multiply(0.5);
                }
                if it.text("texture").is_empty() {
                    painter.rect_filled(rect, CornerRadius::same((4.0 * scale) as u8), fill);
                } else {
                    let tex = image(ui, images, vfs, it.text("texture"));
                    nine_slice(&painter, &tex, rect, it.num("slice", 0.0) as f32, scale, fill);
                }
                crate::text::paint(&painter, rect.center(), egui::Align2::CENTER_CENTER, it.text("text"), font(it.num("size", 16.0)), c32(it.color("text_color")), scale);
            }
            ObjectKind::ProgressBar => {
                let f = (it.num("value", 0.5) / it.num("max", 1.0).max(1e-9)).clamp(0.0, 1.0) as f32;
                painter.rect_filled(rect, CornerRadius::same(3), c32(it.color("back_color")));
                let mut fill = rect;
                fill.set_width(rect.width() * f);
                painter.rect_filled(fill, CornerRadius::same(3), c32(it.color("color")));
            }
            ObjectKind::Image => {
                let path = it.text("texture").to_string();
                if path.is_empty() {
                    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
                } else {
                    let tex = image(ui, images, vfs, &path);
                    nine_slice(&painter, &tex, rect, it.num("slice", 0.0) as f32, scale, c32(it.color("color")));
                }
            }
            ObjectKind::TextInput => {
                let size = it.num("size", 16.0);
                if live {
                    let id = egui::Id::new(("ui text", it.object));
                    let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| it.text("text").to_string());
                    let has_focus = ui.memory(|m| m.has_focus(id.with("edit")));
                    if !has_focus {
                        text = it.text("text").to_string();
                    }
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    child.set_clip_rect(painter.clip_rect());
                    let r = child.add_sized(
                        rect.size(),
                        egui::TextEdit::singleline(&mut text).id(id.with("edit")).hint_text(it.text("placeholder")).font(font(size)),
                    );
                    if r.changed() {
                        events.push(UiEvent::Changed(it.object, text.clone()));
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        events.push(UiEvent::Submitted(it.object, text.clone()));
                    }
                    ui.data_mut(|d| d.insert_temp(id, text));
                } else {
                    painter.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(0x10, 0x10, 0x0e));
                    painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
                    let (t, c) = if it.text("text").is_empty() { (it.text("placeholder"), Color32::GRAY) } else { (it.text("text"), Color32::WHITE) };
                    painter.text(rect.left_center() + egui::vec2(6.0 * scale, 0.0), egui::Align2::LEFT_CENTER, t, font(size), c);
                }
            }
            ObjectKind::Checkbox => {
                let disabled = it.props.get("disabled").is_some_and(|v| v.as_bool());
                let mut checked = it.props.get("checked").is_some_and(|v| v.as_bool());
                let side = (it.num("size", 16.0) as f32 * 1.15 * scale).min(rect.height());
                let b = Rect::from_center_size(Pos2::new(rect.left() + side / 2.0, rect.center().y), egui::vec2(side, side));
                let mut fill = c32(it.color("color"));
                if live && !disabled {
                    let r = ui.interact(shown, ui.id().with(("ui checkbox", it.object)), egui::Sense::click());
                    if r.hovered() {
                        fill = Color32::from_rgb(fill.r().saturating_add(25), fill.g().saturating_add(25), fill.b().saturating_add(25));
                    }
                    if r.clicked() {
                        checked = !checked;
                        events.push(UiEvent::Toggled(it.object, checked));
                    }
                }
                let dim = if disabled { 0.5 } else { 1.0 };
                painter.rect_filled(b, CornerRadius::same((2.0 * scale) as u8), fill.gamma_multiply(dim));
                painter.rect_stroke(b, CornerRadius::same((2.0 * scale) as u8), Stroke::new(scale.max(1.0), c32(it.color("check_color")).gamma_multiply(0.6 * dim)), egui::StrokeKind::Inside);
                if checked {
                    let at = |x: f32, y: f32| Pos2::new(b.left() + b.width() * x, b.top() + b.height() * y);
                    painter.add(egui::Shape::line(vec![at(0.22, 0.52), at(0.42, 0.72), at(0.78, 0.3)], Stroke::new((2.0 * scale).max(1.5), c32(it.color("check_color")).gamma_multiply(dim))));
                }
                let text = c32(it.color("text_color")).gamma_multiply(dim);
                crate::text::paint(&painter, Pos2::new(b.right() + 8.0 * scale, rect.center().y), egui::Align2::LEFT_CENTER, it.text("text"), font(it.num("size", 16.0)), text, scale);
            }
            ObjectKind::Slider => {
                let disabled = it.props.get("disabled").is_some_and(|v| v.as_bool());
                let (min, max) = (it.num("min", 0.0), it.num("max", 1.0));
                let (lo, hi) = (min.min(max), min.max(max));
                let step = it.num("step", 0.0);
                let id = ui.id().with(("ui slider", it.object));
                let mut v = it.num("value", 0.5).clamp(lo, hi);
                let knob = rect.height() / 2.0;
                let (x0, x1) = (rect.left() + knob, rect.right() - knob);
                if live && !disabled {
                    let r = ui.interact(shown, id, egui::Sense::click_and_drag());
                    if let (true, Some(p)) = (r.dragged() || r.clicked(), r.interact_pointer_pos()) {
                        let f = ((p.x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0) as f64;
                        let mut nv = min + f * (max - min);
                        if step > 0.0 {
                            nv = (min + ((nv - min) / step).round() * step).clamp(lo, hi);
                        }
                        if (nv - v).abs() > 1e-9 {
                            events.push(UiEvent::Slid(it.object, nv));
                        }
                        // Shown where it's dragged to (the game catches up a frame later).
                        v = nv;
                    }
                }
                let dim = if disabled { 0.5 } else { 1.0 };
                let f = if (max - min).abs() < 1e-12 { 0.0 } else { ((v - min) / (max - min)).clamp(0.0, 1.0) as f32 };
                let track = Rect::from_min_max(Pos2::new(x0, rect.center().y - rect.height() * 0.17), Pos2::new(x1, rect.center().y + rect.height() * 0.17));
                painter.rect_filled(track, CornerRadius::same((track.height() / 2.0) as u8), c32(it.color("back_color")).gamma_multiply(dim));
                let mut done = track;
                done.set_width(track.width() * f);
                painter.rect_filled(done, CornerRadius::same((track.height() / 2.0) as u8), c32(it.color("color")).gamma_multiply(dim));
                painter.circle_filled(Pos2::new(x0 + (x1 - x0) * f, rect.center().y), knob * 0.9, c32(it.color("color")).gamma_multiply(dim));
            }
            ObjectKind::ScrollBox => {
                let bg = c32(it.color("color"));
                if bg.a() > 0 {
                    painter.rect_filled(rect, 0.0, bg);
                }
                let content = it.num("content_height", 0.0) as f32;
                let most = (content - it.size.y).max(0.0);
                if most <= 0.0 {
                    continue;
                }
                let id = ui.id().with(("ui scroll", it.object));
                // Scrolled here a moment ago: carry on from there (the game catches up a frame later).
                let now = ui.input(|i| i.time);
                let recent = ui.data(|d| d.get_temp::<(f32, f64)>(id)).filter(|(_, t)| now - t < 0.3).map(|(v, _)| v);
                let mut scroll = recent.unwrap_or(it.num("scroll", 0.0) as f32).clamp(0.0, most);
                let before = scroll;
                let bar_w = (6.0 * scale).max(3.0);
                let bar = Rect::from_min_max(Pos2::new(rect.right() - bar_w, rect.top()), rect.right_bottom());
                let thumb_h = (rect.height() * it.size.y / content).max(12.0 * scale).min(rect.height());
                if live {
                    if ui.rect_contains_pointer(shown) {
                        let d = ui.input(|i| i.smooth_scroll_delta.y);
                        if d != 0.0 {
                            scroll = (scroll - d / scale.max(0.01)).clamp(0.0, most);
                        }
                    }
                    let r = ui.interact(bar.intersect(shown), id.with("bar"), egui::Sense::drag());
                    if r.dragged() {
                        let room = (rect.height() - thumb_h).max(1.0);
                        scroll = (scroll + r.drag_delta().y / room * most).clamp(0.0, most);
                    }
                    if scroll != before {
                        let now = ui.input(|i| i.time);
                        ui.data_mut(|d| d.insert_temp(id, (scroll, now)));
                        events.push(UiEvent::Scrolled(it.object, scroll as f64));
                    }
                }
                let y = rect.top() + (rect.height() - thumb_h) * (scroll / most);
                painter.rect_filled(Rect::from_min_size(Pos2::new(bar.left(), y), egui::vec2(bar_w, thumb_h)), CornerRadius::same((bar_w / 2.0) as u8), c32(it.color("bar_color")));
            }
            _ => {}
        }
    }
    events
}

fn image(ui: &egui::Ui, images: &mut HashMap<String, egui::TextureHandle>, vfs: &gex_assets::Vfs, path: &str) -> egui::TextureHandle {
    images
        .entry(path.to_string())
        .or_insert_with(|| {
            let img = gex_assets::load_image(vfs, path).unwrap_or(gex_assets::Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] });
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
            ui.ctx().load_texture(path.to_string(), ci, egui::TextureOptions::NEAREST)
        })
        .clone()
}

/// Draws a picture stretched over `rect`, keeping its edges: `slice` texture pixels on each side
/// stay their size (scaled like the rest of the UI), and the middle stretches. 0 = plain stretch.
fn nine_slice(painter: &egui::Painter, tex: &egui::TextureHandle, rect: Rect, slice: f32, scale: f32, tint: Color32) {
    let [tw, th] = tex.size();
    let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    if slice <= 0.0 || tw == 0 || th == 0 {
        painter.image(tex.id(), rect, full, tint);
        return;
    }
    let (sx, sy) = (slice.min(tw as f32 / 2.0), slice.min(th as f32 / 2.0));
    let (ex, ey) = ((sx * scale).min(rect.width() / 2.0), (sy * scale).min(rect.height() / 2.0));
    let xs = [rect.min.x, rect.min.x + ex, rect.max.x - ex, rect.max.x];
    let ys = [rect.min.y, rect.min.y + ey, rect.max.y - ey, rect.max.y];
    let us = [0.0, sx / tw as f32, 1.0 - sx / tw as f32, 1.0];
    let vs = [0.0, sy / th as f32, 1.0 - sy / th as f32, 1.0];
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for j in 0..3 {
        for i in 0..3 {
            let r = Rect::from_min_max(Pos2::new(xs[i], ys[j]), Pos2::new(xs[i + 1], ys[j + 1]));
            let uv = Rect::from_min_max(Pos2::new(us[i], vs[j]), Pos2::new(us[i + 1], vs[j + 1]));
            if r.width() > 0.0 && r.height() > 0.0 {
                mesh.add_rect_with_uv(r, uv, tint);
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}
