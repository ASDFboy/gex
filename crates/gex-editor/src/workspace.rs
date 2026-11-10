//! The GameMaker-style layout (the default): a row of tabs along the top. Workspace tabs are
//! canvases of floating windows: a scene window (its nodes and their properties, like an
//! object), linked to its events, linked to the scripts they open. Each opened scene gets a
//! tab of its own (like a room) to place and edit instances: the instance list and inspector
//! on the left, the view in the middle. The assets and the console sit beside every tab.
//!
//! The older docked layout is still there (View → Classic docked layout).

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::state::EditorState;
use crate::{code, panels, theme, viewport};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WinKind {
    /// A scene as a template: its nodes, and the properties of the one picked (`node` is its
    /// path in the scene, "" for the root).
    Scene { res: String, node: String },
    /// The events (on_... functions) of a node's scripts.
    Events { res: String, node: String },
    Script { res: String },
    /// Timeline, Profiler or Find in files.
    Tool { name: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Win {
    pub id: u64,
    pub kind: WinKind,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    /// The window this one was opened from (a line joins them).
    pub parent: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub windows: Vec<Win>,
    pub pan: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TopTab {
    Workspace(usize),
    Scene(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<TopTab>,
    pub active: usize,
    pub next_id: u64,
    pub show_assets: bool,
    pub show_console: bool,
    /// The scene being edited last frame (to notice it changing from elsewhere).
    #[serde(skip)]
    pub last_scene: Option<String>,
    #[serde(skip)]
    saved_text: String,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            workspaces: vec![Workspace { name: "Workspace 1".into(), ..Default::default() }],
            tabs: vec![TopTab::Workspace(0)],
            active: 0,
            next_id: 1,
            show_assets: true,
            show_console: true,
            last_scene: None,
            saved_text: String::new(),
        }
    }
}

fn file(dir: &Path) -> PathBuf {
    dir.join(".gex").join("workspace.ron")
}

impl Layout {
    /// The project's saved layout (each project remembers its own), or a fresh one.
    pub fn load(dir: &Path) -> Layout {
        let mut l: Layout = std::fs::read_to_string(file(dir)).ok().and_then(|t| ron::from_str(&t).ok()).unwrap_or_default();
        if l.workspaces.is_empty() {
            l = Layout::default();
        }
        l.tabs.retain(|t| !matches!(t, TopTab::Workspace(i) if *i >= l.workspaces.len()));
        if l.tabs.is_empty() {
            l.tabs.push(TopTab::Workspace(0));
        }
        l.active = l.active.min(l.tabs.len() - 1);
        l.saved_text = ron::to_string(&l).unwrap_or_default();
        l
    }

    /// Writes the layout when it changed.
    pub fn save(&mut self, dir: &Path) {
        let Ok(text) = ron::ser::to_string_pretty(self, Default::default()) else { return };
        let compact = ron::to_string(self).unwrap_or_default();
        if compact == self.saved_text {
            return;
        }
        let f = file(dir);
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if std::fs::write(&f, text).is_ok() {
            self.saved_text = compact;
        }
    }

    pub fn open_scene_tab(&mut self, res: &str) {
        let t = TopTab::Scene(res.to_string());
        match self.tabs.iter().position(|x| *x == t) {
            Some(i) => self.active = i,
            None => {
                self.tabs.push(t);
                self.active = self.tabs.len() - 1;
            }
        }
    }

    /// The workspace to put new windows in: the one showing, else the first.
    fn current_workspace(&self) -> usize {
        match self.tabs.get(self.active) {
            Some(TopTab::Workspace(i)) => *i,
            _ => self.tabs.iter().find_map(|t| if let TopTab::Workspace(i) = t { Some(*i) } else { None }).unwrap_or(0),
        }
    }

    fn show_workspace(&mut self, wi: usize) {
        if let Some(i) = self.tabs.iter().position(|t| *t == TopTab::Workspace(wi)) {
            self.active = i;
        }
    }

    /// Opens a window (or brings an open one forward), beside `near` if given. Gives its id.
    pub fn open_window(&mut self, wi: usize, kind: WinKind, near: Option<u64>) -> u64 {
        let id = self.next_id;
        let ws = &mut self.workspaces[wi];
        if let Some(i) = ws.windows.iter().position(|w| w.kind == kind) {
            let mut w = ws.windows.remove(i);
            if near.is_some() && near != Some(w.id) {
                w.parent = near;
            }
            let id = w.id;
            ws.windows.push(w);
            return id;
        }
        let size = match kind {
            WinKind::Scene { .. } => [320.0, 520.0],
            WinKind::Events { .. } => [260.0, 320.0],
            WinKind::Script { .. } => [620.0, 460.0],
            WinKind::Tool { .. } => [640.0, 300.0],
        };
        let pos = match near.and_then(|n| ws.windows.iter().find(|w| w.id == n)) {
            Some(p) => {
                // To the right of the window it came from, below any already there.
                let x = p.pos[0] + p.size[0] + 50.0;
                let mut y = p.pos[1];
                while ws.windows.iter().any(|w| (w.pos[0] - x).abs() < 20.0 && (w.pos[1] - y).abs() < 20.0) {
                    y += 40.0;
                }
                [x, y]
            }
            None => {
                let n = ws.windows.len() as f32;
                [-ws.pan[0] + 30.0 + 30.0 * (n % 8.0), -ws.pan[1] + 30.0 + 30.0 * (n % 8.0)]
            }
        };
        ws.windows.push(Win { id, kind, pos, size, parent: near });
        self.next_id += 1;
        id
    }

    /// Opens a tool (Timeline, Profiler, Find in files) as a window in the current workspace.
    pub fn open_tool(&mut self, name: &str) {
        let wi = self.current_workspace();
        self.open_window(wi, WinKind::Tool { name: name.to_string() }, None);
        self.show_workspace(wi);
    }

    /// Opens a scene's window in the current workspace and shows that workspace.
    pub fn open_scene_window(&mut self, res: &str) {
        let wi = self.current_workspace();
        self.open_window(wi, WinKind::Scene { res: res.to_string(), node: String::new() }, None);
        self.show_workspace(wi);
    }
}

/// The editor body in the GameMaker-style layout.
pub fn show(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, rs: Option<&egui_wgpu::RenderState>, viewport_tex: &mut Option<egui::TextureId>, previews: &mut std::collections::HashMap<String, egui::TextureHandle>) {
    // The scene being edited changed from elsewhere (an "open" button, :e): show its tab.
    if st.scene_res != l.last_scene {
        if let Some(r) = st.scene_res.clone() {
            l.open_scene_tab(&r);
        }
        l.last_scene = st.scene_res.clone();
    }
    for r in std::mem::take(&mut st.open_windows) {
        l.open_scene_window(&r);
    }
    // Scripts asked for from outside the workspace windows open in the workspace.
    if !st.open_requests.is_empty() {
        let wi = l.current_workspace();
        for r in std::mem::take(&mut st.open_requests) {
            l.open_window(wi, WinKind::Script { res: r }, None);
        }
        l.show_workspace(wi);
    }
    let glass = egui::Frame::new().fill(theme::glass_fill()).stroke(theme::glass_edge()).inner_margin(6.0).corner_radius(4.0);
    if l.show_assets {
        egui::Panel::right("ws assets").resizable(true).default_size(300.0).size_range(180.0..=700.0).frame(glass).show(ui, |ui| {
            // In a child that can't widen the panel (the asset grid fills whatever width it gets).
            let r = ui.available_rect_before_wrap();
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r).id_salt("ws assets inner"));
            inner.set_clip_rect(r.intersect(ui.clip_rect()));
            inner.label(RichText::new("Assets").strong());
            panels::assets(&mut inner, st, previews);
            ui.allocate_rect(r, Sense::hover());
        });
    }
    if l.show_console {
        egui::Panel::bottom("ws console").resizable(true).default_size(130.0).frame(glass).show(ui, |ui| panels::console(ui, st));
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        tab_strip(ui, l, st);
        match l.tabs.get(l.active).cloned() {
            Some(TopTab::Workspace(wi)) => canvas(ui, l, wi, st),
            Some(TopTab::Scene(res)) => scene_tab(ui, l, st, &res, rs, viewport_tex),
            None => {}
        }
    });
}

fn tab_strip(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState) {
    let mut activate = None;
    let mut close = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, t) in l.tabs.iter().enumerate() {
            let active = i == l.active;
            let (label, dirty) = match t {
                TopTab::Workspace(w) => (format!("🖵 {}", l.workspaces[*w].name), false),
                TopTab::Scene(r) => (format!("🎬 {}", r.rsplit('/').next().unwrap_or(r).trim_end_matches(".jscene")), st.is_dirty(r)),
            };
            let text = if dirty { format!("{} *", label) } else { label };
            let r = ui.add(egui::Button::selectable(active, RichText::new(text).color(if active { theme::TEXT } else { theme::TEXT_DIM })));
            if active {
                // A Monokai-green line under the tab being shown.
                let y = r.rect.bottom() + 1.0;
                ui.painter().line_segment([Pos2::new(r.rect.left(), y), Pos2::new(r.rect.right(), y)], Stroke::new(2.0, theme::MK_GREEN));
            }
            if r.clicked() {
                activate = Some(i);
            }
            let closeable = match t {
                TopTab::Workspace(_) => l.tabs.iter().filter(|t| matches!(t, TopTab::Workspace(_))).count() > 1,
                TopTab::Scene(_) => true,
            };
            if closeable && ui.add(egui::Button::new(RichText::new("✖").small().color(theme::TEXT_DIM)).frame(false)).on_hover_text("close").clicked() {
                close = Some(i);
            }
            ui.add_space(6.0);
        }
        if ui.small_button("+").on_hover_text("a new workspace").clicked() {
            let n = l.workspaces.len() + 1;
            l.workspaces.push(Workspace { name: format!("Workspace {}", n), ..Default::default() });
            l.tabs.push(TopTab::Workspace(l.workspaces.len() - 1));
            activate = Some(l.tabs.len() - 1);
        }
    });
    ui.add_space(2.0);
    // While playing, the game stays in its own tab (other scenes open once it stops).
    if let (Some(i), true) = (activate, st.playing.is_some()) {
        if matches!(l.tabs.get(i), Some(TopTab::Scene(r)) if Some(r) != st.scene_res.as_ref()) {
            st.status = "stop the game to switch scenes".into();
            activate = None;
        }
    }
    if let Some(i) = activate {
        l.active = i;
        if let Some(TopTab::Scene(r)) = l.tabs.get(i) {
            let r = r.clone();
            st.open_scene(&r);
            l.last_scene = st.scene_res.clone();
        }
    }
    if let Some(i) = close {
        match l.tabs.remove(i) {
            TopTab::Scene(r) => {
                // Closing the tab keeps unsaved changes if a workspace window still shows the scene.
                let in_use = l.workspaces.iter().any(|w| w.windows.iter().any(|x| matches!(&x.kind, WinKind::Scene { res, .. } | WinKind::Events { res, .. } if *res == r)));
                if !in_use && !st.is_dirty(&r) {
                    st.close_scene_res(&r);
                }
            }
            TopTab::Workspace(w) => {
                l.workspaces.remove(w);
                for t in &mut l.tabs {
                    if let TopTab::Workspace(x) = t {
                        if *x > w {
                            *x -= 1;
                        }
                    }
                }
            }
        }
        l.active = l.active.min(l.tabs.len().saturating_sub(1));
        if let Some(TopTab::Scene(r)) = l.tabs.get(l.active) {
            let r = r.clone();
            st.open_scene(&r);
        }
        l.last_scene = st.scene_res.clone();
    }
}

/// A scene's own tab: its instances and their properties on the left, the view in the middle.
fn scene_tab(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, res: &str, rs: Option<&egui_wgpu::RenderState>, viewport_tex: &mut Option<egui::TextureId>) {
    if st.scene_res.as_deref() != Some(res) && st.playing.is_none() {
        st.open_scene(res);
        l.last_scene = st.scene_res.clone();
    }
    let glass = egui::Frame::new().fill(theme::glass_fill()).stroke(theme::glass_edge()).inner_margin(6.0).corner_radius(4.0);
    egui::Panel::left("scene tab side").resizable(true).default_size(290.0).frame(glass).show(ui, |ui| {
        egui::Panel::bottom("scene tab inspector").resizable(true).default_size(ui.available_height() * 0.6).frame(egui::Frame::NONE.inner_margin(egui::Margin { top: 6, ..Default::default() })).show(ui, |ui| {
            ui.label(RichText::new("Instance").strong());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| panels::inspector(ui, st));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Instances").strong());
            if ui.small_button("🖵 as window").on_hover_text("open this scene's window in the workspace (its nodes, properties and events)").clicked() {
                l.open_scene_window(res);
            }
        });
        panels::hierarchy(ui, st);
    });
    egui::CentralPanel::default().frame(glass).show(ui, |ui| viewport::viewport(ui, st, rs, viewport_tex));
}

/// A workspace: a canvas of windows. Drag the background (or hold the middle button) to look around.
fn canvas(ui: &mut egui::Ui, l: &mut Layout, wi: usize, st: &mut EditorState) {
    let rect = ui.available_rect_before_wrap();
    let bg = ui.allocate_rect(rect, Sense::click_and_drag());
    {
        let ws = &mut l.workspaces[wi];
        if bg.dragged() {
            ws.pan[0] += bg.drag_delta().x;
            ws.pan[1] += bg.drag_delta().y;
        }
        let (middle, delta) = ui.input(|i| (i.pointer.middle_down(), i.pointer.delta()));
        if middle && bg.contains_pointer() {
            ws.pan[0] += delta.x;
            ws.pan[1] += delta.y;
        }
    }
    let pan = Vec2::from(l.workspaces[wi].pan);
    let origin = rect.min + pan;
    // A scene or script dropped from Assets opens as a window where it was dropped.
    if let Some(p) = bg.dnd_release_payload::<panels::AssetPath>() {
        let kind = if p.0.ends_with(".jscene") {
            Some(WinKind::Scene { res: p.0.clone(), node: String::new() })
        } else if p.0.ends_with(".jlb") && p.0.starts_with("res://") {
            Some(WinKind::Script { res: p.0.clone() })
        } else {
            None
        };
        if let (Some(kind), Some(at)) = (kind, bg.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())) {
            let id = l.open_window(wi, kind, None);
            if let Some(w) = l.workspaces[wi].windows.iter_mut().find(|w| w.id == id) {
                w.pos = [at.x - origin.x - 20.0, at.y - origin.y - 10.0];
            }
        }
    }
    let painter = ui.painter_at(rect);
    // A faint dot grid that moves with the canvas.
    let step = 32.0;
    let (ox, oy) = (pan.x.rem_euclid(step), pan.y.rem_euclid(step));
    let dot = Color32::from_rgba_unmultiplied(255, 255, 255, 14);
    let mut y = rect.top() + oy;
    while y < rect.bottom() {
        let mut x = rect.left() + ox;
        while x < rect.right() {
            painter.circle_filled(Pos2::new(x, y), 1.0, dot);
            x += step;
        }
        y += step;
    }
    let wins = l.workspaces[wi].windows.clone();
    if wins.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Double-click a scene or script in Assets to open its window here.\nOpen a scene as its own tab with ⤢ on its window, to place instances.\nDrag the background to look around.",
            egui::FontId::proportional(14.0),
            theme::TEXT_DIM,
        );
    }
    // Lines from each window to the one it was opened from.
    let rect_of = |w: &Win| Rect::from_min_size(origin + Vec2::from(w.pos), Vec2::from(w.size));
    for w in &wins {
        if let Some(p) = w.parent.and_then(|p| wins.iter().find(|x| x.id == p)) {
            let (a, b) = (rect_of(p), rect_of(w));
            let from = Pos2::new(a.right(), a.top() + 40.0);
            let to = Pos2::new(b.left(), b.top() + 14.0);
            let k = ((to.x - from.x).abs() * 0.5).max(40.0);
            let curve = egui::epaint::CubicBezierShape::from_points_stroke([from, from + Vec2::new(k, 0.0), to - Vec2::new(k, 0.0), to], false, Color32::TRANSPARENT, Stroke::new(2.0, theme::alpha(theme::MK_GREEN, 170)));
            painter.add(curve);
        }
    }
    let mut moves: Vec<(u64, Vec2)> = Vec::new();
    let mut resizes: Vec<(u64, Vec2)> = Vec::new();
    let mut front = None;
    let mut close = None;
    let mut opened: Vec<(WinKind, u64)> = Vec::new();
    let mut set_node: Vec<(u64, String)> = Vec::new();
    let mut open_tab: Option<String> = None;
    for w in &wins {
        let wr = rect_of(w);
        if !wr.intersects(rect) {
            continue;
        }
        let clip = wr.intersect(rect);
        let mut wui = ui.new_child(egui::UiBuilder::new().max_rect(wr).id_salt(("ws win", w.id)));
        wui.set_clip_rect(clip);
        let p = wui.painter().clone();
        p.rect_filled(wr.translate(Vec2::new(0.0, 4.0)).expand(2.0), 7.0, Color32::from_black_alpha(70));
        p.rect_filled(wr, 6.0, theme::alpha(Color32::from_rgb(0x1a, 0x1a, 0x17), 240));
        p.rect_stroke(wr, 6.0, theme::glass_edge(), egui::StrokeKind::Inside);
        // Title bar: drag to move.
        let title_r = Rect::from_min_size(wr.min, Vec2::new(wr.width(), 26.0));
        p.add(theme::header(title_r.shrink2(Vec2::new(1.0, 1.0)), false));
        let (icon, title) = match &w.kind {
            WinKind::Scene { res, .. } => ("🎬", format!("{}{}", short(res), if st.is_dirty(res) { " *" } else { "" })),
            WinKind::Events { res, node } => ("⚡", format!("{}: events", if node.is_empty() { short(res) } else { node.rsplit('/').next().unwrap_or(node).to_string() })),
            WinKind::Script { res } => ("📜", format!("{}{}", short(res), if st.scripts.get(res).is_some_and(|s| s.dirty()) { " *" } else { "" })),
            WinKind::Tool { name } => ("🔧", name.clone()),
        };
        p.text(title_r.left_center() + Vec2::new(8.0, 0.0), egui::Align2::LEFT_CENTER, format!("{}  {}", icon, title), egui::FontId::proportional(13.0), theme::TEXT);
        let drag = wui.interact(title_r.shrink2(Vec2::new(0.0, 0.0)).with_max_x(title_r.right() - 26.0), egui::Id::new(("ws title", w.id)), Sense::click_and_drag());
        if drag.dragged() {
            moves.push((w.id, drag.drag_delta()));
        }
        if drag.is_pointer_button_down_on() {
            front = Some(w.id);
        }
        let close_r = Rect::from_center_size(Pos2::new(title_r.right() - 13.0, title_r.center().y), Vec2::splat(18.0));
        let cr = wui.interact(close_r, egui::Id::new(("ws close", w.id)), Sense::click());
        p.text(close_r.center(), egui::Align2::CENTER_CENTER, "✖", egui::FontId::proportional(12.0), if cr.hovered() { theme::MK_PINK } else { theme::TEXT_DIM });
        if cr.clicked() {
            close = Some(w.id);
        }
        // Contents.
        let body = Rect::from_min_max(Pos2::new(wr.left() + 6.0, title_r.bottom() + 4.0), Pos2::new(wr.right() - 6.0, wr.bottom() - 8.0));
        let mut cui = wui.new_child(egui::UiBuilder::new().max_rect(body).id_salt(("ws body", w.id)));
        cui.set_clip_rect(body.intersect(rect));
        let before = st.open_requests.len();
        match &w.kind {
            WinKind::Scene { res, node } => {
                if let Some((new_node, events, tab)) = scene_window(&mut cui, st, res, node, w.id) {
                    if new_node != *node {
                        set_node.push((w.id, new_node.clone()));
                    }
                    if events {
                        opened.push((WinKind::Events { res: res.clone(), node: new_node }, w.id));
                    }
                    if tab {
                        open_tab = Some(res.clone());
                    }
                }
            }
            WinKind::Events { res, node } => {
                if st.playing.is_some() {
                    cui.label(RichText::new("Stop the game to change events.").color(theme::TEXT_DIM));
                } else {
                    let node = node.clone();
                    st.with_scene(res, |st| {
                        let id = st.tree.find_path(None, &node).or(st.tree.root);
                        st.selected = id;
                        st.selection = id.into_iter().collect();
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut cui, |ui| panels::events(ui, st));
                    });
                }
            }
            WinKind::Script { res } => {
                if !st.scripts.contains_key(res) {
                    st.open_script(res, None);
                    st.open_requests.pop();
                }
                code::script_ui(&mut cui, st, res);
            }
            WinKind::Tool { name } => match name.as_str() {
                "Timeline" => crate::timeline::timeline(&mut cui, st),
                "Profiler" => {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut cui, |ui| crate::tools::profiler(ui, st));
                }
                _ => crate::tools::search(&mut cui, st),
            },
        }
        // Scripts this window asked for open beside it.
        let asked: Vec<String> = st.open_requests.drain(before..).collect();
        for r in asked {
            opened.push((WinKind::Script { res: r }, w.id));
        }
        if wui.ui_contains_pointer() && wui.input(|i| i.pointer.primary_pressed()) {
            front = Some(w.id);
        }
        // Corner: drag to resize.
        let grip = Rect::from_min_size(wr.right_bottom() - Vec2::splat(14.0), Vec2::splat(14.0));
        let gr = wui.interact(grip, egui::Id::new(("ws grip", w.id)), Sense::drag());
        for k in 0..3 {
            let o = 4.0 + k as f32 * 4.0;
            p.line_segment([wr.right_bottom() - Vec2::new(o, 2.0), wr.right_bottom() - Vec2::new(2.0, o)], Stroke::new(1.0, theme::TEXT_DIM));
        }
        if gr.dragged() {
            resizes.push((w.id, gr.drag_delta()));
        }
        if gr.hovered() || gr.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }
    }
    let ws = &mut l.workspaces[wi];
    for (id, d) in moves {
        if let Some(w) = ws.windows.iter_mut().find(|w| w.id == id) {
            w.pos[0] += d.x;
            w.pos[1] += d.y;
        }
    }
    for (id, d) in resizes {
        if let Some(w) = ws.windows.iter_mut().find(|w| w.id == id) {
            w.size[0] = (w.size[0] + d.x).max(220.0);
            w.size[1] = (w.size[1] + d.y).max(140.0);
        }
    }
    for (id, node) in set_node {
        if let Some(w) = ws.windows.iter_mut().find(|w| w.id == id) {
            if let WinKind::Scene { node: n, .. } = &mut w.kind {
                *n = node;
            }
        }
    }
    if let Some(id) = front {
        if let Some(i) = ws.windows.iter().position(|w| w.id == id) {
            let w = ws.windows.remove(i);
            ws.windows.push(w);
        }
    }
    if let Some(id) = close {
        ws.windows.retain(|w| w.id != id);
        for w in &mut ws.windows {
            if w.parent == Some(id) {
                w.parent = None;
            }
        }
    }
    for (kind, from) in opened {
        // An events window replaces the one this scene window opened before.
        if let WinKind::Events { res, .. } = &kind {
            let res = res.clone();
            l.workspaces[wi].windows.retain(|w| !(matches!(&w.kind, WinKind::Events { res: r, .. } if *r == res) && w.parent == Some(from)));
        }
        let id = l.open_window(wi, kind, Some(from));
        // Slide the canvas so the new window is in view.
        let ws = &mut l.workspaces[wi];
        if let Some(w) = ws.windows.iter().find(|w| w.id == id) {
            let (right, bottom) = (w.pos[0] + w.size[0] + ws.pan[0], w.pos[1] + w.size[1] + ws.pan[1]);
            if right > rect.width() - 10.0 {
                ws.pan[0] -= right - rect.width() + 20.0;
            }
            if bottom > rect.height() - 10.0 {
                ws.pan[1] -= (bottom - rect.height() + 20.0).min(w.pos[1] + ws.pan[1] - 10.0).max(0.0);
            }
        }
    }
    if let Some(r) = open_tab {
        st.open_scene(&r);
        l.open_scene_tab(&r);
        l.last_scene = st.scene_res.clone();
    }
}

fn short(res: &str) -> String {
    res.rsplit('/').next().unwrap_or(res).to_string()
}

/// A scene window: buttons, its nodes, and the picked node's properties. Gives back the picked
/// node's path, and whether its events or its own tab were asked for.
fn scene_window(ui: &mut egui::Ui, st: &mut EditorState, res: &str, node: &str, win: u64) -> Option<(String, bool, bool)> {
    if st.playing.is_some() {
        ui.label(RichText::new("Stop the game to edit scenes here.").color(theme::TEXT_DIM));
        return Some((node.to_string(), false, false));
    }
    let r = st.with_scene(res, |st| {
        let mut picked = st.tree.find_path(None, node).or(st.tree.root);
        let mut events = false;
        let mut tab = false;
        ui.horizontal(|ui| {
            if ui.button("⤢ Open").on_hover_text("open the scene in its own tab, to place and edit instances").clicked() {
                tab = true;
            }
            if ui.button("⚡ Events").on_hover_text("the events of the picked node's scripts").clicked() {
                events = true;
            }
            let dirty = st.scene_dirty();
            if ui.add_enabled(dirty, egui::Button::new("💾 Save")).clicked() {
                st.save_scene();
            }
        });
        // The scene's nodes (a template can have children: a gun on a player, a hitbox).
        let ids = st.tree.walk();
        if ids.len() > 1 {
            egui::CollapsingHeader::new(format!("Nodes ({})", ids.len())).id_salt(("ws nodes", win)).default_open(ids.len() <= 8).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(140.0).id_salt(("ws nodes scroll", win)).show(ui, |ui| {
                    for id in &ids {
                        let n = &st.tree.nodes[*id];
                        let depth = st.tree.path_of(*id).matches('/').count().saturating_sub(1);
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 12.0);
                            if ui.selectable_label(picked == Some(*id), format!("{} {}", panels::kind_icon(n.kind), n.name)).clicked() {
                                picked = Some(*id);
                            }
                        });
                    }
                });
            });
        }
        st.selected = picked;
        st.selection = picked.into_iter().collect();
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("ws props", win)).show(ui, |ui| panels::inspector(ui, st));
        let path = match st.selected {
            Some(id) if Some(id) != st.tree.root => st.tree.path_of(id),
            _ => String::new(),
        };
        (path, events, tab)
    });
    if r.is_none() {
        ui.label(RichText::new(format!("Couldn't open {}.", res)).color(theme::BAD));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_tabs_and_saving() {
        let mut l = Layout::default();
        let a = l.open_window(0, WinKind::Scene { res: "res://a.jscene".into(), node: String::new() }, None);
        let e = l.open_window(0, WinKind::Events { res: "res://a.jscene".into(), node: String::new() }, Some(a));
        // Opening the same thing again brings it forward instead of making another.
        assert_eq!(l.open_window(0, WinKind::Scene { res: "res://a.jscene".into(), node: String::new() }, None), a);
        assert_eq!(l.workspaces[0].windows.len(), 2);
        let ev = l.workspaces[0].windows.iter().find(|w| w.id == e).unwrap();
        let sc = l.workspaces[0].windows.iter().find(|w| w.id == a).unwrap();
        assert_eq!(ev.parent, Some(a));
        assert!(ev.pos[0] > sc.pos[0] + sc.size[0], "events open to the right");
        l.open_scene_tab("res://a.jscene");
        l.open_scene_tab("res://a.jscene");
        assert_eq!(l.tabs.len(), 2);
        assert_eq!(l.tabs[l.active], TopTab::Scene("res://a.jscene".into()));
        let dir = std::env::temp_dir().join(format!("gex-ws-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        l.save(&dir);
        let back = Layout::load(&dir);
        assert_eq!(back.tabs, l.tabs);
        assert_eq!(back.workspaces[0].windows.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
