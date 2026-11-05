//! Everything the editor knows: the open project, the scene being edited, selection,
//! undo history, open scripts, the running game.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use gex_assets::Vfs;
use gex_core::project::Project;
use gex_core::render::RenderList;
use gex_core::{scene, NodeId, SceneTree};
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
}

impl Script {
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
}

pub struct Playing {
    pub handle: GameHandle,
    pub list: RenderList,
    pub tree: SceneTree,
    pub last: Instant,
    pub keys: HashSet<String>,
    /// Keys pressed since the last frame request (so a quick tap isn't missed).
    pub taps: HashSet<String>,
    pub mouse: [f32; 2],
    pub buttons: HashSet<String>,
    /// The viewport shows the game's own camera (else the editor camera, to look around).
    pub game_camera: bool,
}

pub struct EditorState {
    pub project: Project,
    pub vfs: Vfs,
    pub tree: SceneTree,
    pub scene_res: Option<String>,
    pub scene_saved: String,
    pub selected: Option<NodeId>,
    undo: Vec<SceneTree>,
    redo: Vec<SceneTree>,
    last_change: Option<(String, Instant)>,
    pub logs: Vec<LogLine>,
    pub scripts: HashMap<String, Script>,
    /// Scripts the dock should open as tabs.
    pub open_requests: Vec<String>,
    pub playing: Option<Playing>,
    pub renderer: Option<Renderer>,
    pub mode_3d: bool,
    pub cam2: EditorCamera2D,
    pub cam3: EditorCamera3D,
    pub show_grid: bool,
    /// The 2D camera has been fitted to the game window once.
    pub cam_fitted: bool,
    pub asset_dir: String,
    pub asset_selected: Option<String>,
    pub show_settings: bool,
    pub status: String,
}

impl EditorState {
    pub fn new(project: Project) -> EditorState {
        let vfs = Vfs::new(&project.dir, &project.config);
        let mut st = EditorState {
            project,
            vfs,
            tree: SceneTree::new(),
            scene_res: None,
            scene_saved: String::new(),
            selected: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_change: None,
            logs: Vec::new(),
            scripts: HashMap::new(),
            open_requests: Vec::new(),
            playing: None,
            renderer: None,
            mode_3d: false,
            cam2: EditorCamera2D::default(),
            cam3: EditorCamera3D::default(),
            show_grid: true,
            cam_fitted: false,
            asset_dir: "res://".into(),
            asset_selected: None,
            show_settings: false,
            status: String::new(),
        };
        for w in st.vfs.warnings() {
            st.log(LogKind::Error, w);
        }
        let main = st.project.config.main_scene.clone();
        if st.project.res_to_path(&main).is_some_and(|p| p.is_file()) {
            st.open_scene(&main);
        } else {
            st.tree.add(None, gex_core::Node::new("Root", gex_core::NodeKind::Node2D));
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

    // ---------- scenes ----------

    pub fn open_scene(&mut self, res: &str) {
        let text = match self.project.read_text(res) {
            Ok(t) => t,
            Err(e) => return self.log(LogKind::Error, format!("couldn't open {}: {}", res, e)),
        };
        let loaded = scene::load(&text, &self.reader());
        match loaded {
            Ok(t) => {
                self.tree = t;
                self.scene_res = Some(res.to_string());
                self.selected = self.tree.root;
                self.undo.clear();
                self.redo.clear();
                self.scene_saved = self.scene_text().unwrap_or_default();
                self.mode_3d = self.tree.walk().iter().any(|id| self.tree.nodes[*id].kind.is_3d());
                self.status = format!("opened {}", res);
            }
            Err(e) => self.log(LogKind::Error, format!("{}: {}", res, e)),
        }
    }

    pub fn scene_text(&self) -> Result<String, String> {
        scene::save(&self.tree, &self.reader()).map_err(|e| e.0)
    }

    pub fn scene_dirty(&self) -> bool {
        self.scene_text().map(|t| t != self.scene_saved).unwrap_or(true)
    }

    pub fn save_scene(&mut self) {
        let res = match &self.scene_res {
            Some(r) => r.clone(),
            None => {
                let r = "res://scenes/untitled.jscene".to_string();
                self.scene_res = Some(r.clone());
                r
            }
        };
        let result = self.scene_text().and_then(|text| {
            let p = self.project.res_to_path(&res).ok_or("not a project path")?;
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, &text).map_err(|e| e.to_string())?;
            Ok(text)
        });
        match result {
            Ok(t) => {
                self.scene_saved = t;
                self.status = format!("saved {}", res);
            }
            Err(e) => self.log(LogKind::Error, format!("couldn't save {}: {}", res, e)),
        }
    }

    pub fn save_all(&mut self) {
        self.save_scene();
        let dirty: Vec<String> = self.scripts.values().filter(|s| s.dirty()).map(|s| s.res.clone()).collect();
        for r in dirty {
            self.save_script(&r);
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
        if self.selected.is_some_and(|s| !self.tree.contains(s)) {
            self.selected = self.tree.root;
        }
    }

    // ---------- nodes ----------

    pub fn add_node(&mut self, kind: gex_core::NodeKind) -> NodeId {
        self.will_change("add");
        let mut n = gex_core::Node::new(kind.name(), kind);
        // New 2D nodes appear in the middle of the view.
        if kind.is_2d() && !self.mode_3d {
            let parent_m = self.selected.map(|p| self.tree.global_matrix(p)).unwrap_or_default();
            let local = parent_m.inverse().transform_point3(self.cam2.center.extend(0.0));
            n.transform.position = local;
        }
        let id = self.tree.add(self.selected, n);
        self.selected = Some(id);
        id
    }

    pub fn instance_scene(&mut self, res: &str) -> Option<NodeId> {
        if Some(res) == self.scene_res.as_deref() {
            self.log(LogKind::Error, "a scene can't contain itself");
            return None;
        }
        self.will_change("instance");
        let parent = self.selected.or(self.tree.root);
        let reader = |p: &str| self.project.read_text(p);
        let mut tree = std::mem::take(&mut self.tree);
        let r = scene::instance(&mut tree, parent, res, &reader);
        self.tree = tree;
        match r {
            Ok(id) => {
                self.selected = Some(id);
                Some(id)
            }
            Err(e) => {
                self.log(LogKind::Error, e.0);
                None
            }
        }
    }

    pub fn delete_selected(&mut self) {
        let Some(id) = self.selected else { return };
        if Some(id) == self.tree.root {
            self.status = "the root node can't be deleted".into();
            return;
        }
        self.will_change("delete");
        let parent = self.tree.nodes[id].parent;
        self.tree.remove(id);
        self.selected = parent;
    }

    pub fn duplicate_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let Some(parent) = self.tree.nodes[id].parent else { return };
        self.will_change("duplicate");
        let copy = self.tree.clone();
        let new = self.tree.copy_subtree(&copy, id, Some(parent));
        self.selected = Some(new);
    }

    // ---------- scripts ----------

    pub fn open_script(&mut self, res: &str, line: Option<usize>) {
        if !self.scripts.contains_key(res) {
            let text = self.project.read_text(res).unwrap_or_default();
            self.scripts.insert(
                res.to_string(),
                Script { res: res.to_string(), saved: text.clone(), text, problems: Vec::new(), checked_text: String::from("\0"), goto_line: None },
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
            }
            Err(e) => {
                let m = format!("couldn't save {}: {}", res, e);
                self.log(LogKind::Error, m);
            }
        }
    }

    /// The text of a script: the open buffer if it's open, else the file.
    pub fn script_text(&self, res: &str) -> String {
        match self.scripts.get(res) {
            Some(s) => s.text.clone(),
            None => self.project.read_text(res).unwrap_or_default(),
        }
    }

    /// Makes a new script file for a node and attaches it.
    pub fn new_script_for(&mut self, id: NodeId) -> Option<String> {
        let base: String = self.tree.nodes[id]
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        let base = if base.is_empty() { "script".to_string() } else { base };
        let mut res = format!("res://scripts/{}.jlb", base);
        let mut i = 2;
        while self.project.res_to_path(&res).is_some_and(|p| p.exists()) {
            res = format!("res://scripts/{}_{}.jlb", base, i);
            i += 1;
        }
        let text = format!(
            ">> {} — attached to {}\n>> Outside variables belong to this node; `self` is the node.\n\nDEF on_create() {{\n\t\n}}\n\nDEF on_step(dt) {{\n\t\n}}\n",
            res.trim_start_matches("res://"),
            self.tree.nodes[id].name
        );
        let p = self.project.res_to_path(&res)?;
        let _ = std::fs::create_dir_all(p.parent()?);
        if let Err(e) = std::fs::write(&p, text) {
            self.log(LogKind::Error, format!("couldn't make {}: {}", res, e));
            return None;
        }
        self.will_change("script");
        self.tree.nodes[id].scripts.push(res.clone());
        self.open_script(&res, Some(7));
        Some(res)
    }

    /// Adds an event function to a script (if it isn't there) and opens it there.
    pub fn add_event(&mut self, res: &str, signature: &str) {
        self.open_script(res, None);
        let name: String = signature.chars().take_while(|c| *c != '(').collect();
        let s = self.scripts.get_mut(res).unwrap();
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

    pub fn play(&mut self) {
        if self.playing.is_some() {
            return;
        }
        // Scripts run from disk, so unsaved edits are saved first.
        let dirty: Vec<String> = self.scripts.values().filter(|s| s.dirty()).map(|s| s.res.clone()).collect();
        for r in dirty {
            self.save_script(&r);
        }
        self.logs.retain(|l| l.kind == LogKind::Error && l.file.is_none());
        let handle = GameHandle::start(self.project.clone(), self.tree.clone());
        self.playing = Some(Playing {
            handle,
            list: gex_core::render::extract(&self.tree),
            tree: self.tree.clone(),
            last: Instant::now(),
            keys: HashSet::new(),
            taps: HashSet::new(),
            mouse: [0.0; 2],
            buttons: HashSet::new(),
            game_camera: true,
        });
        self.status = "playing — Shift+F5 or ■ to stop".into();
    }

    pub fn stop(&mut self) {
        if let Some(mut p) = self.playing.take() {
            p.handle.stop();
            for m in p.handle.poll() {
                let (FromGame::Stopped { logs } | FromGame::Frame { logs, .. }) = m;
                self.logs.extend(logs);
            }
            self.status = "stopped".into();
        }
    }

    /// Runs one step of the play loop: ask for a frame, take what came back.
    pub fn pump_game(&mut self) {
        let Some(p) = &mut self.playing else { return };
        let mut stopped = false;
        for m in p.handle.poll() {
            match m {
                FromGame::Frame { list, tree, logs } => {
                    p.list = list;
                    p.tree = tree;
                    self.logs.extend(logs);
                }
                FromGame::Stopped { logs } => {
                    self.logs.extend(logs);
                    stopped = true;
                }
            }
        }
        if stopped {
            self.playing = None;
            self.status = "the game ended".into();
            return;
        }
        let now = Instant::now();
        if !p.handle.waiting {
            let dt = now.duration_since(p.last).as_secs_f64();
            p.last = now;
            let mut keys = p.keys.clone();
            keys.extend(p.taps.drain());
            p.handle.request_frame(dt, keys, p.mouse, p.buttons.clone());
        }
    }

    /// The tree to show in the hierarchy/inspector: the live game's while playing.
    pub fn view_tree(&self) -> &SceneTree {
        match &self.playing {
            Some(p) => &p.tree,
            None => &self.tree,
        }
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
    list.truncate(12);
    let _ = std::fs::create_dir_all(config_dir());
    let text: Vec<String> = list.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let _ = std::fs::write(config_dir().join("recent.txt"), text.join("\n"));
}
