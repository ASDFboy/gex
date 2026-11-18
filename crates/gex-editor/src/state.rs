//! Everything the editor knows: the open project, the blueprint being edited, selection,
//! undo history, open scripts, the running game.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use gex_assets::Vfs;
use gex_core::project::Project;
use gex_core::render::RenderList;
use gex_core::{blueprint, ObjectId, BlueprintTree};
use gex_render::{EditorCamera2D, EditorCamera3D, Renderer};
use gex_script::thread::{FromGame, GameHandle};
use gex_script::{LogKind, LogLine};

pub struct Script {
    pub res: String,
    pub text: String,
    pub saved: String,
    /// Syntax problems from the last check: (line, message).
    pub problems: Vec<(usize, String)>,
    pub checked_text: String,
    /// A line to scroll to and put the cursor on next frame.
    pub goto_line: Option<usize>,
    pub vim: crate::vim::Vim,
    /// The find/replace bar, when open: (find, replace).
    pub find: Option<(String, String)>,
}

impl Script {
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
}

/// A file change made in the editor that can be undone.
pub enum FileOp {
    Renamed { old: String, new: String },
    /// Moved to the project's .trash folder.
    /// `fixed`: blueprint files that used it and were changed, as they were before.
    /// `open`: open blueprints it was taken out of, with how deep their undo history was after.
    Trashed { res: String, at: PathBuf, fixed: Vec<(PathBuf, String)>, open: Vec<(String, usize)> },
    /// New files (imported or made).
    Created(Vec<String>),
}

impl FileOp {
    pub fn describe(&self) -> String {
        let short = |r: &str| r.rsplit('/').next().unwrap_or(r).to_string();
        match self {
            FileOp::Renamed { old, new } => format!("rename {} → {}", short(old), short(new)),
            FileOp::Trashed { res, .. } => format!("delete {}", short(res)),
            FileOp::Created(l) if l.len() == 1 => format!("make {}", short(&l[0])),
            FileOp::Created(l) => format!("import {} files", l.len()),
        }
    }
}

pub struct Playing {
    pub handle: GameHandle,
    pub list: RenderList,
    pub tree: BlueprintTree,
    pub last: Instant,
    pub keys: HashSet<String>,
    /// Keys pressed since the last frame request (so a quick tap isn't missed).
    pub taps: HashSet<String>,
    pub wheel: f32,
    pub ui_events: Vec<gex_script::UiEvent>,
    pub axes: std::collections::HashMap<String, f32>,
    pub mouse: [f32; 2],
    pub buttons: HashSet<String>,
    /// The viewport shows the game's own camera (else the editor camera, to look around).
    pub game_camera: bool,
    /// Just started: the view takes the keyboard so the game can be played straight away.
    pub focus_view: bool,
    /// Raw mouse movement since the last frame request.
    pub mouse_delta: [f32; 2],
    /// The last few seconds of frames: (time between frames, where the game's time went).
    pub stats: std::collections::VecDeque<(f32, gex_script::world::FrameStats)>,
    pub last_frame: Instant,
    /// The mouse is held for the game (MOUSE_LOCK), or Escape let it go for now.
    pub locked: bool,
    pub lock_suspended: bool,
}

/// What dragging a selected object does in the blueprint view.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Move,
    Rotate,
    Scale,
}

/// A blueprint that's open but not the one being edited right now.
pub struct BlueprintDoc {
    pub tree: BlueprintTree,
    pub blueprint_res: Option<String>,
    pub blueprint_saved: String,
    pub selected: Option<ObjectId>,
    pub undo: Vec<BlueprintTree>,
    pub redo: Vec<BlueprintTree>,
    pub mode_3d: bool,
    pub cam2: EditorCamera2D,
    pub cam3: EditorCamera3D,
}

pub struct EditorState {
    pub project: Project,
    pub vfs: Vfs,
    pub tree: BlueprintTree,
    pub blueprint_res: Option<String>,
    pub blueprint_saved: String,
    pub selected: Option<ObjectId>,
    /// Every selected object (`selected` is the main one, shown in the inspector).
    pub selection: Vec<ObjectId>,
    /// Other open blueprints (the one being edited is in the fields above).
    pub docs: Vec<BlueprintDoc>,
    pub tool: Tool,
    undo: Vec<BlueprintTree>,
    redo: Vec<BlueprintTree>,
    last_change: Option<(String, Instant)>,
    pub logs: Vec<LogLine>,
    pub scripts: HashMap<String, Script>,
    /// Scripts the dock should open as tabs.
    pub open_requests: Vec<String>,
    pub playing: Option<Playing>,
    /// The game built and running in a window of its own (the Build button).
    pub build: Option<Build>,
    /// The blueprint to go back to when the game stops (F5 ran it from the main blueprint).
    pub return_to: Option<String>,
    /// Where the object each script belongs to is in the blueprint view (script → screen
    /// point), for the wires of the script windows.
    pub view_anchors: std::collections::HashMap<String, [f32; 2]>,
    pub renderer: Option<Renderer>,
    pub mode_3d: bool,
    pub cam2: EditorCamera2D,
    pub cam3: EditorCamera3D,
    pub show_grid: bool,
    /// Snap steps while dragging: move in 2D (pixels), move and scale in 3D (meters), turn (degrees). 0 is off.
    pub snap2: f32,
    pub snap3: f32,
    pub snap_angle: f32,
    pub show_colliders: bool,
    /// Painting tiles into the selected TileMap2D with this tile (-1 erases).
    pub tile_brush: Option<i32>,
    /// Textures for UI Image objects and tile palettes.
    pub ui_images: std::collections::HashMap<String, eframe::egui::TextureHandle>,
    /// The 2D camera has been fitted to the game window once.
    pub cam_fitted: bool,
    pub asset_dir: String,
    /// Highlight colors of files and folders in the Assets panel (path → "#rrggbb"). Only
    /// for looks; kept in the project's .gex folder.
    pub asset_colors: std::collections::BTreeMap<String, String>,
    pub asset_selected: Option<String>,
    pub show_settings: bool,
    pub show_export: bool,
    pub status: String,
    /// Breakpoints by script (res:// path) → line numbers.
    pub breakpoints: HashMap<String, std::collections::BTreeSet<usize>>,
    /// Shared with the running game, which checks it as it runs.
    pub game_breakpoints: std::sync::Arc<gex_script::thread::Breakpoints>,
    /// Vim keys in the script editor.
    pub vim_enabled: bool,
    /// Particles2D previews in the blueprint view (not while playing).
    pub preview_particles: HashMap<ObjectId, gex_script::particles::Emitter>,
    pub preview_particles_3d: HashMap<ObjectId, gex_script::particles::Emitter3>,
    pub particle_clock: Instant,
    pub search: crate::tools::Search,
    /// The Command reference window is open (View → Commands, F1), and a command to show in it.
    pub commands_open: bool,
    pub commands_show: Option<String>,
    pub timeline: crate::timeline::TimelineState,
    /// Breakpoint conditions: (script, line) → expression.
    pub bp_conditions: HashMap<(String, usize), String>,
    /// A breakpoint condition being edited: (script, line, text).
    pub bp_edit: Option<(String, usize, String)>,
    /// Expressions the debugger shows whenever the game stops.
    pub watches: Vec<String>,
    pub watch_input: String,
    pub previews: crate::preview::Previews,
    /// File changes made in the Assets panel, newest last, for "Undo file change".
    pub file_undo: Vec<FileOp>,
    /// Something asked for the Timeline panel to be shown.
    pub open_timeline: bool,
    /// Something asked for the command line to open (":" in Vim normal mode).
    pub open_command: bool,
}

impl EditorState {
    pub fn new(project: Project) -> EditorState {
        let vfs = Vfs::new(&project.dir, &project.config);
        let mut st = EditorState {
            project,
            vfs,
            tree: BlueprintTree::new(),
            blueprint_res: None,
            blueprint_saved: String::new(),
            selected: None,
            selection: Vec::new(),
            docs: Vec::new(),
            tool: Tool::Move,
            snap2: 0.0,
            snap3: 0.0,
            snap_angle: 0.0,
            undo: Vec::new(),
            redo: Vec::new(),
            last_change: None,
            logs: Vec::new(),
            scripts: HashMap::new(),
            open_requests: Vec::new(),
            playing: None,
            build: None,
            return_to: None,
            view_anchors: Default::default(),
            renderer: None,
            mode_3d: false,
            cam2: EditorCamera2D::default(),
            cam3: EditorCamera3D::default(),
            show_grid: true,
            show_colliders: true,
            tile_brush: None,
            ui_images: Default::default(),
            cam_fitted: false,
            asset_dir: "res://".into(),
            asset_colors: Default::default(),
            asset_selected: None,
            show_settings: false,
            show_export: false,
            status: String::new(),
            breakpoints: HashMap::new(),
            game_breakpoints: Default::default(),
            preview_particles: HashMap::new(),
            preview_particles_3d: HashMap::new(),
            timeline: Default::default(),
            bp_conditions: HashMap::new(),
            bp_edit: None,
            watches: Vec::new(),
            watch_input: String::new(),
            previews: Default::default(),
            file_undo: Vec::new(),
            open_timeline: false,
            search: Default::default(),
            commands_open: false,
            commands_show: None,
            particle_clock: Instant::now(),
            vim_enabled: std::fs::read_to_string(config_dir().join("vim")).map(|s| s.trim() == "on").unwrap_or(false),
            open_command: false,
        };
        for w in st.vfs.warnings() {
            st.log(LogKind::Error, w);
        }
        let main = st.project.config.main_blueprint.clone();
        if st.project.res_to_path(&main).is_some_and(|p| p.is_file()) {
            st.open_blueprint(&main);
        } else {
            st.tree.add(None, gex_core::Object::new("Root", gex_core::ObjectKind::Object2D));
        }
        st
    }

    pub fn log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.logs.push(LogLine { kind, text: text.into(), file: None, line: 0 });
    }

    pub fn reader(&self) -> impl Fn(&str) -> Result<String, String> + '_ {
        move |p: &str| self.project.read_text(p)
    }

    pub fn remount(&mut self) {
        self.vfs = Vfs::new(&self.project.dir, &self.project.config);
        if let Some(r) = &mut self.renderer {
            r.reload_textures();
        }
        for w in self.vfs.warnings() {
            self.log(LogKind::Error, w);
        }
    }

    // ---------- blueprints ----------

    /// Opens a blueprint in its own tab (or switches to it if it's open).
    pub fn open_blueprint(&mut self, res: &str) {
        if self.blueprint_res.as_deref() == Some(res) {
            return;
        }
        if let Some(i) = self.docs.iter().position(|d| d.blueprint_res.as_deref() == Some(res)) {
            self.switch_blueprint(i);
            return;
        }
        if self.blueprint_res.is_some() || self.tree.objects.len() > 1 {
            let doc = self.take_doc();
            self.docs.push(doc);
        }
        self.load_blueprint(res);
    }

    fn take_doc(&mut self) -> BlueprintDoc {
        self.preview_particles.clear();
        self.preview_particles_3d.clear();
        BlueprintDoc {
            tree: std::mem::take(&mut self.tree),
            blueprint_res: self.blueprint_res.take(),
            blueprint_saved: std::mem::take(&mut self.blueprint_saved),
            selected: self.selected.take(),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            mode_3d: self.mode_3d,
            cam2: self.cam2,
            cam3: self.cam3,
        }
    }

    fn put_doc(&mut self, d: BlueprintDoc) {
        self.tree = d.tree;
        self.blueprint_res = d.blueprint_res;
        self.blueprint_saved = d.blueprint_saved;
        self.selected = d.selected;
        self.selection = d.selected.into_iter().collect();
        self.undo = d.undo;
        self.redo = d.redo;
        self.mode_3d = d.mode_3d;
        self.cam2 = d.cam2;
        self.cam3 = d.cam3;
        self.last_change = None;
    }

    /// Makes another open blueprint the one being edited.
    pub fn switch_blueprint(&mut self, i: usize) {
        if i >= self.docs.len() || self.playing.is_some() {
            return;
        }
        let d = self.docs.remove(i);
        let cur = self.take_doc();
        self.docs.insert(i, cur);
        self.put_doc(d);
    }

    /// Runs `f` with another open blueprint as the one being edited, then puts things back
    /// (deleting and renaming files fix other blueprints this way). Loads the blueprint if it isn't open. Not while
    /// playing.
    pub fn with_blueprint<R>(&mut self, res: &str, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if self.blueprint_res.as_deref() == Some(res) {
            return Some(f(self));
        }
        if self.playing.is_some() {
            return None;
        }
        let i = match self.docs.iter().position(|d| d.blueprint_res.as_deref() == Some(res)) {
            Some(i) => i,
            None => {
                let text = self.project.read_text(res).ok()?;
                let tree = blueprint::load(&text, &self.reader()).ok()?;
                let saved = blueprint::save(&tree, &self.reader()).unwrap_or_default();
                let mode_3d = tree.walk().iter().any(|id| tree.objects[*id].kind.is_3d());
                let selected = tree.root;
                self.docs.push(BlueprintDoc { tree, blueprint_res: Some(res.to_string()), blueprint_saved: saved, selected, undo: Vec::new(), redo: Vec::new(), mode_3d, cam2: Default::default(), cam3: Default::default() });
                self.docs.len() - 1
            }
        };
        let other = self.docs.remove(i);
        let (sel, cam_fitted) = (self.selection.clone(), self.cam_fitted);
        let cur = self.swap_out();
        self.put_doc(other);
        let r = f(self);
        let back = self.swap_out();
        self.put_doc(cur);
        self.selection = sel;
        self.cam_fitted = cam_fitted;
        self.docs.insert(i, back);
        Some(r)
    }

    /// The blueprint being edited, taken out (without the side effects of switching tabs).
    fn swap_out(&mut self) -> BlueprintDoc {
        BlueprintDoc {
            tree: std::mem::take(&mut self.tree),
            blueprint_res: self.blueprint_res.take(),
            blueprint_saved: std::mem::take(&mut self.blueprint_saved),
            selected: self.selected.take(),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            mode_3d: self.mode_3d,
            cam2: self.cam2,
            cam3: self.cam3,
        }
    }

    /// Whether an open blueprint has unsaved changes.
    pub fn is_dirty(&self, res: &str) -> bool {
        if self.blueprint_res.as_deref() == Some(res) {
            return self.blueprint_dirty();
        }
        self.docs.iter().find(|d| d.blueprint_res.as_deref() == Some(res)).is_some_and(|d| blueprint::save(&d.tree, &self.reader()).map(|t| t != d.blueprint_saved).unwrap_or(false))
    }

    /// Closes one open blueprint by path (unsaved changes are lost).
    pub fn close_blueprint_res(&mut self, res: &str) {
        if self.blueprint_res.as_deref() == Some(res) {
            self.close_blueprint();
        } else {
            self.docs.retain(|d| d.blueprint_res.as_deref() != Some(res));
        }
    }

    /// Closes the blueprint being edited (unsaved changes are lost), switching to another.
    pub fn close_blueprint(&mut self) {
        if self.playing.is_some() {
            return;
        }
        if self.docs.is_empty() {
            // The last one: nothing is open.
            let _ = self.swap_out();
            self.tree.add(None, gex_core::Object::new("Root", gex_core::ObjectKind::Object2D));
            self.selection.clear();
            self.last_change = None;
            return;
        }
        let d = self.docs.remove(0);
        self.put_doc(d);
    }

    fn load_blueprint(&mut self, res: &str) {
        let text = match self.project.read_text(res) {
            Ok(t) => t,
            Err(e) => return self.log(LogKind::Error, format!("couldn't open {}: {}", res, e)),
        };
        let loaded = blueprint::load(&text, &self.reader());
        match loaded {
            Ok(t) => {
                self.tree = t;
                self.blueprint_res = Some(res.to_string());
                self.selected = self.tree.root;
                self.selection = self.selected.into_iter().collect();
                self.cam_fitted = false;
                self.undo.clear();
                self.redo.clear();
                self.blueprint_saved = self.blueprint_text().unwrap_or_default();
                self.mode_3d = self.tree.walk().iter().any(|id| self.tree.objects[*id].kind.is_3d());
                self.status = format!("opened {}", res);
            }
            Err(e) => self.log(LogKind::Error, format!("{}: {}", res, e)),
        }
    }

    pub fn blueprint_text(&self) -> Result<String, String> {
        blueprint::save(&self.tree, &self.reader()).map_err(|e| e.0)
    }

    pub fn blueprint_dirty(&self) -> bool {
        self.blueprint_text().map(|t| t != self.blueprint_saved).unwrap_or(true)
    }

    pub fn save_blueprint(&mut self) {
        let res = match &self.blueprint_res {
            Some(r) => r.clone(),
            None => {
                let r = "res://blueprints/untitled.jblueprint".to_string();
                self.blueprint_res = Some(r.clone());
                r
            }
        };
        // Other open blueprints with copies of this one: describe them against the file as it was,
        // so after saving they can be rebuilt from the new file with their own changes kept.
        let users: Vec<(usize, Option<String>)> = {
            let read = self.reader();
            self.docs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.tree.objects.iter().any(|(_, n)| n.instance_of.as_deref() == Some(res.as_str())))
                .map(|(i, d)| (i, blueprint::save(&d.tree, &read).ok()))
                .collect()
        };
        let result = self.blueprint_text().and_then(|text| {
            let p = self.project.res_to_path(&res).ok_or("not a project path")?;
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, &text).map_err(|e| e.to_string())?;
            Ok(text)
        });
        match result {
            Ok(t) => {
                self.blueprint_saved = t;
                self.status = format!("saved {}", res);
                for (i, before) in users {
                    let Some(text) = before else { continue };
                    let rebuilt = blueprint::load(&text, &|p: &str| self.project.read_text(p));
                    if let Ok(tree) = rebuilt {
                        let d = &mut self.docs[i];
                        d.undo.push(std::mem::replace(&mut d.tree, tree));
                        d.redo.clear();
                        d.selected = d.tree.root;
                    }
                }
            }
            Err(e) => self.log(LogKind::Error, format!("couldn't save {}: {}", res, e)),
        }
    }

    /// Saves every changed script and every open blueprint (the other tabs' too).
    pub fn save_all(&mut self) {
        self.save_blueprint();
        let dirty: Vec<String> = self.scripts.values().filter(|s| s.dirty()).map(|s| s.res.clone()).collect();
        for r in dirty {
            self.save_script(&r);
        }
        let unsaved: Vec<(usize, String, String)> = {
            let read = self.reader();
            self.docs
                .iter()
                .enumerate()
                .filter_map(|(i, d)| Some((i, d.blueprint_res.clone()?, blueprint::save(&d.tree, &read).ok()?)))
                .filter(|(i, _, t)| *t != self.docs[*i].blueprint_saved)
                .collect()
        };
        for (i, res, text) in unsaved {
            if let Some(p) = self.project.res_to_path(&res) {
                if std::fs::write(&p, &text).is_ok() {
                    self.docs[i].blueprint_saved = text;
                }
            }
        }
    }

    /// Files were changed outside the editor (by REX): what's open and has no unsaved changes
    /// of its own is read again.
    pub fn reload_from_disk(&mut self) {
        if self.playing.is_some() {
            return;
        }
        for sc in self.scripts.values_mut() {
            if !sc.dirty() {
                if let Ok(t) = self.project.read_text(&sc.res) {
                    if t != sc.text {
                        sc.text = t.clone();
                        sc.saved = t;
                    }
                }
            }
        }
        // Other open blueprints are read again when they're next shown.
        let project = &self.project;
        self.docs.retain(|d| {
            let dirty = blueprint::save(&d.tree, &|p: &str| project.read_text(p)).ok().is_some_and(|t| t != d.blueprint_saved);
            dirty
        });
        if let Some(res) = self.blueprint_res.clone() {
            if !self.blueprint_dirty() {
                let on_disk = self.project.read_text(&res).ok().and_then(|t| blueprint::load(&t, &self.reader()).ok());
                if let Some(tree) = on_disk {
                    if blueprint::save(&tree, &self.reader()).ok().as_ref() != Some(&self.blueprint_saved) {
                        let (cam2, cam3, mode) = (self.cam2, self.cam3, self.mode_3d);
                        self.load_blueprint(&res);
                        (self.cam2, self.cam3, self.mode_3d, self.cam_fitted) = (cam2, cam3, mode, true);
                    }
                }
            }
        }
    }

    // ---------- undo ----------

    /// Call before changing the tree. Changes to the same thing within a second share one
    /// undo step (so dragging a value is one step).
    pub fn will_change(&mut self, what: &str) {
        if self.playing.is_some() {
            return;
        }
        let now = Instant::now();
        let merge = self.last_change.as_ref().is_some_and(|(w, t)| w == what && now.duration_since(*t).as_secs_f32() < 1.0);
        if !merge {
            self.undo.push(self.tree.clone());
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_change = Some((what.to_string(), now));
    }

    pub fn undo(&mut self) {
        if let Some(t) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.tree, t));
            self.fix_selection();
            self.last_change = None;
        }
    }
    pub fn redo(&mut self) {
        if let Some(t) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.tree, t));
            self.fix_selection();
            self.last_change = None;
        }
    }
    fn fix_selection(&mut self) {
        let tree = &self.tree;
        self.selection.retain(|s| tree.contains(*s));
        if self.selected.is_some_and(|s| !self.tree.contains(s)) {
            self.selected = self.tree.root;
        }
    }

    // ---------- objects ----------

    /// Adds an object from the "+ Object" menu: at the top of the blueprint, in the middle of the view.
    /// Colliders are the exception: they go on the selected object, fitted to it.
    pub fn add_object(&mut self, kind: gex_core::ObjectKind) -> ObjectId {
        use gex_core::ObjectKind as K;
        let on_selected = matches!(kind, K::Collider2D | K::Collider3D) && self.selected.is_some_and(|s| Some(s) != self.tree.root && !matches!(self.tree.objects[s].kind, K::Collider2D | K::Collider3D));
        let parent = if on_selected { self.selected } else { self.tree.root };
        self.add_child(parent, kind)
    }

    /// Adds an object under a parent. Under the blueprint's root it appears in the middle of the view;
    /// under anything else it sits on its parent.
    pub fn add_child(&mut self, parent: Option<ObjectId>, kind: gex_core::ObjectKind) -> ObjectId {
        self.will_change("add");
        let mut n = gex_core::Object::new(kind.name(), kind);
        let root = self.tree.root;
        let mut parent = parent.or(root);
        // New objects start at 0,0 of what they're put in.
        if kind.is_ui() {
            parent = parent.filter(|s| self.tree.objects[*s].kind.is_ui()).or(root);
        } else if let (gex_core::ObjectKind::Collider2D, Some(p)) = (kind, parent) {
            // A collider starts the size of what it's on.
            let pn = &self.tree.objects[p];
            for k in ["width", "height"] {
                if let Some(v) = pn.prop(k).and_then(gex_core::Variant::as_f64) {
                    n.props.insert(k.into(), gex_core::Variant::Num(v));
                }
            }
        }
        let id = self.tree.add(parent, n);
        self.select(Some(id));
        id
    }

    pub fn instance_blueprint(&mut self, res: &str) -> Option<ObjectId> {
        if Some(res) == self.blueprint_res.as_deref() {
            self.log(LogKind::Error, "a blueprint can't contain itself");
            return None;
        }
        self.will_change("instance");
        let parent = self.selected.or(self.tree.root);
        let reader = |p: &str| self.project.read_text(p);
        let mut tree = std::mem::take(&mut self.tree);
        let r = blueprint::instance(&mut tree, parent, res, &reader);
        self.tree = tree;
        match r {
            Ok(id) => {
                // Named after the blueprint it's an instance of ("enemy", "enemy2"...).
                let stem = res.rsplit('/').next().unwrap_or(res).trim_end_matches(".jblueprint").to_string();
                let taken: Vec<String> = self.tree.objects[id].parent.map(|p| self.tree.objects[p].children.iter().filter(|c| **c != id).map(|c| self.tree.objects[*c].name.clone()).collect()).unwrap_or_default();
                let name = std::iter::once(stem.clone()).chain((2..).map(|i| format!("{}{}", stem, i))).find(|n| !taken.contains(n)).unwrap_or(stem);
                self.tree.objects[id].name = name;
                self.selected = Some(id);
                Some(id)
            }
            Err(e) => {
                self.log(LogKind::Error, e.0);
                None
            }
        }
    }

    /// Saves a branch as its own blueprint file, then puts an instance of that file where the
    /// branch was (so the branch can be reused in other blueprints and stays in sync).
    pub fn save_branch_as_blueprint(&mut self, id: ObjectId) -> Option<String> {
        let n = self.tree.get(id)?;
        if n.from_instance {
            self.status = "this object belongs to an instanced blueprint; open that blueprint to change it".into();
            return None;
        }
        let slug: String = n.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let slug = if slug.trim_matches('_').is_empty() { "branch".to_string() } else { slug };
        let mut res = format!("res://blueprints/{}.jblueprint", slug);
        let mut k = 2;
        while self.project.res_to_path(&res).is_some_and(|p| p.exists()) {
            res = format!("res://blueprints/{}_{}.jblueprint", slug, k);
            k += 1;
        }
        // The branch on its own, with its root at the origin.
        let mut sub = BlueprintTree::new();
        let root = sub.copy_subtree(&self.tree, id, None);
        sub.objects[root].transform = Default::default();
        sub.objects[root].parent = None;
        let saved = blueprint::save(&sub, &self.reader());
        let text = match saved {
            Ok(t) => t,
            Err(e) => {
                self.log(LogKind::Error, e.0);
                return None;
            }
        };
        let path = self.project.res_to_path(&res)?;
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&path, text) {
            self.log(LogKind::Error, format!("{}: {}", path.display(), e));
            return None;
        }
        self.will_change("branch to blueprint");
        let (name, transform, visible, parent) = {
            let n = &self.tree.objects[id];
            (n.name.clone(), n.transform, n.visible, n.parent)
        };
        let index = parent.and_then(|p| self.tree.objects[p].children.iter().position(|c| *c == id));
        let reader = |p: &str| self.project.read_text(p);
        let mut tree = std::mem::take(&mut self.tree);
        let made = blueprint::instance(&mut tree, parent, &res, &reader);
        self.tree = tree;
        match made {
            Ok(new_id) => {
                self.tree.remove(id);
                let n = &mut self.tree.objects[new_id];
                n.name = name;
                n.transform = transform;
                n.visible = visible;
                if let (Some(p), Some(i)) = (parent, index) {
                    self.tree.reparent(new_id, p, Some(i));
                } else if parent.is_none() {
                    self.tree.root = Some(new_id);
                }
                self.selection.clear();
                self.selected = Some(new_id);
                self.log(LogKind::Info, format!("saved the branch as {}", res));
                Some(res)
            }
            Err(e) => {
                self.log(LogKind::Error, e.0);
                None
            }
        }
    }

    /// Turns an instance into ordinary objects of this blueprint (it stops following its file).
    pub fn make_local(&mut self, id: ObjectId) {
        if self.tree.get(id).is_none_or(|n| n.instance_of.is_none()) {
            return;
        }
        self.will_change("make local");
        let mut all = Vec::new();
        self.tree.walk_from(id, &mut all);
        for n in all {
            let object = &mut self.tree.objects[n];
            object.instance_of = None;
            object.from_instance = false;
        }
    }

    // ---------- project files ----------

    /// Renames a project file or folder, fixing references to it on disk and in everything open.
    pub fn rename_asset(&mut self, old: &str, new_name: &str) -> Result<String, String> {
        if self.playing.is_some() {
            return Err("stop the game first".into());
        }
        let (new, fixed) = crate::files::rename(&self.project, old, new_name)?;
        let swap = |s: &str| -> Option<String> {
            if s == old {
                Some(new.clone())
            } else {
                s.strip_prefix(old).filter(|r| r.starts_with('/')).map(|r| format!("{}{}", new, r))
            }
        };
        let fix_tree = |t: &mut BlueprintTree| {
            for (_, n) in t.objects.iter_mut() {
                for sc in n.scripts.iter_mut() {
                    if let Some(x) = swap(sc) {
                        *sc = x;
                    }
                }
                if let Some(x) = n.instance_of.as_deref().and_then(swap) {
                    n.instance_of = Some(x);
                }
                for v in n.props.values_mut() {
                    match v {
                        gex_core::Variant::Res(r) | gex_core::Variant::Text(r) => {
                            if let Some(x) = swap(r) {
                                *r = x;
                            }
                        }
                        _ => {}
                    }
                }
            }
        };
        let fix_text = |t: &mut String| {
            if let Some(x) = crate::files::replace_ref(t, old, &new) {
                *t = x;
            }
        };
        fix_tree(&mut self.tree);
        self.undo.iter_mut().chain(self.redo.iter_mut()).for_each(fix_tree);
        fix_text(&mut self.blueprint_saved);
        if let Some(x) = self.blueprint_res.as_deref().and_then(swap) {
            self.blueprint_res = Some(x);
        }
        for d in &mut self.docs {
            fix_tree(&mut d.tree);
            d.undo.iter_mut().chain(d.redo.iter_mut()).for_each(fix_tree);
            fix_text(&mut d.blueprint_saved);
            if let Some(x) = d.blueprint_res.as_deref().and_then(swap) {
                d.blueprint_res = Some(x);
            }
        }
        let keys: Vec<String> = self.scripts.keys().cloned().collect();
        for k in keys {
            let mut sc = self.scripts.remove(&k).unwrap();
            fix_text(&mut sc.text);
            fix_text(&mut sc.saved);
            let key = match swap(&k) {
                Some(x) => {
                    sc.res = x.clone();
                    x
                }
                None => k,
            };
            self.scripts.insert(key, sc);
        }
        let bps: Vec<String> = self.breakpoints.keys().filter(|k| swap(k).is_some()).cloned().collect();
        for k in bps {
            let v = self.breakpoints.remove(&k).unwrap();
            self.breakpoints.insert(swap(&k).unwrap(), v);
        }
        if let Some(x) = self.asset_selected.as_deref().and_then(swap) {
            self.asset_selected = Some(x);
        }
        let colored: Vec<String> = self.asset_colors.keys().filter(|k| swap(k).is_some()).cloned().collect();
        for k in &colored {
            let v = self.asset_colors.remove(k).unwrap();
            self.asset_colors.insert(swap(k).unwrap(), v);
        }
        if !colored.is_empty() {
            self.save_asset_colors();
        }
        if let Some(x) = swap(&self.asset_dir) {
            self.asset_dir = x;
        }
        // A blueprint's main object is named after it, and keeps up with a new name.
        if let (Some(a), Some(b)) = (old.strip_suffix(".jblueprint"), new.strip_suffix(".jblueprint")) {
            let (a, b) = (a.rsplit('/').next().unwrap_or(a).to_string(), b.rsplit('/').next().unwrap_or(b).to_string());
            if self.blueprint_res.as_deref() == Some(new.as_str()) {
                if let Some(r) = self.tree.root.filter(|r| self.tree.objects[*r].name == a) {
                    self.tree.objects[r].name = b;
                    self.save_blueprint();
                }
            } else if let Some(path) = self.project.res_to_path(&new) {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let first = format!("name: \"{}\"", a);
                    if text.find("name: \"") == text.find(&first) && text.contains(&first) {
                        let _ = std::fs::write(&path, text.replacen(&first, &format!("name: \"{}\"", b), 1));
                    }
                }
            }
        }
        self.renderer.as_mut().map(|r| r.reload_textures());
        self.file_undo.push(FileOp::Renamed { old: old.to_string(), new: new.clone() });
        self.previews.forget(old);
        let msg = if fixed.is_empty() { format!("renamed to {}", new) } else { format!("renamed to {} (fixed {} file(s): {})", new, fixed.len(), fixed.join(", ")) };
        self.log(LogKind::Info, msg);
        Ok(new)
    }

    /// Moves a project file or folder to the hidden .trash folder.
    pub fn delete_asset(&mut self, res: &str) -> Result<(), String> {
        if self.playing.is_some() {
            return Err("stop the game first".into());
        }
        // Open blueprints being deleted (or inside a folder being deleted) close first.
        let inside = format!("{}/", res);
        let open: Vec<String> = self.blueprint_res.iter().chain(self.docs.iter().filter_map(|d| d.blueprint_res.as_ref())).filter(|r| r.as_str() == res || r.starts_with(&inside)).cloned().collect();
        for r in open {
            self.close_blueprint_res(&r);
        }
        let to = crate::files::trash(&self.project, res)?;
        self.scripts.retain(|k, _| k != res && !k.starts_with(&format!("{}/", res)));
        let (fixed, open) = self.forget_asset(res);
        if self.asset_selected.as_deref() == Some(res) {
            self.asset_selected = None;
        }
        self.log(LogKind::Info, format!("moved {} to {} (Undo file change puts it back)", res, to.display()));
        self.file_undo.push(FileOp::Trashed { res: res.to_string(), at: to, fixed, open });
        self.previews.forget(res);
        crate::icons::forget(res);
        Ok(())
    }

    /// After a file or folder is deleted: takes its instances, attached scripts and uses out of
    /// every blueprint. Open blueprints change in the editor (undoable there); the others on disk.
    /// Gives the changed files as they were, for undo.
    fn forget_asset(&mut self, res: &str) -> (Vec<(PathBuf, String)>, Vec<(String, usize)>) {
        let inside = format!("{}/", res);
        let gone = |s: &str| s == res || s.starts_with(&inside);
        let open: Vec<String> = self.blueprint_res.iter().chain(self.docs.iter().filter_map(|d| d.blueprint_res.as_ref())).cloned().collect();
        let mut n = 0;
        let mut changed_open = Vec::new();
        for r in &open {
            let depth = self.with_blueprint(r, |st| {
                let uses = |t: &BlueprintTree| {
                    t.objects.iter().any(|(id, x)| {
                        x.scripts.iter().any(|s| gone(s))
                            || (x.instance_of.as_deref().is_some_and(gone) && !x.from_instance && Some(id) != t.root)
                            || x.props.values().any(|v| matches!(v, gex_core::Variant::Res(p) | gex_core::Variant::Text(p) if p.contains("://") && gone(p)))
                    })
                };
                if !uses(&st.tree) {
                    return None;
                }
                st.will_change("asset deleted");
                st.last_change = None;
                let roots: Vec<ObjectId> = st.tree.objects.iter().filter(|(id, x)| x.instance_of.as_deref().is_some_and(gone) && !x.from_instance && Some(*id) != st.tree.root).map(|(id, _)| id).collect();
                for id in roots {
                    st.tree.remove(id);
                }
                for (_, x) in st.tree.objects.iter_mut() {
                    x.scripts.retain(|s| !gone(s));
                    for v in x.props.values_mut() {
                        if let gex_core::Variant::Res(p) | gex_core::Variant::Text(p) = v {
                            if p.contains("://") && gone(p) {
                                p.clear();
                            }
                        }
                    }
                }
                st.selection.retain(|i| st.tree.contains(*i));
                st.selected = st.selected.filter(|i| st.tree.contains(*i)).or(st.tree.root);
                Some(st.undo.len())
            });
            if let Some(Some(d)) = depth {
                n += 1;
                changed_open.push((r.clone(), d));
            }
        }
        let mut fixed = Vec::new();
        for path in crate::files::blueprint_files(&self.project) {
            if self.project.path_to_res(&path).is_some_and(|r| open.contains(&r)) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Ok(mut file) = blueprint::parse(&text) else { continue };
            if blueprint::remove_refs(&mut file, &gone) && std::fs::write(&path, blueprint::to_text(&file)).is_ok() {
                fixed.push((path, text));
            }
        }
        if n + fixed.len() > 0 {
            self.log(LogKind::Info, format!("took it out of {} blueprint(s) that used it", n + fixed.len()));
        }
        (fixed, changed_open)
    }

    /// Moves a file or folder into another project folder (blueprints and scripts using it follow).
    pub fn move_asset(&mut self, res: &str, dir: &str) -> Result<String, String> {
        let name = res.rsplit('/').next().unwrap_or(res);
        let to = if dir.ends_with("://") { format!("{}{}", dir, name) } else { format!("{}/{}", dir.trim_end_matches('/'), name) };
        if to == res {
            return Ok(to);
        }
        self.rename_asset(res, &to)
    }

    fn asset_colors_file(&self) -> PathBuf {
        self.project.dir.join(".gex").join("colors.json")
    }

    pub fn load_asset_colors(&mut self) {
        self.asset_colors = std::fs::read_to_string(self.asset_colors_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    }

    fn save_asset_colors(&self) {
        let f = self.asset_colors_file();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        if let Ok(t) = serde_json::to_string_pretty(&self.asset_colors) {
            let _ = std::fs::write(f, t);
        }
    }

    /// Gives a file or folder a highlight color in the Assets panel (None takes it away).
    pub fn set_asset_color(&mut self, res: &str, color: Option<String>) {
        match color {
            Some(c) => self.asset_colors.insert(res.to_string(), c),
            None => self.asset_colors.remove(res),
        };
        self.save_asset_colors();
    }

    /// The highlight of a file: its own, or the nearest folder's around it.
    pub fn asset_color(&self, res: &str) -> Option<&str> {
        if self.asset_colors.is_empty() {
            return None;
        }
        let mut at = res;
        loop {
            if let Some(c) = self.asset_colors.get(at) {
                return Some(c);
            }
            at = &at[..at.rfind('/').filter(|i| !at[..*i].ends_with(['/', ':']))?];
        }
    }

    /// Makes a folder in a project folder.
    pub fn new_folder(&mut self, dir: &str) -> Option<String> {
        let res = crate::files::free_name(&self.project, dir, "new_folder")?;
        let name = res.rsplit('/').next()?.to_string();
        match crate::files::new_folder(&self.project, dir, &name) {
            Ok(r) => {
                self.file_undo.push(FileOp::Created(vec![r.clone()]));
                Some(r)
            }
            Err(e) => {
                self.log(LogKind::Error, e);
                None
            }
        }
    }

    /// Copies files from disk into the project (dropped on the window, or picked).
    pub fn import_files(&mut self, files: &[PathBuf]) {
        let dir = if self.asset_dir.starts_with("res://") { self.asset_dir.clone() } else { "res://".to_string() };
        if let Some(p) = self.project.res_to_path(&dir) {
            let _ = std::fs::create_dir_all(p);
        }
        match crate::files::import(&self.project, &dir, files) {
            Ok(made) => {
                if let Some(last) = made.last() {
                    self.asset_dir = dir;
                    self.asset_selected = Some(last.clone());
                }
                self.log(LogKind::Info, format!("imported {}", made.join(", ")));
                self.file_undo.push(FileOp::Created(made));
            }
            Err(e) => self.log(LogKind::Error, format!("import: {}", e)),
        }
    }

    /// A new empty blueprint file in a folder, opened in a tab.
    pub fn new_blueprint_file(&mut self, dir: &str, kind: gex_core::ObjectKind) -> Option<String> {
        let _ = std::fs::create_dir_all(self.project.res_to_path(dir)?);
        // The first blueprint is where the game starts.
        let main = self.project.config.main_blueprint.clone();
        let first = !self.project.res_to_path(&main).is_some_and(|p| p.is_file());
        let res = crate::files::free_name(&self.project, dir, if first { "main.jblueprint" } else { "new_blueprint.jblueprint" })?;
        let path = self.project.res_to_path(&res)?;
        // The blueprint's main object is called what the blueprint is.
        let root = res.rsplit('/').next().unwrap_or("main").trim_end_matches(".jblueprint").to_string();
        let root = root.as_str();
        if first {
            self.project.config.main_blueprint = res.clone();
            let _ = self.project.save();
        }
        if let Err(e) = std::fs::write(&path, gex_core::blueprint::new_blueprint_text(root, kind)) {
            self.log(LogKind::Error, e.to_string());
            return None;
        }
        self.open_blueprint(&res);
        self.file_undo.push(FileOp::Created(vec![res.clone()]));
        Some(res)
    }

    /// A new script file in a folder, opened in a tab.
    pub fn new_script_file(&mut self, dir: &str) -> Option<String> {
        let _ = std::fs::create_dir_all(self.project.res_to_path(dir)?);
        let res = crate::files::free_name(&self.project, dir, "new_script.jlb")?;
        let path = self.project.res_to_path(&res)?;
        if let Err(e) = std::fs::write(&path, ">> Attach this to an object in the Events panel.\n\nDEF on_create() {\n\t\n}\n") {
            self.log(LogKind::Error, e.to_string());
            return None;
        }
        self.open_script(&res, None);
        self.file_undo.push(FileOp::Created(vec![res.clone()]));
        Some(res)
    }

    /// Copies a built-in asset (gex://...) into the project, under res://assets/placeholders,
    /// so it can be edited. Selects the copy.
    pub fn copy_builtin(&mut self, path: &str) {
        let Some(rel) = path.strip_prefix("gex://") else { return };
        let (dir, name) = rel.rsplit_once('/').map(|(d, n)| (format!("res://assets/placeholders/{}", d), n)).unwrap_or(("res://assets/placeholders".into(), rel));
        // Never overwrite an earlier (maybe edited) copy.
        let Some(res) = crate::files::free_name(&self.project, &dir, name) else { return };
        let r = (|| -> Result<(), String> {
            let bytes = self.vfs.read(path)?;
            let to = self.project.res_to_path(&res).ok_or("not a project path")?;
            std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&to, bytes).map_err(|e| e.to_string())
        })();
        match r {
            Ok(()) => {
                self.file_undo.push(FileOp::Created(vec![res.clone()]));
                self.log(LogKind::Info, format!("copied {} to {}", path, res));
                self.asset_dir = res.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                self.asset_selected = Some(res);
            }
            Err(e) => self.log(LogKind::Error, format!("couldn't copy {}: {}", path, e)),
        }
    }

    /// Undoes the newest file change from the Assets panel.
    pub fn undo_file_op(&mut self) {
        let Some(op) = self.file_undo.pop() else { return };
        let depth = self.file_undo.len();
        let r = match &op {
            FileOp::Renamed { old, new } => {
                self.rename_asset(new, old).map(|_| ())
            }
            FileOp::Trashed { res, at, fixed, open } => match self.project.res_to_path(res) {
                Some(to) if to.exists() => Err(format!("{} exists again, so it wasn't put back", res)),
                Some(to) => {
                    let _ = std::fs::create_dir_all(to.parent().unwrap());
                    std::fs::rename(at, &to).map_err(|e| e.to_string()).map(|_| {
                        // The blueprints that used it get it back: files on disk, and open blueprints
                        // that haven't been changed since.
                        for (path, text) in fixed {
                            let _ = std::fs::write(path, text);
                        }
                        for (blueprint, depth) in open {
                            self.with_blueprint(blueprint, |st| {
                                if st.undo.len() == *depth {
                                    st.undo();
                                    st.redo.pop();
                                }
                            });
                        }
                        self.asset_selected = Some(res.clone());
                        self.log(LogKind::Info, format!("put {} back", res));
                    })
                }
                None => Err("not a project path".into()),
            },
            FileOp::Created(list) => {
                let mut r = Ok(());
                for res in list.iter().rev() {
                    // Close a tab showing it first.
                    self.scripts.remove(res);
                    if let Err(e) = self.delete_asset(res) {
                        r = Err(e);
                    }
                }
                r
            }
        };
        // Undoing doesn't itself become something to undo.
        self.file_undo.truncate(depth);
        if let Err(e) = r {
            self.log(LogKind::Error, format!("couldn't undo the file change: {}", e));
        }
    }

    /// Selects just this object.
    pub fn select(&mut self, id: Option<ObjectId>) {
        self.selected = id;
        self.selection = id.into_iter().collect();
    }

    /// Adds or removes an object from the selection (Ctrl+click).
    pub fn toggle_select(&mut self, id: ObjectId) {
        if let Some(i) = self.selection.iter().position(|s| *s == id) {
            self.selection.remove(i);
            if self.selected == Some(id) {
                self.selected = self.selection.last().copied();
            }
        } else {
            self.selection.push(id);
            self.selected = Some(id);
        }
    }

    pub fn is_selected(&self, id: ObjectId) -> bool {
        self.selected == Some(id) || self.selection.contains(&id)
    }

    /// The selected objects that still exist, without any whose parent is also selected.
    pub fn selected_roots(&self) -> Vec<ObjectId> {
        let tree = &self.tree;
        let mut all: Vec<ObjectId> = self.selection.iter().copied().chain(self.selected).filter(|i| tree.contains(*i)).collect();
        all.sort();
        all.dedup();
        let set = all.clone();
        all.into_iter().filter(|i| !set.iter().any(|o| o != i && tree.is_ancestor(*o, *i))).collect()
    }

    pub fn delete_selected(&mut self) {
        let ids = self.selected_roots();
        if ids.is_empty() {
            return;
        }
        if ids.iter().any(|i| Some(*i) == self.tree.root) {
            self.status = "the root object can't be deleted".into();
            return;
        }
        if ids.iter().any(|i| self.tree.objects[*i].from_instance) {
            self.status = "objects inside an instanced blueprint can't be deleted here (hide them, or edit that blueprint)".into();
            return;
        }
        self.will_change("delete");
        let parent = self.tree.objects[ids[0]].parent;
        for id in ids {
            self.tree.remove(id);
        }
        self.select(parent);
    }

    pub fn duplicate_selected(&mut self) {
        let ids = self.selected_roots();
        if ids.is_empty() || ids.iter().any(|i| self.tree.objects[*i].parent.is_none()) {
            return;
        }
        self.will_change("duplicate");
        let copy = self.tree.clone();
        let mut new = Vec::new();
        for id in ids {
            let parent = self.tree.objects[id].parent;
            new.push(self.tree.copy_subtree(&copy, id, parent));
        }
        self.selected = new.last().copied();
        self.selection = new;
    }

    /// The selected objects as text (for the clipboard): a list of blueprint-file object entries.
    pub fn copy_text(&self) -> Option<String> {
        let ids = self.selected_roots();
        if ids.is_empty() {
            return None;
        }
        let read = self.reader();
        let descs: Vec<blueprint::ObjectDesc> = ids.iter().filter_map(|i| blueprint::describe(&self.tree, *i, &read).ok()).collect();
        ron::ser::to_string_pretty(&descs, ron::ser::PrettyConfig::new().struct_names(false)).ok()
    }

    /// Pastes objects copied with `copy_text` (here or from another blueprint) next to the selection.
    pub fn paste_text(&mut self, text: &str) -> bool {
        let Ok(descs) = ron::from_str::<Vec<blueprint::ObjectDesc>>(text) else { return false };
        if descs.is_empty() || self.playing.is_some() {
            return false;
        }
        self.will_change("paste");
        let parent = self.selected.and_then(|s| self.tree.objects.get(s).and_then(|n| n.parent)).or(self.tree.root);
        let file = blueprint::BlueprintFile { format: blueprint::FORMAT, root: blueprint::ObjectDesc { name: "Paste".into(), kind: Some(gex_core::ObjectKind::Object), children: descs, ..Default::default() } };
        let Ok(tmp) = blueprint::load(&blueprint::to_text(&file), &self.reader()) else { return false };
        let troot = tmp.root.unwrap();
        let mut new = Vec::new();
        for c in tmp.objects[troot].children.clone() {
            new.push(self.tree.copy_subtree(&tmp, c, parent));
        }
        self.selected = new.last().copied();
        self.selection = new;
        true
    }

    // ---------- scripts ----------

    pub fn open_script(&mut self, res: &str, line: Option<usize>) {
        if !self.scripts.contains_key(res) {
            let text = self.project.read_text(res).unwrap_or_default();
            self.scripts.insert(
                res.to_string(),
                Script { res: res.to_string(), saved: text.clone(), text, problems: Vec::new(), checked_text: String::from("\0"), goto_line: None, vim: Default::default(), find: None },
            );
        }
        if let Some(s) = self.scripts.get_mut(res) {
            s.goto_line = line;
        }
        self.open_requests.push(res.to_string());
    }

    pub fn save_script(&mut self, res: &str) {
        let Some(s) = self.scripts.get_mut(res) else { return };
        let Some(p) = self.project.res_to_path(res) else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match std::fs::write(&p, &s.text) {
            Ok(()) => {
                s.saved = s.text.clone();
                self.status = format!("saved {}", res);
                if res.ends_with(".jlb") {
                    // A running game picks the change up at once.
                    if let Some(p) = &self.playing {
                        p.handle.reload(res);
                    }
                } else if let Some(r) = &mut self.renderer {
                    // A shader (or other file the renderer read): load it again.
                    r.reload_textures();
                }
            }
            Err(e) => {
                let m = format!("couldn't save {}: {}", res, e);
                self.log(LogKind::Error, m);
            }
        }
    }

    /// Toggles a breakpoint on a script line.
    pub fn toggle_breakpoint(&mut self, res: &str, line: usize) {
        let set = self.breakpoints.entry(res.to_string()).or_default();
        if !set.remove(&line) {
            set.insert(line);
        } else {
            self.bp_conditions.remove(&(res.to_string(), line));
        }
        self.sync_breakpoints();
    }

    /// Gives a breakpoint a condition (making the breakpoint if needed); empty removes it.
    pub fn set_breakpoint_condition(&mut self, res: &str, line: usize, expr: &str) {
        let key = (res.to_string(), line);
        if expr.trim().is_empty() {
            self.bp_conditions.remove(&key);
        } else {
            self.breakpoints.entry(res.to_string()).or_default().insert(line);
            self.bp_conditions.insert(key, expr.trim().to_string());
        }
        if let Some(p) = self.project.res_to_path(res).and_then(|p| p.canonicalize().ok()) {
            self.game_breakpoints.set_condition(p, line, expr);
        }
        self.sync_breakpoints();
    }

    /// Hands the breakpoints to the game (by the files' real paths).
    pub fn sync_breakpoints(&self) {
        for (res, lines) in &self.breakpoints {
            if let Some(p) = self.project.res_to_path(res).and_then(|p| p.canonicalize().ok()) {
                self.game_breakpoints.set(p.clone(), lines.iter().copied().collect());
                for l in lines {
                    self.game_breakpoints.set_condition(p.clone(), *l, self.bp_conditions.get(&(res.clone(), *l)).map(|s| s.as_str()).unwrap_or(""));
                }
            }
        }
        self.game_breakpoints.set_watches(self.watches.clone());
    }

    /// The text of a script: the open buffer if it's open, else the file.
    pub fn script_text(&self, res: &str) -> String {
        match self.scripts.get(res) {
            Some(s) => s.text.clone(),
            None => self.project.read_text(res).unwrap_or_default(),
        }
    }

    /// Makes a new script file for an object and attaches it.
    pub fn new_script_for(&mut self, id: ObjectId) -> Option<String> {
        self.new_script_named(id, "")
    }

    /// Adds an event to an object the way picking it from "Add event" does: in a script of
    /// its own named after the object and the event ("bullet_step"), made and attached if it
    /// isn't there yet, and opened at the event.
    pub fn add_event_script(&mut self, id: ObjectId, signature: &str) {
        let event: String = signature.chars().take_while(|c| *c != '(').collect();
        let event = event.trim_start_matches("on_").to_string();
        let wanted = format!("{}_{}.jlb", Self::script_base(&self.tree.objects[id].name), event);
        // The event may be there already, in any of the object's scripts: go to it.
        let def = format!("{}(", signature.chars().take_while(|c| *c != '(').collect::<String>());
        let scripts = self.tree.objects[id].scripts.clone();
        let has = |s: &&String| {
            let text = self.scripts.get(*s).map(|o| o.text.clone()).or_else(|| self.project.read_text(s).ok()).unwrap_or_default();
            text.lines().any(|l| l.trim_start().strip_prefix("DEF ").is_some_and(|r| r.trim_start().starts_with(&def)))
        };
        let have = scripts.iter().find(has).or_else(|| scripts.iter().find(|s| s.rsplit('/').next() == Some(wanted.as_str()))).cloned();
        if let Some(res) = have.or_else(|| self.new_script_named(id, &event)) {
            self.add_event(&res, signature);
        }
    }

    /// A file name for an object's script: its name in lowercase with underscores.
    fn script_base(name: &str) -> String {
        let base: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        if base.is_empty() { "script".to_string() } else { base }
    }

    /// Makes a script for an object, attaches and opens it. `event`: the event it's for
    /// (part of its name), or "" for a general script.
    fn new_script_named(&mut self, id: ObjectId, event: &str) -> Option<String> {
        let base = Self::script_base(&self.tree.objects[id].name);
        let base = if event.is_empty() { base } else { format!("{}_{}", base, event) };
        // Next to the blueprint it's for (no folders are made for you).
        let dir = self.blueprint_res.as_deref().and_then(|r| r.rsplit_once('/')).map(|(d, _)| d.trim_end_matches('/').to_string()).filter(|d| d.len() > "res:/".len()).unwrap_or_else(|| "res:/".into());
        let mut res = format!("{}/{}.jlb", dir, base);
        let mut i = 2;
        while self.project.res_to_path(&res).is_some_and(|p| p.exists()) {
            res = format!("{}/{}_{}.jlb", dir, base, i);
            i += 1;
        }
        let text = format!(">> {}'s script. `self` is the object.\n", self.tree.objects[id].name);
        let p = self.project.res_to_path(&res)?;
        let _ = std::fs::create_dir_all(p.parent()?);
        if let Err(e) = std::fs::write(&p, text) {
            self.log(LogKind::Error, format!("couldn't make {}: {}", res, e));
            return None;
        }
        self.will_change("script");
        self.tree.objects[id].scripts.push(res.clone());
        self.open_script(&res, None);
        Some(res)
    }

    /// Adds an event function to a script (if it isn't there) and opens it there.
    pub fn add_event(&mut self, res: &str, signature: &str) {
        self.open_script(res, None);
        let name: String = signature.chars().take_while(|c| *c != '(').collect();
        let Some(s) = self.scripts.get_mut(res) else { return };
        let line = s.text.lines().position(|l| {
            let l = l.trim_start();
            l.strip_prefix("DEF ").is_some_and(|r| r.trim_start().starts_with(&format!("{}(", name)))
        });
        let line = match line {
            Some(l) => l + 1,
            None => {
                if !s.text.ends_with('\n') && !s.text.is_empty() {
                    s.text.push('\n');
                }
                let at = s.text.lines().count() + 2;
                s.text.push_str(&format!("\nDEF {} {{\n\t\n}}\n", signature));
                at
            }
        };
        s.goto_line = Some(line + 1);
    }

    // ---------- playing ----------

    /// Before Play (F5): the main blueprint is opened, to run the game from its start; the
    /// one being edited opens again when the game stops.
    pub fn play_from_start(&mut self) {
        let main = self.project.config.main_blueprint.clone();
        if self.blueprint_res.as_deref() == Some(main.as_str()) || !self.project.res_to_path(&main).is_some_and(|p| p.is_file()) {
            return;
        }
        let back = self.blueprint_res.clone();
        self.open_blueprint(&main);
        if self.blueprint_res.as_deref() == Some(main.as_str()) {
            self.return_to = back;
        }
    }

    pub fn play(&mut self) {
        if self.playing.is_some() {
            return;
        }
        if self.blueprint_res.is_none() {
            self.status = "nothing to play yet: make a blueprint first (+ in Assets)".into();
            return;
        }
        // Scripts and the blueprints a game switches to run from disk: everything is saved first.
        self.save_all();
        self.logs.retain(|l| l.kind == LogKind::Error && l.file.is_none());
        self.sync_breakpoints();
        let handle = GameHandle::start_with(self.project.clone(), self.tree.clone(), self.game_breakpoints.clone());
        self.playing = Some(Playing {
            handle,
            list: gex_core::render::extract(&self.tree),
            tree: self.tree.clone(),
            last: Instant::now(),
            keys: HashSet::new(),
            taps: HashSet::new(),
            wheel: 0.0,
            ui_events: Vec::new(),
            axes: Default::default(),
            mouse: [0.0; 2],
            buttons: HashSet::new(),
            game_camera: true,
            focus_view: true,
            mouse_delta: [0.0; 2],
            stats: Default::default(),
            last_frame: Instant::now(),
            locked: false,
            lock_suspended: false,
        });
        self.status = "F5 or F6 stops the game".into();
    }

    pub fn stop(&mut self) {
        if let Some(mut p) = self.playing.take() {
            p.handle.stop();
            for m in p.handle.poll() {
                if let FromGame::Stopped { logs } | FromGame::Frame { logs, .. } = m {
                    self.logs.extend(logs);
                }
            }
            self.status = "stopped".into();
        }
    }

    /// Runs one step of the play loop: ask for a frame, take what came back.
    pub fn pump_game(&mut self) {
        let Some(p) = &mut self.playing else { return };
        let mut stopped = false;
        let mut paused_at = None;
        for m in p.handle.poll() {
            match m {
                FromGame::Frame { list, tree, logs, stats } => {
                    p.stats.push_back((p.last_frame.elapsed().as_secs_f32() * 1000.0, stats));
                    p.last_frame = Instant::now();
                    while p.stats.len() > 240 {
                        p.stats.pop_front();
                    }
                    p.list = list;
                    if let Some(t) = tree {
                        p.tree = t;
                    }
                    self.logs.extend(logs);
                }
                FromGame::Stopped { logs } => {
                    self.logs.extend(logs);
                    stopped = true;
                }
                FromGame::Paused(at) => paused_at = Some(at),
            }
        }
        if stopped {
            self.playing = None;
            self.status = "the game ended".into();
            return;
        }
        if let Some(at) = paused_at {
            // Show where it stopped.
            if let Some(res) = self.project.dir.canonicalize().ok().and_then(|d| std::path::Path::new(&at.file).strip_prefix(&d).ok().map(|r| format!("res://{}", r.to_string_lossy().replace('\\', "/")))) {
                self.open_script(&res, Some(at.line));
            }
            self.status = format!("paused at line {}: F8 continue, F10 step, F5 stop", at.line);
            return;
        }
        let Some(p) = &mut self.playing else { return };
        if p.handle.paused.is_some() {
            return;
        }
        let now = Instant::now();
        if !p.handle.waiting {
            let dt = now.duration_since(p.last).as_secs_f64();
            p.last = now;
            let mut keys = p.keys.clone();
            keys.extend(p.taps.drain());
            p.handle.request_frame(gex_script::FrameInput {
                dt,
                keys,
                mouse: p.mouse,
                buttons: p.buttons.clone(),
                wheel: std::mem::take(&mut p.wheel),
                mouse_delta: std::mem::take(&mut p.mouse_delta),
                axes: p.axes.clone(),
                ui: std::mem::take(&mut p.ui_events),
            });
        }
    }

    /// The tree to show in the hierarchy/inspector: the live game's while playing.
    pub fn view_tree(&self) -> &BlueprintTree {
        match &self.playing {
            Some(p) => &p.tree,
            None => &self.tree,
        }
    }
}

impl EditorState {
    /// Makes a standalone copy of the game in `dest`: the GEX program (renamed after the
    /// game) and the project's own files in dest/game. Files from mounted game installs are
    /// never copied; blueprints or scripts that use them are listed as warnings.
    /// Exports for this computer, named after the project.
    #[allow(dead_code)]
    pub fn export_game(&mut self, dest: &Path) -> Result<Vec<String>, String> {
        let (p, player) = crate::export::players().into_iter().next().ok_or("can't find the GEX player")?;
        self.export_game_for(dest, p, &player, &self.project.config.name.clone())
    }

    /// Build: saves everything, copies the game as it is now to a temporary folder and runs
    /// that in a window of its own, with the project's own window and graphics settings. The
    /// copy is deleted when the game closes; nothing is kept (unlike Export).
    pub fn build_and_run(&mut self) {
        self.build = None; // closes a build that's still running
        self.save_all();
        // Like Play, it starts in the blueprint being edited.
        let start = self.blueprint_res.clone();
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("gex-build-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        let run = || -> Result<Build, String> {
            let mut stack = vec![self.project.dir.clone()];
            while let Some(d) = stack.pop() {
                let out = dir.join(d.strip_prefix(&self.project.dir).unwrap());
                std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
                    let name = e.file_name();
                    if name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                    if e.path().is_dir() {
                        stack.push(e.path());
                    } else {
                        std::fs::copy(e.path(), out.join(&name)).map_err(|x| format!("{}: {}", e.path().display(), x))?;
                    }
                }
            }
            if let Some(start) = &start {
                let mut cfg = self.project.config.clone();
                cfg.main_blueprint = start.clone();
                let text = ron::ser::to_string_pretty(&cfg, ron::ser::PrettyConfig::new().struct_names(false)).map_err(|e| e.to_string())?;
                std::fs::write(dir.join(gex_core::project::PROJECT_FILE), text).map_err(|e| e.to_string())?;
            }
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let mut child = std::process::Command::new(exe)
                .arg("--play")
                .arg(&dir)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("couldn't start the game: {}", e))?;
            // What the game prints comes back to the console.
            let (tx, lines) = std::sync::mpsc::channel();
            if let Some(err) = child.stderr.take() {
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    for l in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                });
            }
            Ok(Build { child, dir: dir.clone(), lines })
        };
        match run() {
            Ok(b) => {
                self.build = Some(b);
                self.status = "Built. The game is running in its own window.".into();
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                self.log(LogKind::Error, format!("Build: {}", e));
            }
        }
    }

    /// Looks after the built game: passes on what it prints, and cleans up once it's closed.
    /// True while it's running.
    pub fn poll_build(&mut self) -> bool {
        let Some(b) = self.build.as_mut() else { return false };
        let lines: Vec<String> = b.lines.try_iter().collect();
        let done = b.child.try_wait().map(|s| s.is_some()).unwrap_or(true);
        for l in lines {
            // The player's own notes (graphics driver and such) aren't the game's.
            if l.starts_with('[') || l.starts_with("ALSA") || l.starts_with("libEGL") || l.trim().is_empty() {
                continue;
            }
            self.log(LogKind::Print, l);
        }
        if done {
            self.build = None;
            self.status = "The built game was closed.".into();
        }
        !done
    }

    /// Exports for one system, with the given player program, as `name`.
    pub fn export_game_for(&mut self, dest: &Path, platform: crate::export::Platform, player: &Path, name: &str) -> Result<Vec<String>, String> {
        use crate::export::Platform;
        self.save_all();
        let name = crate::export::program_name(name);
        let (target, game_dir) = crate::export::layout(dest, platform, &name);
        std::fs::create_dir_all(&game_dir).map_err(|e| format!("{}: {}", game_dir.display(), e))?;
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut program = std::fs::read(player).map_err(|e| format!("reading the player {}: {}", player.display(), e))?;
        let mut notes = Vec::new();
        if !self.project.config.icon.is_empty() {
            match platform {
                Platform::Windows => match gex_assets::load_image(&self.vfs, &self.project.config.icon) {
                    Ok(img) => match crate::export::set_exe_icon(&program, &img.pixels, img.width, img.height) {
                        Ok(p) => program = p,
                        Err(e) => notes.push(format!("the program keeps the GEX icon: {}", e)),
                    },
                    Err(e) => notes.push(format!("couldn't read the icon {}: {}", self.project.config.icon, e)),
                },
                Platform::MacOs => {
                    // Finder wants an .icns; the window and Dock icon is set when the game runs.
                }
                Platform::Linux => {}
            }
        }
        std::fs::write(&target, &program).map_err(|e| format!("writing the program: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        }
        if platform == Platform::MacOs {
            let contents = target.parent().and_then(|p| p.parent()).unwrap();
            std::fs::write(contents.join("Info.plist"), crate::export::info_plist(&name)).map_err(|e| e.to_string())?;
        }
        if platform == Platform::Linux && !cfg!(unix) {
            notes.push("on Linux, the program may need to be marked runnable once: chmod +x".into());
        }
        let mut warnings = notes;
        let schemes: Vec<String> = self.project.config.mounts.iter().map(|m| format!("{}://", m.scheme)).collect();
        let dest_canon = dest.canonicalize().unwrap_or(dest.to_path_buf());
        let mut stack = vec![self.project.dir.clone()];
        let mut files = 0;
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
                let p = e.path();
                let fname = e.file_name().to_string_lossy().into_owned();
                if fname.starts_with('.') || p.canonicalize().map(|c| c == dest_canon || c.starts_with(&dest_canon)).unwrap_or(false) {
                    continue;
                }
                let rel = p.strip_prefix(&self.project.dir).unwrap().to_path_buf();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let out = game_dir.join(&rel);
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                if fname == gex_core::project::PROJECT_FILE {
                    // The exported game reads only its own files.
                    let mut cfg = self.project.config.clone();
                    cfg.mounts.clear();
                    let text = ron::ser::to_string_pretty(&cfg, ron::ser::PrettyConfig::new().struct_names(false)).map_err(|e| e.to_string())?;
                    std::fs::write(&out, text).map_err(|e| e.to_string())?;
                    continue;
                }
                if fname.ends_with(".jblueprint") || fname.ends_with(".jlb") {
                    let text = std::fs::read_to_string(&p).unwrap_or_default();
                    for s in &schemes {
                        if text.contains(s.as_str()) {
                            warnings.push(format!("{} uses files from {} (they aren't included, so they'll be missing)", rel.display(), s));
                        }
                    }
                }
                std::fs::copy(&p, &out).map_err(|e| format!("{}: {}", rel.display(), e))?;
                files += 1;
            }
        }
        self.status = format!("exported {} file(s) to {}", files, dest.display());
        Ok(warnings)
    }
}

/// Where the editor keeps its own settings (recent projects).
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("gex")
}

pub fn recent_projects() -> Vec<PathBuf> {
    std::fs::read_to_string(config_dir().join("recent.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .filter(|p| p.join(gex_core::project::PROJECT_FILE).is_file())
        .collect()
}

pub fn remember_project(dir: &Path) {
    let mut list = recent_projects();
    list.retain(|p| p != dir);
    list.insert(0, dir.to_path_buf());
    list.truncate(40);
    let _ = std::fs::create_dir_all(config_dir());
    let text: Vec<String> = list.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let _ = std::fs::write(config_dir().join("recent.txt"), text.join("\n"));
}

fn write_recent(list: &[PathBuf]) {
    let _ = std::fs::create_dir_all(config_dir());
    let text: Vec<String> = list.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let _ = std::fs::write(config_dir().join("recent.txt"), text.join("\n"));
}

/// Takes a project off the launcher's list (its folder isn't touched).
pub fn forget_project(dir: &Path) {
    let mut list = recent_projects();
    list.retain(|p| p != dir);
    write_recent(&list);
}

/// A project's folder was renamed.
pub fn replace_project(old: &Path, new: &Path) {
    let list: Vec<PathBuf> = recent_projects().into_iter().map(|p| if p == old { new.to_path_buf() } else { p }).collect();
    // recent_projects drops folders that are gone; the new one exists by now.
    let mut list = list;
    if !list.iter().any(|p| p == new) {
        list.insert(0, new.to_path_buf());
    }
    write_recent(&list);
}

/// A built game running in its own window, and the temporary folder it runs from.
pub struct Build {
    child: std::process::Child,
    dir: PathBuf,
    lines: std::sync::mpsc::Receiver<String>,
}

impl Drop for Build {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
