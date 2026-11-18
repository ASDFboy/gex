//! The editor's layout: a tab per open blueprint along the top (in the menu bar). A tab has
//! the blueprint's objects and the picked one's properties and events on the left and the view
//! in the middle; scripts open as small windows floating over the view, wired to the object
//! they belong to. The assets and the console sit beside every tab.
//!
//! The older docked layout is still there (View → Classic docked layout).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::state::EditorState;
use crate::{code, panels, theme, viewport};

/// The tab of the Command reference (the other tabs are blueprints).
pub const COMMANDS_TAB: &str = "gex:commands";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// The open blueprints, in tab order.
    pub tabs: Vec<String>,
    pub active: usize,
    pub show_assets: bool,
    pub show_console: bool,
    /// The console is folded down to its title bar (double-click folds, a click unfolds).
    pub console_min: bool,
    /// The scripts open in each blueprint's tab ("" = with no blueprint open), in tab order.
    pub scripts: BTreeMap<String, Vec<String>>,
    /// Open tool windows (Timeline, Profiler).
    pub tools: Vec<String>,
    /// A script just asked for: its window comes to the front showing it.
    #[serde(skip)]
    script_focus: Option<String>,
    /// The blueprint being edited last frame (to notice it changing from elsewhere).
    #[serde(skip)]
    pub last_blueprint: Option<String>,
    /// While the game plays: the tab to go back to when it stops.
    #[serde(skip)]
    tab_before_play: Option<usize>,
    #[serde(skip)]
    was_playing: bool,
    #[serde(skip)]
    saved_text: String,
    #[serde(skip)]
    file_ops_seen: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            tabs: Vec::new(),
            active: 0,
            show_assets: true,
            show_console: true,
            console_min: false,
            scripts: BTreeMap::new(),
            tools: Vec::new(),
            script_focus: None,
            last_blueprint: None,
            tab_before_play: None,
            was_playing: false,
            saved_text: String::new(),
            file_ops_seen: 0,
        }
    }
}

fn file(dir: &Path) -> PathBuf {
    dir.join(".gex").join("layout.ron")
}

impl Layout {
    /// The project's saved layout (each project remembers its own), or a fresh one.
    pub fn load(dir: &Path) -> Layout {
        let mut l: Layout = std::fs::read_to_string(file(dir)).ok().and_then(|t| ron::from_str(&t).ok()).unwrap_or_default();
        l.active = l.active.min(l.tabs.len().saturating_sub(1));
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
        match self.tabs.iter().position(|x| x == res) {
            Some(i) => self.active = i,
            None => {
                self.tabs.push(res.to_string());
                self.active = self.tabs.len() - 1;
            }
        }
    }

    /// Opens the Timeline or the Profiler as a window over the view.
    pub fn open_tool(&mut self, name: &str) {
        if !self.tools.iter().any(|t| t == name) {
            self.tools.push(name.to_string());
        }
    }
}

/// The blueprint being edited changed from elsewhere (Assets, a shortcut): its tab shows.
fn follow_blueprint(l: &mut Layout, st: &EditorState) {
    if st.blueprint_res != l.last_blueprint {
        if let Some(r) = st.blueprint_res.clone() {
            l.open_blueprint_tab(&r);
        }
        l.last_blueprint = st.blueprint_res.clone();
    }
}

/// After a rename or delete in Assets: tabs and script windows follow a renamed file, and
/// those of a file that's gone close.
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
    let fix = |r: &str| -> String {
        match &renamed {
            Some((old, new)) if r == old => new.clone(),
            Some((old, new)) if r.strip_prefix(old.as_str()).is_some_and(|x| x.starts_with('/')) => format!("{}{}", new, &r[old.len()..]),
            _ => r.to_string(),
        }
    };
    let gone = |r: &str| r.starts_with("res://") && !st.project.res_to_path(r).is_some_and(|p| p.is_file());
    let here = l.tabs.get(l.active).map(|t| fix(t));
    let mut tabs: Vec<String> = Vec::new();
    for t in l.tabs.iter().map(|t| fix(t)) {
        if !tabs.contains(&t) && !gone(&t) {
            tabs.push(t);
        }
    }
    l.tabs = tabs;
    let mut scripts = BTreeMap::new();
    for (k, v) in std::mem::take(&mut l.scripts) {
        let k = fix(&k);
        if k.is_empty() || !gone(&k) {
            let v: Vec<String> = v.iter().map(|r| fix(r)).filter(|r| !gone(r)).collect();
            scripts.insert(k, v);
        }
    }
    l.scripts = scripts;
    l.active = here.and_then(|t| l.tabs.iter().position(|x| *x == t)).unwrap_or(0);
}

pub fn show(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, rs: Option<&egui_wgpu::RenderState>, viewport_tex: &mut Option<egui::TextureId>, previews: &mut std::collections::HashMap<String, egui::TextureHandle>) {
    follow_files(l, st);
    follow_blueprint(l, st);
    // Play shows the game (its blueprint's tab); Stop goes back to where you were.
    let playing = st.playing.is_some();
    if playing && !l.was_playing {
        if let Some(r) = st.blueprint_res.clone() {
            let before = l.active;
            l.open_blueprint_tab(&r);
            l.tab_before_play = (l.active != before).then_some(before);
        }
    } else if !playing && l.was_playing {
        if let Some(r) = st.return_to.take() {
            st.open_blueprint(&r);
            l.tab_before_play = None;
            follow_blueprint(l, st);
        } else if let Some(i) = l.tab_before_play.take().filter(|i| *i < l.tabs.len()) {
            l.active = i;
        }
    }
    l.was_playing = playing;
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
    // The console shows itself when there's something in it (or the game is paused at a breakpoint).
    let paused = st.playing.as_ref().is_some_and(|p| p.handle.paused.is_some());
    // (Notes of the editor's own, like "stopped", don't bring it up.)
    let printed = st.logs.iter().any(|x| x.kind != gex_script::LogKind::Info);
    if l.show_console && (printed || paused) {
        if l.console_min && !paused {
            egui::Panel::bottom("ws console bar").resizable(false).exact_size(30.0).frame(glass).show(ui, |ui| {
                let r = ui.interact(ui.max_rect(), ui.id().with("open"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                ui.horizontal(|ui| {
                    theme::title(ui, "Console");
                    let errors = st.logs.iter().filter(|x| x.kind == gex_script::LogKind::Error).count();
                    if errors > 0 {
                        ui.label(egui::RichText::new(format!("{} error(s)", errors)).small().color(theme::BAD));
                    }
                });
                if r.clicked() {
                    l.console_min = false;
                }
            });
        } else {
            let shown = egui::Panel::bottom("ws console").resizable(true).default_size(130.0).frame(glass).show(ui, |ui| {
                theme::title(ui, "Console");
                panels::console(ui, st);
            });
            if ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| shown.response.rect.contains(p)) {
                l.console_min = true;
            }
        }
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        // Again: the Assets and Console panels above may have just opened a blueprint.
        follow_blueprint(l, st);
        // The Command reference asked for (View → Commands, F1): its tab.
        if std::mem::take(&mut st.commands_open) {
            l.open_blueprint_tab(COMMANDS_TAB);
        }
        // Scripts asked for open in the tab that's showing (from the Commands tab: in the
        // blueprint being edited, which comes back).
        let mut tab = l.tabs.get(l.active).cloned().unwrap_or_default();
        if tab == COMMANDS_TAB && !st.open_requests.is_empty() {
            if let Some(b) = st.blueprint_res.clone() {
                l.open_blueprint_tab(&b);
                tab = b;
            }
        }
        for r in std::mem::take(&mut st.open_requests) {
            let list = l.scripts.entry(tab.clone()).or_default();
            if !list.contains(&r) {
                list.push(r.clone());
            }
            l.script_focus = Some(r);
        }
        let view = match l.tabs.get(l.active).cloned() {
            Some(t) if t == COMMANDS_TAB => {
                let r = ui.available_rect_before_wrap();
                ui.painter().rect_filled(r, 0.0, theme::glass_fill());
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(8.0)).id_salt("commands tab"));
                crate::tools::command_page(&mut inner, st);
                ui.allocate_rect(r, Sense::hover());
                r
            }
            Some(res) => blueprint_tab(ui, l, st, &res, rs, viewport_tex),
            None => nothing_open(ui, st),
        };
        if st.playing.is_none() && tab != COMMANDS_TAB {
            script_windows(ui, l, st, &tab, view);
        }
        tool_windows(ui, l, st, view);
    });
}

/// The tabs (one per open blueprint), drawn in the top bar. Drag one sideways to move it.
pub fn tab_strip(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState) {
    let mut activate = None;
    let mut close = None;
    let mut started = None;
    ui.spacing_mut().item_spacing.x = 3.0;
    ui.add_space(8.0);
    let mut rects = Vec::new();
    for (i, r) in l.tabs.iter().enumerate() {
        let label = if r == COMMANDS_TAB { "Commands" } else { r.rsplit('/').next().unwrap_or(r).trim_end_matches(".jblueprint") };
        let out = theme::tab(ui, label, i == l.active, true, r != COMMANDS_TAB && st.is_dirty(r));
        crate::docs::mark(ui, out.resp.rect, "ui:tabs");
        rects.push(out.resp.rect);
        if out.close || out.resp.middle_clicked() {
            close = Some(i);
        } else if out.resp.clicked() || out.resp.drag_started() {
            activate = Some(i);
        }
        if out.resp.drag_started() {
            started = Some(i);
        }
    }
    if let Some((a, b)) = theme::tab_reorder(ui, "top", &rects, started) {
        l.tabs.swap(a, b);
        l.active = if l.active == a { b } else if l.active == b { a } else { l.active };
        activate = None;
    }
    // While playing, the game stays in its own tab (other blueprints open once it stops).
    if let (Some(i), true) = (activate, st.playing.is_some()) {
        if l.tabs.get(i) != st.blueprint_res.as_ref() && l.tabs[i] != COMMANDS_TAB {
            st.status = "stop the game to switch blueprints".into();
            activate = None;
        }
    }
    if let Some(i) = activate {
        l.active = i;
        let r = l.tabs[i].clone();
        if r != COMMANDS_TAB {
            st.open_blueprint(&r);
            l.last_blueprint = st.blueprint_res.clone();
        }
    }
    // The Commands tab just closes (also while playing).
    if let Some(i) = close.filter(|i| l.tabs[*i] == COMMANDS_TAB) {
        l.tabs.remove(i);
        if l.active > i {
            l.active -= 1;
        }
        l.active = l.active.min(l.tabs.len().saturating_sub(1));
        if let Some(r) = l.tabs.get(l.active).cloned().filter(|r| r != COMMANDS_TAB && st.playing.is_none()) {
            st.open_blueprint(&r);
            l.last_blueprint = st.blueprint_res.clone();
        }
        return;
    }
    if let Some(i) = close.filter(|_| st.playing.is_none()) {
        let r = l.tabs.remove(i);
        // Closing a tab saves what was changed in it and in its scripts.
        if st.is_dirty(&r) {
            st.open_blueprint(&r);
            st.save_blueprint();
        }
        for sc in l.scripts.remove(&r).unwrap_or_default() {
            st.save_script(&sc);
        }
        st.close_blueprint_res(&r);
        if l.active > i {
            l.active -= 1;
        }
        l.active = l.active.min(l.tabs.len().saturating_sub(1));
        // (Landing on the Commands tab, the blueprint being edited is another open one.)
        let next = l.tabs.get(l.active).filter(|t| *t != COMMANDS_TAB).or_else(|| l.tabs.iter().find(|t| *t != COMMANDS_TAB)).cloned();
        if let Some(r) = next {
            st.open_blueprint(&r);
        }
        l.last_blueprint = st.blueprint_res.clone();
    }
}

/// No blueprint is open: says how to get one. Gives the area back (windows float over it).
fn nothing_open(ui: &mut egui::Ui, st: &mut EditorState) -> egui::Rect {
    let rect = ui.available_rect_before_wrap();
    ui.painter().add(theme::vgradient(rect, Color32::from_rgb(0x1e, 0x1e, 0x1e), Color32::from_rgb(0x06, 0x06, 0x06)));
    let has_any = st.project.res_to_path(&st.project.config.main_blueprint).is_some_and(|p| p.is_file());
    let r = egui::Rect::from_center_size(rect.center(), Vec2::new(380.0, 80.0));
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Center)));
    if has_any {
        c.label(RichText::new("Double-click a blueprint in Assets to open it.").color(theme::TEXT_DIM));
    } else if st.playing.is_none() {
        c.label(RichText::new("This project is empty. Start with a blueprint:").color(theme::TEXT_DIM));
        c.add_space(6.0);
        c.horizontal(|ui| {
            ui.add_space(96.0);
            for (label, k) in [("New 2D blueprint", gex_core::ObjectKind::Object2D), ("New 3D blueprint", gex_core::ObjectKind::Object3D)] {
                if ui.button(label).clicked() {
                    st.new_blueprint_file("res://", k);
                }
            }
        });
    }
    ui.allocate_rect(rect, Sense::hover());
    rect
}

/// A blueprint's tab: its objects and their properties on the left, the view in the middle.
/// Gives the view's area back.
fn blueprint_tab(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, res: &str, rs: Option<&egui_wgpu::RenderState>, viewport_tex: &mut Option<egui::TextureId>) -> egui::Rect {
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
        theme::title(ui, "Blueprint");
        panels::hierarchy(ui, st);
    });
    egui::CentralPanel::default().frame(glass).show(ui, |ui| viewport::viewport(ui, st, rs, viewport_tex)).response.rect
}

/// The Timeline and the Profiler, as windows over the view.
fn tool_windows(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, view: egui::Rect) {
    let ctx = ui.ctx().clone();
    let frame = egui::Frame::new().fill(Color32::from_rgb(0x1a, 0x1a, 0x1a)).stroke(Stroke::new(1.0, theme::GRAY)).inner_margin(6.0);
    let mut close = None;
    for (i, name) in l.tools.clone().iter().enumerate() {
        egui::Window::new(name.as_str())
            .id(egui::Id::new(("tool window", name.as_str())))
            .title_bar(false)
            .frame(frame)
            .resizable(true)
            .default_pos(Pos2::new(view.left() + 20.0, view.bottom() - 300.0))
            .default_size([640.0, 260.0])
            .constrain_to(view)
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    theme::title(ui, name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("✖").small()).frame(false)).clicked() {
                            close = Some(i);
                        }
                    });
                });
                ui.separator();
                match name.as_str() {
                    "Timeline" => crate::timeline::timeline(ui, st),
                    _ => {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| crate::tools::profiler(ui, st));
                    }
                }
            });
    }
    if let Some(i) = close {
        l.tools.remove(i);
    }
}

fn short(res: &str) -> String {
    res.rsplit('/').next().unwrap_or(res).to_string()
}

/// The scripts open in a tab: small windows floating over the view, one per object (its
/// scripts are tabs in it), each wired to where its object is.
fn script_windows(ui: &mut egui::Ui, l: &mut Layout, st: &mut EditorState, tab: &str, view: egui::Rect) {
    if l.scripts.get(tab).is_none_or(|v| v.is_empty()) {
        l.script_focus = None;
        return;
    }
    let focus = l.script_focus.take();
    let list = l.scripts.entry(tab.to_string()).or_default();
    // Scripts remembered from last time are loaded again; those whose file is gone close.
    list.retain(|r| st.scripts.contains_key(r) || st.project.res_to_path(r).is_some_and(|p| p.is_file()));
    for r in list.iter() {
        if !st.scripts.contains_key(r) {
            st.open_script(r, None);
            st.open_requests.pop();
        }
    }
    let ctx = ui.ctx().clone();
    let wires = ui.painter().with_clip_rect(view);
    // One window per object: its scripts are tabs in it. (A script no object here has gets
    // a window of its own.)
    let mut owner: std::collections::HashMap<&str, gex_core::ObjectId> = Default::default();
    for (id, n) in st.tree.objects.iter() {
        for sc in &n.scripts {
            owner.entry(sc.as_str()).or_insert(id);
        }
    }
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, res) in list.iter().enumerate() {
        let key = owner.get(res.as_str()).map(|o| format!("{:?}", o)).unwrap_or_else(|| res.clone());
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    drop(owner);
    let mut close = None;
    let mut swap = None;
    for (gi, (key, members)) in groups.iter().enumerate() {
        let at_id = egui::Id::new(("blueprint script at", key));
        let mut showing: String = ctx.data(|d| d.get_temp(at_id)).filter(|r| members.iter().any(|i| list[*i] == *r)).unwrap_or_else(|| list[members[0]].clone());
        let wanted = focus.as_ref().filter(|f| members.iter().any(|i| list[*i] == **f));
        if let Some(f) = wanted {
            showing = f.clone();
        }
        let frame = egui::Frame::new().fill(Color32::from_rgb(0x1a, 0x1a, 0x1a)).stroke(Stroke::new(1.0, theme::GRAY)).inner_margin(6.0);
        // A new window is as big as its script needs (within the view), and opens beside
        // its object.
        let text = st.scripts.get(&showing).map(|s| s.text.as_str()).unwrap_or("");
        let cols = text.lines().map(|x| x.replace('\t', "    ").chars().count()).max().unwrap_or(0) as f32;
        let rows = text.lines().count().max(6) as f32 + 2.0;
        let size = Vec2::new((cols * 8.5 + 110.0).clamp(380.0, (view.width() - 24.0).max(380.0)), (rows * 17.5 + 78.0).clamp(200.0, (view.height() - 24.0).max(200.0)));
        let k = (gi % 6) as f32 * 26.0;
        let pos = match members.iter().find_map(|i| st.view_anchors.get(&list[*i])) {
            Some(a) => {
                let x = if a[0] + 70.0 + size.x <= view.right() { a[0] + 70.0 } else { a[0] - 70.0 - size.x };
                Pos2::new(x.clamp(view.left(), (view.right() - size.x).max(view.left())), (a[1] - 30.0).clamp(view.top(), (view.bottom() - size.y).max(view.top())))
            }
            None => Pos2::new(view.right() - size.x - 20.0 - k, view.top() + 130.0 + k),
        };
        let win_id = egui::Id::new(("blueprint script", key));
        let shown = egui::Window::new(key.as_str())
            .id(win_id)
            .title_bar(false)
            .frame(frame)
            .resizable(true)
            .default_pos(pos)
            .default_size(size)
            .min_size([240.0, 140.0])
            .constrain_to(view)
            .show(&ctx, |ui| {
                egui::ScrollArea::horizontal().id_salt("tabs").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (mut rects, mut started) = (Vec::new(), None);
                        for (n, i) in members.iter().enumerate() {
                            let res = &list[*i];
                            let dirty = st.scripts.get(res).is_some_and(|s| s.dirty());
                            let out = theme::tab(ui, short(res).trim_end_matches(".jlb"), *res == showing, true, dirty);
                            let out = crate::theme::TabOut { resp: out.resp.on_hover_text(res.trim_start_matches("res://")), close: out.close };
                            rects.push(out.resp.rect);
                            if out.close || out.resp.middle_clicked() {
                                close = Some(*i);
                            } else if out.resp.clicked() || out.resp.drag_started() {
                                showing = res.clone();
                            }
                            if out.resp.drag_started() {
                                started = Some(n);
                            }
                        }
                        if let Some((a, b)) = theme::tab_reorder(ui, key.as_str(), &rects, started) {
                            swap = Some((members[a], members[b]));
                        }
                    });
                });
                code::script_ui(ui, st, &showing);
            });
        ctx.data_mut(|d| d.insert_temp(at_id, showing.clone()));
        let Some(shown) = shown else { continue };
        if wanted.is_some() {
            ctx.move_to_top(shown.response.layer_id);
        }
        let r = shown.response.rect;
        // The wire to the object.
        if let Some(a) = members.iter().find_map(|i| st.view_anchors.get(&list[*i])) {
            let from = Pos2::new(a[0], a[1]);
            let left = from.x <= r.center().x;
            let to = Pos2::new(if left { r.left() } else { r.right() }, r.top() + 14.0);
            let bend = Vec2::new(((to.x - from.x).abs() * 0.5).max(30.0) * if left { 1.0 } else { -1.0 }, 0.0);
            if !r.contains(from) {
                wires.add(egui::epaint::CubicBezierShape::from_points_stroke([from, from + bend, to - bend, to], false, Color32::TRANSPARENT, Stroke::new(1.5, Color32::from_gray(0x90))));
                wires.circle_filled(from, 3.0, Color32::from_gray(0x90));
            }
        }
    }
    if let Some((a, b)) = swap {
        list.swap(a, b);
    }
    if let Some(i) = close {
        st.save_script(&list[i]);
        list.remove(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_and_scripts_follow_renames_and_save() {
        let dir = std::env::temp_dir().join(format!("gex-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::templates::create_demo(&dir).unwrap();
        let mut st = EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        let mut l = Layout::default();
        l.open_blueprint_tab("res://blueprints/main.jblueprint");
        l.open_blueprint_tab("res://blueprints/coin.jblueprint");
        l.open_blueprint_tab("res://blueprints/main.jblueprint");
        assert_eq!((l.tabs.len(), l.active), (2, 0));
        l.scripts.insert("res://blueprints/coin.jblueprint".into(), vec!["res://scripts/player.jlb".into()]);
        st.rename_asset("res://blueprints/coin.jblueprint", "gem.jblueprint").unwrap();
        follow_files(&mut l, &st);
        assert_eq!(l.tabs[1], "res://blueprints/gem.jblueprint");
        assert!(l.scripts.contains_key("res://blueprints/gem.jblueprint"));
        l.save(&dir);
        let back = Layout::load(&dir);
        assert_eq!(back.tabs, l.tabs);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
