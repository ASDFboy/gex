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
    Trashed { res: String, at: PathBuf },
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
    pub tree: SceneTree,
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
    /// Raw mouse movement since the last frame request.
    pub mouse_delta: [f32; 2],
    /// The last few seconds of frames: (time between frames, where the game's time went).
    pub stats: std::collections::VecDeque<(f32, gex_script::world::FrameStats)>,
    pub last_frame: Instant,
    /// The mouse is held for the game (MOUSE_LOCK), or Escape let it go for now.
    pub locked: bool,
    pub lock_suspended: bool,
}

/// What dragging a selected node does in the scene view.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Move,
    Rotate,
    Scale,
}

/// A scene that's open but not the one being edited right now.
pub struct SceneDoc {
    pub tree: SceneTree,
    pub scene_res: Option<String>,
    pub scene_saved: String,
    pub selected: Option<NodeId>,
    pub undo: Vec<SceneTree>,
    pub redo: Vec<SceneTree>,
    pub mode_3d: bool,
    pub cam2: EditorCamera2D,
    pub cam3: EditorCamera3D,
}

pub struct EditorState {
    pub project: Project,
    pub vfs: Vfs,
    pub tree: SceneTree,
    pub scene_res: Option<String>,
    pub scene_saved: String,
    pub selected: Option<NodeId>,
    /// Every selected node (`selected` is the main one, shown in the inspector).
    pub selection: Vec<NodeId>,
    /// Other open scenes (the one being edited is in the fields above).
    pub docs: Vec<SceneDoc>,
    pub tool: Tool,
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
    pub show_colliders: bool,
    /// Painting tiles into the selected TileMap2D with this tile (-1 erases).
    pub tile_brush: Option<i32>,
    /// Textures for UI Image nodes and tile palettes.
    pub ui_images: std::collections::HashMap<String, eframe::egui::TextureHandle>,
    /// The 2D camera has been fitted to the game window once.
    pub cam_fitted: bool,
    pub asset_dir: String,
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
    /// Particles2D previews in the scene view (not while playing).
    pub preview_particles: HashMap<NodeId, gex_script::particles::Emitter>,
    pub preview_particles_3d: HashMap<NodeId, gex_script::particles::Emitter3>,
    pub particle_clock: Instant,
    pub search: crate::tools::Search,
    /// Script tabs whose file was renamed: (old, new), for the dock to follow.
    pub renamed_scripts: Vec<(String, String)>,
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
            tree: SceneTree::new(),
            scene_res: None,
            scene_saved: String::new(),
            selected: None,
            selection: Vec::new(),
            docs: Vec::new(),
            tool: Tool::Move,
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
            show_colliders: true,
            tile_brush: None,
            ui_images: Default::default(),
            cam_fitted: false,
            asset_dir: "res://".into(),
            asset_selected: None,
            show_settings: false,
            show_export: false,
            status: String::new(),
            breakpoints: HashMap::new(),
            game_breakpoints: Default::default(),
            preview_particles: HashMap::new(),
            preview_particles_3d: HashMap::new(),
            renamed_scripts: Vec::new(),
            timeline: Default::default(),
            bp_conditions: HashMap::new(),
            bp_edit: None,
            watches: Vec::new(),
            watch_input: String::new(),
            previews: Default::default(),
            file_undo: Vec::new(),
            open_timeline: false,
            search: Default::default(),
            particle_clock: Instant::now(),
            vim_enabled: std::fs::read_to_string(config_dir().join("vim")).map(|s| s.trim() == "on").unwrap_or(false),
            open_command: false,
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

    /// Opens a scene in its own tab (or switches to it if it's open).
    pub fn open_scene(&mut self, res: &str) {
        if self.scene_res.as_deref() == Some(res) {
            return;
        }
        if let Some(i) = self.docs.iter().position(|d| d.scene_res.as_deref() == Some(res)) {
            self.switch_scene(i);
            return;
        }
        if self.scene_res.is_some() || self.tree.nodes.len() > 1 {
            let doc = self.take_doc();
            self.docs.push(doc);
        }
        self.load_scene(res);
    }

    fn take_doc(&mut self) -> SceneDoc {
        self.preview_particles.clear();
        self.preview_particles_3d.clear();
        SceneDoc {
            tree: std::mem::take(&mut self.tree),
            scene_res: self.scene_res.take(),
            scene_saved: std::mem::take(&mut self.scene_saved),
            selected: self.selected.take(),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            mode_3d: self.mode_3d,
            cam2: self.cam2,
            cam3: self.cam3,
        }
    }

    fn put_doc(&mut self, d: SceneDoc) {
        self.tree = d.tree;
        self.scene_res = d.scene_res;
        self.scene_saved = d.scene_saved;
        self.selected = d.selected;
        self.selection = d.selected.into_iter().collect();
        self.undo = d.undo;
        self.redo = d.redo;
        self.mode_3d = d.mode_3d;
        self.cam2 = d.cam2;
        self.cam3 = d.cam3;
        self.last_change = None;
    }

    /// Makes another open scene the one being edited.
    pub fn switch_scene(&mut self, i: usize) {
        if i >= self.docs.len() || self.playing.is_some() {
            return;
        }
        let d = self.docs.remove(i);
        let cur = self.take_doc();
        self.docs.insert(i, cur);
        self.put_doc(d);
    }

    /// Closes the scene being edited (unsaved changes are lost), switching to another.
    pub fn close_scene(&mut self) {
        if self.docs.is_empty() || self.playing.is_some() {
            return;
        }
        let d = self.docs.remove(0);
        self.put_doc(d);
    }

    fn load_scene(&mut self, res: &str) {
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
                self.selection = self.selected.into_iter().collect();
                self.cam_fitted = false;
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
        // Other open scenes with copies of this one: describe them against the file as it was,
        // so after saving they can be rebuilt from the new file with their own changes kept.
        let users: Vec<(usize, Option<String>)> = {
            let read = self.reader();
            self.docs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.tree.nodes.iter().any(|(_, n)| n.instance_of.as_deref() == Some(res.as_str())))
                .map(|(i, d)| (i, scene::save(&d.tree, &read).ok()))
                .collect()
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
                for (i, before) in users {
                    let Some(text) = before else { continue };
                    let rebuilt = scene::load(&text, &|p: &str| self.project.read_text(p));
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
        let tree = &self.tree;
        self.selection.retain(|s| tree.contains(*s));
        if self.selected.is_some_and(|s| !self.tree.contains(s)) {
            self.selected = self.tree.root;
        }
    }

    // ---------- nodes ----------

    pub fn add_node(&mut self, kind: gex_core::NodeKind) -> NodeId {
        self.will_change("add");
        let mut n = gex_core::Node::new(kind.name(), kind);
        if kind.is_ui() {
            n.transform.position = gex_core::glam::Vec3::new(40.0, 40.0, 0.0);
            let id = self.tree.add(self.selected.filter(|s| self.tree.nodes[*s].kind.is_ui()).or(self.tree.root), n);
            self.select(Some(id));
            return id;
        }
        // New 2D nodes appear in the middle of the view.
        if kind.is_2d() && !self.mode_3d {
            let parent_m = self.selected.map(|p| self.tree.global_matrix(p)).unwrap_or_default();
            let local = parent_m.inverse().transform_point3(self.cam2.center.extend(0.0));
            n.transform.position = local;
        }
        let id = self.tree.add(self.selected, n);
        self.select(Some(id));
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

    /// Saves a branch as its own scene file, then puts an instance of that file where the
    /// branch was (so the branch can be reused in other scenes and stays in sync).
    pub fn save_branch_as_scene(&mut self, id: NodeId) -> Option<String> {
        let n = self.tree.get(id)?;
        if n.from_instance {
            self.status = "this node belongs to an instanced scene; open that scene to change it".into();
            return None;
        }
        let slug: String = n.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let slug = if slug.trim_matches('_').is_empty() { "branch".to_string() } else { slug };
        let mut res = format!("res://scenes/{}.jscene", slug);
        let mut k = 2;
        while self.project.res_to_path(&res).is_some_and(|p| p.exists()) {
            res = format!("res://scenes/{}_{}.jscene", slug, k);
            k += 1;
        }
        // The branch on its own, with its root at the origin.
        let mut sub = SceneTree::new();
        let root = sub.copy_subtree(&self.tree, id, None);
        sub.nodes[root].transform = Default::default();
        sub.nodes[root].parent = None;
        let saved = scene::save(&sub, &self.reader());
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
        self.will_change("branch to scene");
        let (name, transform, visible, parent) = {
            let n = &self.tree.nodes[id];
            (n.name.clone(), n.transform, n.visible, n.parent)
        };
        let index = parent.and_then(|p| self.tree.nodes[p].children.iter().position(|c| *c == id));
        let reader = |p: &str| self.project.read_text(p);
        let mut tree = std::mem::take(&mut self.tree);
        let made = scene::instance(&mut tree, parent, &res, &reader);
        self.tree = tree;
        match made {
            Ok(new_id) => {
                self.tree.remove(id);
                let n = &mut self.tree.nodes[new_id];
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

    /// Turns an instance into ordinary nodes of this scene (it stops following its file).
    pub fn make_local(&mut self, id: NodeId) {
        if self.tree.get(id).is_none_or(|n| n.instance_of.is_none()) {
            return;
        }
        self.will_change("make local");
        let mut all = Vec::new();
        self.tree.walk_from(id, &mut all);
        for n in all {
            let node = &mut self.tree.nodes[n];
            node.instance_of = None;
            node.from_instance = false;
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
        let fix_tree = |t: &mut SceneTree| {
            for (_, n) in t.nodes.iter_mut() {
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
        fix_text(&mut self.scene_saved);
        if let Some(x) = self.scene_res.as_deref().and_then(swap) {
            self.scene_res = Some(x);
        }
        for d in &mut self.docs {
            fix_tree(&mut d.tree);
            d.undo.iter_mut().chain(d.redo.iter_mut()).for_each(fix_tree);
            fix_text(&mut d.scene_saved);
            if let Some(x) = d.scene_res.as_deref().and_then(swap) {
                d.scene_res = Some(x);
            }
        }
        let keys: Vec<String> = self.scripts.keys().cloned().collect();
        for k in keys {
            let mut sc = self.scripts.remove(&k).unwrap();
            fix_text(&mut sc.text);
            fix_text(&mut sc.saved);
            let key = match swap(&k) {
                Some(x) => {
                    self.renamed_scripts.push((k.clone(), x.clone()));
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
        if let Some(x) = swap(&self.asset_dir) {
            self.asset_dir = x;
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
        if self.scene_res.as_deref() == Some(res) || self.docs.iter().any(|d| d.scene_res.as_deref() == Some(res)) {
            return Err("close that scene first".into());
        }
        let to = crate::files::trash(&self.project, res)?;
        self.scripts.retain(|k, _| k != res && !k.starts_with(&format!("{}/", res)));
        if self.asset_selected.as_deref() == Some(res) {
            self.asset_selected = None;
        }
        self.log(LogKind::Info, format!("moved {} to {} (Undo file change puts it back)", res, to.display()));
        self.file_undo.push(FileOp::Trashed { res: res.to_string(), at: to });
        self.previews.forget(res);
        Ok(())
    }

    /// Copies files from disk into the project (dropped on the window, or picked).
    pub fn import_files(&mut self, files: &[PathBuf]) {
        let dir = if self.asset_dir.starts_with("res://") { self.asset_dir.clone() } else { "res://assets".to_string() };
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

    /// A new empty scene file in a folder, opened in a tab.
    pub fn new_scene_file(&mut self, dir: &str, kind: gex_core::NodeKind) -> Option<String> {
        let res = crate::files::free_name(&self.project, dir, "new_scene.jscene")?;
        let path = self.project.res_to_path(&res)?;
        let root = if kind.is_3d() { "World" } else { "Main" };
        if let Err(e) = std::fs::write(&path, gex_core::scene::new_scene_text(root, kind)) {
            self.log(LogKind::Error, e.to_string());
            return None;
        }
        self.open_scene(&res);
        self.file_undo.push(FileOp::Created(vec![res.clone()]));
        Some(res)
    }

    /// A new script file in a folder, opened in a tab.
    pub fn new_script_file(&mut self, dir: &str) -> Option<String> {
        let res = crate::files::free_name(&self.project, dir, "new_script.jlb")?;
        let path = self.project.res_to_path(&res)?;
        if let Err(e) = std::fs::write(&path, ">> Attach this to a node in the Events panel.\n\nDEF on_create() {\n\t\n}\n") {
            self.log(LogKind::Error, e.to_string());
            return None;
        }
        self.open_script(&res, None);
        self.file_undo.push(FileOp::Created(vec![res.clone()]));
        Some(res)
    }

    /// Undoes the newest file change from the Assets panel.
    pub fn undo_file_op(&mut self) {
        let Some(op) = self.file_undo.pop() else { return };
        let depth = self.file_undo.len();
        let r = match &op {
            FileOp::Renamed { old, new } => {
                let name = old.rsplit('/').next().unwrap_or(old).to_string();
                self.rename_asset(new, &name).map(|_| ())
            }
            FileOp::Trashed { res, at } => match self.project.res_to_path(res) {
                Some(to) if to.exists() => Err(format!("{} exists again, so it wasn't put back", res)),
                Some(to) => {
                    let _ = std::fs::create_dir_all(to.parent().unwrap());
                    std::fs::rename(at, &to).map_err(|e| e.to_string()).map(|_| {
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

    /// Selects just this node.
    pub fn select(&mut self, id: Option<NodeId>) {
        self.selected = id;
        self.selection = id.into_iter().collect();
    }

    /// Adds or removes a node from the selection (Ctrl+click).
    pub fn toggle_select(&mut self, id: NodeId) {
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

    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selected == Some(id) || self.selection.contains(&id)
    }

    /// The selected nodes that still exist, without any whose parent is also selected.
    pub fn selected_roots(&self) -> Vec<NodeId> {
        let tree = &self.tree;
        let mut all: Vec<NodeId> = self.selection.iter().copied().chain(self.selected).filter(|i| tree.contains(*i)).collect();
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
            self.status = "the root node can't be deleted".into();
            return;
        }
        if ids.iter().any(|i| self.tree.nodes[*i].from_instance) {
            self.status = "nodes inside an instanced scene can't be deleted here (hide them, or edit that scene)".into();
            return;
        }
        self.will_change("delete");
        let parent = self.tree.nodes[ids[0]].parent;
        for id in ids {
            self.tree.remove(id);
        }
        self.select(parent);
    }

    pub fn duplicate_selected(&mut self) {
        let ids = self.selected_roots();
        if ids.is_empty() || ids.iter().any(|i| self.tree.nodes[*i].parent.is_none()) {
            return;
        }
        self.will_change("duplicate");
        let copy = self.tree.clone();
        let mut new = Vec::new();
        for id in ids {
            let parent = self.tree.nodes[id].parent;
            new.push(self.tree.copy_subtree(&copy, id, parent));
        }
        self.selected = new.last().copied();
        self.selection = new;
    }

    /// The selected nodes as text (for the clipboard): a list of scene-file node entries.
    pub fn copy_text(&self) -> Option<String> {
        let ids = self.selected_roots();
        if ids.is_empty() {
            return None;
        }
        let read = self.reader();
        let descs: Vec<scene::NodeDesc> = ids.iter().filter_map(|i| scene::describe(&self.tree, *i, &read).ok()).collect();
        ron::ser::to_string_pretty(&descs, ron::ser::PrettyConfig::new().struct_names(false)).ok()
    }

    /// Pastes nodes copied with `copy_text` (here or from another scene) next to the selection.
    pub fn paste_text(&mut self, text: &str) -> bool {
        let Ok(descs) = ron::from_str::<Vec<scene::NodeDesc>>(text) else { return false };
        if descs.is_empty() || self.playing.is_some() {
            return false;
        }
        self.will_change("paste");
        let parent = self.selected.and_then(|s| self.tree.nodes.get(s).and_then(|n| n.parent)).or(self.tree.root);
        let file = scene::SceneFile { format: scene::FORMAT, root: scene::NodeDesc { name: "Paste".into(), kind: Some(gex_core::NodeKind::Node), children: descs, ..Default::default() } };
        let Ok(tmp) = scene::load(&scene::to_text(&file), &self.reader()) else { return false };
        let troot = tmp.root.unwrap();
        let mut new = Vec::new();
        for c in tmp.nodes[troot].children.clone() {
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
            mouse_delta: [0.0; 2],
            stats: Default::default(),
            last_frame: Instant::now(),
            locked: false,
            lock_suspended: false,
        });
        self.status = "playing — Shift+F5 or ■ to stop".into();
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
            self.status = format!("paused at line {} — F8 continue, F10 step, Shift+F5 stop", at.line);
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
    pub fn view_tree(&self) -> &SceneTree {
        match &self.playing {
            Some(p) => &p.tree,
            None => &self.tree,
        }
    }
}

impl EditorState {
    /// Makes a standalone copy of the game in `dest`: the GEX program (renamed after the
    /// game) and the project's own files in dest/game. Files from mounted game installs are
    /// never copied; scenes or scripts that use them are listed as warnings.
    /// Exports for this computer, named after the project.
    #[allow(dead_code)]
    pub fn export_game(&mut self, dest: &Path) -> Result<Vec<String>, String> {
        let (p, player) = crate::export::players().into_iter().next().ok_or("can't find the GEX player")?;
        self.export_game_for(dest, p, &player, &self.project.config.name.clone())
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
                if fname.ends_with(".jscene") || fname.ends_with(".jlb") {
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
    list.truncate(12);
    let _ = std::fs::create_dir_all(config_dir());
    let text: Vec<String> = list.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let _ = std::fs::write(config_dir().join("recent.txt"), text.join("\n"));
}
