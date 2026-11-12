//! The blueprint view: draws the blueprint (or the running game) with gex-render, and handles
//! camera controls, picking, dragging objects and dropping assets.

use crate::docs::Tip as _;
use std::collections::HashSet;

use eframe::egui::{self, Color32, Pos2, Rect, Sense};
use egui_wgpu::wgpu;
use gex_core::glam::{Mat4, Quat, Vec2, Vec3};
use gex_core::render::{extract, RenderList};
use gex_core::{Object, ObjectId, ObjectKind, Variant};
use gex_render::camera::ray_box;
use gex_render::{Cameras, Overlay, Renderer};

use crate::panels::AssetPath;
use crate::gizmo::{Gizmo, Handle};
use crate::state::{EditorState, Tool};
use crate::theme;

const SEL: [f32; 4] = [0.4, 0.75, 0.96, 1.0];

/// egui key → the key names .jlb uses.
pub fn key_name(k: egui::Key) -> Option<String> {
    use egui::Key::*;
    Some(match k {
        ArrowLeft => "left".into(),
        ArrowRight => "right".into(),
        ArrowUp => "up".into(),
        ArrowDown => "down".into(),
        Space => "space".into(),
        Enter => "enter".into(),
        Escape => "escape".into(),
        Tab => "tab".into(),
        Backspace => "backspace".into(),
        Delete => "delete".into(),
        Insert => "insert".into(),
        Home => "home".into(),
        End => "end".into(),
        PageUp => "pageup".into(),
        PageDown => "pagedown".into(),
        _ => {
            let n = k.name();
            if n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric()) {
                n.to_ascii_lowercase()
            } else if n.starts_with('F') && n[1..].parse::<u32>().is_ok() {
                n.to_ascii_lowercase()
            } else {
                return None;
            }
        }
    })
}

pub fn input_keys(i: &egui::InputState) -> HashSet<String> {
    let mut keys: HashSet<String> = i.keys_down.iter().filter_map(|k| key_name(*k)).collect();
    if i.modifiers.shift {
        keys.insert("shift".into());
    }
    if i.modifiers.ctrl {
        keys.insert("ctrl".into());
    }
    if i.modifiers.alt {
        keys.insert("alt".into());
    }
    keys
}

pub fn mouse_buttons(i: &egui::InputState) -> HashSet<String> {
    let mut b = HashSet::new();
    if i.pointer.primary_down() {
        b.insert("left".to_string());
    }
    if i.pointer.secondary_down() {
        b.insert("right".to_string());
    }
    if i.pointer.middle_down() {
        b.insert("middle".to_string());
    }
    b
}

/// Renders a list into the viewport texture and paints it (plus labels) into `rect`.
pub fn paint_list(ui: &egui::Ui, rs: &egui_wgpu::RenderState, renderer: &mut Option<Renderer>, tex_id: &mut Option<egui::TextureId>, vfs: &gex_assets::Vfs, rect: Rect, list: &RenderList, cams: &Cameras, overlay: &Overlay) -> Vec<String> {
    let r = renderer.get_or_insert_with(|| Renderer::new(&rs.device, &rs.queue));
    let new_target = r.render(&rs.device, &rs.queue, vfs, cams.size, list, cams, overlay);
    let view = r.target_view().unwrap();
    {
        let mut egui_r = rs.renderer.write();
        match tex_id {
            None => *tex_id = Some(egui_r.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear)),
            Some(id) if new_target => egui_r.update_egui_texture_from_wgpu_texture(&rs.device, view, wgpu::FilterMode::Linear, *id),
            _ => {}
        }
    }
    let painter = ui.painter_at(rect);
    painter.image(tex_id.unwrap(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    // Text: drawn by egui on top, at the label's 2D position.
    if cams.draw_2d {
        let ppp = ui.ctx().pixels_per_point();
        for l in &list.labels {
            let s = cams.world_2d_to_screen(l.pos) / ppp;
            let zoom = (cams.view_proj_2d.x_axis.x * cams.size[0] as f32 / 2.0).abs();
            let c = l.color;
            let color = Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (c[3] * 255.0) as u8);
            let fam = crate::text::family(ui.ctx(), vfs, &l.font);
            crate::text::paint(&painter, rect.min + egui::vec2(s.x, s.y), egui::Align2::LEFT_TOP, &l.text, egui::FontId::new((l.size * zoom / ppp).max(4.0), fam), color, zoom / ppp);
        }
    }
    // Label3D: text at a 3D point, facing the screen.
    if cams.draw_3d {
        let ppp = ui.ctx().pixels_per_point();
        for l in &list.labels_3d {
            let c = cams.view_proj_3d * l.pos.extend(1.0);
            if c.w <= 0.01 {
                continue;
            }
            let ndc = c.truncate() / c.w;
            if ndc.x.abs() > 1.2 || ndc.y.abs() > 1.2 || ndc.z > 1.0 {
                continue;
            }
            let at = Pos2::new(rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(), rect.min.y + (0.5 - ndc.y * 0.5) * rect.height());
            let cl = l.color;
            let color = Color32::from_rgba_unmultiplied((cl[0] * 255.0) as u8, (cl[1] * 255.0) as u8, (cl[2] * 255.0) as u8, (cl[3] * 255.0) as u8);
            let fam = crate::text::family(ui.ctx(), vfs, &l.font);
            crate::text::paint(&painter, at, egui::Align2::CENTER_BOTTOM, &l.text, egui::FontId::new((l.size / ppp).max(4.0), fam), color, 1.0 / ppp);
        }
    }
    std::mem::take(&mut r.problems)
}

fn grid_2d(o: &mut Overlay, cams: &Cameras) {
    let a = cams.screen_to_2d(Vec2::ZERO);
    let b = cams.screen_to_2d(Vec2::new(cams.size[0] as f32, cams.size[1] as f32));
    let (min, max) = (a.min(b), a.max(b));
    let span = (max.x - min.x).max(1.0);
    let step = [16.0f32, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0].into_iter().find(|s| span / s < 60.0).unwrap_or(2048.0);
    let c = [1.0, 1.0, 1.0, 0.07];
    let mut x = (min.x / step).floor() * step;
    while x <= max.x {
        o.line2([x, min.y], [x, max.y], if x == 0.0 { [0.9, 0.3, 0.3, 0.6] } else { c });
        x += step;
    }
    let mut y = (min.y / step).floor() * step;
    while y <= max.y {
        o.line2([min.x, y], [max.x, y], if y == 0.0 { [0.3, 0.9, 0.3, 0.6] } else { c });
        y += step;
    }
}

fn grid_3d(o: &mut Overlay) {
    for i in -20..=20 {
        let f = i as f32;
        let c = if i == 0 { [0.6, 0.6, 0.6, 0.5] } else { [1.0, 1.0, 1.0, 0.09] };
        o.line3(Vec3::new(f, 0.0, -20.0), Vec3::new(f, 0.0, 20.0), if i == 0 { [0.3, 0.3, 0.9, 0.8] } else { c });
        o.line3(Vec3::new(-20.0, 0.0, f), Vec3::new(20.0, 0.0, f), if i == 0 { [0.9, 0.3, 0.3, 0.8] } else { c });
    }
}

fn corners(m: Mat4, size: Vec2, centered: bool) -> [[f32; 2]; 4] {
    let off = if centered { -size / 2.0 } else { Vec2::ZERO };
    [Vec2::ZERO, Vec2::new(size.x, 0.0), size, Vec2::new(0.0, size.y)].map(|p| {
        let w = m.transform_point3((p + off).extend(0.0));
        [w.x, w.y]
    })
}

fn box_lines(o: &mut Overlay, m: Mat4, c: [f32; 4]) {
    let p = |x: f32, y: f32, z: f32| m.transform_point3(Vec3::new(x, y, z) * 0.5);
    for (a, b) in [
        ((-1., -1., -1.), (1., -1., -1.)),
        ((1., -1., -1.), (1., 1., -1.)),
        ((1., 1., -1.), (-1., 1., -1.)),
        ((-1., 1., -1.), (-1., -1., -1.)),
        ((-1., -1., 1.), (1., -1., 1.)),
        ((1., -1., 1.), (1., 1., 1.)),
        ((1., 1., 1.), (-1., 1., 1.)),
        ((-1., 1., 1.), (-1., -1., 1.)),
        ((-1., -1., -1.), (-1., -1., 1.)),
        ((1., -1., -1.), (1., -1., 1.)),
        ((1., 1., -1.), (1., 1., 1.)),
        ((-1., 1., -1.), (-1., 1., 1.)),
    ] {
        o.line3(p(a.0, a.1, a.2), p(b.0, b.1, b.2), c);
    }
}

/// The overlay for editing: grid, colliders, gizmos, selection.
fn editor_overlay(st: &EditorState, list: &RenderList, cams: &Cameras) -> Overlay {
    let mut o = Overlay::default();
    let tree = st.view_tree();
    if st.mode_3d {
        if st.show_grid {
            grid_3d(&mut o);
        }
        for id in tree.walk() {
            let n = &tree.objects[id];
            if matches!(n.kind, ObjectKind::Camera3D | ObjectKind::Light3D | ObjectKind::Object3D) {
                let m = tree.global_matrix(id);
                let p = m.w_axis.truncate();
                let c = if n.kind == ObjectKind::Light3D { [1.0, 0.85, 0.3, 0.9] } else { [0.8, 0.8, 0.8, 0.8] };
                for ax in [Vec3::X, Vec3::Y, Vec3::Z] {
                    o.line3(p - ax * 0.2, p + ax * 0.2, c);
                }
                o.line3(p, p + m.transform_vector3(Vec3::NEG_Z).normalize_or_zero() * 0.8, c);
            }
        }
        for id in tree.walk() {
            let n = &tree.objects[id];
            if matches!(n.kind, ObjectKind::Body3D | ObjectKind::Collider3D) {
                let color = if n.kind == ObjectKind::Collider3D { [0.95, 0.75, 0.3, 0.8] } else { [0.3, 0.9, 0.5, 0.7] };
                let size = if matches!(n.text("shape"), "sphere" | "ball") {
                    Vec3::splat(n.num("radius", 0.5) as f32 * 2.0)
                } else {
                    Vec3::new(n.num("width", 1.0) as f32, n.num("height", 1.0) as f32, n.num("depth", 1.0) as f32)
                };
                box_lines(&mut o, tree.global_matrix(id) * Mat4::from_scale(size), color);
            }
        }
        if let Some(sel) = st.selected.filter(|s| tree.contains(*s)) {
            let m = tree.global_matrix(sel);
            if tree.objects[sel].kind == ObjectKind::Mesh3D {
                box_lines(&mut o, m * Mat4::from_scale(Vec3::splat(1.02)), SEL);
            } else {
                box_lines(&mut o, m * Mat4::from_scale(Vec3::splat(0.4)), SEL);
            }
        }
    } else {
        if st.show_grid {
            grid_2d(&mut o, cams);
        }
        // The game window, when there's no Camera2D: (0,0) to the window size.
        if list.camera2d.is_none() {
            let [w, h] = st.project.config.window_size;
            o.quad2([[0.0, 0.0], [w as f32, 0.0], [w as f32, h as f32], [0.0, h as f32]], [0.6, 0.6, 0.9, 0.5]);
        }
        for c in list.colliders.iter().filter(|_| st.show_colliders) {
            o.quad2(corners(c.model, c.size, true), [0.3, 0.9, 0.5, 0.8]);
        }
        for sel in st.selection.iter().copied().chain(st.selected).filter(|s| tree.contains(*s)) {
            let n = &tree.objects[sel];
            let m = tree.global_matrix(sel);
            match n.kind {
                ObjectKind::Sprite2D | ObjectKind::Collider2D => {
                    let size = Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32);
                    let centered = n.kind == ObjectKind::Collider2D || n.prop("centered").is_none_or(|v| v.as_bool());
                    o.quad2(corners(m, size + Vec2::splat(2.0), centered), SEL);
                }
                _ => {
                    let p = m.w_axis.truncate().truncate();
                    let r = 8.0 / (cams.view_proj_2d.x_axis.x * cams.size[0] as f32 / 2.0).abs().max(0.01);
                    o.quad2([[p.x - r, p.y - r], [p.x + r, p.y - r], [p.x + r, p.y + r], [p.x - r, p.y + r]], SEL);
                }
            }
        }
    }
    o
}

/// The object under a point in the view.
fn pick(st: &EditorState, list: &RenderList, cams: &Cameras, screen: Vec2) -> Option<ObjectId> {
    let tree = &st.tree;
    if st.mode_3d {
        let (o, d) = cams.ray(screen);
        let mut best: Option<(f32, ObjectId)> = None;
        for m in &list.meshes {
            let model = if gex_render::mesh::primitive(&m.mesh) == "plane" { m.model * Mat4::from_scale(Vec3::new(1.0, 0.02, 1.0)) } else { m.model };
            if let Some(t) = ray_box(o, d, model) {
                if best.is_none_or(|b| t < b.0) {
                    best = Some((t, m.object));
                }
            }
        }
        return best.map(|b| b.1);
    }
    let w = cams.screen_to_2d(screen);
    let zoom = (cams.view_proj_2d.x_axis.x * cams.size[0] as f32 / 2.0).abs().max(0.01);
    // UI objects sit on top (in the editor, game-window pixels are world pixels).
    let [dw, dh] = st.project.config.window_size;
    for u in list.ui.iter().rev() {
        let tl = u.top_left(Vec2::new(dw as f32, dh as f32));
        if w.x >= tl.x && w.y >= tl.y && w.x <= tl.x + u.size.x && w.y <= tl.y + u.size.y {
            return Some(u.object);
        }
    }
    // Labels, then sprites from the top layer down, then point-like objects.
    for l in list.labels.iter().rev() {
        let size = Vec2::new(l.text.chars().count() as f32 * l.size * 0.55, l.size * 1.2);
        if w.x >= l.pos.x && w.y >= l.pos.y && w.x <= l.pos.x + size.x && w.y <= l.pos.y + size.y {
            return Some(l.object);
        }
    }
    for s in list.sprites.iter().rev() {
        let off = if s.centered { -s.size / 2.0 } else { Vec2::ZERO };
        let local = s.model.inverse().transform_point3(w.extend(0.0)).truncate() - off;
        if local.x >= 0.0 && local.y >= 0.0 && local.x <= s.size.x && local.y <= s.size.y {
            return Some(s.object);
        }
    }
    for c in &list.colliders {
        let local = c.model.inverse().transform_point3(w.extend(0.0)).truncate();
        if local.x.abs() <= c.size.x / 2.0 && local.y.abs() <= c.size.y / 2.0 {
            return Some(c.object);
        }
    }
    tree.walk().into_iter().find(|id| {
        let n = &tree.objects[*id];
        matches!(n.kind, ObjectKind::Object2D | ObjectKind::Camera2D) && tree.global_matrix(*id).w_axis.truncate().truncate().distance(w) * zoom < 10.0
    })
}

#[derive(Clone)]
struct Drag {
    /// The dragged objects and their world transforms when the drag began.
    objects: Vec<(ObjectId, Mat4)>,
    /// The grabbed object's position.
    pivot: Vec3,
    screen_start: Vec2,
    /// The handle grabbed, and the gizmo as it was then.
    handle: Handle,
    giz: Gizmo,
}

pub fn viewport(ui: &mut egui::Ui, st: &mut EditorState, rs: Option<&egui_wgpu::RenderState>, textures: &mut Option<egui::TextureId>) {
    let playing = st.playing.is_some();
    crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:blueprint_view");
    ui.horizontal(|ui| {
        let a = ui.selectable_label(!st.mode_3d, "2D");
        if a.clicked() {
            st.mode_3d = false;
        }
        let b = ui.selectable_label(st.mode_3d, "3D");
        if b.clicked() {
            st.mode_3d = true;
        }
        crate::docs::mark(ui, a.rect.union(b.rect), "ui:2d3d");
        ui.separator();
        for (t, label, key) in [(Tool::Move, "Move", "1"), (Tool::Rotate, "Rotate", "2"), (Tool::Scale, "Scale", "3")] {
            if crate::docs::on(ui.selectable_label(st.tool == t, label).tip(format!("Key: {}", key)), ui, "ui:tools").clicked() {
                st.tool = t;
            }
        }
        ui.separator();
        let snap0 = ui.label("Snap").rect;
        let (step, unit, speed) = if st.mode_3d { (&mut st.snap3, " m", 0.05) } else { (&mut st.snap2, " px", 1.0) };
        ui.add(egui::DragValue::new(step).range(0.0..=10000.0).speed(speed).suffix(unit)).tip("Moving and scaling go in steps of this (scaling in 2D: steps of 0.25). 0 is off.\nHolding Ctrl while dragging flips snapping.");
        ui.add(egui::DragValue::new(&mut st.snap_angle).range(0.0..=180.0).speed(1.0).suffix("°")).tip("Rotating goes in steps of this. 0 is off.\nHolding Ctrl while dragging flips snapping.");
        crate::docs::mark(ui, snap0.union(ui.min_rect().intersect(egui::Rect::everything_right_of(snap0.left()))), "ui:snap");
        ui.separator();
        let r = ui.checkbox(&mut st.show_grid, "Grid").tip("Shows or hides the grid in the blueprint view. It isn't part of the game.");
        crate::docs::mark(ui, r.rect, "ui:grid");
        let r = ui.checkbox(&mut st.show_colliders, "Colliders").tip("Outline 2D colliders and bodies (in the editor and while playing)");
        crate::docs::mark(ui, r.rect, "ui:colliders");
        if let Some(p) = &mut st.playing {
            ui.separator();
            let r = ui.checkbox(&mut p.game_camera, "Game camera").tip("Off: look around with the editor camera while the game runs");
            crate::docs::mark(ui, r.rect, "ui:game_camera");
            let view: Option<egui::Id> = ui.ctx().data(|d| d.get_temp(egui::Id::new("gex viewport id")));
            if ui.ctx().memory(|m| m.focused()) != view || view.is_none() {
                ui.label(egui::RichText::new("Click the view to give the game the keyboard").small().color(theme::WARN));
            }
        }
    });
    let size = ui.available_size().max(egui::vec2(16.0, 16.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let Some(rs) = rs else {
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "No GPU renderer (wgpu) available", egui::FontId::proportional(16.0), theme::BAD);
        return;
    };
    let ppp = ui.ctx().pixels_per_point();
    let px = [(rect.width() * ppp) as u32, (rect.height() * ppp) as u32];
    if !st.cam_fitted && px[0] > 64 && px[1] > 64 {
        // Start with the whole game window in view.
        let [w, h] = st.project.config.window_size;
        st.cam2.center = Vec2::new(w as f32 / 2.0, h as f32 / 2.0);
        st.cam2.zoom = (px[0] as f32 / w as f32).min(px[1] as f32 / h as f32) * 0.9;
        st.cam_fitted = true;
    }
    let to_px = |p: Pos2| Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
    let game_cam = st.playing.as_ref().is_some_and(|p| p.game_camera);
    let mut list = match &st.playing {
        Some(p) => p.list.clone(),
        None => {
            let mut l = extract(&st.tree);
            l.graphics = st.project.config.graphics.clone();
            preview_particles(ui.ctx(), st, &mut l);
            l
        }
    };
    let cams = if game_cam { Cameras::for_game(&list, px, st.project.config.window_size) } else { Cameras::for_editor(st.mode_3d, &st.cam2, &st.cam3, px) };

    // ---- input ----
    let (scroll, pointer, delta, secondary, middle, shift, keys) = ui.input(|i| {
        (i.smooth_scroll_delta.y, i.pointer.hover_pos(), i.pointer.delta(), i.pointer.secondary_down(), i.pointer.middle_down(), i.modifiers.shift, input_keys(i))
    });
    if resp.clicked() || resp.drag_started() {
        resp.request_focus();
    }
    // The app's shortcuts still work while the view has focus (it isn't a text field).
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("gex viewport id"), resp.id));
    if let Some(p) = &mut st.playing {
        // Feed the game while the view has focus (or holds the mouse).
        if (p.locked || std::mem::take(&mut p.focus_view)) && !resp.has_focus() {
            resp.request_focus();
        }
        if resp.clicked() && p.lock_suspended {
            p.lock_suspended = false;
        }
        if resp.has_focus() || resp.hovered() || p.locked {
            if game_cam && (resp.has_focus() || p.locked) {
                let d = ui.input(crate::app::mouse_motion);
                p.mouse_delta[0] += d[0];
                p.mouse_delta[1] += d[1];
            }
            if resp.has_focus() {
                // Keys tapped between two frames still count as pressed once.
                let taps: Vec<String> = ui.input(|i| {
                    i.events.iter().filter_map(|e| match e { egui::Event::Key { key, pressed: true, .. } => key_name(*key), _ => None }).collect()
                });
                p.keys.retain(|k| keys.contains(k) || p.taps.contains(k));
                p.taps.extend(taps);
                p.keys.extend(keys.iter().cloned());
            } else {
                p.keys.clear();
            }
            p.buttons = if resp.hovered() || p.locked { ui.input(mouse_buttons) } else { HashSet::new() };
            if resp.hovered() && game_cam {
                p.wheel += scroll / 50.0;
            }
            if let Some(pp) = pointer {
                let m = cams.screen_to_2d(to_px(pp));
                p.mouse = [m.x, m.y];
            }
        } else {
            p.keys.clear();
            p.buttons.clear();
        }
        if resp.has_focus() {
            // Keep Tab and arrows in the game instead of moving focus around.
            ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false }));
        }
    }
    if !game_cam && resp.hovered() {
        if scroll != 0.0 {
            if st.mode_3d {
                st.cam3.zoom(scroll / 50.0);
            } else if let Some(pp) = pointer {
                // Zoom about the pointer.
                let before = cams.screen_to_2d(to_px(pp));
                st.cam2.zoom = (st.cam2.zoom * (1.0 + scroll / 400.0)).clamp(0.05, 40.0);
                let c2 = Cameras::for_editor(false, &st.cam2, &st.cam3, px);
                let after = c2.screen_to_2d(to_px(pp));
                st.cam2.center += before - after;
            }
        }
        if st.mode_3d {
            if secondary && !shift {
                st.cam3.orbit(delta.x, delta.y);
            } else if middle || (secondary && shift) {
                st.cam3.pan(delta.x, delta.y);
            }
        } else if secondary || middle {
            st.cam2.center -= Vec2::new(delta.x, delta.y) * ppp / st.cam2.zoom;
        }
    }

    // W A S D move the editor camera while the view has the keyboard; Shift goes much faster.
    if !game_cam && resp.has_focus() && !ui.input(|i| i.modifiers.command || i.modifiers.alt) {
        let f = |k: &str| keys.contains(k) as i32 as f32;
        let (fwd, right, up) = (f("w") - f("s"), f("d") - f("a"), f("e") - f("q"));
        if fwd != 0.0 || right != 0.0 || up != 0.0 {
            let k = ui.input(|i| i.stable_dt).min(0.1) * 60.0 * if shift { 5.0 } else { 1.0 };
            if st.mode_3d {
                st.cam3.fly(fwd * k, right * k, up * k);
            } else {
                st.cam2.center += Vec2::new(right, -fwd) * 9.0 * k * ppp / st.cam2.zoom;
            }
            ui.ctx().request_repaint();
        }
    }

    // Painting tiles into the selected TileMap2D.
    let painting = !playing && !st.mode_3d && st.tile_brush.is_some() && st.selected.is_some_and(|s| st.tree.get(s).is_some_and(|n| n.kind == ObjectKind::TileMap2D));
    if painting {
        let down = ui.input(|i| i.pointer.primary_down());
        if let (true, true, Some(pp)) = (down, resp.hovered() || resp.dragged(), pointer) {
            let id = st.selected.unwrap();
            let w = cams.screen_to_2d(to_px(pp));
            let n = &st.tree.objects[id];
            let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
            let local = st.tree.global_matrix(id).inverse().transform_point3(w.extend(0.0));
            let (c, r) = ((local.x / tw).floor() as i32, (local.y / th).floor() as i32);
            let brush = if ui.input(|i| i.modifiers.shift) { -1 } else { st.tile_brush.unwrap() };
            let mut grid = gex_core::tiles::parse(n.text("tiles"));
            if c >= 0 && r >= 0 && gex_core::tiles::get(&grid, c, r) != brush {
                st.will_change(&format!("paint {:?}", id));
                gex_core::tiles::set(&mut grid, c, r, brush);
                st.tree.objects[id].props.insert("tiles".into(), Variant::Text(gex_core::tiles::to_text(&grid)));
            }
        }
    }
    // Select, drag objects (move / rotate / scale), box select, drop assets (editing only).
    let mut gizmo_draw: Option<(Gizmo, Option<Handle>)> = None;
    if !playing && !painting {
        let drag_id = egui::Id::new("viewport drag");
        let ctrl = ui.input(|i| i.modifiers.command);
        // The handles on the selected object.
        let giz = st.selected.filter(|s| st.tree.contains(*s) && (st.tree.objects[*s].kind.is_3d() == st.mode_3d)).and_then(|s| {
            let m = st.tree.global_matrix(s);
            Gizmo::new(&cams, st.mode_3d, st.tool, m.w_axis.truncate(), m.to_scale_rotation_translation().1, ppp)
        });
        let dragging = ui.data(|m| m.get_temp::<Drag>(drag_id));
        gizmo_draw = match (&dragging, giz, pointer) {
            (Some(d), _, _) => Some((d.giz, Some(d.handle))),
            (None, Some(g), Some(pp)) if resp.hovered() => Some((g, g.hit(&cams, to_px(pp), ppp))),
            (None, Some(g), _) => Some((g, None)),
            _ => None,
        };
        if resp.drag_started_by(egui::PointerButton::Primary) {
            // Where the button went down (the pointer may already have moved on).
            if let Some(pp) = ui.input(|i| i.pointer.press_origin()).or(pointer) {
                let grabbed = giz.and_then(|g| g.hit(&cams, to_px(pp), ppp).map(|h| (g, h)));
                let hit = if grabbed.is_some() { None } else { pick(st, &list, &cams, to_px(pp)) };
                if let Some((g, handle)) = grabbed {
                    let objects: Vec<(ObjectId, Mat4)> = st.selected_roots().into_iter().map(|i| (i, st.tree.global_matrix(i))).collect();
                    ui.data_mut(|d| d.insert_temp(drag_id, Drag { objects, pivot: g.pivot, screen_start: to_px(pp), handle, giz: g }));
                }
                match hit {
                    Some(h) => {
                        if !st.is_selected(h) {
                            if ctrl {
                                st.toggle_select(h);
                            } else {
                                st.select(Some(h));
                            }
                        }
                        let objects: Vec<(ObjectId, Mat4)> = st.selected_roots().into_iter().map(|i| (i, st.tree.global_matrix(i))).collect();
                        let hm = st.tree.global_matrix(h);
                        let pivot = hm.w_axis.truncate();
                        if let Some(g) = Gizmo::new(&cams, st.mode_3d, st.tool, pivot, hm.to_scale_rotation_translation().1, ppp) {
                            ui.data_mut(|d| d.insert_temp(drag_id, Drag { objects, pivot, screen_start: to_px(pp), handle: Handle::Free, giz: g }));
                        }
                    }
                    None => {}
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let (Some(d), Some(pp)) = (ui.data(|m| m.get_temp::<Drag>(drag_id)), pointer) {
                let now = to_px(pp);
                let g = d.giz;
                let drag = now - d.screen_start;
                let view = g.toward_viewer(&cams);
                // What the drag means, once for every dragged object.
                let mut move_by = Vec3::ZERO;
                let mut turn = Quat::IDENTITY;
                let mut grow = Vec3::ONE;
                match st.tool {
                    Tool::Move => {
                        move_by = match d.handle {
                            Handle::Axis(i) => g.axes[i] * g.along(&cams, i, drag),
                            // Two axes: slide on their plane. The object itself: on the plane facing the camera.
                            h => {
                                let normal = if let Handle::Plane(i) = h { g.axes[i] } else { view };
                                match (g.on_plane(&cams, d.screen_start, normal), g.on_plane(&cams, now, normal)) {
                                    (Some(a), Some(b)) => b - a,
                                    _ => Vec3::ZERO,
                                }
                            }
                        };
                        let set = if st.mode_3d { st.snap3 } else { st.snap2 };
                        if ctrl != (set > 0.0) {
                            let step = if set > 0.0 { set } else if st.mode_3d { 0.25 } else { 16.0 };
                            move_by = (move_by / step).round() * step;
                        }
                    }
                    Tool::Rotate => {
                        if let Some(c) = g.px(&cams, g.pivot) {
                            let ang = |p: Vec2| (p.y - c.y).atan2(p.x - c.x);
                            let mut a = ang(now) - ang(d.screen_start);
                            if ctrl != (st.snap_angle > 0.0) {
                                let step = (if st.snap_angle > 0.0 { st.snap_angle } else { 15.0 }).to_radians();
                                a = (a / step).round() * step;
                            }
                            let axis = if let Handle::Axis(i) = d.handle { g.axes[i] } else { view };
                            // The screen's y points down: clockwise there is a negative turn about an axis facing the viewer.
                            let sign = if !st.mode_3d { 1.0 } else if axis.dot(view) >= 0.0 { -1.0 } else { 1.0 };
                            turn = Quat::from_axis_angle(axis, a * sign);
                        }
                    }
                    Tool::Scale => {
                        let f = |amount: f32| (1.0 + amount / g.k).max(0.01);
                        match d.handle {
                            Handle::Axis(i) => grow[i] = f(g.along(&cams, i, drag)),
                            Handle::Plane(i) => {
                                let (a, b) = ((i + 1) % 3, (i + 2) % 3);
                                let k = f((g.along(&cams, a, drag) + g.along(&cams, b, drag)) / 2.0);
                                grow[a] = k;
                                grow[b] = k;
                            }
                            Handle::Free => {
                                if let Some(c) = g.px(&cams, g.pivot) {
                                    let d0 = d.screen_start.distance(c);
                                    let k = if d0 > 8.0 * ppp { (now.distance(c) / d0).max(0.01) } else { f((drag.x - drag.y) / (78.0 * ppp) * g.k) };
                                    grow = Vec3::splat(k);
                                }
                            }
                        }
                    }
                }
                {
                    st.will_change("drag");
                    for (id, start_m) in &d.objects {
                        if !st.tree.contains(*id) {
                            continue;
                        }
                        let (s0, r0, t0) = start_m.to_scale_rotation_translation();
                        let uniform = grow.x == grow.y && grow.y == grow.z;
                        // Scaling every axis also spreads the other dragged objects out from the grabbed one.
                        let t = d.pivot + turn * (t0 - d.pivot) * if uniform { grow.x } else { 1.0 } + move_by;
                        let mut s1 = s0 * grow;
                        let set = if st.mode_3d { st.snap3 } else { st.snap2 };
                        if st.tool == Tool::Scale && ctrl != (set > 0.0) {
                            let step = if st.mode_3d && set > 0.0 { set } else { 0.25 };
                            for i in 0..3 {
                                if grow[i] != 1.0 {
                                    s1[i] = ((s1[i] / step).round() * step).max(step);
                                }
                            }
                        }
                        let world = Mat4::from_scale_rotation_translation(s1, turn * r0, t);
                        let parent_inv = st.tree.objects[*id].parent.map(|p| st.tree.global_matrix(p).inverse()).unwrap_or(Mat4::IDENTITY);
                        let (s, r, t) = (parent_inv * world).to_scale_rotation_translation();
                        let (ry, rx, rz) = r.to_euler(gex_core::glam::EulerRot::YXZ);
                        let tr = &mut st.tree.objects[*id].transform;
                        tr.position = t;
                        tr.scale = s;
                        tr.rotation = Vec3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees()) + Vec3::ZERO; // + 0 turns -0 into 0
                    }
                }
            }
        }
        if resp.drag_stopped() {
            ui.data_mut(|d| d.remove::<Drag>(drag_id));
        }
        if resp.clicked() {
            if let Some(pp) = pointer.filter(|pp| giz.is_none_or(|g| g.hit(&cams, to_px(*pp), ppp).is_none())) {
                let hit = pick(st, &list, &cams, to_px(pp));
                match (hit, ctrl) {
                    (Some(h), true) => st.toggle_select(h),
                    (h, _) => st.select(h.or(st.tree.root)),
                }
            }
        }
        if let (Some(asset), Some(pp)) = (resp.dnd_release_payload::<AssetPath>(), pointer) {
            let at = pointer_world(st, &cams, to_px(pp), 0.0).unwrap_or(Vec3::ZERO);
            drop_asset(st, &asset.0, at);
        }
    }

    // ---- draw ----
    let overlay = if game_cam {
        // Colliders stay visible while playing (unless switched off), as a debugging aid.
        let mut o = Overlay::default();
        for c in list.colliders.iter().filter(|_| st.show_colliders) {
            o.quad2(corners(c.model, c.size, true), [0.3, 0.9, 0.5, 0.5]);
        }
        o
    } else {
        editor_overlay(st, &list, &cams)
    };
    st.view_anchors.clear();
    if !playing {
        for (id, n) in st.tree.objects.iter() {
            if n.scripts.is_empty() {
                continue;
            }
            let p = st.tree.global_matrix(id).w_axis.truncate();
            let at = if st.mode_3d { cams.world_3d_to_screen(p) } else { Some(cams.world_2d_to_screen(p.truncate())) };
            if let Some(at) = at {
                let at = rect.min + egui::vec2(at.x, at.y) / ppp;
                for sc in &n.scripts {
                    st.view_anchors.entry(sc.clone()).or_insert([at.x, at.y]);
                }
            }
        }
    }
    if !game_cam {
        // While editing, what's behind the blueprint is a plain gradient: the view is drawn see-through over it.
        list.clear = [0.0; 4];
        let painter = ui.painter_at(rect);
        painter.add(theme::vgradient(rect, Color32::from_rgb(0x36, 0x36, 0x36), Color32::from_rgb(0x1c, 0x1c, 0x1c)));
    }
    let problems = paint_list(ui, rs, &mut st.renderer, textures, &st.vfs, rect, &list, &cams, &overlay);
    if !playing {
        compass(ui, rect, st.mode_3d, &cams);
    }
    for p in problems {
        st.log(gex_script::LogKind::Error, p);
    }
    // UI objects, in game-window pixels.
    let [dw, dh] = st.project.config.window_size;
    let design = Vec2::new(dw as f32, dh as f32);
    let events = {
        let world_ui = !game_cam && !st.mode_3d;
        let fit = (px[0] as f32 / design.x).min(px[1] as f32 / design.y);
        let map = |p: Vec2| -> Pos2 {
            let s = if world_ui { cams.world_2d_to_screen(p) } else { (p - design / 2.0) * fit + Vec2::new(px[0] as f32, px[1] as f32) / 2.0 };
            rect.min + egui::vec2(s.x / ppp, s.y / ppp)
        };
        let scale = if world_ui { st.cam2.zoom / ppp } else { fit / ppp };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        crate::ui_layer::paint_ui(&mut child, &list.ui, design, &map, scale, playing && game_cam, &mut st.ui_images, &st.vfs)
    };
    if game_cam {
        paint_fade(ui, rect, list.fade);
    }
    if let Some(p) = &mut st.playing {
        p.ui_events.extend(events);
    }
    if let Some((g, lit)) = gizmo_draw {
        g.paint(&ui.painter_at(rect), &cams, rect.min, ppp, lit);
    }
    if resp.has_focus() && playing {
        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, theme::GOOD), egui::StrokeKind::Inside);
    }
}

/// Where the pointer is in the world: 2D world pixels, or on the horizontal plane y = plane_y in 3D.
fn pointer_world(st: &EditorState, cams: &Cameras, screen: Vec2, plane_y: f32) -> Option<Vec3> {
    if st.mode_3d {
        let (o, d) = cams.ray(screen);
        if d.y.abs() < 1e-5 {
            return None;
        }
        let t = (plane_y - o.y) / d.y;
        (t > 0.0).then(|| o + d * t)
    } else {
        Some(cams.screen_to_2d(screen).extend(0.0))
    }
}

fn drop_asset(st: &mut EditorState, path: &str, at: Vec3) {
    let lower = path.to_ascii_lowercase();
    let place = |st: &mut EditorState, id: ObjectId| {
        let parent_inv = st.tree.objects[id].parent.map(|p| st.tree.global_matrix(p).inverse()).unwrap_or(Mat4::IDENTITY);
        st.tree.objects[id].transform.position = parent_inv.transform_point3(at);
    };
    if lower.ends_with(".jblueprint") {
        if let Some(id) = st.instance_blueprint(path) {
            place(st, id);
        }
    } else if gex_assets::model::is_model(&lower) {
        st.will_change("drop");
        let name: String = path.rsplit('/').next().unwrap_or("Model").split('.').next().unwrap_or("Model").to_string();
        let mut n = Object::new(name, ObjectKind::Mesh3D);
        n.props.insert("mesh".into(), Variant::Text(path.to_string()));
        n.props.insert("color".into(), Variant::Color([1.0; 4]));
        if lower.ends_with(".bsp") {
            n.props.insert("collision".into(), Variant::Bool(true));
        }
        let id = st.tree.add(st.tree.root, n);
        place(st, id);
        st.selected = Some(id);
        st.mode_3d = true;
    } else if lower.ends_with(".wav") || lower.ends_with(".mp3") || lower.ends_with(".ogg") || lower.ends_with(".flac") {
        st.will_change("drop");
        let mut n = Object::new("Sound", ObjectKind::Sound);
        n.props.insert("stream".into(), Variant::Res(path.to_string()));
        let id = st.tree.add(st.tree.root, n);
        st.selected = Some(id);
    } else if gex_assets::image_load::is_image(&lower) {
        st.will_change("drop");
        let name: String = path.rsplit('/').next().unwrap_or("Sprite").split('.').next().unwrap_or("Sprite").to_string();
        let id = if st.mode_3d {
            let mut n = Object::new(name, ObjectKind::Mesh3D);
            n.props.insert("texture".into(), Variant::Res(path.to_string()));
            st.tree.add(st.tree.root, n)
        } else {
            let mut n = Object::new(name, ObjectKind::Sprite2D);
            n.props.insert("texture".into(), Variant::Res(path.to_string()));
            if let Ok(img) = gex_assets::load_image(&st.vfs, path) {
                n.props.insert("width".into(), Variant::Num(img.width as f64));
                n.props.insert("height".into(), Variant::Num(img.height as f64));
            }
            st.tree.add(st.tree.root, n)
        };
        place(st, id);
        st.selected = Some(id);
    } else {
        st.status = format!("{} can't be placed in a blueprint yet", path);
    }
}

/// Particles2D objects play in the editor too, so their look can be tuned without running the game.
fn preview_particles(ctx: &egui::Context, st: &mut EditorState, list: &mut gex_core::render::RenderList) {
    let now = std::time::Instant::now();
    let dt = now.duration_since(st.particle_clock).as_secs_f32().min(0.1);
    st.particle_clock = now;
    let any = st.tree.walk().into_iter().any(|i| matches!(st.tree.objects[i].kind, ObjectKind::Particles2D | ObjectKind::Particles3D));
    if !any && st.preview_particles.is_empty() && st.preview_particles_3d.is_empty() {
        return;
    }
    gex_script::particles::update(&st.tree, &mut st.preview_particles, dt, &[], st.project.config.graphics.particles as f64);
    gex_script::particles::draw(&st.tree, &st.preview_particles, list);
    gex_script::particles::update_3d(&st.tree, &mut st.preview_particles_3d, dt, &[], st.project.config.graphics.particles as f64);
    gex_script::particles::draw_3d(&st.tree, &st.preview_particles_3d, list);
    if st.preview_particles.values().any(|e| e.count() > 0) || st.preview_particles_3d.values().any(|e| e.count() > 0) {
        ctx.request_repaint();
    }
}

/// A blueprint-change fade over the whole game view.
pub fn paint_fade(ui: &egui::Ui, rect: Rect, c: [f32; 4]) {
    if c[3] > 0.0 {
        let to = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        ui.painter().with_clip_rect(rect).rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(to(c[0]), to(c[1]), to(c[2]), to(c[3])));
    }
}

/// The little see-through cube in the view's corner: which way X, Y and Z point, and which
/// side of the world you're looking at.
fn compass(ui: &egui::Ui, view: Rect, mode_3d: bool, cams: &Cameras) {
    let p = ui.painter_at(view);
    let c = Pos2::new(view.right() - 58.0, view.top() + 62.0);
    let font = egui::FontId::proportional(10.0);
    let axis_colors = [Color32::from_rgb(0xf9, 0x26, 0x72), Color32::from_rgb(0xa6, 0xe2, 0x2e), Color32::from_rgb(0x66, 0xd9, 0xef)];
    if !mode_3d {
        // 2D: x goes right, y goes down.
        let o = c + egui::vec2(-18.0, -18.0);
        for (d, name, col) in [(egui::vec2(36.0, 0.0), "X", axis_colors[0]), (egui::vec2(0.0, 36.0), "Y", axis_colors[1])] {
            p.arrow(o, d, egui::Stroke::new(1.5, col));
            p.text(o + d * 1.28, egui::Align2::CENTER_CENTER, name, font.clone(), col);
        }
        return;
    }
    let f = (cams.focus - cams.eye).normalize_or_zero();
    let right = f.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(f);
    let h = 24.0;
    let c = Pos2::new(view.right() - 70.0, view.top() + 72.0);
    let flat = |v: Vec3| c + egui::vec2(v.dot(right), -v.dot(up)) * h;
    // A solid cube, seen the way the camera looks: each side that faces you is drawn, lit
    // from above, with its name written on it (lying on the side, turning with it).
    // Faces: the way they face, their name, and the name's "right" and "up" on the face.
    let faces = [
        (Vec3::X, "Right", Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, "Left", Vec3::Z, Vec3::Y),
        (Vec3::Y, "Top", Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, "Bottom", Vec3::X, Vec3::Z),
        (Vec3::Z, "Front", Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, "Back", Vec3::NEG_X, Vec3::Y),
    ];
    // The axes, out of the cube's sides: those pointing away are drawn first (behind it).
    let axes = [(Vec3::X, "X", axis_colors[0]), (Vec3::Y, "Y", axis_colors[1]), (Vec3::Z, "Z", axis_colors[2])];
    let axis = |d: Vec3, name: &str, col: Color32| {
        if d.dot(f).abs() < 0.95 {
            p.line_segment([flat(d), flat(d * 1.75)], egui::Stroke::new(2.0, col));
            p.text(flat(d * 2.05), egui::Align2::CENTER_CENTER, name, font.clone(), col);
        }
    };
    for (d, name, col) in axes {
        if d.dot(f) > 0.0 {
            axis(d, name, col);
        }
    }
    let atlas = ui.ctx().fonts(|f| f.font_image_size());
    let edge = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120));
    for (n, name, a, b) in faces {
        let facing = -n.dot(f);
        if facing <= 0.02 {
            continue;
        }
        let quad: Vec<Pos2> = [a + b, a - b, -a - b, -a + b].iter().map(|k| flat(n + *k)).collect();
        let light = (0.62 + 0.30 * n.y + 0.10 * n.z - 0.06 * n.x).clamp(0.3, 1.0);
        let g = (light * 235.0) as u8;
        p.add(egui::Shape::convex_polygon(quad, Color32::from_rgba_unmultiplied(g, g, (g as f32 * 1.04).min(255.0) as u8, 225), edge));
        // The name: the text's own triangles, laid onto the side.
        let galley = p.layout_no_wrap(name.to_string(), egui::FontId::proportional(22.0), Color32::from_rgb(0x18, 0x18, 0x1c));
        let size = galley.size();
        let k = 1.5 / size.x.max(52.0);
        let mut mesh = egui::Mesh::default();
        for row in &galley.rows {
            let base = mesh.vertices.len() as u32;
            for v in &row.row.visuals.mesh.vertices {
                let q = row.pos + v.pos.to_vec2();
                let at = n * 1.001 + a * ((q.x - size.x / 2.0) * k) - b * ((q.y - size.y / 2.0) * k);
                mesh.vertices.push(egui::epaint::Vertex { pos: flat(at), uv: Pos2::new(v.uv.x / atlas[0] as f32, v.uv.y / atlas[1] as f32), color: Color32::from_rgba_unmultiplied(0x18, 0x18, 0x1c, (facing.min(1.0) * 400.0).min(255.0) as u8) });
            }
            mesh.indices.extend(row.row.visuals.mesh.indices.iter().map(|i| base + i));
        }
        p.add(egui::Shape::mesh(mesh));
    }
    for (d, name, col) in axes {
        if d.dot(f) <= 0.0 {
            axis(d, name, col);
        }
    }
}
