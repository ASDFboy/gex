//! The first screen: your projects (open, rename, delete) and making a new one.

use crate::docs::Tip as _;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};
use gex_core::project::{Project, PROJECT_FILE};

use crate::{state, templates, theme};

struct Entry {
    dir: PathBuf,
    /// The project's name, or why its file can't be read.
    name: Result<String, String>,
    /// When it was last worked on.
    used: Option<std::time::SystemTime>,
    /// The GitHub repository it's kept in ("owner/repo", lowercase).
    repo: Option<String>,
}

#[derive(Clone)]
enum Dialog {
    Rename { dir: PathBuf, name: String, folder_too: bool },
    Delete { dir: PathBuf, name: String },
}

pub struct Launcher {
    entries: Option<Vec<Entry>>,
    filter: String,
    name: String,
    /// The folder new projects are made in (each in its own folder inside).
    pub location: String,
    /// Keep the new project on GitHub (when signed in).
    on_github: Option<bool>,
    #[cfg_attr(windows, allow(dead_code))]
    open_path: String,
    dialog: Option<Dialog>,
    focus_name: bool,
    pub error: String,
    /// "Join someone" was clicked.
    pub join: bool,
    /// "Settings" was clicked.
    pub settings: bool,
    /// A project made to be kept on GitHub: (folder, name), to link once it's open.
    pub link: Option<(PathBuf, String)>,
}

impl Default for Launcher {
    fn default() -> Self {
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
        // Where the last project was made, or a "GEX Projects" folder.
        let location = std::fs::read_to_string(state::config_dir().join("projects_dir.txt")).ok().map(|s| s.trim().to_string()).filter(|s| Path::new(s).is_dir()).unwrap_or_else(|| home.join("GEX Projects").to_string_lossy().into_owned());
        Launcher { entries: None, filter: String::new(), name: "My Game".into(), location, on_github: None, open_path: String::new(), dialog: None, focus_name: true, error: String::new(), join: false, settings: false, link: None }
    }
}

/// A folder name for a project name: "My Game!" → "my-game".
fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() {
            s.extend(c.to_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "project".into() } else { s }
}

/// "today", "3 days ago", or the date.
fn ago(t: std::time::SystemTime) -> String {
    let days = t.elapsed().map(|d| d.as_secs() / 86400).unwrap_or(0);
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=13 => format!("{} days ago", days),
        14..=59 => format!("{} weeks ago", days / 7),
        60..=729 => format!("{} months ago", days / 30),
        _ => format!("{} years ago", days / 365),
    }
}

fn load() -> Vec<Entry> {
    let mut v: Vec<Entry> = state::recent_projects()
        .into_iter()
        .map(|dir| {
            let used = [dir.join(".gex").join("layout.ron"), dir.join(PROJECT_FILE)].iter().filter_map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok()).max();
            let repo = crate::github::link_of(&dir).map(|l| format!("{}/{}", l.owner, l.repo).to_lowercase());
            Entry { name: Project::peek(&dir).map(|c| c.name), dir, used, repo }
        })
        .collect();
    // Last worked on first (the date shown beside each), so the list reads in order; projects
    // with no date keep their place after the rest.
    v.sort_by(|a, b| b.used.cmp(&a.used));
    v
}

/// A square with the project's first letter, colored by its name.
fn tile(ui: &mut egui::Ui, name: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let colors = [theme::MK_CYAN, theme::MK_GREEN, theme::MK_ORANGE, theme::MK_PINK, theme::MK_PURPLE, theme::MK_YELLOW];
    let c = colors[name.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % colors.len()];
    ui.painter().rect_filled(rect, 0.0, theme::mix(c, Color32::BLACK, 0.55));
    ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, size)), 0.0, c);
    let letter = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    ui.painter().text(rect.center() + egui::vec2(1.0, 0.0), egui::Align2::CENTER_CENTER, letter, egui::FontId::proportional(size * 0.5), Color32::WHITE);
}

impl Launcher {
    /// Reads the project list again next time it shows.
    pub fn refresh(&mut self) {
        self.entries = None;
    }

    /// The folder a new project with the typed name would get (never one that's taken).
    fn new_dir(&self) -> PathBuf {
        let base = Path::new(&self.location).join(slug(&self.name));
        (1..1000).map(|i| if i == 1 { base.clone() } else { PathBuf::from(format!("{}-{}", base.display(), i)) }).find(|p| !p.exists()).unwrap_or(base)
    }

    fn create(&mut self, on_github: bool) -> Option<PathBuf> {
        let dir = self.new_dir();
        let name = self.name.trim().to_string();
        match templates::create_project(&dir, &name) {
            Ok(p) => {
                if on_github {
                    self.link = Some((p.dir.clone(), name));
                }
                let _ = std::fs::create_dir_all(state::config_dir());
                let _ = std::fs::write(state::config_dir().join("projects_dir.txt"), &self.location);
                Some(p.dir)
            }
            Err(e) => {
                self.error = e;
                None
            }
        }
    }

    /// Draws the screen. Gives a project folder to open.
    pub fn show(&mut self, ui: &mut egui::Ui, gh: &mut crate::github::Panel) -> Option<PathBuf> {
        let mut open: Option<PathBuf> = None;
        gh.keep_listing(ui.ctx());
        let mut get: Option<crate::github::Repo> = None;
        // A project folder dropped on the window opens.
        if let Some(p) = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| !p.as_os_str().is_empty())) {
            let dir = if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
            if dir.join(PROJECT_FILE).is_file() {
                open = Some(dir);
            } else {
                self.error = format!("{} isn't a GEX project (there's no {} in it)", dir.display(), PROJECT_FILE);
            }
        }
        let entries = self.entries.take().unwrap_or_else(load);
        let act: std::cell::Cell<Option<(usize, &'static str)>> = std::cell::Cell::new(None);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            // One column of readable width in the middle, however wide the window is.
            let full = ui.available_rect_before_wrap();
            let w = full.width().min(1040.0) - 48.0;
            let area = egui::Rect::from_min_size(egui::pos2(full.center().x - w / 2.0, full.top() + (full.height() * 0.07).min(60.0)), egui::vec2(w, full.height() - 80.0));
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
            let ui = &mut ui;
            ui.horizontal(|ui| {
                // The logo is pixel art: drawn at a whole multiple of its size, unblurred.
                let logo = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(egui::Id::new("gex logo"))).unwrap_or_else(|| {
                    let img = image::load_from_memory(crate::LOGO).map(|i| i.to_rgba8()).unwrap_or_default();
                    let c = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                    let t = ui.ctx().load_texture("gex logo", c, egui::TextureOptions::NEAREST);
                    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("gex logo"), t.clone()));
                    t
                });
                ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::vec2(64.0, 64.0)));
                ui.add_space(4.0);
                ui.label(RichText::new("GEX").size(34.0).strong().color(theme::ACCENT));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new("2D and 3D game engine, scripted in JLB").color(theme::TEXT_DIM));
                    ui.label(RichText::new(format!("version {}", env!("CARGO_PKG_VERSION"))).small().color(theme::TEXT_DIM));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Settings").tip("GEX's own settings, for every project: your GitHub sign-in, and updates").clicked() {
                        self.settings = true;
                    }
                    if let Some(a) = &gh.account {
                        ui.label(RichText::new(format!("GitHub: {}", a.login)).small().color(theme::TEXT_DIM));
                    }
                });
            });
            ui.add_space(22.0);
            let side = 330.0;
            let list_w = ui.available_width() - side - 28.0;
            // Projects on GitHub that aren't on this computer yet.
            let remote: Vec<crate::github::Repo> = gh.remote.iter().filter(|r| {
                let key = format!("{}/{}", r.owner, r.name).to_lowercase();
                !entries.iter().any(|e| e.repo.as_deref() == Some(key.as_str()))
            }).cloned().collect();
            ui.horizontal_top(|ui| {
                // ---- your projects
                ui.vertical(|ui| {
                    ui.set_width(list_w);
                    ui.horizontal(|ui| {
                        theme::title(ui, &format!("Projects ({})", entries.len() + remote.len()));
                        if gh.listing() {
                            ui.spinner().tip("Looking for your projects on GitHub");
                        }
                        if entries.len() > 4 {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search").desired_width(160.0));
                            });
                        }
                    });
                    ui.add_space(6.0);
                    if entries.is_empty() && remote.is_empty() {
                        egui::Frame::new().fill(theme::glass_fill()).stroke(theme::glass_edge()).inner_margin(18.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label("No projects yet.");
                            ui.label(RichText::new(if gh.account.is_some() { "Make one on the right. Projects you keep on GitHub show up here on every computer." } else { "Make one on the right. Signed in to GitHub (Settings), projects you keep there show up here on every computer." }).color(theme::TEXT_DIM));
                        });
                    }
                    let needle = self.filter.trim().to_lowercase();
                    egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 8.0).show(ui, |ui| {
                        for (i, e) in entries.iter().enumerate() {
                            let name = e.name.clone().unwrap_or_else(|_| e.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                            if !needle.is_empty() && !name.to_lowercase().contains(&needle) && !e.dir.to_string_lossy().to_lowercase().contains(&needle) {
                                continue;
                            }
                            let (rect, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::click());
                            let hot = ui.ctx().animate_bool_with_time(row.id, row.hovered(), 0.08);
                            ui.painter().rect(rect, 0.0, theme::mix(theme::glass_fill(), Color32::from_rgb(0x2c, 0x2c, 0x2c), hot), theme::glass_edge(), egui::StrokeKind::Inside);
                            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(10.0, 10.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                            tile(&mut inner, &name, 36.0);
                            inner.add_space(6.0);
                            inner.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                match &e.name {
                                    Ok(_) => ui.label(RichText::new(&name).size(15.0).strong()),
                                    Err(why) => ui.label(RichText::new(format!("{} (can't be read)", name)).size(15.0).color(theme::WARN)).tip(why),
                                };
                                ui.add(egui::Label::new(RichText::new(e.dir.to_string_lossy()).small().color(theme::TEXT_DIM)).truncate());
                            });
                            inner.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let (mr, more) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                                if more.hovered() {
                                    ui.painter().rect_filled(mr, 0.0, theme::GRAY_LIGHT);
                                }
                                for dx in [-5.0, 0.0, 5.0] {
                                    ui.painter().circle_filled(mr.center() + egui::vec2(dx, 0.0), 1.5, if more.hovered() { theme::TEXT } else { theme::TEXT_DIM });
                                }
                                let more = more.tip("Rename, delete, more");
                                let menu = |ui: &mut egui::Ui| {
                                    theme::fade_in(ui);
                                    for (label, what) in [("Open", "open"), ("Rename…", "rename"), ("Show in file manager", "reveal"), ("Remove from this list", "forget")] {
                                        if ui.button(label).clicked() {
                                            act.set(Some((i, what)));
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    if ui.button(RichText::new("Delete…").color(theme::BAD)).clicked() {
                                        act.set(Some((i, "delete")));
                                        ui.close();
                                    }
                                };
                                egui::Popup::menu(&more).show(menu);
                                row.context_menu(menu);
                                if let Some(t) = e.used {
                                    ui.label(RichText::new(ago(t)).small().color(theme::TEXT_DIM));
                                }
                                if let Some(r) = &e.repo {
                                    ui.label(RichText::new("GitHub").small().color(theme::ACCENT)).tip(format!("Kept on GitHub (github.com/{}): saves to and loads from there by itself", r));
                                }
                                if row.clicked() && !more.contains_pointer() {
                                    act.set(Some((i, "open")));
                                }
                            });
                            row.on_hover_cursor(egui::CursorIcon::PointingHand);
                            ui.add_space(4.0);
                        }
                        for r in &remote {
                            if !needle.is_empty() && !r.name.to_lowercase().contains(&needle) {
                                continue;
                            }
                            let key = format!("{}/{}", r.owner, r.name);
                            let busy = gh.getting.as_deref() == Some(key.as_str());
                            let (rect, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), egui::Sense::click());
                            let hot = ui.ctx().animate_bool_with_time(row.id, row.hovered(), 0.08);
                            ui.painter().rect(rect, 0.0, theme::mix(theme::glass_fill(), Color32::from_rgb(0x2c, 0x2c, 0x2c), hot), egui::Stroke::new(1.0, theme::alpha(theme::ACCENT_DIM, 120)), egui::StrokeKind::Inside);
                            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(10.0, 10.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                            tile(&mut inner, &r.name, 36.0);
                            inner.add_space(6.0);
                            inner.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(&r.name).size(15.0).strong());
                                ui.add(egui::Label::new(RichText::new(format!("On GitHub ({}), not on this computer yet: click to get it", key)).small().color(theme::TEXT_DIM)).truncate());
                            });
                            inner.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if busy {
                                    ui.spinner();
                                    ui.label(RichText::new("Getting…").small().color(theme::TEXT_DIM));
                                } else {
                                    ui.label(RichText::new("GitHub").small().color(theme::ACCENT));
                                    if !r.pushed.is_empty() {
                                        ui.label(RichText::new(format!("changed {}", r.pushed)).small().color(theme::TEXT_DIM));
                                    }
                                }
                            });
                            if row.clicked() && !gh.busy() {
                                get = Some(r.clone());
                            }
                            row.on_hover_cursor(egui::CursorIcon::PointingHand).tip(format!("Gets it into {} and opens it; from then on it saves to and loads from GitHub by itself", Path::new(&self.location).display()));
                            ui.add_space(4.0);
                        }
                    });
                    if !gh.remote_error.is_empty() && gh.account.is_some() {
                        ui.label(RichText::new(format!("Couldn't look for your projects on GitHub: {}", gh.remote_error)).small().color(theme::WARN));
                    }
                });
                ui.add_space(28.0);
                // ---- a new one, or one from elsewhere
                ui.vertical(|ui| {
                    ui.set_width(side);
                    theme::title(ui, "New project");
                    ui.add_space(6.0);
                    egui::Frame::new().fill(theme::glass_fill()).stroke(theme::glass_edge()).inner_margin(14.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Name").small().color(theme::TEXT_DIM));
                        let r = ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY));
                        if std::mem::take(&mut self.focus_name) {
                            r.request_focus();
                        }
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.add_space(6.0);
                        ui.label(RichText::new("Made in").small().color(theme::TEXT_DIM));
                        ui.horizontal(|ui| {
                            let browse = cfg!(windows);
                            ui.add(egui::TextEdit::singleline(&mut self.location).desired_width(ui.available_width() - if browse { 74.0 } else { 0.0 }));
                            #[cfg(windows)]
                            if ui.button("Browse…").clicked() {
                                if let Some(p) = rfd::FileDialog::new().set_directory(&self.location).pick_folder() {
                                    self.location = p.to_string_lossy().into_owned();
                                }
                            }
                        });
                        let dir = self.new_dir();
                        ui.add(egui::Label::new(RichText::new(format!("Folder: {}", dir.display())).small().color(theme::TEXT_DIM)).truncate()).tip(dir.to_string_lossy());
                        ui.add_space(8.0);
                        let mut keep = false;
                        if let Some(a) = &gh.account {
                            let on = self.on_github.get_or_insert(a.new_projects);
                            ui.checkbox(on, "Keep it on GitHub").tip(format!("In a private repository on {}'s GitHub: it saves to and loads from there by itself, and shows up on the start screen of your other computers", a.login));
                            keep = *on;
                        } else {
                            ui.label(RichText::new("Sign in to GitHub in Settings to keep projects there (the same on all your computers).").small().color(theme::TEXT_DIM));
                        }
                        ui.add_space(10.0);
                        let ok = !self.name.trim().is_empty() && !self.location.trim().is_empty();
                        let go = ui.add_enabled_ui(ok, |ui| theme::primary_button(ui, "Create project", theme::ACCENT_DIM)).inner.clicked();
                        if ok && (go || enter) {
                            open = self.create(keep);
                        }
                    });
                    ui.add_space(18.0);
                    theme::title(ui, "Open a project from disk");
                    ui.add_space(6.0);
                    #[cfg(windows)]
                    if ui.button("Choose a folder…").tip(format!("The folder with {} in it", PROJECT_FILE)).clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            open = Some(p);
                        }
                    }
                    #[cfg(not(windows))]
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut self.open_path).hint_text("project folder").desired_width(ui.available_width() - 60.0));
                        if ui.button("Open").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            open = Some(PathBuf::from(self.open_path.trim()));
                        }
                    });
                    ui.label(RichText::new("Or drop a project folder on this window.").small().color(theme::TEXT_DIM));
                    ui.add_space(18.0);
                    theme::title(ui, "Work on someone's project");
                    ui.add_space(6.0);
                    if ui.button("Join someone…").tip("Someone shares their project (File → Work together); join it with their address and invite code to work on it together, live.").clicked() {
                        self.join = true;
                    }
                    for e in [&self.error, &gh.error] {
                        if !e.is_empty() {
                            ui.add_space(10.0);
                            ui.label(RichText::new(e.as_str()).color(theme::BAD));
                        }
                    }
                });
            });
        });
        if let Some(r) = get {
            if self.location.trim().is_empty() {
                self.error = "Choose where projects go first (\"Made in\", under New project).".into();
            } else {
                self.error.clear();
                gh.get(ui.ctx(), r, Path::new(self.location.trim()));
            }
        }
        let mut reload = false;
        if let Some((i, what)) = act.get() {
            let e = &entries[i];
            let name = e.name.clone().unwrap_or_default();
            match what {
                "open" => open = Some(e.dir.clone()),
                "rename" => self.dialog = Some(Dialog::Rename { dir: e.dir.clone(), name, folder_too: false }),
                "delete" => self.dialog = Some(Dialog::Delete { dir: e.dir.clone(), name }),
                "reveal" => crate::files::reveal(&e.dir),
                _ => {
                    state::forget_project(&e.dir);
                    reload = true;
                }
            }
        }
        self.entries = (!reload).then_some(entries);
        self.dialogs(ui.ctx());
        if open.is_some() {
            self.entries = None;
            self.error.clear();
        }
        open
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialog.clone() else { return };
        let mut keep = true;
        let mut done = false;
        let title = match &d {
            Dialog::Rename { .. } => "Rename project",
            Dialog::Delete { .. } => "Delete project",
        };
        egui::Window::new(title).collapsible(false).resizable(false).open(&mut keep).anchor(egui::Align2::CENTER_CENTER, [0.0, -40.0]).show(ctx, |ui| {
            theme::fade_in(ui);
            ui.set_width(380.0);
            match &mut d {
                Dialog::Rename { dir, name, folder_too } => {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    r.request_focus();
                    ui.checkbox(folder_too, "Rename its folder to match");
                    if *folder_too {
                        ui.label(RichText::new(format!("{} → {}", dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), slug(name))).small().color(theme::TEXT_DIM));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let ok = !name.trim().is_empty();
                        let go = ui.add_enabled_ui(ok, |ui| theme::primary_button(ui, "Rename", theme::ACCENT_DIM)).inner.clicked();
                        if ok && (go || ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            match rename(dir, name.trim(), *folder_too) {
                                Ok(()) => done = true,
                                Err(e) => self.error = e,
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
                Dialog::Delete { dir, name } => {
                    ui.label(RichText::new(if name.is_empty() { "This project".to_string() } else { name.clone() }).strong());
                    ui.label(RichText::new(dir.to_string_lossy()).small().color(theme::TEXT_DIM));
                    ui.add_space(4.0);
                    ui.label("The whole folder goes to the Recycle Bin: every blueprint, script and asset in it. You can restore it from there.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if theme::primary_button(ui, "Move to Recycle Bin", Color32::from_rgb(0xc8, 0x1e, 0x5c)).clicked() {
                            match trash::delete(&*dir) {
                                Ok(()) => {
                                    state::forget_project(dir);
                                    done = true;
                                }
                                Err(e) => self.error = format!("couldn't delete {}: {}", dir.display(), e),
                            }
                        }
                        if ui.button("Only remove from the list").tip("Leaves the folder alone").clicked() {
                            state::forget_project(dir);
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                }
            }
        });
        if done || !keep || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dialog = None;
            self.entries = None;
        } else {
            self.dialog = Some(d);
        }
    }
}

/// Gives a project a new name (in its project file), and optionally its folder too.
fn rename(dir: &Path, name: &str, folder_too: bool) -> Result<(), String> {
    let mut p = Project { dir: dir.to_path_buf(), config: Project::peek(dir)? };
    p.config.name = name.to_string();
    p.save()?;
    if folder_too {
        let to = dir.with_file_name(slug(name));
        if to != dir {
            if to.exists() {
                return Err(format!("{} already exists, so the folder kept its name", to.display()));
            }
            std::fs::rename(dir, &to).map_err(|e| format!("the name changed, but the folder couldn't be renamed: {}", e))?;
            state::replace_project(dir, &to);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn folder_names_and_renaming() {
        assert_eq!(super::slug("  My Game! 2 "), "my-game-2");
        assert_eq!(super::slug("???"), "project");
        let dir = std::env::temp_dir().join(format!("gex-launcher-{}", std::process::id())).join("old-name");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
        crate::templates::create_project(&dir, "Old").unwrap();
        super::rename(&dir, "New Name", true).unwrap();
        let moved = dir.with_file_name("new-name");
        assert_eq!(gex_core::project::Project::peek(&moved).unwrap().name, "New Name");
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
