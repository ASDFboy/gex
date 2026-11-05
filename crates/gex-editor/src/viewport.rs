//! The scene view: draws the scene (or the running game) with gex-render, and handles
//! camera controls, picking, dragging nodes and dropping assets.

use std::collections::HashSet;

use eframe::egui::{self, Color32, Pos2, Rect, Sense};
use egui_wgpu::wgpu;
use gex_core::glam::{Mat4, Vec2, Vec3};
use gex_core::render::{extract, RenderList};
use gex_core::{Node, NodeId, NodeKind, Variant};
use gex_render::camera::ray_box;
use gex_render::{Cameras, Overlay, Renderer};

use crate::panels::AssetPath;
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
            painter.text(rect.min + egui::vec2(s.x, s.y), egui::Align2::LEFT_TOP, &l.text, egui::FontId::proportional((l.size * zoom / ppp).max(4.0)), color);
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
    let c = [0.3, 0.4, 0.5, 0.18];
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
        let c = if i == 0 { [0.6, 0.6, 0.6, 0.5] } else { [0.4, 0.45, 0.5, 0.25] };
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
            let n = &tree.nodes[id];
            if matches!(n.kind, NodeKind::Camera3D | NodeKind::Light3D | NodeKind::Node3D) {
                let m = tree.global_matrix(id);
                let p = m.w_axis.truncate();
                let c = if n.kind == NodeKind::Light3D { [1.0, 0.85, 0.3, 0.9] } else { [0.8, 0.8, 0.8, 0.8] };
                for ax in [Vec3::X, Vec3::Y, Vec3::Z] {
                    o.line3(p - ax * 0.2, p + ax * 0.2, c);
                }
                o.line3(p, p + m.transform_vector3(Vec3::NEG_Z).normalize_or_zero() * 0.8, c);
            }
        }
        for id in tree.walk() {
            let n = &tree.nodes[id];
            if n.kind == NodeKind::Body3D {
                let size = if matches!(n.text("shape"), "sphere" | "ball") {
                    Vec3::splat(n.num("radius", 0.5) as f32 * 2.0)
                } else {
                    Vec3::new(n.num("width", 1.0) as f32, n.num("height", 1.0) as f32, n.num("depth", 1.0) as f32)
                };
                box_lines(&mut o, tree.global_matrix(id) * Mat4::from_scale(size), [0.3, 0.9, 0.5, 0.7]);
            }
        }
        if let Some(sel) = st.selected.filter(|s| tree.contains(*s)) {
            let m = tree.global_matrix(sel);
            if tree.nodes[sel].kind == NodeKind::Mesh3D {
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
        for c in &list.colliders {
            o.quad2(corners(c.model, c.size, true), [0.3, 0.9, 0.5, 0.8]);
        }
        for sel in st.selection.iter().copied().chain(st.selected).filter(|s| tree.contains(*s)) {
            let n = &tree.nodes[sel];
            let m = tree.global_matrix(sel);
            match n.kind {
                NodeKind::Sprite2D | NodeKind::Collider2D => {
                    let size = Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32);
                    let centered = n.kind == NodeKind::Collider2D || n.prop("centered").is_none_or(|v| v.as_bool());
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

/// The node under a point in the view.
fn pick(st: &EditorState, list: &RenderList, cams: &Cameras, screen: Vec2) -> Option<NodeId> {
    let tree = &st.tree;
    if st.mode_3d {
        let (o, d) = cams.ray(screen);
        let mut best: Option<(f32, NodeId)> = None;
        for m in &list.meshes {
            let model = if gex_render::mesh::primitive(&m.mesh) == "plane" { m.model * Mat4::from_scale(Vec3::new(1.0, 0.02, 1.0)) } else { m.model };
            if let Some(t) = ray_box(o, d, model) {
                if best.is_none_or(|b| t < b.0) {
                    best = Some((t, m.node));
                }
            }
        }
        return best.map(|b| b.1);
    }
    let w = cams.screen_to_2d(screen);
    let zoom = (cams.view_proj_2d.x_axis.x * cams.size[0] as f32 / 2.0).abs().max(0.01);
    // UI nodes sit on top (in the editor, game-window pixels are world pixels).
    let [dw, dh] = st.project.config.window_size;
    for u in list.ui.iter().rev() {
        let tl = u.top_left(Vec2::new(dw as f32, dh as f32));
        if w.x >= tl.x && w.y >= tl.y && w.x <= tl.x + u.size.x && w.y <= tl.y + u.size.y {
            return Some(u.node);
        }
    }
    // Labels, then sprites from the top layer down, then point-like nodes.
    for l in list.labels.iter().rev() {
        let size = Vec2::new(l.text.chars().count() as f32 * l.size * 0.55, l.size * 1.2);
        if w.x >= l.pos.x && w.y >= l.pos.y && w.x <= l.pos.x + size.x && w.y <= l.pos.y + size.y {
            return Some(l.node);
        }
    }
    for s in list.sprites.iter().rev() {
        let off = if s.centered { -s.size / 2.0 } else { Vec2::ZERO };
        let local = s.model.inverse().transform_point3(w.extend(0.0)).truncate() - off;
        if local.x >= 0.0 && local.y >= 0.0 && local.x <= s.size.x && local.y <= s.size.y {
            return Some(s.node);
        }
    }
    for c in &list.colliders {
        let local = c.model.inverse().transform_point3(w.extend(0.0)).truncate();
        if local.x.abs() <= c.size.x / 2.0 && local.y.abs() <= c.size.y / 2.0 {
            return Some(c.node);
        }
    }
    tree.walk().into_iter().find(|id| {
        let n = &tree.nodes[*id];
        matches!(n.kind, NodeKind::Node2D | NodeKind::Camera2D) && tree.global_matrix(*id).w_axis.truncate().truncate().distance(w) * zoom < 10.0
    })
}

#[derive(Clone)]
struct Drag {
    /// The dragged nodes and their world transforms when the drag began.
    nodes: Vec<(NodeId, Mat4)>,
    /// Pointer in the world when it began, and the grabbed node's position.
    start: Vec3,
    plane_y: f32,
    pivot: Vec3,
    screen_start: Vec2,
}

pub fn viewport(ui: &mut egui::Ui, st: &mut EditorState, rs: Option<&egui_wgpu::RenderState>, textures: &mut Option<egui::TextureId>) {
    let playing = st.playing.is_some();
    ui.horizontal(|ui| {
        if ui.selectable_label(!st.mode_3d, "2D").clicked() {
            st.mode_3d = false;
        }
        if ui.selectable_label(st.mode_3d, "3D").clicked() {
            st.mode_3d = true;
        }
        ui.separator();
        for (t, label, key) in [(Tool::Move, "↔ Move", "W"), (Tool::Rotate, "⟲ Rotate", "E"), (Tool::Scale, "⛶ Scale", "R")] {
            if ui.selectable_label(st.tool == t, label).on_hover_text(format!("{} (hold Ctrl to snap)", key)).clicked() {
                st.tool = t;
            }
        }
        ui.separator();
        ui.checkbox(&mut st.show_grid, "grid");
        if let Some(p) = &mut st.playing {
            ui.separator();
            ui.checkbox(&mut p.game_camera, "game camera").on_hover_text("off: look around with the editor camera while the game runs");
            ui.label(egui::RichText::new("▶ running — click the view to give it the keyboard").color(theme::GOOD));
        } else {
            ui.separator();
            ui.label(egui::RichText::new(if st.mode_3d { "RMB orbit · MMB/Shift+RMB pan · wheel zoom · RMB+WASD fly" } else { "RMB/MMB pan · wheel zoom · LMB select & drag" }).color(theme::TEXT_DIM).small());
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
    let list = match &st.playing {
        Some(p) => p.list.clone(),
        None => {
            let mut l = extract(&st.tree);
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
        // Feed the game while the view has focus.
        if resp.has_focus() || resp.hovered() {
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
            p.buttons = if resp.hovered() { ui.input(mouse_buttons) } else { HashSet::new() };
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
                let f = |k: &str| keys.contains(k) as i32 as f32;
                st.cam3.fly(f("w") - f("s"), f("d") - f("a"), f("e") - f("q"));
            } else if middle || (secondary && shift) {
                st.cam3.pan(delta.x, delta.y);
            }
        } else if secondary || middle {
            st.cam2.center -= Vec2::new(delta.x, delta.y) * ppp / st.cam2.zoom;
        }
    }

    // Painting tiles into the selected TileMap2D.
    let painting = !playing && !st.mode_3d && st.tile_brush.is_some() && st.selected.is_some_and(|s| st.tree.get(s).is_some_and(|n| n.kind == NodeKind::TileMap2D));
    if painting {
        let down = ui.input(|i| i.pointer.primary_down());
        if let (true, true, Some(pp)) = (down, resp.hovered() || resp.dragged(), pointer) {
            let id = st.selected.unwrap();
            let w = cams.screen_to_2d(to_px(pp));
            let n = &st.tree.nodes[id];
            let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
            let local = st.tree.global_matrix(id).inverse().transform_point3(w.extend(0.0));
            let (c, r) = ((local.x / tw).floor() as i32, (local.y / th).floor() as i32);
            let brush = if ui.input(|i| i.modifiers.shift) { -1 } else { st.tile_brush.unwrap() };
            let mut grid = gex_core::tiles::parse(n.text("tiles"));
            if c >= 0 && r >= 0 && gex_core::tiles::get(&grid, c, r) != brush {
                st.will_change(&format!("paint {:?}", id));
                gex_core::tiles::set(&mut grid, c, r, brush);
                st.tree.nodes[id].props.insert("tiles".into(), Variant::Text(gex_core::tiles::to_text(&grid)));
            }
        }
    }
    // Select, drag nodes (move / rotate / scale), box select, drop assets (editing only).
    let mut box_rect: Option<Rect> = None;
    if !playing && !painting {
        let drag_id = egui::Id::new("viewport drag");
        let ctrl = ui.input(|i| i.modifiers.command);
        if resp.drag_started_by(egui::PointerButton::Primary) {
            // Where the button went down (the pointer may already have moved on).
            if let Some(pp) = ui.input(|i| i.pointer.press_origin()).or(pointer) {
                let hit = pick(st, &list, &cams, to_px(pp)).filter(|h| Some(*h) != st.tree.root);
                match hit {
                    Some(h) => {
                        if !st.is_selected(h) {
                            if ctrl {
                                st.toggle_select(h);
                            } else {
                                st.select(Some(h));
                            }
                        }
                        let nodes: Vec<(NodeId, Mat4)> = st.selected_roots().into_iter().filter(|i| Some(*i) != st.tree.root).map(|i| (i, st.tree.global_matrix(i))).collect();
                        let plane_y = st.tree.global_matrix(h).w_axis.y;
                        let start = pointer_world(st, &cams, to_px(pp), plane_y).unwrap_or(Vec3::ZERO);
                        let pivot = st.tree.global_matrix(h).w_axis.truncate();
                        ui.data_mut(|d| d.insert_temp(drag_id, Drag { nodes, start, plane_y, pivot, screen_start: to_px(pp) }));
                    }
                    None if !st.mode_3d => {
                        ui.data_mut(|d| d.insert_temp(drag_id.with("box"), pp));
                    }
                    None => {}
                }
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            if let (Some(d), Some(pp)) = (ui.data(|m| m.get_temp::<Drag>(drag_id)), pointer) {
                let now = to_px(pp);
                if let Some(at) = pointer_world(st, &cams, now, d.plane_y) {
                    st.will_change("drag");
                    let tool = st.tool;
                    for (id, start_m) in &d.nodes {
                        if !st.tree.contains(*id) {
                            continue;
                        }
                        let (s0, r0, t0) = start_m.to_scale_rotation_translation();
                        let world = match tool {
                            Tool::Move => {
                                let mut delta = at - d.start;
                                if !st.mode_3d {
                                    delta.z = 0.0;
                                }
                                let mut t = t0 + delta;
                                if ctrl && !st.mode_3d {
                                    t.x = (t.x / 16.0).round() * 16.0;
                                    t.y = (t.y / 16.0).round() * 16.0;
                                } else if ctrl {
                                    t = (t * 4.0).round() / 4.0;
                                }
                                Mat4::from_scale_rotation_translation(s0, r0, t)
                            }
                            Tool::Rotate => {
                                let angle = if st.mode_3d {
                                    (now.x - d.screen_start.x) * 0.01
                                } else {
                                    let a0 = (d.start - d.pivot).truncate();
                                    let a1 = (at - d.pivot).truncate();
                                    a1.y.atan2(a1.x) - a0.y.atan2(a0.x)
                                };
                                let mut angle = angle;
                                if ctrl {
                                    let step = 15f32.to_radians();
                                    angle = (angle / step).round() * step;
                                }
                                let q = if st.mode_3d { gex_core::glam::Quat::from_rotation_y(angle) } else { gex_core::glam::Quat::from_rotation_z(angle) };
                                // Rotate about the pivot (the node that was grabbed).
                                let t = d.pivot + q * (t0 - d.pivot);
                                Mat4::from_scale_rotation_translation(s0, q * r0, t)
                            }
                            Tool::Scale => {
                                let f = if st.mode_3d {
                                    (1.0 - (now.y - d.screen_start.y) * 0.01).max(0.01)
                                } else {
                                    let d0 = (d.start - d.pivot).truncate().length().max(1.0);
                                    ((at - d.pivot).truncate().length() / d0).max(0.01)
                                };
                                Mat4::from_scale_rotation_translation(s0 * f, r0, d.pivot + (t0 - d.pivot) * f)
                            }
                        };
                        let parent_inv = st.tree.nodes[*id].parent.map(|p| st.tree.global_matrix(p).inverse()).unwrap_or(Mat4::IDENTITY);
                        let (s, r, t) = (parent_inv * world).to_scale_rotation_translation();
                        let (ry, rx, rz) = r.to_euler(gex_core::glam::EulerRot::YXZ);
                        let tr = &mut st.tree.nodes[*id].transform;
                        tr.position = t;
                        tr.scale = s;
                        tr.rotation = Vec3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees());
                    }
                }
            }
            if let (Some(b), Some(pp)) = (ui.data(|m| m.get_temp::<Pos2>(drag_id.with("box"))), pointer) {
                box_rect = Some(Rect::from_two_pos(b, pp));
            }
        }
        if resp.drag_stopped() {
            ui.data_mut(|d| d.remove::<Drag>(drag_id));
            if let (Some(b), Some(pp)) = (ui.data_mut(|m| m.remove_temp::<Pos2>(drag_id.with("box"))), pointer) {
                // Box select: nodes whose position is inside the box.
                let r = Rect::from_two_pos(b, pp);
                let (a, z) = (cams.screen_to_2d(to_px(r.min)), cams.screen_to_2d(to_px(r.max)));
                let (lo, hi) = (a.min(z), a.max(z));
                let found: Vec<NodeId> = st
                    .tree
                    .walk()
                    .into_iter()
                    .filter(|i| Some(*i) != st.tree.root && !st.tree.nodes[*i].from_instance)
                    .filter(|i| {
                        let p = st.tree.global_matrix(*i).w_axis.truncate().truncate();
                        p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y
                    })
                    .collect();
                if !ctrl {
                    st.selection.clear();
                }
                for f in found {
                    if !st.selection.contains(&f) {
                        st.selection.push(f);
                    }
                }
                st.selected = st.selection.last().copied().or(st.tree.root);
            }
        }
        if resp.clicked() {
            if let Some(pp) = pointer {
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
        // Colliders stay visible while playing, as a debugging aid.
        let mut o = Overlay::default();
        for c in &list.colliders {
            o.quad2(corners(c.model, c.size, true), [0.3, 0.9, 0.5, 0.5]);
        }
        o
    } else {
        editor_overlay(st, &list, &cams)
    };
    let problems = paint_list(ui, rs, &mut st.renderer, textures, &st.vfs, rect, &list, &cams, &overlay);
    for p in problems {
        st.log(gex_script::LogKind::Error, p);
    }
    // UI nodes, in game-window pixels.
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
    if let Some(p) = &mut st.playing {
        p.ui_events.extend(events);
    }
    if let Some(b) = box_rect {
        ui.painter().rect(b, 0.0, theme::ACCENT.gamma_multiply(0.12), egui::Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
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
    let place = |st: &mut EditorState, id: NodeId| {
        let parent_inv = st.tree.nodes[id].parent.map(|p| st.tree.global_matrix(p).inverse()).unwrap_or(Mat4::IDENTITY);
        st.tree.nodes[id].transform.position = parent_inv.transform_point3(at);
    };
    if lower.ends_with(".jscene") {
        if let Some(id) = st.instance_scene(path) {
            place(st, id);
        }
    } else if gex_assets::model::is_model(&lower) {
        st.will_change("drop");
        let name: String = path.rsplit('/').next().unwrap_or("Model").split('.').next().unwrap_or("Model").to_string();
        let mut n = Node::new(name, NodeKind::Mesh3D);
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
        let mut n = Node::new("Sound", NodeKind::Sound);
        n.props.insert("stream".into(), Variant::Res(path.to_string()));
        let id = st.tree.add(st.tree.root, n);
        st.selected = Some(id);
    } else if gex_assets::image_load::is_image(&lower) {
        st.will_change("drop");
        let name: String = path.rsplit('/').next().unwrap_or("Sprite").split('.').next().unwrap_or("Sprite").to_string();
        let id = if st.mode_3d {
            let mut n = Node::new(name, NodeKind::Mesh3D);
            n.props.insert("texture".into(), Variant::Res(path.to_string()));
            st.tree.add(st.tree.root, n)
        } else {
            let mut n = Node::new(name, NodeKind::Sprite2D);
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
        st.status = format!("{} can't be placed in a scene yet", path);
    }
}

/// Particles2D nodes play in the editor too, so their look can be tuned without running the game.
fn preview_particles(ctx: &egui::Context, st: &mut EditorState, list: &mut gex_core::render::RenderList) {
    let now = std::time::Instant::now();
    let dt = now.duration_since(st.particle_clock).as_secs_f32().min(0.1);
    st.particle_clock = now;
    let any = st.tree.walk().into_iter().any(|i| st.tree.nodes[i].kind == NodeKind::Particles2D);
    if !any && st.preview_particles.is_empty() {
        return;
    }
    gex_script::particles::update(&st.tree, &mut st.preview_particles, dt);
    gex_script::particles::draw(&st.tree, &st.preview_particles, list);
    if st.preview_particles.values().any(|e| e.count() > 0) {
        ctx.request_repaint();
    }
}
