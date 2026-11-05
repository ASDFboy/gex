//! Draws UI nodes (Panel, Button, Label, TextInput, ProgressBar, Image) with egui on top of
//! the game view, and turns clicks and typing into events for the game.

use std::collections::HashMap;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke};
use gex_core::glam::Vec2;
use gex_core::render::UiItem;
use gex_core::NodeKind;
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
    let painter = ui.painter().clone();
    for it in items {
        let tl = it.top_left(design);
        let rect = Rect::from_min_max(to_screen(tl), to_screen(tl + it.size));
        let font = |size: f64| egui::FontId::proportional(((size as f32) * scale).max(4.0));
        match it.kind {
            NodeKind::Panel => {
                let r = CornerRadius::same((it.num("radius", 4.0) as f32 * scale).clamp(0.0, 255.0) as u8);
                painter.rect_filled(rect, r, c32(it.color("color")));
                let b = it.num("border", 1.0) as f32 * scale;
                if b > 0.0 {
                    painter.rect_stroke(rect, r, Stroke::new(b, c32(it.color("border_color"))), egui::StrokeKind::Inside);
                }
            }
            NodeKind::Label => {
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
                painter.text(at, align, it.text("text"), font(it.num("size", 18.0)), c32(it.color("color")));
            }
            NodeKind::Button => {
                let disabled = it.props.get("disabled").is_some_and(|v| v.as_bool());
                let id = ui.id().with(("ui button", it.node));
                let resp = if live && !disabled { Some(ui.interact(rect, id, egui::Sense::click())) } else { None };
                let mut fill = c32(it.color("color"));
                if let Some(r) = &resp {
                    if r.is_pointer_button_down_on() {
                        fill = fill.gamma_multiply(0.75);
                    } else if r.hovered() {
                        fill = Color32::from_rgb(fill.r().saturating_add(25), fill.g().saturating_add(25), fill.b().saturating_add(25));
                    }
                    if r.clicked() {
                        events.push(UiEvent::Pressed(it.node));
                    }
                }
                if disabled {
                    fill = fill.gamma_multiply(0.5);
                }
                painter.rect_filled(rect, CornerRadius::same((4.0 * scale) as u8), fill);
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, it.text("text"), font(it.num("size", 16.0)), c32(it.color("text_color")));
            }
            NodeKind::ProgressBar => {
                let f = (it.num("value", 0.5) / it.num("max", 1.0).max(1e-9)).clamp(0.0, 1.0) as f32;
                painter.rect_filled(rect, CornerRadius::same(3), c32(it.color("back_color")));
                let mut fill = rect;
                fill.set_width(rect.width() * f);
                painter.rect_filled(fill, CornerRadius::same(3), c32(it.color("color")));
            }
            NodeKind::Image => {
                let path = it.text("texture").to_string();
                if path.is_empty() {
                    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
                } else {
                    let tex = images.entry(path.clone()).or_insert_with(|| {
                        let img = gex_assets::load_image(vfs, &path).unwrap_or(gex_assets::Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] });
                        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
                        ui.ctx().load_texture(path.clone(), ci, egui::TextureOptions::NEAREST)
                    });
                    painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), c32(it.color("color")));
                }
            }
            NodeKind::TextInput => {
                let size = it.num("size", 16.0);
                if live {
                    let id = egui::Id::new(("ui text", it.node));
                    let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| it.text("text").to_string());
                    let has_focus = ui.memory(|m| m.has_focus(id.with("edit")));
                    if !has_focus {
                        text = it.text("text").to_string();
                    }
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    let r = child.add_sized(
                        rect.size(),
                        egui::TextEdit::singleline(&mut text).id(id.with("edit")).hint_text(it.text("placeholder")).font(font(size)),
                    );
                    if r.changed() {
                        events.push(UiEvent::Changed(it.node, text.clone()));
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        events.push(UiEvent::Submitted(it.node, text.clone()));
                    }
                    ui.data_mut(|d| d.insert_temp(id, text));
                } else {
                    painter.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(0x12, 0x15, 0x1b));
                    painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
                    let (t, c) = if it.text("text").is_empty() { (it.text("placeholder"), Color32::GRAY) } else { (it.text("text"), Color32::WHITE) };
                    painter.text(rect.left_center() + egui::vec2(6.0 * scale, 0.0), egui::Align2::LEFT_CENTER, t, font(size), c);
                }
            }
            _ => {}
        }
    }
    events
}
