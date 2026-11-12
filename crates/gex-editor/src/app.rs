//! The editor window: launcher (project library) or the docked editor.

use crate::docs::Tip as _;
use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use gex_core::project::{MountConfig, MountKind, Project};
use gex_script::LogKind;

use crate::state::{self, EditorState};
use crate::{code, panels, theme, tools, viewport};

#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    Blueprint,
    Hierarchy,
    Inspector,
    Events,
    Assets,
    Console,
    Script(String),
    Profiler,
    Search,
    Timeline,
}

struct Viewer<'a> {
    st: &'a mut EditorState,
    rs: Option<&'a egui_wgpu::RenderState>,
    viewport_tex: &'a mut Option<egui::TextureId>,
    previews: &'a mut HashMap<String, egui::TextureHandle>,
    closed: Vec<String>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(format!("{:?}", tab))
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Blueprint => {
                let name = self.st.blueprint_res.as_deref().unwrap_or("untitled").rsplit('/').next().unwrap_or("").to_string();
                let dirty = if self.st.playing.is_none() && self.st.blueprint_dirty() { " *" } else { "" };
                format!("🎬 {}{}", name, dirty).into()
            }
            Tab::Hierarchy => "🌲 Hierarchy".into(),
            Tab::Inspector => "🔧 Inspector".into(),
            Tab::Events => "⚡ Events".into(),
            Tab::Assets => "📁 Assets".into(),
            Tab::Console => {
                let n = self.st.logs.iter().filter(|l| l.kind == LogKind::Error).count();
                if n > 0 { format!("🖵 Console ({})", n).into() } else { "🖵 Console".into() }
            }
            Tab::Script(r) => {
                let dirty = self.st.scripts.get(r).is_some_and(|s| s.dirty());
                format!("📜 {}{}", r.rsplit('/').next().unwrap_or(r), if dirty { " *" } else { "" }).into()
            }
            Tab::Profiler => "📈 Profiler".into(),
            Tab::Search => "🔍 Find in files".into(),
            Tab::Timeline => "🎞 Timeline".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Blueprint => {
                blueprint_tabs(ui, self.st);
                viewport::viewport(ui, self.st, self.rs, self.viewport_tex)
            }
            Tab::Hierarchy => panels::hierarchy(ui, self.st),
            Tab::Inspector => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| panels::inspector(ui, self.st));
            }
            Tab::Events => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| panels::events(ui, self.st));
            }
            Tab::Assets => panels::assets(ui, self.st, self.previews),
            Tab::Console => panels::console(ui, self.st),
            Tab::Script(r) => code::script_ui(ui, self.st, r),
            Tab::Profiler => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| tools::profiler(ui, self.st));
            }
            Tab::Search => tools::search(ui, self.st),
            Tab::Timeline => crate::timeline::timeline(ui, self.st),
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        matches!(tab, Tab::Script(_) | Tab::Profiler | Tab::Search | Tab::Timeline)
    }

    fn on_close(&mut self, tab: &mut Tab) -> egui_dock::widgets::tab_viewer::OnCloseResponse {
        if let Tab::Script(r) = tab {
            self.closed.push(r.clone());
        }
        egui_dock::widgets::tab_viewer::OnCloseResponse::Close
    }
}

/// The strip of open blueprints above the blueprint view.
fn blueprint_tabs(ui: &mut egui::Ui, st: &mut EditorState) {
    if st.docs.is_empty() {
        return;
    }
    let mut switch = None;
    let mut close = false;
    ui.horizontal(|ui| {
        let name = |r: &Option<String>| r.as_deref().unwrap_or("untitled").rsplit('/').next().unwrap_or("").to_string();
        ui.add(egui::Button::selectable(true, name(&st.blueprint_res)));
        if ui.small_button("✖").tip("close this blueprint (Ctrl+W)").clicked() {
            close = true;
        }
        for (i, d) in st.docs.iter().enumerate() {
            if ui.add(egui::Button::selectable(false, name(&d.blueprint_res))).clicked() {
                switch = Some(i);
            }
        }
    });
    if let Some(i) = switch {
        st.switch_blueprint(i);
    }
    if close {
        st.close_blueprint();
    }
}

fn layout_file() -> PathBuf {
    state::config_dir().join("layout.ron")
}

fn saved_dock() -> Option<DockState<Tab>> {
    let text = std::fs::read_to_string(layout_file()).ok()?;
    let mut d: DockState<Tab> = ron::from_str(&text).ok()?;
    // Script tabs from last time don't carry over (their buffers aren't open).
    d.retain_tabs(|t| !matches!(t, Tab::Script(_)));
    d.find_tab(&Tab::Blueprint)?;
    Some(d)
}

fn save_dock(d: &DockState<Tab>) {
    let mut d = d.clone();
    d.retain_tabs(|t| !matches!(t, Tab::Script(_)));
    if let Ok(t) = ron::to_string(&d) {
        let _ = std::fs::create_dir_all(state::config_dir());
        let _ = std::fs::write(layout_file(), t);
    }
}

/// Brings a panel to the front, opening it if it was closed (tool panels go with the Console).
fn show_tab(dock: &mut DockState<Tab>, t: Tab) {
    if let Some(p) = dock.find_tab(&t) {
        let _ = dock.set_active_tab(p);
        return;
    }
    let tool = matches!(t, Tab::Timeline | Tab::Profiler | Tab::Search | Tab::Console | Tab::Assets);
    if let Some(p) = dock.find_tab(&Tab::Console).filter(|_| tool) {
        dock.set_focused_node_and_surface(egui_dock::NodePath { surface: p.surface, node: p.node });
    }
    dock.push_to_focused_leaf(t);
}

fn default_dock() -> DockState<Tab> {
    let mut d = DockState::new(vec![Tab::Blueprint]);
    let t = d.main_surface_mut();
    let [center, _left] = t.split_left(NodeIndex::root(), 0.18, vec![Tab::Hierarchy]);
    let [center, right] = t.split_right(center, 0.74, vec![Tab::Inspector, Tab::Events]);
    let _ = right;
    let [_c, _b] = t.split_below(center, 0.72, vec![Tab::Console, Tab::Assets, Tab::Timeline]);
    d
}

pub struct EditorApp {
    st: Option<EditorState>,
    dock: DockState<Tab>,
    viewport_tex: Option<egui::TextureId>,
    previews: HashMap<String, egui::TextureHandle>,
    // Launcher fields.
    launcher: crate::launcher::Launcher,
    rex: crate::rex::Rex,
    // Neovim-style command line.
    cmd: Option<String>,
    settings: SettingsForm,
    pads: crate::gamepad::Pads,
    /// The window holds and hides the mouse for the game right now.
    cursor_locked: bool,
    /// The GameMaker-style layout (workspaces and blueprint tabs) of the open project.
    ws: crate::workspace::Layout,
    /// Use the older docked layout instead.
    classic: bool,
    ws_saved_at: std::time::Instant,
}

fn classic_file() -> PathBuf {
    state::config_dir().join("classic_layout")
}

/// Hides the mouse and holds it in the window (or gives it back).
pub fn lock_cursor(ctx: &egui::Context, on: bool) {
    if on {
        // Locked where the platform has it; elsewhere (X11) confined, which still gives raw motion.
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(if cfg!(target_os = "linux") { egui::CursorGrab::Confined } else { egui::CursorGrab::Locked }));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
    } else {
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
    }
}

/// Raw mouse movement in this frame's events.
pub fn mouse_motion(i: &egui::InputState) -> [f32; 2] {
    let mut d = [0.0; 2];
    for e in &i.events {
        if let egui::Event::MouseMoved(v) = e {
            d[0] += v.x;
            d[1] += v.y;
        }
    }
    d
}

#[derive(Default)]
struct SettingsForm {
    loaded: bool,
    name: String,
    main_blueprint: String,
    hl2_path: String,
    hl2_games: String,
    hl2_on: bool,
    message: String,
    window: [u32; 2],
    /// Which page: General, Graphics, Physics, Input, Half-Life 2.
    tab: usize,
    /// (action name, keys as "space, w, pad_a").
    actions: Vec<(String, String)>,
    fullscreen: bool,
    resizable: bool,
    vsync: bool,
    icon: String,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext, project: Option<PathBuf>) -> EditorApp {
        theme::apply(&cc.egui_ctx);
        gex_script::commands::register();
        let mut app = EditorApp {
            st: None,
            dock: default_dock(),
            viewport_tex: None,
            previews: HashMap::new(),
            launcher: Default::default(),
            rex: Default::default(),
            cmd: None,
            settings: SettingsForm::default(),
            pads: crate::gamepad::Pads::new(),
            cursor_locked: false,
            ws: Default::default(),
            classic: classic_file().exists(),
            ws_saved_at: std::time::Instant::now(),
        };
        if let Some(p) = project {
            app.open_project(&p);
        }
        app
    }

    fn open_project(&mut self, dir: &std::path::Path) {
        match Project::open(dir) {
            Ok(p) => {
                state::remember_project(&p.dir);
                self.ws = crate::workspace::Layout::load(&p.dir);
                let st = EditorState::new(p);
                // The blueprint opened at start doesn't take over the tab that was showing.
                if !self.ws.tabs.iter().any(|t| matches!(t, crate::workspace::TopTab::Blueprint(r) if Some(r) == st.blueprint_res.as_ref())) {
                    if let Some(r) = &st.blueprint_res {
                        self.ws.tabs.push(crate::workspace::TopTab::Blueprint(r.clone()));
                    }
                }
                self.ws.last_blueprint = st.blueprint_res.clone();
                self.st = Some(st);
                self.dock = saved_dock().unwrap_or_else(default_dock);
                self.viewport_tex = None;
                self.previews.clear();
                self.settings.loaded = false;
            }
            Err(e) => self.launcher.error = e,
        }
    }

    fn launcher(&mut self, ui: &mut egui::Ui) {
        if let Some(dir) = self.launcher.show(ui) {
            self.open_project(&dir);
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        let Some(st) = &mut self.st else { return };
        let mut close = false;
        let bar_bg = ui.painter().add(egui::Shape::Noop);
        let top = ui.cursor().top();
        theme::window_drag(ui, egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=top + 24.0));
        let bar = egui::MenuBar::new().ui(ui, |ui| {
            crate::docs::menu(ui, "File", |ui| { crate::theme::fade_in(ui);
                if theme::menu_item(ui, "Save all", "Ctrl+S").tip("Saves every changed blueprint and script.").clicked() {
                    st.save_all();
                    ui.close();
                }
                crate::docs::menu(ui, "Open blueprint", |ui| { crate::theme::fade_in(ui);
                    for s in panels::list_files(st, "res://", "jblueprint") {
                        if ui.button(&s).clicked() {
                            st.open_blueprint(&s);
                            ui.close();
                        }
                    }
                });
                crate::docs::menu(ui, "New blueprint", |ui| { crate::theme::fade_in(ui);
                    for k in [gex_core::ObjectKind::Object2D, gex_core::ObjectKind::Object3D, gex_core::ObjectKind::Object] {
                        if ui.button(format!("{} root", k.name())).clicked() {
                            let mut res = "res://blueprints/new_blueprint.jblueprint".to_string();
                            let mut i = 2;
                            while st.project.res_to_path(&res).is_some_and(|p| p.exists()) {
                                res = format!("res://blueprints/new_blueprint_{}.jblueprint", i);
                                i += 1;
                            }
                            if let Some(p) = st.project.res_to_path(&res) {
                                let _ = std::fs::create_dir_all(p.parent().unwrap());
                                let _ = std::fs::write(&p, gex_core::blueprint::new_blueprint_text("Root", k));
                                st.open_blueprint(&res);
                            }
                            ui.close();
                        }
                    }
                });
                if ui.button("Set as main blueprint").tip("The game starts in this blueprint").clicked() {
                    if let Some(r) = st.blueprint_res.clone() {
                        st.project.config.main_blueprint = r;
                        let _ = st.project.save();
                    }
                    ui.close();
                }
                ui.separator();
                if ui.button("Export game…").tip("A folder with the game, ready to run or share").clicked() {
                    st.show_export = true;
                    ui.close();
                }
                if ui.button("Project settings…").tip("The game's name, window, start blueprint, input actions, graphics and physics.").clicked() {
                    st.show_settings = true;
                    ui.close();
                }
                if ui.button("Close project").tip("Back to the start screen. Unsaved changes are kept in memory only until then: save first.").clicked() {
                    st.stop();
                    close = true;
                    ui.close();
                }
            });
            crate::docs::menu(ui, "Edit", |ui| { crate::theme::fade_in(ui);
                if theme::menu_item(ui, "Undo", "Ctrl+Z").tip("Takes back the last change to the blueprint being edited.").clicked() {
                    st.undo();
                    ui.close();
                }
                if theme::menu_item(ui, "Redo", "Ctrl+Y").tip("Puts back what Undo took away.").clicked() {
                    st.redo();
                    ui.close();
                }
                let file_op = st.file_undo.last().map(|o| o.describe());
                if ui.add_enabled(file_op.is_some() && st.playing.is_none(), egui::Button::new(format!("Undo file change{}", file_op.map(|d| format!(": {}", d)).unwrap_or_default()))).clicked() {
                    st.undo_file_op();
                    ui.close();
                }
                ui.separator();
                if theme::menu_item(ui, "Duplicate", "Ctrl+D").tip("Makes a copy of the selected objects next to them.").clicked() {
                    st.duplicate_selected();
                    ui.close();
                }
                if theme::menu_item(ui, "Delete", "Del").tip("Removes the selected objects (and what's inside them) from the blueprint.").clicked() {
                    st.delete_selected();
                    ui.close();
                }
            });
            crate::docs::menu(ui, "Object", |ui| panels::add_object_menu(ui, st));
            crate::docs::menu(ui, "View", |ui| { crate::theme::fade_in(ui);
                if self.classic {
                    for t in [Tab::Blueprint, Tab::Hierarchy, Tab::Inspector, Tab::Events, Tab::Assets, Tab::Console, Tab::Timeline, Tab::Profiler] {
                        let name = match &t {
                            Tab::Search => "Find in files".to_string(),
                            t => format!("{:?}", t),
                        };
                        if ui.button(name).clicked() {
                            show_tab(&mut self.dock, t);
                            ui.close();
                        }
                    }
                    if ui.button("Reset layout").tip("Puts the docked panels back where they started.").clicked() {
                        self.dock = default_dock();
                        ui.close();
                    }
                } else {
                    ui.checkbox(&mut self.ws.show_assets, "Assets").tip("Shows or hides the Assets panel.");
                    ui.checkbox(&mut self.ws.show_console, "Console").tip("Shows or hides the Console panel.");
                    ui.separator();
                    for t in ["Timeline", "Profiler"] {
                        if ui.button(t).clicked() {
                            self.ws.open_tool(t);
                            ui.close();
                        }
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.rex.open, "REX").tip("The assistant: answers questions about GEX and your project, and can make changes for you. It runs Claude Code with your own sign-in.");
                if theme::menu_item(ui, "Find in files", "Ctrl+F").tip("Searches every script and blueprint in the project.").clicked() {
                    st.search.open = true;
                    st.search.focus = true;
                    ui.close();
                }
                ui.separator();
                let mut classic = self.classic;
                if ui.checkbox(&mut classic, "Classic docked layout").tip("Panels docked around one blueprint view, instead of workspaces and blueprint tabs").changed() {
                    self.classic = classic;
                    if classic {
                        let _ = std::fs::create_dir_all(state::config_dir());
                        let _ = std::fs::write(classic_file(), "on");
                    } else {
                        let _ = std::fs::remove_file(classic_file());
                    }
                    // Scripts already open show in the layout switched to.
                    let open: Vec<String> = st.scripts.keys().cloned().collect();
                    st.open_requests.extend(open);
                    ui.close();
                }
            });
            // Play controls in the middle.
            ui.add_space(24.0);
            let playing = st.playing.is_some();
            if !playing {
                if theme::primary_button(ui, "▶  Play", egui::Color32::from_rgb(0x86, 0xb8, 0x22)).tip("Run this blueprint (F5)").clicked() {
                    st.play();
                }
            } else if theme::primary_button(ui, "■  Stop", egui::Color32::from_rgb(0xc8, 0x1e, 0x5c)).tip("Stop the game (F5)").clicked() {
                st.stop();
            }
            crate::docs::mark(ui, egui::Rect::from_min_max(egui::pos2(ui.min_rect().right() - 80.0, ui.min_rect().top()), ui.min_rect().max), "ui:play");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::window_buttons(ui);
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new(&st.project.config.name).strong().color(theme::ACCENT));
            });
        });
        let full = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), bar.response.rect.y_range()).expand2(egui::vec2(8.0, 2.0));
        crate::docs::mark(ui, full, "ui:menubar");
        ui.painter().set(bar_bg, theme::header(full, false));
        if close {
            self.st = None;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let Some(st) = &mut self.st else { return };
        let view: Option<egui::Id> = ctx.data(|d| d.get_temp(egui::Id::new("gex viewport id")));
        let focused = ctx.memory(|m| m.focused());
        let typing = ctx.egui_wants_keyboard_input() && (focused.is_none() || focused != view);
        let (f5, shift, save, undo, redo, dup, del, colon) = ctx.input_mut(|i| {
            (
                i.key_pressed(egui::Key::F5),
                i.modifiers.shift,
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::D),
                i.key_pressed(egui::Key::Delete),
                i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == ":")),
            )
        });
        let mut find_in_files = false;
        let (copy, paste, close_blueprint, tool) = ctx.input_mut(|i| {
            let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            let paste = i.events.iter().find_map(|e| match e {
                egui::Event::Paste(t) => Some(t.clone()),
                _ => None,
            });
            let close = i.consume_key(egui::Modifiers::COMMAND, egui::Key::W);
            if i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::F) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::F) {
                find_in_files = true;
            }
            let plain = !i.modifiers.command && !i.modifiers.alt;
            let tool = if plain && i.key_pressed(egui::Key::Num1) {
                Some(state::Tool::Move)
            } else if plain && i.key_pressed(egui::Key::Num2) {
                Some(state::Tool::Rotate)
            } else if plain && i.key_pressed(egui::Key::Num3) {
                Some(state::Tool::Scale)
            } else {
                None
            };
            (copy, paste, close, tool)
        });
        let (f8, f10) = ctx.input(|i| (i.key_pressed(egui::Key::F8), i.key_pressed(egui::Key::F10)));
        if let Some(p) = &mut st.playing {
            if f8 {
                p.handle.resume();
            }
            if f10 {
                p.handle.step();
            }
        }
        if f5 {
            if shift || st.playing.is_some() {
                st.stop();
            } else {
                st.play();
            }
        }
        if save {
            st.save_all();
        }
        if find_in_files {
            st.search.open = true;
            st.search.focus = true;
        }
        crate::tools::search_window(ctx, st);
        if std::mem::take(&mut st.open_timeline) {
            if self.classic {
                show_tab(&mut self.dock, Tab::Timeline);
            } else {
                self.ws.open_tool("Timeline");
            }
        }
        if typing {
            return;
        }
        if undo {
            st.undo();
        }
        if redo {
            st.redo();
        }
        if dup {
            st.duplicate_selected();
        }
        if del && st.playing.is_none() {
            st.delete_selected();
        }
        if (colon || std::mem::take(&mut st.open_command)) && st.playing.is_none() && self.cmd.is_none() {
            self.cmd = Some(String::new());
        }
        if st.playing.is_none() {
            if let Some(t) = tool {
                st.tool = t;
            }
            if copy {
                if let Some(t) = st.copy_text() {
                    ctx.copy_text(t);
                    st.status = format!("copied {} object(s)", st.selected_roots().len());
                }
            }
            if let Some(t) = paste {
                if st.paste_text(&t) {
                    st.status = "pasted".into();
                }
            }
            if close_blueprint {
                st.close_blueprint();
            }
        }
    }

    /// The Neovim-style command line (press `:`).
    fn command_line(&mut self, ui: &mut egui::Ui) {
        let Some(st) = &mut self.st else { return };
        let status_bg = ui.painter().add(egui::Shape::Noop);
        let status = egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(cmd) = &mut self.cmd {
                    ui.label(RichText::new(":").monospace().color(theme::ACCENT));
                    let r = ui.add(egui::TextEdit::singleline(cmd).font(egui::TextStyle::Monospace).frame(egui::Frame::NONE).desired_width(f32::INFINITY));
                    r.request_focus();
                    let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                    if esc {
                        self.cmd = None;
                    } else if enter {
                        let c = std::mem::take(cmd);
                        self.cmd = None;
                        run_command(st, &c, &mut self.dock);
                    }
                } else {
                    let mode = if st.playing.is_some() { RichText::new("PLAYING").color(theme::GOOD) } else { RichText::new("EDITING").color(theme::TEXT_DIM) };
                    ui.label(mode.small().strong());
                    // Messages start with a capital, wherever they came from.
                    let mut msg = st.status.clone();
                    if let Some(c) = msg.chars().next() {
                        msg.replace_range(..c.len_utf8(), &c.to_uppercase().to_string());
                    }
                    ui.add(egui::Label::new(RichText::new(msg).color(theme::TEXT_DIM)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(": commands").small().color(theme::TEXT_DIM)).tip("Press : for the command line (:w save, :e file, :play, :add Kind...)");
                        if let Some(p) = &st.playing {
                            let n = p.stats.len().min(30);
                            if n > 0 {
                                let avg: f32 = p.stats.iter().rev().take(n).map(|(ms, _)| *ms).sum::<f32>() / n as f32;
                                let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
                                let c = if fps >= 55.0 { theme::GOOD } else if fps >= 30.0 { theme::WARN } else { theme::BAD };
                                ui.label(RichText::new(format!("{:.0} FPS", fps)).small().color(c)).tip("Frames per second (View → Profiler for more)");
                            }
                        }
                        if let Some(sel) = st.selected.filter(|s| st.view_tree().contains(*s)) {
                            ui.label(RichText::new(st.view_tree().path_of(sel)).small().color(theme::TEXT_DIM));
                        }
                    });
                }
            });
        });
        ui.painter().set(status_bg, theme::header(status.response.rect, true));
        crate::docs::mark(ui, status.response.rect, "ui:status");
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(st) = &mut self.st else { return };
        if !st.show_settings {
            self.settings.loaded = false;
            return;
        }
        let f = &mut self.settings;
        if !f.loaded {
            f.loaded = true;
            f.name = st.project.config.name.clone();
            f.main_blueprint = st.project.config.main_blueprint.clone();
            f.message.clear();
            f.window = st.project.config.window_size;
            f.actions = st.project.config.actions.iter().map(|(k, v)| (k.clone(), v.join(", "))).collect();
            f.fullscreen = st.project.config.fullscreen;
            f.resizable = st.project.config.resizable;
            f.vsync = st.project.config.vsync;
            f.icon = st.project.config.icon.clone();
            f.hl2_on = false;
            f.hl2_games = "ep2, episodic, hl2".into();
            f.hl2_path.clear();
            for m in &st.project.config.mounts {
                if let (true, MountKind::SourceGame { install, games }) = (m.scheme == "hl2", &m.kind) {
                    f.hl2_on = true;
                    f.hl2_path = install.to_string_lossy().into_owned();
                    if !games.is_empty() {
                        f.hl2_games = games.join(", ");
                    }
                }
            }
        }
        let mut open = true;
        egui::Window::new("Project settings").collapsible(false).open(&mut open).default_width(560.0).default_height(560.0).show(ctx, |ui| {
            theme::fade_in(ui);
            ui.horizontal(|ui| {
                for (i, t) in ["General", "Graphics", "Physics", "Input", "Half-Life 2"].iter().enumerate() {
                    ui.selectable_value(&mut f.tab, i, *t);
                }
            });
            ui.separator();
            if f.tab == 1 || f.tab == 2 {
                let graphics = f.tab == 1;
                ui.label(
                    RichText::new(if graphics {
                        "How the game looks. Changes show in the view at once; Save keeps them. Scripts change them while playing with SET_GRAPHICS(\"name\", value)."
                    } else {
                        "How physics works. Save keeps changes. Scripts change them while playing with SET_PHYSICS(\"name\", value)."
                    })
                    .small()
                    .color(theme::TEXT_DIM),
                );
                egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                    if graphics {
                        let cfg = &mut st.project.config.graphics;
                        let changes = settings_rows(ui, gex_core::settings::Graphics::FIELDS, &|k| cfg.get(k));
                        for (k, v) in changes {
                            let _ = cfg.set(&k, &v);
                        }
                    } else {
                        let cfg = &mut st.project.config.physics;
                        let changes = settings_rows(ui, gex_core::settings::PhysicsSettings::FIELDS, &|k| cfg.get(k));
                        for (k, v) in changes {
                            let _ = cfg.set(&k, &v);
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        f.message = match st.project.save() {
                            Ok(()) => "Saved.".into(),
                            Err(e) => e,
                        };
                    }
                    if ui.button("Reset to defaults").tip("Every setting on this page").clicked() {
                        if graphics {
                            st.project.config.graphics = Default::default();
                        } else {
                            st.project.config.physics = Default::default();
                        }
                    }
                    if !f.message.is_empty() {
                        ui.colored_label(theme::GOOD, &f.message);
                    }
                });
                return;
            }
            if f.tab == 0 {
            egui::Grid::new("proj").num_columns(2).show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut f.name);
                ui.end_row();
                ui.label("Main blueprint");
                egui::ComboBox::from_id_salt("main blueprint").selected_text(f.main_blueprint.trim_start_matches("res://")).width(300.0).show_ui(ui, |ui| {
                    for sc in blueprint_files(&st.project) {
                        let label = sc.trim_start_matches("res://").to_string();
                        ui.selectable_value(&mut f.main_blueprint, sc, label);
                    }
                });
                ui.end_row();
                ui.label("Window size");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.window[0]).range(160..=7680).suffix(" px"));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut f.window[1]).range(120..=4320).suffix(" px"));
                    for (w, h) in [(1280, 720), (1920, 1080), (640, 360), (320, 180)] {
                        if ui.small_button(format!("{}×{}", w, h)).clicked() {
                            f.window = [w, h];
                        }
                    }
                });
                ui.end_row();
                ui.label("Window");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.fullscreen, "Full screen").tip("start the exported game full screen (Alt+Enter or F11 switches)");
                    ui.checkbox(&mut f.resizable, "Resizable");
                    ui.checkbox(&mut f.vsync, "Vsync").tip("no tearing; frames capped to the screen's refresh rate");
                });
                ui.end_row();
                ui.label("Icon");
                let r = ui.add(egui::TextEdit::singleline(&mut f.icon).hint_text("drop a .png here").desired_width(300.0));
                if let Some(p) = r.dnd_release_payload::<panels::AssetPath>() {
                    f.icon = p.0.clone();
                }
                ui.end_row();
            });
            }
            if f.tab == 3 {
            theme::title(ui, "Input actions");
            ui.label(
                RichText::new("Name an action once and give it any keys: then KEY_DOWN(\"jump\") and on_press_jump() work with all of them. Keys: a–z, 0–9, space, enter, escape, shift, left, right, up, down, mouse_left, mouse_right, pad_a, pad_b, pad_x, pad_y, pad_left, pad_start…")
                    .small()
                    .color(theme::TEXT_DIM),
            );
            let mut remove = None;
            for (i, (name, keys)) in f.actions.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized([120.0, 20.0], egui::TextEdit::singleline(name).hint_text("jump"));
                    ui.add_sized([300.0, 20.0], egui::TextEdit::singleline(keys).hint_text("space, w, pad_a"));
                    if ui.small_button("✖").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                f.actions.remove(i);
            }
            if ui.small_button("+ Action").clicked() {
                f.actions.push((String::new(), String::new()));
            }
            }
            if f.tab == 4 {
            theme::title(ui, "Half-Life 2 assets");
            ui.label(
                RichText::new("Reads textures, materials and other files straight from your own Steam install (hl2://…). Nothing is copied into the project, and exported games never include these files.")
                    .small()
                    .color(theme::TEXT_DIM),
            );
            ui.checkbox(&mut f.hl2_on, "Mount Half-Life 2 at hl2://");
            ui.add_enabled_ui(f.hl2_on, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Install folder");
                    ui.add(egui::TextEdit::singleline(&mut f.hl2_path).hint_text(r"…\steamapps\common\Half-Life 2").desired_width(300.0));
                    if let Some(p) = browse_button(ui) {
                        f.hl2_path = p;
                    }
                    if ui.button("Detect").tip("look in Steam's library folders").clicked() {
                        match gex_assets::source::find_hl2() {
                            Some(p) => f.hl2_path = p.to_string_lossy().into_owned(),
                            None => f.message = "Couldn't find Half-Life 2 through Steam; enter the folder yourself.".into(),
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Game folders, first wins");
                    ui.text_edit_singleline(&mut f.hl2_games);
                });
            });
            }
            if !f.message.is_empty() {
                ui.colored_label(theme::WARN, &f.message);
            }
            ui.separator();
            if ui.button("Save").clicked() {
                st.project.config.name = f.name.clone();
                st.project.config.main_blueprint = f.main_blueprint.clone();
                st.project.config.window_size = f.window;
                st.project.config.fullscreen = f.fullscreen;
                st.project.config.resizable = f.resizable;
                st.project.config.vsync = f.vsync;
                st.project.config.icon = f.icon.trim().to_string();
                st.project.config.actions = f
                    .actions
                    .iter()
                    .map(|(n, k)| (n.trim().to_lowercase().replace(' ', "_"), k.split([',', ' ']).map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect::<Vec<_>>()))
                    .filter(|(n, k)| !n.is_empty() && !k.is_empty())
                    .collect();
                st.project.config.mounts.retain(|m| m.scheme != "hl2");
                if f.hl2_on && !f.hl2_path.trim().is_empty() {
                    let games = f.hl2_games.split(',').map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect();
                    st.project.config.mounts.push(MountConfig { scheme: "hl2".into(), kind: MountKind::SourceGame { install: PathBuf::from(f.hl2_path.trim()), games } });
                }
                match st.project.save() {
                    Ok(()) => {
                        st.remount();
                        let files: usize = st
                            .vfs
                            .mounts
                            .iter()
                            .map(|m| match &m.imp {
                                gex_assets::vfs::MountImpl::Source(s) => s.file_count(),
                                _ => 0,
                            })
                            .sum();
                        f.message = if f.hl2_on { format!("Mounted: {} files in archives. Browse them in Assets → hl2://", files) } else { "Saved.".into() };
                    }
                    Err(e) => f.message = e,
                }
            }
        });
        if !open {
            st.show_settings = false;
        }
    }
}

impl EditorApp {
    fn export_window(&mut self, ctx: &egui::Context) {
        let Some(st) = &mut self.st else { return };
        if !st.show_export {
            return;
        }
        let mem = egui::Id::new("export folder");
        let mut folder: String = ctx.data(|d| d.get_temp(mem)).unwrap_or_else(|| st.project.dir.join("export").to_string_lossy().into_owned());
        let mut open = true;
        let mut result: Option<Result<Vec<String>, String>> = ctx.data(|d| d.get_temp(mem.with("result")));
        let players = crate::export::players();
        let mut pick: usize = ctx.data(|d| d.get_temp(mem.with("platform"))).unwrap_or(0).min(players.len().saturating_sub(1));
        let mut name: String = ctx.data(|d| d.get_temp(mem.with("name"))).unwrap_or_else(|| st.project.config.name.clone());
        egui::Window::new("Export game").collapsible(false).open(&mut open).default_width(520.0).show(ctx, |ui| {
            theme::fade_in(ui);
            ui.label("Makes a folder you can zip up and run anywhere: the GEX player, renamed after your game, plus your project's files. Double-clicking the program starts the game.");
            egui::Grid::new("export grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Folder");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut folder).desired_width(320.0));
                    if let Some(p) = browse_button(ui) {
                        folder = p;
                    }
                });
                ui.end_row();
                ui.label("Program name");
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0));
                ui.end_row();
                ui.label("For");
                ui.horizontal(|ui| {
                    for (i, (p, _)) in players.iter().enumerate() {
                        ui.radio_value(&mut pick, i, p.label());
                    }
                });
                ui.end_row();
                ui.label("Icon");
                if st.project.config.icon.is_empty() {
                    ui.label(RichText::new("none (set one in File → Project settings)").color(theme::TEXT_DIM));
                } else {
                    ui.label(&st.project.config.icon);
                }
                ui.end_row();
            });
            if players.len() < 3 {
                ui.label(RichText::new("To export for other systems, put their GEX players in a \"players\" folder next to GEX (gex-windows.exe, gex-linux, gex-macos). The download includes them.").small().color(theme::TEXT_DIM));
            }
            if ui.button("Export").clicked() {
                if let Some((p, path)) = players.get(pick) {
                    result = Some(st.export_game_for(std::path::Path::new(&folder), *p, path, &name));
                }
            }
            match &result {
                Some(Ok(w)) if w.is_empty() => {
                    ui.colored_label(theme::GOOD, "Done.");
                }
                Some(Ok(w)) => {
                    ui.colored_label(theme::GOOD, "Done, but:");
                    for x in w {
                        ui.colored_label(theme::WARN, x);
                    }
                }
                Some(Err(e)) => {
                    ui.colored_label(theme::BAD, e);
                }
                None => {}
            }
        });
        ctx.data_mut(|d| {
            d.insert_temp(mem, folder);
            d.insert_temp(mem.with("platform"), pick);
            d.insert_temp(mem.with("name"), name);
            if let Some(r) = result {
                d.insert_temp(mem.with("result"), r);
            }
        });
        if !open {
            st.show_export = false;
            ctx.data_mut(|d| d.remove::<Result<Vec<String>, String>>(mem.with("result")));
        }
    }
}

fn run_command(st: &mut EditorState, c: &str, dock: &mut DockState<Tab>) {
    let mut parts = c.trim().splitn(2, ' ');
    let head = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    match head {
        "w" | "write" => st.save_all(),
        "e" | "edit" => {
            let res = if arg.contains("://") { arg.to_string() } else { format!("res://{}", arg) };
            if res.ends_with(".jblueprint") {
                st.open_blueprint(&res);
            } else {
                st.open_script(&res, None);
            }
        }
        "play" | "run" => st.play(),
        "stop" => st.stop(),
        "add" | "object" => match gex_core::ObjectKind::from_name(arg) {
            Some(k) => {
                st.add_object(k);
            }
            None => st.status = format!("no object kind called {:?}", arg),
        },
        "del" | "delete" => st.delete_selected(),
        "dup" => st.duplicate_selected(),
        "name" | "rename" => {
            if let Some(id) = st.selected {
                st.will_change("rename");
                st.tree.objects[id].name = arg.to_string();
            }
        }
        "2d" => st.mode_3d = false,
        "3d" => st.mode_3d = true,
        "u" | "undo" => st.undo(),
        "redo" => st.redo(),
        "find" | "sel" => match st.tree.find_path(None, arg).or_else(|| st.tree.find_named(arg)) {
            Some(id) => st.selected = Some(id),
            None => st.status = format!("no object {:?}", arg),
        },
        "script" => {
            if let Some(id) = st.selected {
                st.new_script_for(id);
            }
        }
        "layout" => *dock = default_dock(),
        "vim" => {
            st.vim_enabled = !st.vim_enabled;
            let _ = std::fs::create_dir_all(state::config_dir());
            let _ = std::fs::write(state::config_dir().join("vim"), if st.vim_enabled { "on" } else { "off" });
            st.status = format!("Vim keys {}", if st.vim_enabled { "on" } else { "off" });
        }
        "branch" => {
            if let Some(id) = st.selected {
                st.save_branch_as_blueprint(id);
            }
        }
        "q" | "quit" => std::process::exit(0),
        "" => {}
        _ => {
            st.status = "commands: w · e <file> · play · stop · add <Kind> · del · dup · rename <name> · find <path> · script · branch · vim · 2d · 3d · u · redo · layout · q".into()
        }
    }
}

/// Every .jblueprint in the project, as res:// paths.
fn blueprint_files(p: &Project) -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x == "jblueprint") {
                out.push(path);
            }
        }
    }
    let mut v = Vec::new();
    walk(&p.dir, &mut v);
    let mut r: Vec<String> = v.iter().filter_map(|x| p.path_to_res(x)).collect();
    r.sort();
    r
}

/// A "Browse…" button with the native folder dialog (Windows; elsewhere, type the path).
fn browse_button(ui: &mut egui::Ui) -> Option<String> {
    #[cfg(windows)]
    {
        if ui.button("Browse…").clicked() {
            return rfd::FileDialog::new().pick_folder().map(|p| p.to_string_lossy().into_owned());
        }
    }
    #[cfg(not(windows))]
    let _ = ui;
    None
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        crate::startup::mark_running();
        let ctx = ui.ctx().clone();
        theme::backdrop(&ctx);
        theme::window_resize(&ctx);
        if self.st.is_none() {
            egui::Panel::top("launcher bar").frame(egui::Frame::NONE).show(ui, |ui| {
                let top = ui.cursor().top();
                theme::window_drag(ui, egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=top + 24.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), theme::window_buttons);
            });
            self.launcher(ui);
            return;
        }
        self.shortcuts(&ctx);
        // Middle-click documentation, for what the last frame drew under the pointer.
        crate::docs::show(&ctx);
        if let Some(st) = &mut self.st {
            if let Some(p) = &mut st.playing {
                let (keys, axes) = self.pads.poll();
                p.taps.extend(keys);
                p.axes = axes;
            }
            st.pump_game();
            if let Some(p) = &st.playing {
                repaint_capped(&ctx, p.list.graphics.fps_cap, frame_start);
            }
            // MOUSE_LOCK: hold the mouse while the game wants it; Escape lets go until the
            // game view is clicked again.
            if let Some(p) = &mut st.playing {
                if self.cursor_locked && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    p.lock_suspended = true;
                }
            }
            let want = st.playing.as_ref().is_some_and(|p| p.list.mouse_locked && !p.lock_suspended && p.game_camera) && ctx.input(|i| i.focused);
            if want != self.cursor_locked {
                lock_cursor(&ctx, want);
                self.cursor_locked = want;
            }
            if let Some(p) = &mut st.playing {
                p.locked = self.cursor_locked;
            }
            // Files dropped on the window go into the project.
            let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
            if !dropped.is_empty() {
                st.import_files(&dropped);
            }
            // Script tabs whose files were renamed.
            for (old, new) in std::mem::take(&mut st.renamed_scripts) {
                for (_, t) in self.dock.iter_all_tabs_mut() {
                    if *t == Tab::Script(old.clone()) {
                        *t = Tab::Script(new.clone());
                    }
                }
            }
            // Script tabs to open / close (the workspace layout opens its own windows).
            let requests = if self.classic { std::mem::take(&mut st.open_requests) } else { Vec::new() };
            if self.classic {
                // The classic layout has no blueprint windows: opening a blueprint edits it.
                for r in std::mem::take(&mut st.open_windows) {
                    st.open_blueprint(&r);
                }
            }
            for r in requests {
                let t = Tab::Script(r);
                match self.dock.find_tab(&t) {
                    Some(p) => {
                        let _ = self.dock.set_active_tab(p);
                    }
                    None => {
                        // Next to the blueprint view.
                        match self.dock.find_tab(&Tab::Blueprint) {
                            Some(p) => {
                                let _ = self.dock.set_focused_node_and_surface(p.node_path());
                                self.dock.push_to_focused_leaf(t);
                            }
                            None => self.dock.push_to_first_leaf(t),
                        }
                    }
                }
            }
        }
        self.menu(ui);
        if self.st.is_none() {
            return;
        }
        self.command_line(ui);
        if let Some(st) = &mut self.st {
            self.rex.show(ui, st);
            if std::mem::take(&mut self.rex.reload) {
                st.reload_from_disk();
            }
        }
        self.settings_window(&ctx);
        self.export_window(&ctx);
        let rs = frame.wgpu_render_state();
        if !self.classic {
            let st = self.st.as_mut().unwrap();
            egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(4.0)).show(ui, |ui| {
                crate::workspace::show(ui, &mut self.ws, st, rs, &mut self.viewport_tex, &mut self.previews);
            });
            if self.ws_saved_at.elapsed().as_secs_f32() > 2.0 {
                self.ws.save(&st.project.dir);
                self.ws_saved_at = std::time::Instant::now();
            }
            return;
        }
        let st = self.st.as_mut().unwrap();
        let mut viewer = Viewer { st, rs, viewport_tex: &mut self.viewport_tex, previews: &mut self.previews, closed: Vec::new() };
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let mut style = Style::from_egui(ui.style().as_ref());
            // Glass panels: see-through bodies with a faint light edge; tab strips a shade darker.
            style.tab_bar.bg_fill = theme::alpha(egui::Color32::BLACK, 170);
            style.tab_bar.hline_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 16);
            style.tab.tab_body.bg_fill = theme::glass_fill();
            style.tab.tab_body.stroke = theme::glass_edge();
            style.tab.tab_body.corner_radius = egui::CornerRadius::ZERO;
            for t in [&mut style.tab.active, &mut style.tab.inactive, &mut style.tab.focused, &mut style.tab.hovered, &mut style.tab.active_with_kb_focus, &mut style.tab.inactive_with_kb_focus, &mut style.tab.focused_with_kb_focus] {
                t.corner_radius = egui::CornerRadius::ZERO;
            }
            for t in [&mut style.tab.active, &mut style.tab.focused, &mut style.tab.active_with_kb_focus, &mut style.tab.focused_with_kb_focus] {
                t.bg_fill = theme::glass_fill();
                t.outline_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22);
                t.text_color = theme::TEXT;
            }
            style.tab.focused.text_color = theme::ACCENT;
            for t in [&mut style.tab.inactive, &mut style.tab.inactive_with_kb_focus] {
                t.bg_fill = egui::Color32::TRANSPARENT;
                t.outline_color = egui::Color32::TRANSPARENT;
                t.text_color = theme::TEXT_DIM;
            }
            style.tab.hovered.bg_fill = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10);
            style.tab.hovered.text_color = theme::TEXT;
            style.separator.color_idle = egui::Color32::from_rgba_unmultiplied(0, 0, 0, 0);
            style.separator.color_hovered = theme::alpha(theme::ACCENT, 140);
            style.separator.color_dragged = theme::ACCENT;
            style.main_surface_border_stroke = egui::Stroke::NONE;
            DockArea::new(&mut self.dock).style(style).show_inside(ui, &mut viewer);
        });
        for r in viewer.closed {
            if self.st.as_ref().unwrap().scripts.get(&r).is_some_and(|s| !s.dirty()) {
                self.st.as_mut().unwrap().scripts.remove(&r);
            }
        }
    }

    fn on_exit(&mut self) {
        save_dock(&self.dock);
        if let Some(st) = &mut self.st {
            self.ws.save(&st.project.dir);
            st.stop();
        }
    }
}

// ---------------------------------------------------------------- player

/// Just the game, full-window (`gex --play <project>`).
pub struct PlayerApp {
    design: [u32; 2],
    handle: gex_script::thread::GameHandle,
    vfs: gex_assets::Vfs,
    list: gex_core::render::RenderList,
    renderer: Option<gex_render::Renderer>,
    tex: Option<egui::TextureId>,
    last: std::time::Instant,
    ended: bool,
    ui_events: Vec<gex_script::UiEvent>,
    ui_images: HashMap<String, egui::TextureHandle>,
    pads: crate::gamepad::Pads,
    /// Keys and buttons that went down since the last game frame, so a quick tap (down and
    /// up between two frames) still counts as pressed once.
    taps: std::collections::HashSet<String>,
    tap_buttons: std::collections::HashSet<String>,
    mouse_delta: [f32; 2],
    cursor_locked: bool,
    /// The window state last asked of the OS (fullscreen, title).
    applied_full: Option<bool>,
    applied_title: Option<String>,
}

impl PlayerApp {
    pub fn new(project: Project) -> Result<PlayerApp, String> {
        gex_script::commands::register();
        let text = project.read_text(&project.config.main_blueprint)?;
        let tree = gex_core::blueprint::load(&text, &|p: &str| project.read_text(p)).map_err(|e| e.0)?;
        let vfs = gex_assets::Vfs::new(&project.dir, &project.config);
        let mut list = gex_core::render::extract(&tree);
        list.graphics = project.config.graphics.clone();
        Ok(PlayerApp { design: project.config.window_size, handle: gex_script::thread::GameHandle::start(project, tree), vfs, list, renderer: None, tex: None, last: std::time::Instant::now(), ended: false, ui_events: Vec::new(), ui_images: HashMap::new(), pads: crate::gamepad::Pads::new(), taps: Default::default(), tap_buttons: Default::default(), mouse_delta: [0.0; 2], cursor_locked: false, applied_full: None, applied_title: None })
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        crate::startup::mark_running();
        let ctx = ui.ctx().clone();
        for m in self.handle.poll() {
            match m {
                gex_script::thread::FromGame::Frame { list, logs, .. } => {
                    self.list = list;
                    for l in logs {
                        eprintln!("{}", l.text);
                    }
                }
                gex_script::thread::FromGame::Stopped { logs } => {
                    for l in logs {
                        eprintln!("{}", l.text);
                    }
                    self.ended = true;
                }
                gex_script::thread::FromGame::Paused(_) => self.handle.resume(),
            }
        }
        if self.ended {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let now = std::time::Instant::now();
        let (mut keys, mut buttons) = ctx.input(|i| (viewport::input_keys(i), viewport::mouse_buttons(i)));
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Key { key, pressed: true, .. } => self.taps.extend(viewport::key_name(*key)),
                    egui::Event::PointerButton { button, pressed: true, .. } => {
                        let b = match button {
                            egui::PointerButton::Primary => "left",
                            egui::PointerButton::Secondary => "right",
                            egui::PointerButton::Middle => "middle",
                            _ => continue,
                        };
                        self.tap_buttons.insert(b.to_string());
                    }
                    _ => {}
                }
            }
        });
        let d = ctx.input(mouse_motion);
        self.mouse_delta[0] += d[0];
        self.mouse_delta[1] += d[1];
        // FULLSCREEN / WINDOW_TITLE from the game; Alt+Enter or F11 toggles full screen too.
        if ctx.input(|i| (i.modifiers.alt && i.key_pressed(egui::Key::Enter)) || i.key_pressed(egui::Key::F11)) {
            let now = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!now));
        }
        if self.list.fullscreen.is_some() && self.list.fullscreen != self.applied_full {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.list.fullscreen.unwrap()));
            self.applied_full = self.list.fullscreen;
        }
        if self.list.window_title.is_some() && self.list.window_title != self.applied_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.list.window_title.clone().unwrap()));
            self.applied_title = self.list.window_title.clone();
        }
        // MOUSE_LOCK, let go while the window isn't focused.
        let want = self.list.mouse_locked && ctx.input(|i| i.focused);
        if want != self.cursor_locked {
            lock_cursor(&ctx, want);
            self.cursor_locked = want;
        }
        let (pad_keys, axes) = self.pads.poll();
        keys.extend(pad_keys);
        let Some(rs) = frame.wgpu_render_state() else { return };
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let rect = ui.max_rect();
            let ppp = ctx.pixels_per_point();
            let px = [(rect.width() * ppp) as u32, (rect.height() * ppp) as u32];
            let cams = gex_render::Cameras::for_game(&self.list, px, self.design);
            let m = ctx.input(|i| i.pointer.hover_pos()).map(|p| cams.screen_to_2d(gex_core::glam::Vec2::new(p.x * ppp, p.y * ppp))).unwrap_or_default();
            if !self.handle.waiting {
                keys.extend(self.taps.drain());
                buttons.extend(self.tap_buttons.drain());
                self.handle.request_frame(gex_script::FrameInput {
                    dt: now.duration_since(self.last).as_secs_f64(),
                    keys,
                    mouse: [m.x, m.y],
                    buttons,
                    wheel: ctx.input(|i| i.smooth_scroll_delta.y) / 50.0,
                    mouse_delta: std::mem::take(&mut self.mouse_delta),
                    axes,
                    ui: std::mem::take(&mut self.ui_events),
                });
                self.last = now;
            }
            let probs = viewport::paint_list(ui, rs, &mut self.renderer, &mut self.tex, &self.vfs, rect, &self.list, &cams, &gex_render::Overlay::default());
            for p in probs {
                eprintln!("{}", p);
            }
            let design = gex_core::glam::Vec2::new(self.design[0] as f32, self.design[1] as f32);
            let fit = (px[0] as f32 / design.x).min(px[1] as f32 / design.y);
            let map = |p: gex_core::glam::Vec2| {
                let s = (p - design / 2.0) * fit + gex_core::glam::Vec2::new(px[0] as f32, px[1] as f32) / 2.0;
                rect.min + egui::vec2(s.x / ppp, s.y / ppp)
            };
            let ev = crate::ui_layer::paint_ui(ui, &self.list.ui, design, &map, fit / ppp, true, &mut self.ui_images, &self.vfs);
            self.ui_events.extend(ev);
            viewport::paint_fade(ui, rect, self.list.fade);
        });
        repaint_capped(&ctx, self.list.graphics.fps_cap, frame_start);
    }
}

/// Asks for the next frame: at once, or (with a frame-rate cap) when the frame's time is up.
fn repaint_capped(ctx: &egui::Context, cap: u32, frame_start: std::time::Instant) {
    if cap == 0 {
        ctx.request_repaint();
    } else {
        let budget = std::time::Duration::from_secs_f64(1.0 / cap as f64);
        ctx.request_repaint_after(budget.saturating_sub(frame_start.elapsed()));
    }
}

/// Editors for a list of settings, in their sections. Gives back the ones changed.
fn settings_rows(ui: &mut egui::Ui, fields: &[gex_core::settings::FieldInfo], get: &dyn Fn(&str) -> Option<gex_core::Variant>) -> Vec<(String, gex_core::Variant)> {
    use gex_core::Variant;
    let mut changes = Vec::new();
    let mut sections: Vec<&str> = Vec::new();
    for f in fields {
        if !sections.contains(&f.section) {
            sections.push(f.section);
        }
    }
    for sec in sections {
        egui::CollapsingHeader::new(sec).id_salt(("settings", sec, fields.len())).default_open(true).show(ui, |ui| {
            egui::Grid::new(("settings grid", sec, fields.len())).num_columns(2).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
                for f in fields.iter().filter(|f| f.section == sec) {
                    let Some(mut v) = get(f.name) else { continue };
                    ui.label(f.name).tip(f.doc);
                    let changed = match (&mut v, f.kind) {
                        (Variant::Text(cur), "choice") => {
                            let mut ch = false;
                            egui::ComboBox::from_id_salt(("setting", f.name)).selected_text(cur.as_str()).show_ui(ui, |ui| {
                                for c in f.choices {
                                    if ui.selectable_label(cur == c, *c).clicked() && cur != c {
                                        *cur = c.to_string();
                                        ch = true;
                                    }
                                }
                            });
                            ch
                        }
                        (Variant::Num(n), "whole") => {
                            let mut k = *n as i64;
                            let r = ui.add(egui::Slider::new(&mut k, f.min as i64..=f.max as i64));
                            *n = k as f64;
                            r.changed()
                        }
                        (Variant::Num(n), _) if f.max > f.min => ui.add(egui::Slider::new(n, f.min..=f.max).max_decimals(3)).changed(),
                        (Variant::Num(n), _) => ui.add(egui::DragValue::new(n).speed(0.05)).changed(),
                        (Variant::Bool(b), _) => ui.checkbox(b, "").changed(),
                        (Variant::Color(c), _) => {
                            let mut c32 = egui::Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (c[3] * 255.0) as u8);
                            let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::OnlyBlend);
                            if r.changed() {
                                let [rr, g, b, a] = c32.to_srgba_unmultiplied();
                                *c = [rr as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0];
                            }
                            r.changed()
                        }
                        (Variant::Vec2(p), _) => {
                            let mut ch = false;
                            ui.horizontal(|ui| {
                                ch |= ui.add(egui::DragValue::new(&mut p[0]).prefix("x ").speed(1.0)).changed();
                                ch |= ui.add(egui::DragValue::new(&mut p[1]).prefix("y ").speed(1.0)).changed();
                            });
                            ch
                        }
                        (Variant::Vec3(p), _) => {
                            let mut ch = false;
                            ui.horizontal(|ui| {
                                for (k, l) in ["x ", "y ", "z "].iter().enumerate() {
                                    ch |= ui.add(egui::DragValue::new(&mut p[k]).prefix(*l).speed(0.05)).changed();
                                }
                            });
                            ch
                        }
                        _ => false,
                    };
                    if changed {
                        changes.push((f.name.to_string(), v));
                    }
                    ui.end_row();
                }
            });
        });
    }
    changes
}
