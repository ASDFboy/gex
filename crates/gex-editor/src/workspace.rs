//! The GameMaker-style layout (the default): a row of tabs along the top. Workspace tabs are
//! canvases of floating windows: a blueprint window (its objects and their properties, like an
//! object), linked to its events, linked to the scripts they open. Each opened blueprint gets a
//! tab of its own (like a room) to place and edit instances: the instance list and inspector
//! on the left, the view in the middle. The assets and the console sit beside every tab.
//!
//! The older docked layout is still there (View → Classic docked layout).

use crate::docs::Tip as _;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::state::EditorState;
use crate::{code, panels, theme, viewport};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WinKind {
    /// A blueprint as a template: its objects, and the properties of the one picked (`object` is its
    /// path in the blueprint, "" for the root).
    Blueprint { res: String, object: String },
    /// The events (on_... functions) of an object's scripts.
    Events { res: String, object: String },
    Script { res: String },
    /// Timeline or Profiler.
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
    /// Where the view is gliding to (after a window opens).
    #[serde(skip)]
    pan_to: Option<[f32; 2]>,
    /// The windows (and their sizes) the last arrangement was made for.
    #[serde(skip)]
    arranged: Option<Vec<(u64, [i32; 2])>>,
}

impl Workspace {
    /// Lays the windows out so none overlap: each window opened on its own starts a row (new
    /// rows go under the others), and windows opened from another sit to its right.
    fn arrange(&mut self) {
        const GAP: f32 = 36.0;
        fn place(ws: &mut [Win], kids: &std::collections::HashMap<u64, Vec<usize>>, i: usize, x: f32, y: f32) -> f32 {
            ws[i].pos = [x, y];
            let (id, [w, h]) = (ws[i].id, ws[i].size);
            let mut cy = y;
            for &k in kids.get(&id).map(|v| v.as_slice()).unwrap_or(&[]) {
                cy += place(ws, kids, k, x + w + GAP, cy) + GAP;
            }
            h.max(cy - GAP - y)
        }
        let ids: Vec<u64> = self.windows.iter().map(|w| w.id).collect();
        let mut order: Vec<usize> = (0..self.windows.len()).collect();
        order.sort_by_key(|i| self.windows[*i].id);
        let mut kids: std::collections::HashMap<u64, Vec<usize>> = Default::default();
        let mut roots = Vec::new();
        for i in order {
            match self.windows[i].parent.filter(|p| ids.contains(p) && *p != self.windows[i].id) {
                Some(p) => kids.entry(p).or_default().push(i),
                None => roots.push(i),
            }
        }
        let mut y = 0.0;
        for r in roots {
            y += place(&mut self.windows, &kids, r, 0.0, y) + GAP;
        }
    }

    /// Re-arranges when windows were opened, closed or resized, and glides the view to new ones.
    fn keep_arranged(&mut self, view: Vec2) {
        let mut now: Vec<(u64, [i32; 2])> = self.windows.iter().map(|w| (w.id, [w.size[0] as i32, w.size[1] as i32])).collect();
        now.sort();
        let Some(before) = self.arranged.replace(now.clone()) else { return };
        if before == now {
            return;
        }
        self.arrange();
        let fresh: Vec<u64> = now.iter().map(|n| n.0).filter(|id| !before.iter().any(|b| b.0 == *id)).collect();
        let rect_of = |w: &Win| Rect::from_min_size(Pos2::from(w.pos), Vec2::from(w.size));
        let Some(mut bb) = self.windows.iter().filter(|w| fresh.contains(&w.id)).map(rect_of).reduce(|a, b| a.union(b)) else { return };
        // The windows they were opened from stay in view too, when there's room.
        let mut up: Vec<u64> = self.windows.iter().filter(|w| fresh.contains(&w.id)).filter_map(|w| w.parent).collect();
        while let Some(p) = up.pop() {
            if let Some(w) = self.windows.iter().find(|w| w.id == p) {
                let both = bb.union(rect_of(w));
                if both.width() <= view.x - 40.0 && both.height() <= view.y - 40.0 {
                    bb = both;
                    up.extend(w.parent);
                }
            }
        }
        self.pan_to = Some([20.0 - bb.min.x, 20.0 - bb.min.y]);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TopTab {
    Workspace(usize),
    Blueprint(String),
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
    /// The blueprint being edited last frame (to notice it changing from elsewhere).
    #[serde(skip)]
    pub last_blueprint: Option<String>,
    /// While the game plays: the tab to go back to when it stops.
    #[serde(skip)]
    tab_before_play: Option<usize>,
    #[serde(skip)]
    was_playing: bool,
    /// How many file changes (renames, deletes) the tabs and windows have caught up with.
    #[serde(skip)]
    file_ops_seen: usize,
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
            last_blueprint: None,
            tab_before_play: None,
            was_playing: false,
            file_ops_seen: 0,
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

    pub fn open_blueprint_tab(&mut self, res: &str) {
        let t = TopTab::Blueprint(res.to_string());
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
            WinKind::Blueprint { .. } => [320.0, 520.0],
            WinKind::Events { .. } => [260.0, 320.0],
            WinKind::Script { .. } => [720.0, 520.0],
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

    /// Opens a tool (Timeline, Profiler) as a window in the current workspace.
    pub fn open_tool(&mut self, name: &str) {
        let wi = self.current_workspace();
        self.open_window(wi, WinKind::Tool { name: name.to_string() }, None);
        self.show_workspace(wi);
    }

    /// Opens a blueprint's window in the current workspace and shows that workspace.
    pub fn open_blueprint_window(&mut self, res: &str) {
        let wi = self.current_workspace();
        let fresh = !self.workspaces[wi].windows.iter().any(|w| matches!(&w.kind, WinKind::Blueprint { res: r, .. } if r == res));
        let id = self.open_window(wi, WinKind::Blueprint { res: res.to_string(), object: String::new() }, None);
        if fresh {
            // Its events come with it, like an object's in GameMaker.
            self.open_window(wi, WinKind::Events { res: res.to_string(), object: String::new() }, Some(id));
        }
        self.show_workspace(wi);
    }
}

/// The editor body in the GameMaker-style layout.
/// The blueprint being edited changed from elsewhere (Assets, an "open" button, :e): show its tab.
fn follow_blueprint(l: &mut Layout, st: &EditorState) {
    if st.blueprint_res != l.last_blueprint {
        if let Some(r) = st.blueprint_res.clone() {
            l.open_blueprint_tab(&r);
        }
        l.last_blueprint = st.blueprint_res.clone();
    }
}

/// After a rename or delete in Assets: tabs and windows follow a renamed file, and those of a
/// file that's gone close.
fn follow_files(l: &mut Layout, st: &EditorState) {
    let n = st.file_undo.len();
    if n == l.file_ops_seen {
        return;
    }
    let renamed = match st.file_undo.last() {
        Some(crate::state::FileOp::Renamed { old, new }) if n > l.file_ops_seen => Some((old.clone(), new.clone())),
        _ => None,
    };
    l.file_ops_seen = n;
    let here = l.tabs.get(l.active).cloned();
    let fix = |r: &mut String| {
        if let Some((old, new)) = &renamed {
            if r == old {
                *r = new.clone();
            }
        }
    };
    let gone = |r: &str| r.starts_with("res://") && !st.project.res_to_path(r).is_some_and(|p| p.is_file());
    for t in &mut l.tabs {
        if let TopTab::Blueprint(r) = t {
            fix(r);
        }
    }
    let mut seen: Vec<TopTab> = Vec::new();
    l.tabs.retain(|t| {
        let keep = !seen.contains(t) && !matches!(t, TopTab::Blueprint(r) if gone(r));
        seen.push(t.clone());
        keep
    });
    for w in &mut l.workspaces {
        for x in &mut w.windows {
            match &mut x.kind {
                WinKind::Blueprint { res, .. } | WinKind::Events { res, .. } | WinKind::Script { res } => fix(res),
                WinKind::Tool { .. } => {}
            }
        }
        w.windows.retain(|x| !matches!(&x.kind, WinKind::Blueprint { res, .. } | WinKind::Events { res, .. } | WinKind::Script { res } if gone(res)));
    }
    let here = here.map(|mut t| {
        if let TopTab::Blueprint(r) = &mut t {
            fix(r);
        }
        t
    });
    l.active = here.and_then(|t| l.tabs.iter().position(|x| *x == t)).unwrap_or(0);
}

pub fn show(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, rs: Option<&egui_wgpu::RenderState>, viewport_tex: &mut Option<egui::TextureId>, previews: &mut std::collections::HashMap<String, egui::TextureHandle>) {
    follow_files(l, st);
    follow_blueprint(l, st);
    for r in std::mem::take(&mut st.open_windows) {
        l.open_blueprint_window(&r);
    }
    // Play shows the game (its blueprint's tab); Stop goes back to where you were.
    let playing = st.playing.is_some();
    if playing && !l.was_playing {
        if let Some(r) = st.blueprint_res.clone() {
            let before = l.active;
            l.open_blueprint_tab(&r);
            l.tab_before_play = (l.active != before).then_some(before);
        }
    } else if !playing && l.was_playing {
        if let Some(i) = l.tab_before_play.take().filter(|i| *i < l.tabs.len()) {
            l.active = i;
        }
    }
    l.was_playing = playing;
    // Scripts asked for from outside the workspace windows open in the workspace.
    if !st.open_requests.is_empty() {
        let wi = l.current_workspace();
        for r in std::mem::take(&mut st.open_requests) {
            l.open_window(wi, WinKind::Script { res: r }, None);
        }
        l.show_workspace(wi);
    }
    let glass = egui::Frame::new().fill(theme::glass_fill()).stroke(theme::glass_edge()).inner_margin(6.0);
    if l.show_assets {
        egui::Panel::right("ws assets").resizable(true).default_size(300.0).size_range(180.0..=700.0).frame(glass).show(ui, |ui| {
            // In a child that can't widen the panel (the asset grid fills whatever width it gets).
            let r = ui.available_rect_before_wrap();
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r).id_salt("ws assets inner"));
            inner.set_clip_rect(r.intersect(ui.clip_rect()));
            theme::title(&mut inner, "Assets");
            panels::assets(&mut inner, st, previews);
            ui.allocate_rect(r, Sense::hover());
        });
    }
    if l.show_console {
        egui::Panel::bottom("ws console").resizable(true).default_size(130.0).frame(glass).show(ui, |ui| {
            theme::title(ui, "Console");
            panels::console(ui, st);
        });
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        // Again: the Assets and Console panels above may have just opened a blueprint.
        follow_blueprint(l, st);
        match l.tabs.get(l.active).cloned() {
            Some(TopTab::Workspace(wi)) => canvas(ui, l, wi, st),
            Some(TopTab::Blueprint(res)) => blueprint_tab(ui, l, st, &res, rs, viewport_tex),
            None => {}
        }
    });
}

/// The tabs (workspaces, then open blueprints), drawn in the top bar. Drag one sideways to
/// reorder.
pub fn tab_strip(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState) {
    let mut activate = None;
    let mut close = None;
    let mut swap = None;
    ui.spacing_mut().item_spacing.x = 3.0;
    ui.add_space(8.0);
    let mut rects: Vec<Rect> = Vec::new();
    for (i, t) in l.tabs.iter().enumerate() {
        let active = i == l.active;
        let (label, dirty) = match t {
            TopTab::Workspace(w) => (l.workspaces[*w].name.clone(), false),
            TopTab::Blueprint(r) => (r.rsplit('/').next().unwrap_or(r).trim_end_matches(".jblueprint").to_string(), st.is_dirty(r)),
        };
        let closeable = match t {
            TopTab::Workspace(_) => l.tabs.iter().filter(|t| matches!(t, TopTab::Workspace(_))).count() > 1,
            TopTab::Blueprint(_) => true,
        };
        let out = theme::tab(ui, &label, active, closeable, dirty);
        crate::docs::mark(ui, out.resp.rect, "ui:tabs");
        rects.push(out.resp.rect);
        if out.close {
            close = Some(i);
        } else if out.resp.clicked() || out.resp.drag_started() {
            activate = Some(i);
        }
        // Dragged past the middle of a neighbor: they trade places.
        if let (true, Some(p)) = (out.resp.dragged(), ui.ctx().pointer_latest_pos()) {
            if i > 0 && p.x < rects[i - 1].center().x {
                swap = Some((i, i - 1));
            }
            if swap.is_none() {
                // The neighbor to the right isn't laid out yet: go by this tab's own width.
                if i + 1 < l.tabs.len() && p.x > out.resp.rect.right() + 40.0 {
                    swap = Some((i, i + 1));
                }
            }
        }
    }
    let (rect, r) = ui.allocate_exact_size(Vec2::new(26.0, 24.0), Sense::click());
    if r.hovered() {
        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0x2a, 0x2a, 0x2a));
    }
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(15.0), theme::TEXT_DIM);
    if r.tip("New workspace").clicked() {
        let n = l.workspaces.len() + 1;
        l.workspaces.push(Workspace { name: format!("Workspace {}", n), ..Default::default() });
        l.tabs.push(TopTab::Workspace(l.workspaces.len() - 1));
        activate = Some(l.tabs.len() - 1);
    }
    if let Some((a, b)) = swap {
        l.tabs.swap(a, b);
        l.active = if l.active == a { b } else if l.active == b { a } else { l.active };
        activate = None;
    }
    // While playing, the game stays in its own tab (other blueprints open once it stops).
    if let (Some(i), true) = (activate, st.playing.is_some()) {
        if matches!(l.tabs.get(i), Some(TopTab::Blueprint(r)) if Some(r) != st.blueprint_res.as_ref()) {
            st.status = "stop the game to switch blueprints".into();
            activate = None;
        }
    }
    if let Some(i) = activate {
        l.active = i;
        if let Some(TopTab::Blueprint(r)) = l.tabs.get(i) {
            let r = r.clone();
            st.open_blueprint(&r);
            l.last_blueprint = st.blueprint_res.clone();
        }
    }
    if let Some(i) = close {
        match l.tabs.remove(i) {
            TopTab::Blueprint(r) => {
                // Closing the tab keeps unsaved changes if a workspace window still shows the blueprint.
                let in_use = l.workspaces.iter().any(|w| w.windows.iter().any(|x| matches!(&x.kind, WinKind::Blueprint { res, .. } | WinKind::Events { res, .. } if *res == r)));
                if !in_use && !st.is_dirty(&r) {
                    st.close_blueprint_res(&r);
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
        if let Some(TopTab::Blueprint(r)) = l.tabs.get(l.active) {
            let r = r.clone();
            st.open_blueprint(&r);
        }
        l.last_blueprint = st.blueprint_res.clone();
    }
}

/// A blueprint's own tab: its instances and their properties on the left, the view in the middle.
fn blueprint_tab(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, res: &str, rs: Option<&egui_wgpu::RenderState>, viewport_tex: &mut Option<egui::TextureId>) {
    if st.blueprint_res.as_deref() != Some(res) && st.playing.is_none() {
        st.open_blueprint(res);
        l.last_blueprint = st.blueprint_res.clone();
    }
    let glass = egui::Frame::new().fill(theme::glass_fill()).stroke(theme::glass_edge()).inner_margin(6.0);
    egui::Panel::left("blueprint tab side").resizable(true).default_size(290.0).frame(glass).show(ui, |ui| {
        egui::Panel::bottom("blueprint tab inspector").resizable(true).default_size(ui.available_height() * 0.6).frame(egui::Frame::NONE.inner_margin(egui::Margin { top: 6, ..Default::default() })).show(ui, |ui| {
            let id = egui::Id::new("blueprint tab side events");
            let mut page: usize = ui.data(|d| d.get_temp(id).unwrap_or(0));
            theme::tab_row(ui, &["Properties", "Events"], &mut page);
            ui.data_mut(|d| d.insert_temp(id, page));
            let ev = page == 1;
            if ev {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| panels::events(ui, st));
            } else {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| panels::inspector(ui, st));
            }
        });
        ui.horizontal(|ui| {
            theme::title(ui, "Instances");
            if ui.small_button("Open as window").tip("Open this blueprint's window in the workspace (its objects, properties and events)").clicked() {
                l.open_blueprint_window(res);
            }
        });
        panels::hierarchy(ui, st);
    });
    egui::CentralPanel::default().frame(glass).show(ui, |ui| viewport::viewport(ui, st, rs, viewport_tex));
}

/// A workspace: a canvas of windows. Drag the background (or hold the middle button) to look around.
fn canvas(ui: &mut egui::Ui, l: &mut Layout, wi: usize, st: &mut EditorState) {
    let rect = ui.available_rect_before_wrap();
    crate::docs::mark(ui, rect, "ui:workspace");
    let bg = ui.allocate_rect(rect, Sense::click_and_drag());
    {
        let ws = &mut l.workspaces[wi];
        ws.keep_arranged(rect.size());
        if bg.dragged() || ui.input(|i| i.pointer.middle_down()) {
            ws.pan_to = None;
        }
        if let Some(to) = ws.pan_to {
            let k = 1.0 - (-ui.input(|i| i.stable_dt).min(0.1) * 14.0).exp();
            ws.pan[0] += (to[0] - ws.pan[0]) * k;
            ws.pan[1] += (to[1] - ws.pan[1]) * k;
            if (to[0] - ws.pan[0]).abs() + (to[1] - ws.pan[1]).abs() < 0.5 {
                ws.pan = to;
                ws.pan_to = None;
            }
            ui.ctx().request_repaint();
        }
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
    // A blueprint or script dropped from Assets opens as a window where it was dropped.
    if let Some(p) = bg.dnd_release_payload::<panels::AssetPath>() {
        let kind = if p.0.ends_with(".jblueprint") {
            Some(WinKind::Blueprint { res: p.0.clone(), object: String::new() })
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
    if wins.is_empty() && st.blueprint_res.is_none() && st.playing.is_none() {
        // A project with nothing in it yet: offer the first blueprint.
        let r = Rect::from_center_size(rect.center(), Vec2::new(360.0, 80.0));
        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Center)));
        c.label(RichText::new("This project is empty. Start with a blueprint:").color(theme::TEXT_DIM));
        c.add_space(6.0);
        c.horizontal(|ui| {
            ui.add_space(86.0);
            for (label, k) in [("New 2D blueprint", gex_core::ObjectKind::Object2D), ("New 3D blueprint", gex_core::ObjectKind::Object3D)] {
                if ui.button(label).clicked() {
                    st.new_blueprint_file("res://blueprints", k);
                }
            }
        });
    } else if wins.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Double-click a blueprint or script in Assets to open it here.\nDrag the background to look around.",
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
            let curve = egui::epaint::CubicBezierShape::from_points_stroke([from, from + Vec2::new(k, 0.0), to - Vec2::new(k, 0.0), to], false, Color32::TRANSPARENT, Stroke::new(2.0, theme::alpha(theme::TEXT_DIM, 150)));
            painter.add(curve);
        }
    }
    let mut moves: Vec<(u64, Vec2)> = Vec::new();
    let mut resizes: Vec<(u64, Vec2)> = Vec::new();
    let mut front = None;
    let mut close = None;
    let mut opened: Vec<(WinKind, u64)> = Vec::new();
    let mut set_object: Vec<(u64, String)> = Vec::new();
    let mut open_tab: Option<String> = None;
    for w in &wins {
        let wr = rect_of(w);
        if !wr.intersects(rect) {
            continue;
        }
        let clip = wr.intersect(rect);
        let mut wui = ui.new_child(egui::UiBuilder::new().max_rect(wr).id_salt(("ws win", w.id)));
        wui.set_clip_rect(clip);
        theme::fade_in(&mut wui);
        let p = wui.painter().clone();
        p.rect_filled(wr.translate(Vec2::new(0.0, 3.0)).expand(1.0), 0.0, Color32::from_black_alpha(80));
        p.rect_filled(wr, 0.0, Color32::from_rgb(0x1a, 0x1a, 0x1a));
        p.rect_stroke(wr, 0.0, theme::glass_edge(), egui::StrokeKind::Inside);
        // Title bar: drag to move.
        let title_r = Rect::from_min_size(wr.min, Vec2::new(wr.width(), 26.0));
        p.add(theme::header(title_r.shrink2(Vec2::new(1.0, 1.0)), false));
        let (icon, title) = match &w.kind {
            WinKind::Blueprint { res, .. } => ("Blueprint", format!("{}{}", short(res), if st.is_dirty(res) { " *" } else { "" })),
            WinKind::Events { res, object } => ("Events", format!("{}", if object.is_empty() { short(res) } else { object.rsplit('/').next().unwrap_or(object).to_string() })),
            WinKind::Script { res } => ("Script", format!("{}{}", short(res), if st.scripts.get(res).is_some_and(|s| s.dirty()) { " *" } else { "" })),
            WinKind::Tool { name } => ("", name.clone()),
        };
        let tr = p.text(title_r.left_center() + Vec2::new(8.0, 0.0), egui::Align2::LEFT_CENTER, icon.to_uppercase(), egui::FontId::proportional(10.0), theme::TEXT_DIM);
        p.text(Pos2::new(if icon.is_empty() { tr.left() } else { tr.right() + 8.0 }, title_r.center().y), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(13.0), theme::TEXT);
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
            WinKind::Blueprint { res, object } => {
                if let Some((new_object, events, tab)) = blueprint_window(&mut cui, st, res, object, w.id) {
                    if new_object != *object {
                        set_object.push((w.id, new_object.clone()));
                    }
                    if events {
                        opened.push((WinKind::Events { res: res.clone(), object: new_object }, w.id));
                    }
                    if tab {
                        open_tab = Some(res.clone());
                    }
                }
            }
            WinKind::Events { res, object } => {
                if st.playing.is_some() {
                    cui.label(RichText::new("Stop the game to change events.").color(theme::TEXT_DIM));
                } else {
                    let object = object.clone();
                    st.with_blueprint(res, |st| {
                        let id = st.tree.find_path(None, &object).or(st.tree.root);
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
    for (id, object) in set_object {
        if let Some(w) = ws.windows.iter_mut().find(|w| w.id == id) {
            if let WinKind::Blueprint { object: n, .. } = &mut w.kind {
                *n = object;
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
        // An events window replaces the one this blueprint window opened before.
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
        st.open_blueprint(&r);
        l.open_blueprint_tab(&r);
        l.last_blueprint = st.blueprint_res.clone();
    }
}

fn short(res: &str) -> String {
    res.rsplit('/').next().unwrap_or(res).to_string()
}

/// A blueprint window: buttons, its objects, and the picked object's properties. Gives back the picked
/// object's path, and whether its events or its own tab were asked for.
fn blueprint_window(ui: &mut egui::Ui, st: &mut EditorState, res: &str, object: &str, win: u64) -> Option<(String, bool, bool)> {
    if st.playing.is_some() {
        ui.label(RichText::new("Stop the game to edit blueprints here.").color(theme::TEXT_DIM));
        return Some((object.to_string(), false, false));
    }
    let r = st.with_blueprint(res, |st| {
        let mut picked = st.tree.find_path(None, object).or(st.tree.root);
        let mut events = false;
        let mut tab = false;
        ui.horizontal(|ui| {
            if ui.button("Open in tab").tip("Open the blueprint in its own tab, to place and edit instances").clicked() {
                tab = true;
            }
            if ui.button("Events").tip("The events of the picked object's scripts").clicked() {
                events = true;
            }
            let dirty = st.blueprint_dirty();
            if ui.add_enabled(dirty, egui::Button::new("Save")).tip("Saves this blueprint (Ctrl+S saves everything)").clicked() {
                st.save_blueprint();
            }
        });
        // The blueprint's objects (a template can have children: a gun on a player, a hitbox).
        let ids = st.tree.walk();
        if ids.len() > 1 {
            egui::CollapsingHeader::new(format!("Objects ({})", ids.len())).id_salt(("ws objects", win)).default_open(ids.len() <= 8).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(140.0).id_salt(("ws objects scroll", win)).show(ui, |ui| {
                    for id in &ids {
                        let n = &st.tree.objects[*id];
                        let depth = st.tree.path_of(*id).matches('/').count().saturating_sub(1);
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 12.0);
                            if crate::icons::row(ui, st, &crate::icons::object(&st.tree, *id), RichText::new(&n.name), picked == Some(*id)).clicked() {
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
        let a = l.open_window(0, WinKind::Blueprint { res: "res://a.jblueprint".into(), object: String::new() }, None);
        let e = l.open_window(0, WinKind::Events { res: "res://a.jblueprint".into(), object: String::new() }, Some(a));
        // Opening the same thing again brings it forward instead of making another.
        assert_eq!(l.open_window(0, WinKind::Blueprint { res: "res://a.jblueprint".into(), object: String::new() }, None), a);
        assert_eq!(l.workspaces[0].windows.len(), 2);
        let ev = l.workspaces[0].windows.iter().find(|w| w.id == e).unwrap();
        let sc = l.workspaces[0].windows.iter().find(|w| w.id == a).unwrap();
        assert_eq!(ev.parent, Some(a));
        assert!(ev.pos[0] > sc.pos[0] + sc.size[0], "events open to the right");
        l.open_blueprint_tab("res://a.jblueprint");
        l.open_blueprint_tab("res://a.jblueprint");
        assert_eq!(l.tabs.len(), 2);
        assert_eq!(l.tabs[l.active], TopTab::Blueprint("res://a.jblueprint".into()));
        let dir = std::env::temp_dir().join(format!("gex-ws-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        l.save(&dir);
        let back = Layout::load(&dir);
        assert_eq!(back.tabs, l.tabs);
        assert_eq!(back.workspaces[0].windows.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
