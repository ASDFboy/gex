//! The editor window: launcher (project library) or the editor (a tab per open blueprint).

use crate::docs::Tip as _;
use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, RichText};
use gex_core::project::{MountConfig, MountKind, Project};

use crate::state::{self, EditorState};
use crate::{panels, theme, viewport};

pub struct EditorApp {
    st: Option<EditorState>,
    viewport_tex: Option<egui::TextureId>,
    previews: HashMap<String, egui::TextureHandle>,
    // Launcher fields.
    launcher: crate::launcher::Launcher,
    // Neovim-style command line.
    cmd: Option<String>,
    settings: SettingsForm,
    pads: crate::gamepad::Pads,
    /// The window holds and hides the mouse for the game right now.
    cursor_locked: bool,
    /// The layout (a tab per open blueprint) of the open project.
    ws: crate::workspace::Layout,
    ws_saved_at: std::time::Instant,
    /// Errors in the console that have been seen (by closing the console or clearing it);
    /// while there are more, the window shows the error icon.
    errors_seen: usize,
    error_icon: bool,
    /// Checks for a newer GEX when the editor starts.
    updater: crate::update::Updater,
    /// File → Work together.
    collab_panel: crate::collab::Panel,
    /// Joining someone's project: its files are on their way.
    joining: Option<crate::collab::Session>,
    /// GitHub: the account (GEX settings), linking (Project settings → GitHub), the projects
    /// on GitHub (the start screen).
    github: crate::github::Panel,
    /// The GEX settings window, open at a page (0 GitHub, 1 Updates).
    gex_settings: Option<usize>,
    /// The open project's syncing with GitHub (when it's linked), and the project it was
    /// looked for in.
    syncer: Option<crate::github::Syncer>,
    sync_looked: Option<std::path::PathBuf>,
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

/// Project settings' GitHub page.
const GITHUB_TAB: usize = 6;

#[derive(Default)]
struct SettingsForm {
    loaded: bool,
    name: String,
    main_blueprint: String,
    hl2_path: String,
    hl2_games: String,
    hl2_on: bool,
    message: String,
    /// Which page: General, Graphics, Physics, Input, Half-Life 2.
    tab: usize,
    /// (action name, keys as "space, w, pad_a").
    actions: Vec<(String, String)>,
    icon: String,
    /// The settings as they were when the window opened (Cancel puts them back).
    before: Option<gex_core::project::ProjectConfig>,
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext, project: Option<PathBuf>) -> EditorApp {
        theme::apply(&cc.egui_ctx);
        gex_script::commands::register();
        // Left over from the assistant GEX used to have.
        for f in ["rex_context.md", "claude_path.txt"] {
            let _ = std::fs::remove_file(state::config_dir().join(f));
        }
        let mut app = EditorApp {
            st: None,
            viewport_tex: None,
            previews: HashMap::new(),
            launcher: Default::default(),
            cmd: None,
            settings: SettingsForm::default(),
            pads: crate::gamepad::Pads::new(),
            cursor_locked: false,
            ws: Default::default(),
            ws_saved_at: std::time::Instant::now(),
            errors_seen: 0,
            error_icon: false,
            updater: crate::update::Updater::start(),
            collab_panel: Default::default(),
            joining: None,
            github: Default::default(),
            gex_settings: None,
            syncer: None,
            sync_looked: None,
        };
        if let Some(p) = project {
            app.open_project(&p);
        }
        app
    }

    /// The project closing: its last changes go to GitHub (when it's linked).
    fn stop_sync(&mut self) {
        // (In the background: the editor goes on while it's sent.)
        if let Some(s) = self.syncer.take() {
            std::thread::spawn(move || s.finish(std::time::Duration::from_secs(120)));
        }
        self.sync_looked = None;
    }

    /// Syncing with GitHub: started for a linked project, and results in.
    fn github(&mut self, ctx: &egui::Context) {
        self.github.update(ctx, self.st.as_mut(), &mut self.syncer);
        if let Some(d) = self.github.got.take() {
            if let Some(st) = &mut self.st {
                st.stop();
                st.save_all();
                self.ws.save(&st.project.dir);
            }
            self.open_project(&d);
        }
        let Some(st) = &mut self.st else { return };
        if std::mem::take(&mut self.github.link_failed) {
            st.show_settings = true;
            self.settings.tab = GITHUB_TAB;
        }
        if self.sync_looked.as_deref() != Some(st.project.dir.as_path()) {
            self.sync_looked = Some(st.project.dir.clone());
            self.github.project_changed();
            // (A copy joined through Work together is the sharer's to sync, not this computer's.)
            let joined = st.collab.as_ref().is_some_and(|c| !c.host);
            self.syncer = crate::github::link_of(&st.project.dir).filter(|_| !joined).map(|l| crate::github::Syncer::start(ctx, &st.project.dir, l));
        }
        // Open files changed by another program are noticed (checked every couple of seconds).
        st.check_outside_changes();
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
        if let Some(s) = &mut self.syncer {
            s.tick(ctx, st);
        }
    }

    fn open_project(&mut self, dir: &std::path::Path) {
        self.stop_sync();
        // (The start screen lists it first when it's back.)
        self.launcher.refresh();
        match Project::open(dir) {
            Ok(mut p) => {
                // Half-Life 2, mounted once, is mounted in every project.
                state::apply_shared_hl2(&mut p);
                state::remember_project(&p.dir);
                self.ws = crate::workspace::Layout::load(&p.dir);
                let mut st = EditorState::new(p);
                st.load_asset_colors();
                // The blueprint opened at start doesn't take over the tab that was showing.
                // The tabs (and the one showing) are as they were left; a project opened for
                // the first time shows its first blueprint.
                use crate::workspace::COMMANDS_TAB;
                self.ws.tabs.retain(|t| t == COMMANDS_TAB || st.project.res_to_path(t).is_some_and(|p| p.is_file()));
                self.ws.active = self.ws.active.min(self.ws.tabs.len().saturating_sub(1));
                // The blueprint to edit: the tab showing, or (on the Commands tab) another one.
                let showing = self.ws.tabs.get(self.ws.active).filter(|t| *t != COMMANDS_TAB).or_else(|| self.ws.tabs.iter().find(|t| *t != COMMANDS_TAB)).cloned();
                match showing {
                    Some(t) => st.open_blueprint(&t),
                    None => self.ws.tabs.extend(st.blueprint_res.clone()),
                }
                self.ws.last_blueprint = st.blueprint_res.clone();
                self.st = Some(st);
                self.viewport_tex = None;
                self.previews.clear();
                self.settings.loaded = false;
            }
            Err(e) => self.launcher.error = e,
        }
    }

    fn launcher(&mut self, ui: &mut egui::Ui) {
        if let Some(dir) = self.launcher.show(ui, &mut self.github) {
            self.open_project(&dir);
            // Made to be kept on GitHub: linked in the background, then it syncs.
            if let Some((d, name)) = self.launcher.link.take() {
                if self.st.as_ref().is_some_and(|st| st.project.dir == d) {
                    self.github.link(ui.ctx(), &d, &name);
                }
            }
        }
        if std::mem::take(&mut self.launcher.join) {
            self.collab_panel.open = true;
        }
        if std::mem::take(&mut self.launcher.settings) {
            self.gex_settings = Some(0);
        }
    }

    /// Working together: the window, joining, and the session itself.
    fn collab(&mut self, ctx: &egui::Context) {
        if let Some(j) = &mut self.joining {
            crate::collab::tick_joining(ctx, j);
            if let Some(why) = j.ended.take() {
                self.collab_panel.message = format!("Couldn't join: {}", why);
                self.collab_panel.open = true;
                self.joining = None;
            } else if j.ready {
                let mut s = self.joining.take().unwrap();
                let dir = s.dir.clone();
                // The project open until now is saved, not lost, as the joined one takes its place.
                if let Some(st) = &mut self.st {
                    st.save_all();
                    self.ws.save(&st.project.dir);
                }
                self.open_project(&dir);
                match &mut self.st {
                    Some(st) if st.project.dir == dir => {
                        s.opened = true;
                        st.collab = Some(s);
                    }
                    _ => self.collab_panel.message = "The project arrived but didn't open.".into(),
                }
            }
        }
        if let Some(st) = &mut self.st {
            crate::collab::tick(ctx, st);
            if let Some(why) = st.collab.as_mut().and_then(|s| s.ended.take()) {
                st.collab = None;
                st.log(gex_script::LogKind::Info, format!("Working together ended: {}. Your copy of the project stays here.", why));
                self.collab_panel.message = format!("Working together ended: {}. Your copy of the project stays here.", why);
                self.collab_panel.open = true;
            }
        }
        if let Some(j) = crate::collab::window(ctx, &mut self.collab_panel, self.st.as_mut(), &mut self.joining) {
            self.joining = Some(j);
        }
    }

    /// Updates: a bar when there's a new version (to install, or installed), and the Updates
    /// page (GEX settings → Updates).
    fn update_bar(&mut self, ui: &mut egui::Ui) {
        use crate::update::State;
        let progress = self.updater.poll();
        match &self.updater.state {
            State::Checking => ui.ctx().request_repaint_after(std::time::Duration::from_secs(1)),
            State::Downloading(_) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)),
            _ => {}
        }
        self.gex_settings_window(ui.ctx());
        let (rel, ready) = match self.updater.state.clone() {
            State::Available(r) => (r, false),
            State::Downloading(r) => (r, false),
            State::Ready(r) => (r, true),
            _ => return,
        };
        if self.updater.dismissed {
            return;
        }
        egui::Panel::top("update bar").frame(egui::Frame::NONE.fill(theme::GRAY).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(f) = progress {
                    ui.label(RichText::new(format!("Downloading GEX {}…", rel.version)).color(theme::TEXT));
                    ui.add(egui::ProgressBar::new(f).desired_width(200.0).show_percentage());
                    return;
                }
                let what = if ready { format!("GEX {} is installed (this is {}). Restart to use it.", rel.version, crate::update::VERSION) } else { format!("GEX {} is out (this is {}).", rel.version, crate::update::VERSION) };
                ui.label(RichText::new(what).color(theme::TEXT));
                if !rel.notes.trim().is_empty() && ui.button("What's new").clicked() {
                    self.updater.window = true;
                    self.updater.notes = true;
                }
                if ready {
                    let label = if self.st.is_some() { "Save all and restart" } else { "Restart" };
                    if ui.button(label).clicked() {
                        let dir = self.st.as_mut().map(|st| {
                            st.save_all();
                            self.ws.save(&st.project.dir);
                            st.project.dir.clone()
                        });
                        crate::update::restart(ui.ctx(), self.updater.exe(), dir.as_deref());
                    }
                } else {
                    if ui.button("Install").tip("Downloads it, checks it and puts it in place; you choose when to restart").clicked() {
                        self.updater.install();
                    }
                    if ui.button("Skip this version").clicked() {
                        self.updater.skip();
                    }
                }
                if ui.button("Later").tip(if ready { "The new version starts next time you open GEX" } else { "Ask again next time" }).clicked() {
                    self.updater.dismissed = true;
                }
            });
        });
    }

    /// GEX settings (the start screen's Settings, File → GEX settings): for every project on
    /// this computer. Its Updates page is also where the update bar's "What's new" goes.
    fn gex_settings_window(&mut self, ctx: &egui::Context) {
        use crate::update::{Mode, State};
        if std::mem::take(&mut self.updater.window) {
            self.gex_settings = Some(1);
        }
        let Some(mut tab) = self.gex_settings else {
            self.updater.notes = false;
            return;
        };
        let mut open = true;
        egui::Window::new("GEX settings").open(&mut open).resizable(false).collapsible(false).default_width(460.0).show(ctx, |ui| {
            theme::fade_in(ui);
            ui.set_width(460.0);
            theme::tab_row(ui, &["GitHub", "Updates"], &mut tab);
            if tab == 0 {
                self.github.account_page(ui);
                return;
            }
            let u = &mut self.updater;
            theme::title(ui, "Updates");
            ui.label(RichText::new(format!("GEX {}", crate::update::VERSION)).strong().color(theme::TEXT));
            let status = match &u.state {
                State::Off => "Not checked.".to_string(),
                State::Checking => "Checking for a new version…".to_string(),
                State::Available(r) => format!("GEX {} is out.", r.version),
                State::Downloading(r) => format!("Downloading GEX {}…", r.version),
                State::Ready(r) => format!("GEX {} is installed: restart to use it.", r.version),
                State::UpToDate => "Up to date.".to_string(),
                State::Failed(e) => format!("Couldn't update: {}", e),
            };
            ui.label(RichText::new(status).color(theme::TEXT_DIM));
            ui.separator();
            let before = u.settings.mode;
            ui.radio_value(&mut u.settings.mode, Mode::Auto, "Install new versions by itself (then offer a restart)");
            ui.radio_value(&mut u.settings.mode, Mode::Ask, "Tell me first, and wait for Install");
            ui.radio_value(&mut u.settings.mode, Mode::Off, "Don't check");
            if u.settings.mode != before {
                u.settings.save();
            }
            ui.label(RichText::new("GEX checks GitHub (ASDFboy/gex) when it starts and every 6 hours. Downloads are checked against the release's SHA-256 list before they're used. The old version is removed once the new one runs.").small().color(theme::TEXT_DIM));
            ui.horizontal(|ui| {
                if ui.add_enabled(!matches!(u.state, State::Checking | State::Downloading(_) | State::Ready(_)), egui::Button::new("Check now")).clicked() {
                    u.settings.skip = None;
                    u.check();
                }
                if let State::Available(_) = u.state {
                    if ui.button("Install").clicked() {
                        u.install();
                    }
                }
            });
            let notes = match &u.state {
                State::Available(r) | State::Downloading(r) | State::Ready(r) => Some(r.notes.clone()),
                _ => None,
            };
            if let Some(n) = notes.filter(|n| !n.trim().is_empty()) {
                ui.separator();
                egui::CollapsingHeader::new("What's new").default_open(u.notes).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.label(n);
                    });
                });
            }
        });
        self.gex_settings = open.then_some(tab);
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        let Some(st) = &mut self.st else { return };
        let mut close = false;
        // Another project to open instead (File → Open project).
        let mut switch: Option<std::path::PathBuf> = None;
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
                ui.separator();
                crate::docs::menu(ui, "Open project", |ui| { crate::theme::fade_in(ui);
                    let here = st.project.dir.clone();
                    let recent: Vec<std::path::PathBuf> = state::recent_projects().into_iter().filter(|d| *d != here && d.join(gex_core::project::PROJECT_FILE).is_file()).take(10).collect();
                    for d in &recent {
                        let name = Project::peek(d).map(|c| c.name).unwrap_or_else(|_| d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                        if ui.button(&name).tip(d.to_string_lossy()).clicked() {
                            switch = Some(d.clone());
                            ui.close();
                        }
                    }
                    if recent.is_empty() {
                        ui.label(RichText::new("No other projects yet").color(theme::TEXT_DIM));
                    }
                    ui.separator();
                    #[cfg(windows)]
                    if ui.button("Choose a folder…").clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            switch = Some(p);
                        }
                        ui.close();
                    }
                    if ui.button("Start screen").tip("Your projects (and the ones on your GitHub), and making a new one").clicked() {
                        st.stop();
                        close = true;
                        ui.close();
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
                if ui.button(if st.collab.is_some() { "Working together…" } else { "Work together…" }).tip("Share this project so others can work on it with you, live, or join someone else's").clicked() {
                    self.collab_panel.open = true;
                    ui.close();
                }
                if ui.button(if self.syncer.is_some() { "Kept on GitHub…" } else { "Keep on GitHub…" }).tip("Keep this project the same on all your computers, through a private repository on your GitHub account (Project settings → GitHub)").clicked() {
                    st.show_settings = true;
                    self.settings.tab = GITHUB_TAB;
                    ui.close();
                }
                if ui.button("Export game…").tip("A folder with the game, ready to run or share").clicked() {
                    st.show_export = true;
                    ui.close();
                }
                if ui.button("Project settings…").tip("The game's name, window, start blueprint, input actions, graphics and physics.").clicked() {
                    st.show_settings = true;
                    ui.close();
                }
                if ui.button("GEX settings…").tip("For every project on this computer: your GitHub sign-in, and updates").clicked() {
                    self.gex_settings = Some(0);
                    ui.close();
                }
                ui.separator();
                if ui.button("Close project").tip("Saves what's changed and goes back to the start screen.").clicked() {
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
                let editing = st.playing.is_none();
                let any = st.selected.is_some();
                if ui.add_enabled_ui(editing && any, |ui| theme::menu_item(ui, "Cut", "Ctrl+X")).inner.tip("Copies the selected objects and removes them.").clicked() {
                    if let Some(t) = st.copy_selected() {
                        ui.ctx().copy_text(t);
                        st.delete_selected();
                    }
                    ui.close();
                }
                if ui.add_enabled_ui(any, |ui| theme::menu_item(ui, "Copy", "Ctrl+C")).inner.tip("Copies the selected objects (with what's inside them), to paste here or in another blueprint.").clicked() {
                    if let Some(t) = st.copy_selected() {
                        ui.ctx().copy_text(t);
                    }
                    ui.close();
                }
                if ui.add_enabled_ui(editing && !st.clipboard.is_empty(), |ui| theme::menu_item(ui, "Paste", "Ctrl+V")).inner.tip("Puts the copied objects in, next to the selection.").clicked() {
                    let t = st.clipboard.clone();
                    if st.paste_text(&t) {
                        st.status = "pasted".into();
                    }
                    ui.close();
                }
                if ui.add_enabled_ui(editing && any, |ui| theme::menu_item(ui, "Duplicate", "Ctrl+D")).inner.tip("Makes a copy of the selected objects next to them.").clicked() {
                    st.duplicate_selected();
                    ui.close();
                }
                if ui.add_enabled_ui(editing && any, |ui| theme::menu_item(ui, "Rename", "F2")).inner.tip("Renames the selected object, in the Blueprint panel.").clicked() {
                    st.renaming = st.selected;
                    ui.close();
                }
                if ui.add_enabled_ui(editing && any, |ui| theme::menu_item(ui, "Delete", "Del")).inner.tip("Removes the selected objects (and what's inside them) from the blueprint.").clicked() {
                    st.delete_selected();
                    ui.close();
                }
                ui.separator();
                if ui.add_enabled_ui(editing, |ui| theme::menu_item(ui, "Select all", "Ctrl+A")).inner.tip("Selects every object in the blueprint.").clicked() {
                    st.select_all();
                    ui.close();
                }
            });
            crate::docs::menu(ui, "View", |ui| { crate::theme::fade_in(ui);
                ui.checkbox(&mut self.ws.show_assets, "Assets").tip("Shows or hides the Assets panel.");
                ui.checkbox(&mut self.ws.show_console, "Console").tip("Shows or hides the Console panel.");
                ui.separator();
                for t in ["Timeline", "Profiler"] {
                    if ui.button(t).clicked() {
                        self.ws.open_tool(t);
                        ui.close();
                    }
                }
                ui.separator();
                if theme::menu_item(ui, "Commands", "F1").tip("Every engine command for scripts, explained, with examples; and the events.").clicked() {
                    st.commands_open = true;
                    ui.close();
                }
                if theme::menu_item(ui, "Find in files", "Ctrl+F").tip("Searches every script and blueprint in the project.").clicked() {
                    st.search.open = true;
                    st.search.focus = true;
                    ui.close();
                }
            });
            // Play controls in the middle.
            ui.add_space(24.0);
            let playing = st.playing.is_some();
            if !playing {
                if theme::primary_button(ui, "▶  Play", egui::Color32::from_rgb(0x86, 0xb8, 0x22)).tip("Run this blueprint (F6). F5 runs the game from its main blueprint.").clicked() {
                    st.play();
                }
            } else if theme::primary_button(ui, "■  Stop", egui::Color32::from_rgb(0xc8, 0x1e, 0x5c)).tip("Stop the game (F5 or F6)").clicked() {
                st.stop();
            }
            crate::docs::mark(ui, egui::Rect::from_min_max(egui::pos2(ui.min_rect().right() - 80.0, ui.min_rect().top()), ui.min_rect().max), "ui:play");
            // Build: the game as players get it, in a window of its own.
            let building = st.poll_build();
            if building {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            }
            let b = theme::primary_button(ui, if building { "■  Build" } else { "▶  Build" }, egui::Color32::from_rgb(0xe6, 0xc2, 0x1e));
            crate::docs::mark(ui, b.rect, "ui:build");
            if b.on_hover_text(if building { "Close the built game" } else { "Build the game and run it in its own window, with the project's window and graphics settings. Nothing is kept when it closes." }).clicked() {
                if building {
                    st.build = None;
                } else {
                    st.build_and_run();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::window_buttons(ui);
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new(&st.project.config.name).strong().color(theme::ACCENT));
                if let Some(s) = &self.syncer {
                    let (word, color) = s.status();
                    let tip = match (&s.problem, s.syncing()) {
                        (Some(p), _) => format!("Sync with GitHub: {}", p),
                        (None, true) => "Sending what you saved and bringing in what you did on your other computers".into(),
                        _ => format!("Kept in sync through github.com/{}/{}. Click for more.", s.link.owner, s.link.repo),
                    };
                    if ui.add(egui::Label::new(RichText::new(word).small().color(color)).sense(egui::Sense::click())).tip(tip).clicked() {
                        st.show_settings = true;
                        self.settings.tab = GITHUB_TAB;
                    }
                }
                if crate::collab::people_bar(ui, st) {
                    self.collab_panel.open = true;
                }
                ui.add_space(8.0);
                // The tabs take what's left between Play and the project's name.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Max), |ui| {
                    egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                        ui.horizontal(|ui| crate::workspace::tab_strip(ui, &mut self.ws, st));
                    });
                });
            });
        });
        let full = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), bar.response.rect.y_range()).expand2(egui::vec2(8.0, 2.0));
        crate::docs::mark(ui, full, "ui:menubar");
        ui.painter().set(bar_bg, theme::header(full, false));
        if let Some(d) = switch {
            // What's changed is saved before the other project takes this one's place.
            if let Some(st) = &mut self.st {
                st.stop();
                st.save_all();
                self.ws.save(&st.project.dir);
            }
            self.open_project(&d);
            return;
        }
        if close {
            if let Some(st) = &mut self.st {
                st.save_all();
                self.ws.save(&st.project.dir);
            }
            self.stop_sync();
            self.st = None;
            // The project's windows go with it.
            self.collab_panel.open = false;
            self.github.project_changed();
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let Some(st) = &mut self.st else { return };
        let view: Option<egui::Id> = ctx.data(|d| d.get_temp(egui::Id::new("gex viewport id")));
        let focused = ctx.memory(|m| m.focused());
        // Typing in a text box (a script, a name, a number being typed): the keys are its. A
        // focused button or list row doesn't count (clicking one focuses it).
        let typing = ctx.text_edit_focused() && focused != view;
        let (f5, f6, shift, save, undo, redo, dup, del, colon) = ctx.input_mut(|i| {
            (
                i.key_pressed(egui::Key::F5),
                i.key_pressed(egui::Key::F6),
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
        let (copy, cut, all, rename, paste, close_blueprint, tool) = ctx.input_mut(|i| {
            let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            let cut = i.events.iter().any(|e| matches!(e, egui::Event::Cut));
            let all = i.modifiers.command && i.key_pressed(egui::Key::A);
            let rename = i.key_pressed(egui::Key::F2);
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
            (copy, cut, all, rename, paste, close, tool)
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
        // F5: the game from its start (the main blueprint); F6: the blueprint being edited.
        // Either stops a game that's running.
        if f5 || f6 {
            if shift || st.playing.is_some() {
                st.stop();
            } else {
                if f5 {
                    st.play_from_start();
                }
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
        // F1 anywhere (outside a script, where F1 on a command opens its page): the Command reference.
        if !typing && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1)) {
            st.commands_open = true;
        }
        crate::tools::search_window(ctx, st);
        if std::mem::take(&mut st.open_timeline) {
            self.ws.open_tool("Timeline");
        }
        if std::mem::take(&mut st.open_sprite) {
            self.ws.open_tool("Sprite");
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
            if copy || cut {
                if let Some(t) = st.copy_selected() {
                    ctx.copy_text(t);
                    if cut {
                        st.delete_selected();
                        st.status = "cut".into();
                    }
                }
            }
            if all {
                st.select_all();
            }
            if rename {
                st.renaming = st.selected;
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
        // No bar along the bottom: the command line shows only while it's being typed in,
        // and messages appear for a few seconds over the bottom left corner.
        if let Some(cmd) = &mut self.cmd {
            let bg = ui.painter().add(egui::Shape::Noop);
            let bar = egui::Panel::bottom("command line").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(":").monospace().color(theme::ACCENT));
                    let r = ui.add(egui::TextEdit::singleline(cmd).font(egui::TextStyle::Monospace).frame(egui::Frame::NONE).desired_width(f32::INFINITY));
                    r.request_focus();
                });
            });
            ui.painter().set(bg, theme::header(bar.response.rect, true));
            crate::docs::mark(ui, bar.response.rect, "ui:status");
            let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if esc {
                self.cmd = None;
            } else if enter {
                let c = self.cmd.take().unwrap_or_default();
                run_command(st, &c, &mut self.ws);
            }
        }
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let id = egui::Id::new("status toast");
        let (shown, since): (String, f64) = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
        let since = if shown != st.status { now } else { since };
        ctx.data_mut(|d| d.insert_temp(id, (st.status.clone(), since)));
        let age = now - since;
        if !st.status.is_empty() && age < 4.0 {
            let fade = ((4.0 - age) / 0.6).clamp(0.0, 1.0) as f32;
            let mut msg = st.status.clone();
            if let Some(c) = msg.chars().next() {
                msg.replace_range(..c.len_utf8(), &c.to_uppercase().to_string());
            }
            egui::Area::new(id).anchor(egui::Align2::LEFT_BOTTOM, [10.0, -10.0]).order(egui::Order::Foreground).interactable(false).show(&ctx, |ui| {
                ui.set_opacity(fade);
                egui::Frame::new().fill(egui::Color32::from_rgb(0x26, 0x26, 0x26)).stroke(theme::glass_edge()).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                    ui.label(RichText::new(msg).color(theme::TEXT));
                });
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(if age > 3.3 { 16 } else { 300 }));
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(st) = &mut self.st else { return };
        if !st.show_settings {
            self.settings.loaded = false;
            return;
        }
        let f = &mut self.settings;
        let (gh, syncer) = (&mut self.github, &mut self.syncer);
        if !f.loaded {
            f.loaded = true;
            f.before = Some(st.project.config.clone());
            f.name = st.project.config.name.clone();
            f.main_blueprint = st.project.config.main_blueprint.clone();
            f.message.clear();
            f.actions = st.project.config.actions.iter().map(|(k, v)| (k.clone(), v.join(", "))).collect();
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
        let mut cancel = false;
        egui::Window::new("Project settings").collapsible(false).open(&mut open).default_width(560.0).default_height(560.0).show(ctx, |ui| {
            theme::fade_in(ui);
            theme::tab_row(ui, &["General", "Display", "Graphics", "Physics", "Input", "Half-Life 2", "GitHub"], &mut f.tab);
            if f.tab == GITHUB_TAB {
                gh.project_page(ui, st, syncer);
                return;
            }
            if (1..=3).contains(&f.tab) {
                let (display, graphics) = (f.tab == 1, f.tab == 2);
                ui.label(
                    RichText::new(if display {
                        "The game window: its resolution and how the picture fills the window or screen. Save keeps changes. Scripts change them while playing with SET_DISPLAY(\"name\", value)."
                    } else if graphics {
                        "How the game looks. Changes show in the view at once; Save keeps them. Scripts change them while playing with SET_GRAPHICS(\"name\", value)."
                    } else {
                        "How physics works. Save keeps changes. Scripts change them while playing with SET_PHYSICS(\"name\", value)."
                    })
                    .small()
                    .color(theme::TEXT_DIM),
                );
                if display {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Resolution").small().color(theme::TEXT_DIM));
                        let d = &mut st.project.config.display;
                        for (w, h) in [(1920, 1080), (1280, 720), (640, 360), (320, 180), (1024, 768)] {
                            if ui.selectable_label(d.width == w && d.height == h, format!("{}×{}", w, h)).clicked() {
                                d.width = w;
                                d.height = h;
                            }
                        }
                    });
                }
                egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                    if display {
                        let cfg = &mut st.project.config.display;
                        let changes = settings_rows(ui, gex_core::settings::Display::FIELDS, &|k| cfg.get(k));
                        for (k, v) in changes {
                            let _ = cfg.set(&k, &v);
                        }
                    } else if graphics {
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
                    if ui.button("Reset to defaults").tip("Every setting on this page (Save keeps it, Cancel takes it back)").clicked() {
                        if display {
                            st.project.config.display = Default::default();
                        } else if graphics {
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
                ui.label("Icon");
                let r = ui.add(egui::TextEdit::singleline(&mut f.icon).hint_text("drop a .png here").desired_width(300.0));
                if let Some(p) = r.dnd_release_payload::<panels::AssetPath>() {
                    f.icon = p.0.clone();
                }
                ui.end_row();
            });
            }
            if f.tab == 4 {
            theme::title(ui, "Input actions");
            ui.label(
                RichText::new("Name an action once and give it any keys: then KEY_DOWN(\"jump\") and on_press_jump() work with all of them. Keys: a–z, 0–9, space, enter, escape, shift, left, right, up, down, mouse_left, mouse_right, pad_a, pad_b, pad_x, pad_y, pad_left, pad_start… Scripts rebind keys while playing with SET_ACTION(\"jump\", [\"space\", \"w\"]).")
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
            if f.tab == 5 {
            theme::title(ui, "Half-Life 2 assets");
            ui.label(
                RichText::new("Reads textures, materials and other files straight from your own Steam install (hl2://…). Mounting it here mounts it for every project on this computer (and unmounting unmounts it everywhere). Nothing is copied into the project, and exported games never include these files.")
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
            let mut save = false;
            ui.horizontal(|ui| {
                save = theme::primary_button(ui, "Save", theme::ACCENT_DIM).tip("Keeps the changes on every page").clicked();
                if ui.button("Cancel").tip("Puts every setting back as it was when this window opened").clicked() {
                    if let Some(b) = f.before.clone() {
                        st.project.config = b;
                    }
                    cancel = true;
                }
            });
            if save {
                st.project.config.name = f.name.clone();
                st.project.config.main_blueprint = f.main_blueprint.clone();
                st.project.config.icon = f.icon.trim().to_string();
                st.project.config.actions = f
                    .actions
                    .iter()
                    .map(|(n, k)| (n.trim().to_lowercase().replace(' ', "_"), k.split([',', ' ']).map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect::<Vec<_>>()))
                    .filter(|(n, k)| !n.is_empty() && !k.is_empty())
                    .collect();
                let before: Vec<MountConfig> = st.project.config.mounts.iter().filter(|m| m.scheme == "hl2").cloned().collect();
                st.project.config.mounts.retain(|m| m.scheme != "hl2");
                let mut hl2 = None;
                if f.hl2_on && !f.hl2_path.trim().is_empty() {
                    let games = f.hl2_games.split(',').map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect();
                    let m = MountConfig { scheme: "hl2".into(), kind: MountKind::SourceGame { install: PathBuf::from(f.hl2_path.trim()), games } };
                    st.project.config.mounts.push(m.clone());
                    hl2 = Some(m);
                }
                // Mounting (or unmounting) Half-Life 2 counts for every project on this computer.
                if before.first() != hl2.as_ref() || state::shared_hl2().is_none() && hl2.is_some() {
                    state::set_shared_hl2(hl2.as_ref());
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
                        f.message = if f.hl2_on && f.tab == 5 { format!("Mounted: {} files in archives. Browse them in Assets → hl2://", files) } else { "Saved.".into() };
                    }
                    Err(e) => f.message = e,
                }
            }
        });
        if !open || cancel {
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

fn run_command(st: &mut EditorState, c: &str, ws: &mut crate::workspace::Layout) {
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
        "layout" => {
            // The panels back as they start: Assets and the Console shown, tool windows closed.
            ws.show_assets = true;
            ws.show_console = true;
            ws.console_min = false;
            ws.tools.clear();
        }
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

impl EditorApp {
    /// The window's icon turns to the error one when a game (played or built) reports an
    /// error, until the console is cleared or closed.
    fn error_icon(&mut self, ctx: &egui::Context) {
        let errors = self.st.as_ref().map_or(0, |st| st.logs.iter().filter(|l| l.kind == gex_script::LogKind::Error).count());
        if errors < self.errors_seen || !self.ws.show_console || self.ws.console_min {
            self.errors_seen = errors;
        }
        let want = errors > self.errors_seen;
        if want != self.error_icon {
            self.error_icon = want;
            let icon = crate::png_icon(if want { crate::ERROR_ICON } else { crate::ICON });
            ctx.send_viewport_cmd(egui::ViewportCommand::Icon(icon.map(std::sync::Arc::new)));
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        crate::startup::mark_running();
        let ctx = ui.ctx().clone();
        theme::backdrop(&ctx);
        theme::window_resize(&ctx);
        self.update_bar(ui);
        self.collab(&ctx);
        self.github(&ctx);
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
        self.error_icon(&ctx);
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
        }
        self.menu(ui);
        if self.st.is_none() {
            return;
        }
        self.command_line(ui);
        self.settings_window(&ctx);
        self.export_window(&ctx);
        let rs = frame.wgpu_render_state();
        let st = self.st.as_mut().unwrap();
        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(4.0)).show(ui, |ui| {
            crate::workspace::show(ui, &mut self.ws, st, rs, &mut self.viewport_tex, &mut self.previews);
        });
        if self.ws_saved_at.elapsed().as_secs_f32() > 2.0 {
            self.ws.save(&st.project.dir);
            self.ws_saved_at = std::time::Instant::now();
        }
    }

    fn on_exit(&mut self) {
        if let Some(st) = &mut self.st {
            self.ws.save(&st.project.dir);
            st.stop();
            // Synced with GitHub: the last changes go up.
            if let Some(s) = self.syncer.take() {
                s.finish(std::time::Duration::from_secs(8));
            }
            // Working together: others are told it ended (a moment for that to go out).
            if st.collab.take().is_some() {
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
    }
}

// ---------------------------------------------------------------- player

/// Just the game, full-window (`gex --play <project>`).
pub struct PlayerApp {
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
    /// The window settings last asked of the OS (None until the first frame).
    applied: Option<gex_core::settings::Display>,
    applied_title: Option<String>,
    /// Full screen as the window last reported it.
    last_real_full: Option<bool>,
    /// The player switched full screen: hold it until the game's frames agree...
    pending_full: Option<bool>,
    /// ...and tell the game in the next frame's input.
    report_full: Option<bool>,
}

impl PlayerApp {
    pub fn new(project: Project) -> Result<PlayerApp, String> {
        gex_script::commands::register();
        gex_script::world::STANDALONE.store(true, std::sync::atomic::Ordering::Relaxed);
        let text = project.read_text(&project.config.main_blueprint)?;
        let tree = gex_core::blueprint::load(&text, &|p: &str| project.read_text(p)).map_err(|e| e.0)?;
        let vfs = gex_assets::Vfs::new(&project.dir, &project.config);
        let mut list = gex_core::render::extract(&tree);
        list.graphics = project.config.graphics.clone();
        list.display = project.config.display.clone();
        Ok(PlayerApp { handle: { let main = project.config.main_blueprint.clone(); gex_script::thread::GameHandle::start(project, tree, &main) }, vfs, list, renderer: None, tex: None, last: std::time::Instant::now(), ended: false, ui_events: Vec::new(), ui_images: HashMap::new(), pads: crate::gamepad::Pads::new(), taps: Default::default(), tap_buttons: Default::default(), mouse_delta: [0.0; 2], cursor_locked: false, applied: None, applied_title: None, last_real_full: None, pending_full: None, report_full: None })
    }
}

impl PlayerApp {
    /// Asks the OS for the window the display settings describe: on the first frame, and
    /// whenever the game changes them.
    fn apply_display(&mut self, ctx: &egui::Context) {
        use egui::ViewportCommand as V;
        // Frames the game made before it heard about the player's own switch still have the old
        // value: keep the player's choice until the game's frames agree.
        if let Some(v) = self.pending_full {
            if self.list.display.fullscreen == v {
                self.pending_full = None;
            } else {
                self.list.display.fullscreen = v;
            }
        }
        let d = self.list.display.clone();
        let old = self.applied.clone();
        if old.as_ref() == Some(&d) {
            return;
        }
        let first = old.is_none();
        if first || old.as_ref().is_some_and(|o| o.fullscreen != d.fullscreen) {
            ctx.send_viewport_cmd(V::Fullscreen(d.fullscreen));
        }
        if first || old.as_ref().is_some_and(|o| o.resizable != d.resizable) {
            ctx.send_viewport_cmd(V::Resizable(d.resizable));
        }
        let resized = old.as_ref().is_some_and(|o| o.width != d.width || o.height != d.height || o.window_scale != d.window_scale || (o.fullscreen && !d.fullscreen));
        if !d.fullscreen && (first || resized) {
            window_size(ctx, &d);
        }
        self.applied = Some(d);
    }

    /// The player switched full screen (F11, Alt+Enter, or the OS did): `send` asks the
    /// window for it; either way the game is told.
    fn player_fullscreen(&mut self, ctx: &egui::Context, v: bool, send: bool) {
        if send {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(v));
        }
        if !v {
            window_size(ctx, &self.list.display);
        }
        if let Some(a) = &mut self.applied {
            a.fullscreen = v;
        }
        self.list.display.fullscreen = v;
        self.pending_full = Some(v);
        self.report_full = Some(v);
    }
}

/// Sizes the game's window (not full screen): the resolution times window_scale in real
/// pixels, kept inside the screen and centred on it.
fn window_size(ctx: &egui::Context, d: &gex_core::settings::Display) {
    use egui::ViewportCommand as V;
    let ppp = ctx.pixels_per_point();
    let mut size = egui::vec2(d.width as f32, d.height as f32) * d.window_scale / ppp;
    match ctx.input(|i| i.viewport().monitor_size) {
        Some(m) => {
            size *= (m.x * 0.92 / size.x).min(m.y * 0.88 / size.y).min(1.0);
            ctx.send_viewport_cmd(V::InnerSize(size));
            ctx.send_viewport_cmd(V::OuterPosition(((m - size) / 2.0).to_pos2().max(egui::Pos2::ZERO)));
        }
        None => ctx.send_viewport_cmd(V::InnerSize(size)),
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
                    print_logs(logs);
                }
                gex_script::thread::FromGame::Stopped { logs } => {
                    print_logs(logs);
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
        // Display settings from the game (SET_DISPLAY, FULLSCREEN) and WINDOW_TITLE; Alt+Enter or F11 toggles full screen too.
        let real = ctx.input(|i| i.viewport().fullscreen);
        if ctx.input(|i| (i.modifiers.alt && i.key_pressed(egui::Key::Enter)) || i.key_pressed(egui::Key::F11)) {
            let now = real.unwrap_or(self.list.display.fullscreen);
            self.player_fullscreen(&ctx, !now, true);
        } else if let (Some(r), Some(last)) = (real, self.last_real_full) {
            // Switched some other way (by the OS): the game should know too.
            if r != last && self.pending_full.is_none() && self.applied.as_ref().is_some_and(|a| a.fullscreen != r) {
                self.player_fullscreen(&ctx, r, false);
            }
        }
        self.last_real_full = real;
        self.apply_display(&ctx);
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
            // The game is drawn at its resolution and scaled into the window (scale_mode).
            let d = &self.list.display;
            let design = d.size();
            let gv = gex_render::GameView::new(&d.scale_mode, px, design);
            let view = egui::Rect::from_min_size(rect.min + egui::vec2(gv.rect[0], gv.rect[1]) / ppp, egui::vec2(gv.rect[2], gv.rect[3]) / ppp);
            let cams = gex_render::Cameras::for_game(&self.list, gv.render, design);
            let m = ctx.input(|i| i.pointer.hover_pos()).map(|p| cams.screen_to_2d(gv.to_render(gex_core::glam::Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp)))).unwrap_or_default();
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
                    fullscreen: self.report_full.take(),
                    profile: false,
                });
                self.last = now;
            }
            // The game draws its own pointer (SET_CURSOR), or hides it.
            if self.list.hide_cursor && ui.rect_contains_pointer(view) {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            }
            if view != rect {
                ui.painter().rect_filled(rect, 0.0, viewport::bar_color(d));
            }
            let nearest = d.scale_filter == "pixel" && d.scale_mode != "expand";
            let probs = viewport::paint_list(ui, rs, &mut self.renderer, &mut self.tex, &self.vfs, view, &self.list, &cams, &gex_render::Overlay::default(), nearest);
            for p in probs {
                eprintln!("ERROR: {}", p);
            }
            let design = gex_core::glam::Vec2::new(design[0] as f32, design[1] as f32);
            let (map, scale) = viewport::ui_mapping(&self.list.display, design, view);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child.set_clip_rect(view);
            let ev = crate::ui_layer::paint_ui(&mut child, &self.list.ui, design, &map, scale, true, &mut self.ui_images, &self.vfs);
            self.ui_events.extend(ev);
            viewport::paint_fade(ui, rect, self.list.fade);
        });
        repaint_capped(&ctx, self.list.graphics.fps_cap, frame_start);
    }
}

/// What the game printed, on stderr: the editor's Build reads it back into its console
/// (errors marked so they show as errors there).
fn print_logs(logs: Vec<gex_script::LogLine>) {
    for l in logs {
        match l.kind {
            gex_script::LogKind::Error => eprintln!("ERROR: {}", l.text),
            _ => eprintln!("{}", l.text),
        }
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
                        (Variant::Res(cur), "file") => {
                            let r = ui.add(egui::TextEdit::singleline(cur).hint_text("res://shaders/...wgsl").desired_width(220.0));
                            r.changed()
                        }
                        (Variant::Num(n), "whole") => {
                            let mut k = *n as i64;
                            // Wide ranges (resolution, shadow map size) are typed or dragged, not slid.
                            let r = if f.max - f.min > 2000.0 {
                                ui.add(egui::DragValue::new(&mut k).range(f.min as i64..=f.max as i64).speed(1.0))
                            } else {
                                ui.add(egui::Slider::new(&mut k, f.min as i64..=f.max as i64))
                            };
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
