//! A running game: the node tree plus every script attached to it, driven one frame at
//! a time. Lives on one thread (the .jlb interpreter isn't thread-safe); see `thread.rs`
//! for running it beside the editor.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use gex_core::input::Input;
use gex_core::project::Project;
use gex_core::{scene, NodeId, NodeKind, SceneTree, Variant};
use jlb::embed::{fault_location, fault_report, named_table, num};
use jlb::{Env, Fault, Interp, Value};

use crate::commands::{self, EngineHost};
use crate::convert::{id_value, to_value, to_variant};

pub const PHYSICS_HZ: f64 = 60.0;

#[derive(Clone, Debug, PartialEq)]
pub enum LogKind {
    Print,
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub kind: LogKind,
    pub text: String,
    /// Where an error happened: the script file on disk and the line.
    pub file: Option<String>,
    pub line: usize,
}

/// A node as a task tag, and back.
pub fn node_tag(id: NodeId) -> u64 {
    use slotmap::Key;
    id.data().as_ffi()
}
pub fn tag_node(tag: u64) -> NodeId {
    NodeId::from(slotmap::KeyData::from_ffi(tag))
}

/// Nodes in this group keep running while the game is paused (a pause menu).
pub const PAUSE_GROUP: &str = "while_paused";

/// Events that stop for paused nodes. Custom events (SEND / EMIT), on_create and on_destroy
/// still run.
fn pausable(handler: &str) -> bool {
    ["on_step", "on_physics", "on_draw", "on_timer", "on_key_", "on_press_", "on_release_", "on_mouse_", "on_collide", "on_pressed", "on_text_", "on_animation_end", "on_tween_end", "when_"]
        .iter()
        .any(|p| handler.starts_with(p))
}

/// Where a frame's time went (for the editor's profiler).
#[derive(Clone, Debug, Default)]
pub struct FrameStats {
    pub total_ms: f32,
    pub scripts_ms: f32,
    pub physics_ms: f32,
    /// The slowest event handlers this frame: (script, handler, ms).
    pub slowest: Vec<(String, String, f32)>,
    pub nodes: usize,
    pub sprites: usize,
    pub meshes: usize,
}

/// One frame's input from the window.
#[derive(Clone, Debug, Default)]
pub struct FrameInput {
    pub dt: f64,
    /// Keys (and gamepad buttons, "pad_a"...) held down.
    pub keys: HashSet<String>,
    pub mouse: [f32; 2],
    pub buttons: HashSet<String>,
    /// Mouse wheel movement this frame (positive is up).
    pub wheel: f32,
    /// Raw mouse movement since the last frame (works while the mouse is locked).
    pub mouse_delta: [f32; 2],
    /// Gamepad sticks and triggers: "left_x", "left_y", "right_x", "right_y", "lt", "rt".
    pub axes: HashMap<String, f32>,
    pub ui: Vec<UiEvent>,
}

#[derive(Clone, Debug)]
pub enum UiEvent {
    Pressed(NodeId),
    Changed(NodeId, String),
    Submitted(NodeId, String),
}

fn jle_vec(x: f32, y: f32) -> glam::Vec3 {
    glam::Vec3::new(x, y, 0.0)
}

/// A screen fade: out (to `color`, then the scene changes) or in (from it).
#[derive(Clone, Debug)]
pub struct Fade {
    pub to_scene: Option<String>,
    pub time: f32,
    pub length: f32,
    pub color: [f32; 4],
}

impl Fade {
    /// How much of the screen the color covers now, 0 to 1.
    pub fn amount(&self) -> f32 {
        let t = (self.time / self.length.max(0.001)).clamp(0.0, 1.0);
        if self.to_scene.is_some() {
            t
        } else {
            1.0 - t
        }
    }
}

pub(crate) enum Pending {
    /// Run `handler` on one node's scripts, or on every node's when `target` is None.
    Event { target: Option<NodeId>, handler: String, args: Vec<Value> },
    /// A node (and its subtree) just appeared: attach scripts, then on_create.
    Created(NodeId),
    Destroy(NodeId),
}

pub(crate) struct Timer {
    pub node: NodeId,
    pub name: String,
    pub left: f64,
    pub every: Option<f64>,
}

/// What scripts can reach through engine commands, while the interpreter runs.
pub struct WorldState {
    pub tree: SceneTree,
    pub input: Input,
    pub time: f64,
    pub dt: f64,
    pub project: Project,
    pub logs: Vec<LogLine>,
    pub quit: bool,
    pub change_scene: Option<String>,
    /// Node whose script is running now (what NULL / no target means in commands).
    pub current: Option<NodeId>,
    /// Collision owners overlapping right now.
    pub touching: HashSet<(NodeId, NodeId)>,
    pub(crate) queue: VecDeque<Pending>,
    pub(crate) timers: Vec<Timer>,
    pub physics: crate::physics::Physics,
    pub audio: crate::audio::Audio,
    pub vfs: gex_assets::Vfs,
    pub axes: HashMap<String, f32>,
    pub wheel: f32,
    pub mouse_delta: [f32; 2],
    /// A script asked for the mouse to be hidden and held (first-person look).
    pub mouse_locked: bool,
    pub fullscreen: Option<bool>,
    pub window_title: Option<String>,
    /// DRAW_* commands from this frame.
    pub draws: Vec<crate::commands::Draw>,
    /// Sprite animations: node → (animation, time, ended).
    pub(crate) anims: HashMap<NodeId, (String, f64, bool)>,
    /// Model crossfades: node → (from animation, its time, progress 0..1, length).
    pub(crate) anim_blends: HashMap<NodeId, (String, f32, f32, f32)>,
    /// Tilemap collision boxes: node → (what they were made from, boxes).
    pub(crate) tile_boxes: HashMap<NodeId, (String, Vec<crate::physics::StaticBox2>)>,
    /// Collision meshes made from Mesh3D models: node → (what they were made from, mesh).
    pub(crate) mesh_colliders: HashMap<NodeId, (String, crate::physics::StaticMesh)>,
    /// REMEMBER'd values: they outlive CHANGE_SCENE (but not the game).
    pub shared: HashMap<String, serde_json::Value>,
    /// A fade to a color (before a CHANGE_SCENE) or from it (after one).
    pub fade: Option<Fade>,
    /// Where the 2D camera looks after smoothing and limits.
    pub cam2_center: Option<glam::Vec2>,
    /// SHAKE: strength left (pixels in 2D, centimetres in 3D) and how fast it fades per second.
    pub shake: (f32, f32),
    shake_seed: u32,
    pub(crate) tweens: Vec<crate::tween::Tween>,
    /// Animators playing a clip: node → (clip, seconds in).
    pub(crate) clips_playing: HashMap<NodeId, (String, f32)>,
    /// Loaded .anim files.
    pub(crate) anim_files: HashMap<String, Arc<gex_core::anim::AnimFile>>,
    /// PAUSE: only nodes in the "while_paused" group keep getting frame, input and timer events.
    pub paused: bool,
    /// TIME_SCALE: 0.5 is slow motion, 2 is double speed.
    pub time_scale: f64,
    /// Particles2D emitters and their live particles.
    pub(crate) particles: HashMap<NodeId, crate::particles::Emitter>,
    pub(crate) particles_3d: HashMap<NodeId, crate::particles::Emitter3>,
}

impl WorldState {
    /// An Animator's clips (loaded once).
    pub fn anim_file(&mut self, animator: NodeId) -> Result<Arc<gex_core::anim::AnimFile>, String> {
        let res = self.tree.nodes[animator].text("clips").to_string();
        if res.is_empty() {
            return Err(format!("the Animator {} has no clips file yet (make clips in the Timeline)", self.tree.nodes[animator].name));
        }
        if let Some(f) = self.anim_files.get(&res) {
            return Ok(f.clone());
        }
        let f = Arc::new(gex_core::anim::AnimFile::parse(&self.project.read_text(&res)?).map_err(|e| format!("{}: {}", res, e))?);
        self.anim_files.insert(res, f.clone());
        Ok(f)
    }

    /// Paused, and this node isn't one that runs while paused.
    pub fn frozen(&self, id: NodeId) -> bool {
        self.paused && !self.tree.get(id).is_some_and(|n| n.groups.iter().any(|g| g == PAUSE_GROUP))
    }

    pub fn log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.logs.push(LogLine { kind, text: text.into(), file: None, line: 0 });
    }
}

struct ScriptInst {
    res: String,
    env: Env,
    /// The on_/when_ functions this script defines.
    handlers: HashSet<String>,
    when_was: HashMap<String, bool>,
    dead: bool,
}

pub struct Game {
    /// Timings gathered during the frame.
    pub stats: FrameStats,
    pub(crate) it: Interp,
    pub st: Rc<RefCell<WorldState>>,
    scripts: HashMap<NodeId, Vec<ScriptInst>>,
    physics_acc: f64,
    started: bool,
}

impl Drop for Game {
    fn drop(&mut self) {
        // Let task threads end rather than wait forever.
        self.it.shutdown_tasks();
    }
}

/// PRINT output goes to the game's log.
struct LogWriter(Rc<RefCell<WorldState>>, Vec<u8>);
impl Write for LogWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.1.extend_from_slice(b);
        while let Some(i) = self.1.iter().position(|c| *c == b'\n') {
            let line: Vec<u8> = self.1.drain(..=i).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]).trim_end_matches('\r').to_string();
            self.0.borrow_mut().log(LogKind::Print, text);
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Game {
    /// A game over `tree` (usually a copy of the scene being edited, so node ids match).
    pub fn new(project: Project, tree: SceneTree, cancel: Option<Arc<AtomicBool>>) -> Game {
        commands::register();
        let st = Rc::new(RefCell::new(WorldState {
            tree,
            input: Input { actions: project.config.actions.clone(), ..Default::default() },
            time: 0.0,
            dt: 0.0,
            project: project.clone(),
            logs: Vec::new(),
            quit: false,
            change_scene: None,
            current: None,
            touching: HashSet::new(),
            queue: VecDeque::new(),
            timers: Vec::new(),
            physics: crate::physics::Physics::new(project.config.gravity_2d, project.config.gravity_3d),
            audio: Default::default(),
            vfs: gex_assets::Vfs::new(&project.dir, &project.config),
            axes: HashMap::new(),
            wheel: 0.0,
            mouse_delta: [0.0; 2],
            mouse_locked: false,
            fullscreen: None,
            window_title: None,
            draws: Vec::new(),
            anims: HashMap::new(),
            anim_blends: HashMap::new(),
            mesh_colliders: HashMap::new(),
            tile_boxes: HashMap::new(),
            particles: HashMap::new(),
            particles_3d: HashMap::new(),
            tweens: Vec::new(),
            clips_playing: HashMap::new(),
            anim_files: HashMap::new(),
            paused: false,
            time_scale: 1.0,
            shared: HashMap::new(),
            fade: None,
            cam2_center: None,
            shake: (0.0, 0.0),
            shake_seed: 12345,
        }));
        let mut it = Interp::embedded(project.dir.clone());
        it.set_output(Box::new(LogWriter(st.clone(), Vec::new())));
        it.host = Some(Box::new(EngineHost { st: st.clone() }));
        it.cancel = cancel;
        Game { stats: FrameStats::default(), it, st, scripts: HashMap::new(), physics_acc: 0.0, started: false }
    }

    /// Loads a scene file as a fresh game.
    pub fn from_scene(project: Project, scene_res: &str, cancel: Option<Arc<AtomicBool>>) -> Result<Game, String> {
        let text = project.read_text(scene_res)?;
        let p2 = project.clone();
        let tree = scene::load(&text, &|p: &str| p2.read_text(p)).map_err(|e| e.0)?;
        Ok(Game::new(project, tree, cancel))
    }

    pub fn tree(&self) -> std::cell::Ref<'_, SceneTree> {
        std::cell::Ref::map(self.st.borrow(), |s| &s.tree)
    }

    pub fn take_logs(&mut self) -> Vec<LogLine> {
        std::mem::take(&mut self.st.borrow_mut().logs)
    }

    /// Attaches every script and runs on_create, parents first.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.spawn_map_props();
        {
            // Animators with an autoplay clip start it.
            let mut st = self.st.borrow_mut();
            let auto: Vec<(NodeId, String)> = st.tree.walk().into_iter().filter(|i| st.tree.nodes[*i].kind == NodeKind::Animator && !st.tree.nodes[*i].text("autoplay").is_empty()).map(|i| (i, st.tree.nodes[i].text("autoplay").to_string())).collect();
            for (id, clip) in auto {
                st.clips_playing.insert(id, (clip, 0.0));
            }
        }
        let ids = self.st.borrow().tree.walk();
        self.attach_all(&ids);
        self.autoplay(&ids);
        for id in &ids {
            self.run(*id, "on_create", vec![]);
        }
        self.drain();
    }

    /// Maps (a Mesh3D showing a .bsp) bring their physics props to life: each becomes a
    /// Body3D (its outline as the shape) holding the model, under the map's node.
    fn spawn_map_props(&mut self) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let maps: Vec<NodeId> = st
            .tree
            .walk()
            .into_iter()
            .filter(|i| {
                let n = &st.tree.nodes[*i];
                n.kind == NodeKind::Mesh3D && n.text("mesh").to_ascii_lowercase().ends_with(".bsp") && n.prop("map_props").is_none_or(|v| v.as_bool())
            })
            .collect();
        for map in maps {
            let mesh = st.tree.nodes[map].text("mesh").to_string();
            let scheme = mesh.split_once("://").map(|s| s.0).unwrap_or("res").to_string();
            let Ok(model) = gex_assets::model::cached(&st.vfs, &mesh) else { continue };
            for e in &model.entities {
                let get = |k: &str| e.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str()).unwrap_or("");
                let class = get("classname").to_ascii_lowercase();
                if !(class.starts_with("prop_physics") || class == "prop_ragdoll") {
                    continue;
                }
                let file = get("model").replace('\\', "/");
                if file.is_empty() {
                    continue;
                }
                let m = gex_assets::model::entity_transform(e);
                let (_, r, t) = m.to_scale_rotation_translation();
                let (y, x, z) = r.to_euler(glam::EulerRot::YXZ);
                let name = match get("targetname") {
                    "" => file.rsplit('/').next().unwrap_or("prop").trim_end_matches(".mdl").to_string(),
                    n => n.to_string(),
                };
                let mut body = gex_core::Node::new(name, NodeKind::Body3D);
                body.transform.position = t;
                body.transform.rotation = glam::Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
                body.props.insert("shape".into(), Variant::Text("hull".into()));
                body.groups.push("map_props".into());
                let b = st.tree.add(Some(map), body);
                let mut look = gex_core::Node::new("Model", NodeKind::Mesh3D);
                look.props.insert("mesh".into(), Variant::Text(format!("{}://{}", scheme, file.to_ascii_lowercase())));
                st.tree.add(Some(b), look);
            }
            // Doors and other moving brushes become nodes of their own (USE_DOOR opens them),
            // and the map itself is drawn without them.
            let base = gex_assets::model::map_base(&mesh).to_string();
            for e in &model.entities {
                let get = |k: &str| e.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str()).unwrap_or("");
                let class = get("classname").to_ascii_lowercase();
                let brush = gex_assets::model::MOVERS.contains(&class.as_str());
                if !brush && class != "prop_door_rotating" {
                    continue;
                }
                let name = match get("targetname") {
                    "" => class.clone(),
                    n => n.to_string(),
                };
                let mut door = gex_core::Node::new(name, NodeKind::Mesh3D);
                let place = gex_assets::model::entity_transform(e);
                let (_, r, t) = place.to_scale_rotation_translation();
                let (y, x, z) = r.to_euler(glam::EulerRot::YXZ);
                door.transform.position = t;
                if !brush {
                    door.transform.rotation = glam::Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
                }
                let model_path = if brush {
                    format!("{}{}", base, get("model").replace('*', "#*"))
                } else {
                    format!("{}://{}", scheme, get("model").replace('\\', "/").to_ascii_lowercase())
                };
                door.props.insert("mesh".into(), Variant::Text(model_path.clone()));
                door.props.insert("collision".into(), Variant::Bool(true));
                door.props.insert("door".into(), Variant::Text(class.clone()));
                door.props.insert("door_open".into(), Variant::Bool(false));
                let num = |k: &str, d: f64| get(k).trim().parse::<f64>().unwrap_or(d);
                if class == "func_door" || class == "func_movelinear" || class == "func_button" {
                    // Slides along movedir by its own size, less the lip.
                    let a: Vec<f32> = get("movedir").split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    let (pitch, yaw) = (a.first().copied().unwrap_or(0.0).to_radians(), a.get(1).copied().unwrap_or(0.0).to_radians());
                    let d = glam::Vec3::new(yaw.cos() * pitch.cos(), -pitch.sin(), -(yaw.sin() * pitch.cos()));
                    let size = gex_assets::model::cached(&st.vfs, &model_path).map(|m| m.max - m.min).unwrap_or(glam::Vec3::ONE);
                    let travel = if class == "func_door" || class == "func_button" {
                        (d.abs().dot(size) - num("lip", 0.0) as f32 * 0.0254).max(0.0)
                    } else {
                        num("movedistance", 100.0) as f32 * 0.0254
                    };
                    let m = d * travel;
                    door.props.insert("door_move_x".into(), Variant::Num(m.x as f64));
                    door.props.insert("door_move_y".into(), Variant::Num(m.y as f64));
                    door.props.insert("door_move_z".into(), Variant::Num(m.z as f64));
                    door.props.insert("door_time".into(), Variant::Num((travel as f64 / (num("speed", 100.0).max(1.0) * 0.0254)).clamp(0.1, 20.0)));
                } else {
                    let turn = num("distance", 90.0);
                    door.props.insert("door_turn".into(), Variant::Num(turn));
                    door.props.insert("door_time".into(), Variant::Num((turn.abs() / num("speed", 100.0).max(1.0)).clamp(0.1, 20.0)));
                }
                door.groups.push("map_doors".into());
                st.tree.add(Some(map), door);
            }
            st.tree.nodes[map].props.insert("mesh".into(), Variant::Text(format!("{}?static", base)));
        }
    }

    /// Starts Sound nodes that play by themselves.
    fn autoplay(&mut self, ids: &[NodeId]) {
        let mut st = self.st.borrow_mut();
        for id in ids {
            let auto = st.tree.get(*id).is_some_and(|n| n.kind == NodeKind::Sound && n.prop("autoplay").is_some_and(|v| v.as_bool()));
            if auto {
                if let Err(e) = crate::commands::play_node(&mut st, *id) {
                    st.log(LogKind::Error, e);
                }
            }
        }
    }

    fn attach_all(&mut self, ids: &[NodeId]) {
        for id in ids {
            let scripts = match self.st.borrow().tree.get(*id) {
                Some(n) => n.scripts.clone(),
                None => continue,
            };
            for res in scripts {
                self.attach(*id, &res);
            }
        }
    }

    fn attach(&mut self, id: NodeId, res: &str) {
        let Some(path) = self.st.borrow().project.res_to_path(res) else {
            self.st.borrow_mut().log(LogKind::Error, format!("{} isn't a project path (scripts live in res://)", res));
            return;
        };
        let handlers = scan_handlers(&path);
        let (me, before) = self.self_table(id);
        self.st.borrow_mut().current = Some(id);
        match self.it.load_module(&path, &[("self", me)]) {
            Ok(env) => {
                self.read_self_back(id, &env, &before);
                self.scripts.entry(id).or_default().push(ScriptInst {
                    res: res.to_string(),
                    env,
                    handlers,
                    when_was: HashMap::new(),
                    dead: false,
                })
            }
            Err(f) => self.report(&f, res),
        }
    }

    /// Gives the interpreter a debugger hook (breakpoints and stepping).
    pub fn set_pause_hook(&mut self, hook: jlb::host::PauseHook) {
        self.it.pause = Some(hook);
    }

    /// Loads a changed script again on every node that uses it, keeping the values of
    /// outside variables it still has (so a running game keeps its state).
    pub fn reload_script(&mut self, res: &str) -> usize {
        let Some(path) = self.st.borrow().project.res_to_path(res) else { return 0 };
        let handlers = scan_handlers(&path);
        let users: Vec<(NodeId, usize)> = self
            .scripts
            .iter()
            .flat_map(|(id, list)| list.iter().enumerate().filter(|(_, s)| s.res == res).map(move |(i, _)| (*id, i)))
            .collect();
        let mut done = 0;
        for (id, i) in users {
            let old = self.scripts[&id][i].env.clone();
            let (me, before) = self.self_table(id);
            self.st.borrow_mut().current = Some(id);
            match self.it.load_module(&path, &[("self", me)]) {
                Ok(env) => {
                    self.read_self_back(id, &env, &before);
                    for (k, v) in Interp::env_vars(&old) {
                        if !v.is_func() && &*k != "self" && Interp::env_get(&env, &k).is_some() {
                            Interp::env_set(&env, &k, v);
                        }
                    }
                    let s = &mut self.scripts.get_mut(&id).unwrap()[i];
                    s.env = env;
                    s.handlers = handlers.clone();
                    s.dead = false;
                    done += 1;
                }
                Err(f) => self.report(&f, res),
            }
        }
        if done > 0 {
            self.st.borrow_mut().log(LogKind::Info, format!("reloaded {} on {} node(s)", res, done));
        }
        done
    }

    /// Gives every running task (START) its turn: up to its next WAIT, AWAIT or a slice of
    /// statements. Tasks see their node's `self` like events do.
    fn run_tasks(&mut self) {
        let tags = self.it.live_task_tags();
        if tags.is_empty() {
            return;
        }
        let mut nodes: Vec<NodeId> = tags.into_iter().map(tag_node).collect();
        nodes.sort();
        nodes.dedup();
        let mut envs = Vec::new();
        for id in nodes {
            if !self.st.borrow().tree.contains(id) {
                continue;
            }
            let Some(list) = self.scripts.get(&id) else { continue };
            let list: Vec<Env> = list.iter().map(|s| s.env.clone()).collect();
            for env in list {
                let (me, before) = self.self_table(id);
                Interp::env_set(&env, "self", me);
                envs.push((id, env, before));
            }
        }
        self.st.borrow_mut().current = None;
        let clock = self.st.borrow().time;
        let r = self.it.run_tasks(clock);
        for (id, env, before) in envs {
            self.read_self_back(id, &env, &before);
        }
        if let Err(f) = r {
            self.report(&f, "a task");
        }
        self.drain();
    }

    fn report(&mut self, f: &Fault, res: &str) {
        let mut st = self.st.borrow_mut();
        match f {
            Fault::End => st.quit = true,
            Fault::Cancelled => st.log(LogKind::Info, "stopped"),
            _ => {
                let text = fault_report(f).unwrap_or_default();
                let (file, line) = fault_location(f).unwrap_or_default();
                st.logs.push(LogLine {
                    kind: LogKind::Error,
                    text: format!("{}  [{} is turned off until the game restarts]", text, res),
                    file: Some(file).filter(|f| !f.is_empty()),
                    line,
                });
            }
        }
    }

    /// The `self` table a script sees, and what it held (to tell what the script changed).
    fn self_table(&self, id: NodeId) -> (Value, HashMap<String, Variant>) {
        let st = self.st.borrow();
        let Some(n) = st.tree.get(id) else { return (Value::Null, HashMap::new()) };
        let t = &n.transform;
        let mut snap: Vec<(String, Variant)> = vec![
            ("name".into(), Variant::Text(n.name.clone())),
            ("x".into(), Variant::Num(t.position.x as f64)),
            ("y".into(), Variant::Num(t.position.y as f64)),
            ("z".into(), Variant::Num(t.position.z as f64)),
            ("rotation".into(), Variant::Num(t.rotation.z as f64)),
            ("rotation_x".into(), Variant::Num(t.rotation.x as f64)),
            ("rotation_y".into(), Variant::Num(t.rotation.y as f64)),
            ("scale_x".into(), Variant::Num(t.scale.x as f64)),
            ("scale_y".into(), Variant::Num(t.scale.y as f64)),
            ("scale_z".into(), Variant::Num(t.scale.z as f64)),
            ("visible".into(), Variant::Bool(n.visible)),
        ];
        for (k, v) in &n.props {
            snap.push((k.clone(), v.clone()));
        }
        let mut entries: Vec<(&str, Value)> = vec![("id", id_value(id)), ("kind", Value::text(n.kind.name()))];
        for (k, v) in &snap {
            entries.push((k.as_str(), to_value(v)));
        }
        let table = named_table(entries);
        (table, snap.into_iter().collect())
    }

    /// Applies what a script changed in `self` back to the node.
    fn read_self_back(&mut self, id: NodeId, env: &Env, before: &HashMap<String, Variant>) {
        let Some(Value::Table(t, _)) = Interp::env_get(env, "self") else { return };
        let mut st = self.st.borrow_mut();
        let Some(n) = st.tree.get_mut(id) else { return };
        for (k, slot) in t.named.iter() {
            let k: &str = k;
            if k == "id" || k == "kind" {
                continue;
            }
            let like = before.get(k).or_else(|| n.props.get(k));
            let Some(v) = to_variant(&slot.borrow(), like) else { continue };
            if before.get(k) == Some(&v) {
                continue;
            }
            let f = v.as_f64().unwrap_or(0.0) as f32;
            let tr = &mut n.transform;
            match k {
                "name" => n.name = v.as_str().unwrap_or(&n.name).to_string(),
                "x" => tr.position.x = f,
                "y" => tr.position.y = f,
                "z" => tr.position.z = f,
                "rotation" | "rotation_z" => tr.rotation.z = f,
                "rotation_x" => tr.rotation.x = f,
                "rotation_y" => tr.rotation.y = f,
                "scale_x" => tr.scale.x = f,
                "scale_y" => tr.scale.y = f,
                "scale_z" => tr.scale.z = f,
                "visible" => n.visible = v.as_bool(),
                _ => {
                    n.props.insert(k.to_string(), v);
                }
            }
        }
    }

    /// Runs `handler` in each of the node's scripts that defines it.
    pub fn run(&mut self, id: NodeId, handler: &str, args: Vec<Value>) {
        let count = self.scripts.get(&id).map_or(0, |s| s.len());
        for i in 0..count {
            self.run_one(id, i, handler, args.clone());
        }
    }

    fn run_one(&mut self, id: NodeId, i: usize, handler: &str, args: Vec<Value>) -> Option<Value> {
        let (env, res) = {
            let s = self.scripts.get(&id)?.get(i)?;
            if s.dead || !s.handlers.contains(handler) {
                return None;
            }
            (s.env.clone(), s.res.clone())
        };
        if pausable(handler) && self.st.borrow().frozen(id) {
            return None;
        }
        let f = Interp::env_get(&env, handler)?;
        if !f.is_func() {
            return None;
        }
        let (me, before) = self.self_table(id);
        Interp::env_set(&env, "self", me);
        self.st.borrow_mut().current = Some(id);
        // Tasks this event STARTs belong to this node.
        self.it.event_tag = node_tag(id);
        let t0 = std::time::Instant::now();
        let r = self.it.call_value(&f, args);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.stats.scripts_ms += ms;
        if ms > 0.05 {
            self.stats.slowest.push((res.clone(), handler.to_string(), ms));
            self.stats.slowest.sort_by(|a, b| b.2.total_cmp(&a.2));
            self.stats.slowest.truncate(5);
        }
        self.read_self_back(id, &env, &before);
        match r {
            Ok(v) => Some(v),
            Err(f) => {
                self.report(&f, &res);
                if !matches!(f, Fault::End) {
                    if let Some(s) = self.scripts.get_mut(&id).and_then(|v| v.get_mut(i)) {
                        s.dead = true;
                    }
                }
                None
            }
        }
    }

    /// Runs `handler` on every node that has it, parents first.
    pub fn broadcast(&mut self, handler: &str, args: Vec<Value>) {
        let ids: Vec<NodeId> = self.st.borrow().tree.walk();
        for id in ids {
            if self.scripts.get(&id).is_some_and(|s| s.iter().any(|s| s.handlers.contains(handler))) {
                self.run(id, handler, args.clone());
            }
        }
    }

    /// One frame of the game (the simple form, for tools and tests).
    pub fn frame(&mut self, dt: f64, keys_down: HashSet<String>, mouse: [f32; 2], mouse_down: HashSet<String>) {
        self.step(FrameInput { dt, keys: keys_down, mouse, buttons: mouse_down, ..Default::default() });
    }

    /// One frame of the game.
    pub fn step(&mut self, input: FrameInput) {
        self.start();
        let real_dt = input.dt.clamp(0.0, 0.25);
        let moved;
        let paused;
        let dt;
        {
            let mut st = self.st.borrow_mut();
            moved = st.input.mouse != input.mouse;
            st.input.next_frame(input.keys, input.mouse, input.buttons);
            st.axes = input.axes;
            st.wheel = input.wheel;
            st.mouse_delta = input.mouse_delta;
            paused = st.paused;
            // While paused, the nodes that still run see real time; the rest is frozen.
            dt = if paused { real_dt } else { real_dt * st.time_scale.max(0.0) };
            st.dt = dt;
            if !paused {
                st.time += dt;
            }
            st.draws.clear();
        }
        // UI first: a click belongs to the frame it happened in.
        for ev in input.ui {
            self.ui_event(ev);
        }
        // Input events.
        let (pressed, released, clicks, mouse) = {
            let st = self.st.borrow();
            let mut p: Vec<String> = st.input.pressed.iter().cloned().collect();
            let mut r: Vec<String> = st.input.released.iter().cloned().collect();
            let mut c: Vec<String> = st.input.mouse_pressed.iter().cloned().collect();
            p.sort();
            r.sort();
            c.sort();
            (p, r, c, st.input.mouse)
        };
        for k in pressed {
            self.broadcast("on_key_pressed", vec![Value::text(&k)]);
            self.broadcast(&format!("on_press_{}", k), vec![]);
        }
        for k in released {
            self.broadcast("on_key_released", vec![Value::text(&k)]);
            self.broadcast(&format!("on_release_{}", k), vec![]);
        }
        for b in clicks {
            self.broadcast("on_mouse_pressed", vec![Value::text(&b), num(mouse[0] as f64), num(mouse[1] as f64)]);
        }
        if moved {
            self.broadcast("on_mouse_moved", vec![num(mouse[0] as f64), num(mouse[1] as f64)]);
        }
        if input.wheel != 0.0 {
            self.broadcast("on_mouse_wheel", vec![num(input.wheel as f64)]);
        }
        self.tile_statics();
        // Fixed-rate physics and collisions.
        if !paused {
            self.physics_acc += dt;
        }
        let step = 1.0 / PHYSICS_HZ;
        let mut steps = 0;
        while self.physics_acc >= step && steps < 5 {
            self.physics_acc -= step;
            steps += 1;
            self.broadcast("on_physics", vec![num(step)]);
            {
                let t0 = std::time::Instant::now();
                let mut st = self.st.borrow_mut();
                let st = &mut *st;
                st.physics.step(&mut st.tree);
                self.stats.physics_ms += t0.elapsed().as_secs_f32() * 1000.0;
            }
            self.collisions();
        }
        self.broadcast("on_step", vec![num(dt)]);
        self.broadcast("on_draw", vec![]);
        self.animate(dt);
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let frozen: Vec<NodeId> = if st.paused { st.particles.keys().copied().filter(|i| st.frozen(*i)).collect() } else { Vec::new() };
            crate::particles::update(&st.tree, &mut st.particles, dt as f32, &frozen);
            let frozen3: Vec<NodeId> = if st.paused { st.particles_3d.keys().copied().filter(|i| st.frozen(*i)).collect() } else { Vec::new() };
            crate::particles::update_3d(&st.tree, &mut st.particles_3d, dt as f32, &frozen3);
        }
        self.timers(dt);
        let finished = {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let frozen: Vec<NodeId> = if st.paused { st.tweens.iter().map(|t| t.node).filter(|i| st.frozen(*i)).collect() } else { Vec::new() };
            crate::tween::update(&mut st.tree, &mut st.tweens, dt, &frozen)
        };
        for (id, prop) in finished {
            self.run(id, "on_tween_end", vec![Value::text(&prop)]);
        }
        self.play_clips(dt as f32);
        if !self.st.borrow().paused {
            self.run_tasks();
        }
        self.camera_and_fade(dt as f32);
        {
            // Sound nodes report whether they're still playing.
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            st.audio.tidy();
            let playing: Vec<(NodeId, bool)> = st.tree.walk().into_iter().filter(|i| st.tree.nodes[*i].kind == NodeKind::Sound).map(|i| (i, st.audio.by_node.contains_key(&i))).collect();
            // Where the "ears" are: the 3D camera, else the 2D camera (or the screen's middle).
            let cam3 = st.tree.walk().into_iter().find(|i| st.tree.nodes[*i].kind == NodeKind::Camera3D && st.tree.nodes[*i].prop("current").is_none_or(|v| v.as_bool()));
            // ...and which way is right, for the balance.
            let (ear, right) = match cam3 {
                Some(c) => {
                    let m = st.tree.global_matrix(c);
                    (Some((m.w_axis.truncate(), true)), m.x_axis.truncate().normalize_or(glam::Vec3::X))
                }
                None => {
                    let [w, h] = st.project.config.window_size;
                    let c = st.cam2_center.unwrap_or(glam::Vec2::new(w as f32 / 2.0, h as f32 / 2.0));
                    (Some((c.extend(0.0), false)), glam::Vec3::X)
                }
            };
            for (i, p) in playing {
                let n = &st.tree.nodes[i];
                if p {
                    let mut vol = n.num("volume", 1.0) as f32;
                    if let (true, Some((ear, three))) = (n.prop("spatial").is_some_and(|v| v.as_bool()), ear) {
                        let at = st.tree.global_matrix(i).w_axis.truncate();
                        let range = match n.num("range", 0.0) as f32 {
                            r if r > 0.0 => r,
                            _ if three => 25.0,
                            _ => 1000.0,
                        };
                        let d = if three { at.distance(ear) } else { at.truncate().distance(ear.truncate()) };
                        let g = (1.0 - d / range).clamp(0.0, 1.0);
                        vol *= g * g;
                        // Left or right of the listener.
                        let off = at - ear;
                        let side = if three { off.normalize_or_zero().dot(right) } else { (off.x / (range * 0.5)).clamp(-1.0, 1.0) };
                        if let Some(id) = st.audio.by_node.get(&i).copied() {
                            st.audio.set_pan(id, (side * 0.9).clamp(-1.0, 1.0));
                        }
                    } else if let Some(id) = st.audio.by_node.get(&i).copied() {
                        st.audio.set_pan(id, n.num("pan", 0.0) as f32);
                    }
                    if let Some(id) = st.audio.by_node.get(&i).copied() {
                        st.audio.set_volume(id, vol);
                    }
                }
                st.tree.nodes[i].props.insert("playing".into(), Variant::Bool(p));
            }
        }
        self.conditions();
        self.drain();
    }

    /// A click or text change from the UI.
    pub fn ui_event(&mut self, ev: UiEvent) {
        match ev {
            UiEvent::Pressed(id) => {
                let name = match self.st.borrow().tree.get(id) {
                    Some(n) => n.name.clone(),
                    None => return,
                };
                self.run(id, "on_pressed", vec![]);
                let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
                self.broadcast(&format!("on_pressed_{}", slug), vec![]);
            }
            UiEvent::Changed(id, text) => {
                if let Some(n) = self.st.borrow_mut().tree.get_mut(id) {
                    n.props.insert("text".into(), Variant::Text(text.clone()));
                }
                self.run(id, "on_text_changed", vec![Value::text(&text)]);
            }
            UiEvent::Submitted(id, text) => self.run(id, "on_text_submitted", vec![Value::text(&text)]),
        }
    }

    /// Sprite sheet animations: moves each animated Sprite2D to its current frame.
    fn animate(&mut self, dt: f64) {
        let mut ended = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let ids: Vec<NodeId> = st.tree.walk().into_iter().filter(|i| st.tree.nodes[*i].kind == NodeKind::Sprite2D).collect();
            for id in ids {
                if st.frozen(id) {
                    continue;
                }
                let n = &st.tree.nodes[id];
                let name = n.text("animation").to_string();
                if name.is_empty() {
                    st.anims.remove(&id);
                    continue;
                }
                let Some(frames) = gex_core::tiles::animation_frames(n.text("animations"), &name) else { continue };
                if frames.is_empty() {
                    continue;
                }
                let fps = n.num("anim_fps", 10.0).max(0.0);
                let looped = n.prop("anim_loop").is_none_or(|v| v.as_bool());
                let a = st.anims.entry(id).or_insert_with(|| (name.clone(), 0.0, false));
                if a.0 != name {
                    *a = (name.clone(), 0.0, false);
                } else {
                    a.1 += dt;
                }
                let mut k = (a.1 * fps).floor() as usize;
                if k >= frames.len() {
                    if looped {
                        k %= frames.len();
                    } else {
                        k = frames.len() - 1;
                        if !a.2 {
                            a.2 = true;
                            ended.push((id, name.clone()));
                        }
                    }
                }
                st.tree.nodes[id].props.insert("frame".into(), Variant::Num(frames[k] as f64));
            }
        }
        // Model (Mesh3D) animations.
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let ids: Vec<NodeId> = st.tree.walk().into_iter().filter(|i| st.tree.nodes[*i].kind == NodeKind::Mesh3D && !st.frozen(*i)).collect();
            for id in ids {
                let n = &st.tree.nodes[id];
                let name = n.text("animation").to_string();
                let mesh = n.text("mesh").to_string();
                if name.is_empty() || !gex_assets::model::is_model(&mesh) {
                    continue;
                }
                let Ok(model) = gex_assets::model::cached(&st.vfs, &mesh) else { continue };
                let Some(anim) = model.animation(&name) else { continue };
                let speed = n.num("anim_speed", 1.0);
                let looped = n.prop("anim_loop").is_none_or(|v| v.as_bool());
                let blend_len = n.num("anim_blend", 0.2).max(0.0) as f32;
                let a = st.anims.entry(id).or_insert_with(|| (name.clone(), n.num("anim_time", 0.0), false));
                if a.0 != name {
                    // Crossfade from the old animation, frozen where it was.
                    if blend_len > 0.0 && !a.0.is_empty() {
                        st.anim_blends.insert(id, (a.0.clone(), a.1 as f32, 0.0, blend_len));
                    }
                    *a = (name.clone(), 0.0, false);
                }
                a.1 += dt * speed;
                if let Some(b) = st.anim_blends.get_mut(&id) {
                    b.2 += dt as f32 / b.3.max(1e-3);
                    if b.2 >= 1.0 {
                        st.anim_blends.remove(&id);
                    }
                }
                let Some(a) = st.anims.get_mut(&id) else { continue };
                let dur = anim.duration as f64;
                if dur > 0.0 && a.1 >= dur {
                    if looped {
                        a.1 %= dur;
                    } else {
                        a.1 = dur;
                        if !a.2 {
                            a.2 = true;
                            ended.push((id, name.clone()));
                        }
                    }
                }
                let t = a.1;
                st.tree.nodes[id].props.insert("anim_time".into(), Variant::Num(t));
            }
        }
        for (id, name) in ended {
            self.run(id, "on_animation_end", vec![Value::text(&name)]);
        }
    }

    /// Solid tiles become static physics boxes (rows of tiles merged into one box).
    fn tile_statics(&mut self) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let mut boxes = Vec::new();
        let mut keep = HashSet::new();
        for id in st.tree.walk() {
            let n = &st.tree.nodes[id];
            if n.kind != NodeKind::TileMap2D || !n.prop("solid").is_none_or(|v| v.as_bool()) {
                continue;
            }
            keep.insert(id);
            let m = st.tree.global_matrix(id);
            let key = format!("{}|{}|{:?}|{:?}|{:?}", n.text("tiles"), n.text("solid_tiles"), n.props.get("tile_width"), n.props.get("tile_height"), m);
            if let Some((k, b)) = st.tile_boxes.get(&id) {
                if *k == key {
                    boxes.extend_from_slice(b);
                    continue;
                }
            }
            let first = boxes.len();
            let grid = gex_core::tiles::parse(n.text("tiles"));
            let only = gex_core::tiles::solid_set(n.text("solid_tiles"));
            let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
            let solid = |t: i32| t >= 0 && only.as_ref().is_none_or(|o| o.contains(&t));
            for (r, row) in grid.iter().enumerate() {
                let mut c = 0;
                while c < row.len() {
                    if !solid(row[c]) {
                        c += 1;
                        continue;
                    }
                    let start = c;
                    while c < row.len() && solid(row[c]) {
                        c += 1;
                    }
                    let a = m.transform_point3(jle_vec(start as f32 * tw, r as f32 * th));
                    let b = m.transform_point3(jle_vec(c as f32 * tw, (r + 1) as f32 * th));
                    let (lo, hi) = (a.min(b).truncate(), a.max(b).truncate());
                    boxes.push(crate::physics::StaticBox2 { owner: id, center: (lo + hi) / 2.0, half: (hi - lo) / 2.0 });
                }
            }
            st.tile_boxes.insert(id, (key, boxes[first..].to_vec()));
        }
        st.tile_boxes.retain(|k, _| keep.contains(k));
        st.physics.statics_2d = boxes;
        // Mesh3D nodes with collision on become static triangle meshes.
        let mut meshes = Vec::new();
        let mut keep = HashSet::new();
        for id in st.tree.walk() {
            let n = &st.tree.nodes[id];
            if n.kind != NodeKind::Mesh3D || !n.prop("collision").is_some_and(|v| v.as_bool()) {
                continue;
            }
            let mesh = n.text("mesh").to_string();
            let m = st.tree.global_matrix(id);
            let key = format!("{}{:?}", mesh, m);
            keep.insert(id);
            if st.mesh_colliders.get(&id).is_none_or(|c| c.0 != key) {
                let (v, t) = if gex_assets::model::is_model(&mesh) {
                    match gex_assets::model::cached(&st.vfs, &mesh) {
                        Ok(model) => model.triangles(),
                        Err(e) => {
                            st.logs.push(LogLine { kind: LogKind::Error, text: format!("collision for {}: {}", n.name, e), file: None, line: 0 });
                            (Vec::new(), Vec::new())
                        }
                    }
                } else {
                    // Primitives: their unit box (a plane is a thin slab under its surface).
                    let (lo, hi) = if mesh == "plane" { (-0.05, 0.0) } else { (-0.5, 0.5) };
                    let c: Vec<glam::Vec3> = (0..8).map(|i| glam::Vec3::new(if i & 1 == 0 { -0.5 } else { 0.5 }, if i & 2 == 0 { lo } else { hi }, if i & 4 == 0 { -0.5 } else { 0.5 })).collect();
                    let t = vec![[0, 1, 3], [0, 3, 2], [4, 6, 7], [4, 7, 5], [0, 4, 5], [0, 5, 1], [2, 3, 7], [2, 7, 6], [0, 2, 6], [0, 6, 4], [1, 5, 7], [1, 7, 3]];
                    (c, t)
                };
                let v: Vec<glam::Vec3> = v.into_iter().map(|p| m.transform_point3(p)).collect();
                st.mesh_colliders.insert(id, (key, (id, std::sync::Arc::new(v), std::sync::Arc::new(t))));
            }
            if let Some((_, c)) = st.mesh_colliders.get(&id) {
                if !c.1.is_empty() {
                    meshes.push(c.clone());
                }
            }
        }
        st.mesh_colliders.retain(|k, _| keep.contains(k));
        st.physics.meshes_3d = meshes;
        // Body3D with shape "hull": the outline of its first Mesh3D child's model.
        let mut keep = HashSet::new();
        for id in st.tree.walk() {
            let n = &st.tree.nodes[id];
            if n.kind != NodeKind::Body3D || !matches!(n.text("shape"), "hull" | "model") {
                continue;
            }
            let Some(child) = n.children.iter().copied().find(|c| st.tree.nodes[*c].kind == NodeKind::Mesh3D) else { continue };
            let c = &st.tree.nodes[child];
            let mesh = c.text("mesh").to_string();
            let local = c.transform.matrix();
            let key = format!("{}{:?}", mesh, local);
            keep.insert(id);
            if st.physics.hulls.get(&id).is_some_and(|h| h.0 == key) {
                continue;
            }
            let pts: Vec<glam::Vec3> = if gex_assets::model::is_model(&mesh) {
                match gex_assets::model::cached(&st.vfs, &mesh) {
                    Ok(model) => model.parts.iter().flat_map(|p| p.positions.iter().map(|v| local.transform_point3(*v))).collect(),
                    Err(_) => Vec::new(),
                }
            } else {
                (0..8).map(|i| local.transform_point3(glam::Vec3::new(if i & 1 == 0 { -0.5 } else { 0.5 }, if i & 2 == 0 { -0.5 } else { 0.5 }, if i & 4 == 0 { -0.5 } else { 0.5 }))).collect()
            };
            st.physics.hulls.insert(id, (key, std::sync::Arc::new(pts)));
        }
        st.physics.hulls.retain(|k, _| keep.contains(k));
    }

    /// Animators move their clips on; a clip that ends tells the Animator's parent.
    fn play_clips(&mut self, dt: f32) {
        let mut ended = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let ids: Vec<NodeId> = st.clips_playing.keys().copied().collect();
            for id in ids {
                if !st.tree.contains(id) {
                    st.clips_playing.remove(&id);
                    continue;
                }
                if st.frozen(id) {
                    continue;
                }
                let Ok(file) = st.anim_file(id) else { continue };
                let (name, mut t) = st.clips_playing[&id].clone();
                let Some(clip) = file.clip(&name) else {
                    st.clips_playing.remove(&id);
                    continue;
                };
                t += dt * st.tree.nodes[id].num("speed", 1.0) as f32;
                let len = clip.length.max(0.001);
                let mut done = false;
                if t >= len {
                    if clip.looped {
                        t %= len;
                    } else {
                        t = len;
                        done = true;
                    }
                }
                gex_core::anim::apply(&mut st.tree, id, clip, t);
                if done {
                    st.clips_playing.remove(&id);
                    ended.push((st.tree.nodes[id].parent.unwrap_or(id), id, name));
                } else {
                    st.clips_playing.insert(id, (name, t));
                }
            }
        }
        for (owner, id, name) in ended {
            self.run(owner, "on_clip_end", vec![Value::text(&name)]);
            if owner != id {
                self.run(id, "on_clip_end", vec![Value::text(&name)]);
            }
        }
    }

    /// Smooths and limits the 2D camera, runs down SHAKE, and moves fades along.
    fn camera_and_fade(&mut self, dt: f32) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        if let Some(f) = &mut st.fade {
            f.time += dt;
            if f.time >= f.length {
                match f.to_scene.clone() {
                    Some(scene) => {
                        if st.change_scene.is_none() {
                            st.change_scene = Some(scene);
                        }
                    }
                    None => st.fade = None,
                }
            }
        }
        st.shake.0 = (st.shake.0 - st.shake.1 * dt).max(0.0);
        let cam = st.tree.walk().into_iter().find(|i| {
            let n = &st.tree.nodes[*i];
            n.kind == NodeKind::Camera2D && n.prop("current").is_none_or(|v| v.as_bool())
        });
        let Some(cam) = cam else {
            st.cam2_center = None;
            return;
        };
        let n = &st.tree.nodes[cam];
        let target = st.tree.global_matrix(cam).w_axis.truncate().truncate();
        let speed = n.num("smoothing", 0.0) as f32;
        let mut c = match st.cam2_center {
            Some(prev) if speed > 0.0 => prev + (target - prev) * (1.0 - (-speed * dt).exp()),
            _ => target,
        };
        if n.prop("limits").is_some_and(|v| v.as_bool()) {
            let zoom = n.num("zoom", 1.0).max(0.01) as f32;
            let [w, h] = st.project.config.window_size;
            let half = glam::Vec2::new(w as f32, h as f32) / (2.0 * zoom);
            let (l, t, r, b) = (n.num("limit_left", 0.0) as f32, n.num("limit_top", 0.0) as f32, n.num("limit_right", w as f64) as f32, n.num("limit_bottom", h as f64) as f32);
            c.x = if r - l < half.x * 2.0 { (l + r) / 2.0 } else { c.x.clamp(l + half.x, r - half.x) };
            c.y = if b - t < half.y * 2.0 { (t + b) / 2.0 } else { c.y.clamp(t + half.y, b - half.y) };
        }
        st.cam2_center = Some(c);
    }

    fn shake_offset(st: &WorldState) -> glam::Vec2 {
        if st.shake.0 <= 0.0 {
            return glam::Vec2::ZERO;
        }
        // Cheap noise from the time: a new jolt every frame.
        let k = (st.time * 1000.0) as u32 ^ st.shake_seed;
        let h = |x: u32| {
            let x = x.wrapping_mul(0x9E37_79B1).rotate_left(13).wrapping_mul(0x85EB_CA77);
            (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
        };
        glam::Vec2::new(h(k), h(k ^ 0xABCD)) * st.shake.0
    }

    /// What to draw this frame: the tree plus anything scripts drew with DRAW_*.
    pub fn render_list(&self) -> gex_core::render::RenderList {
        let st = self.st.borrow();
        let mut list = gex_core::render::extract(&st.tree);
        crate::particles::draw(&st.tree, &st.particles, &mut list);
        crate::particles::draw_3d(&st.tree, &st.particles_3d, &mut list);
        let ragdolls: HashMap<NodeId, std::sync::Arc<Vec<glam::Mat4>>> = st.physics.ragdoll_poses(&st.tree).into_iter().map(|(id, p)| (id, std::sync::Arc::new(p))).collect();
        for m in &mut list.meshes {
            if let Some(p) = ragdolls.get(&m.node) {
                m.pose = Some(p.clone());
            }
            if let Some((prev, t, k, _)) = st.anim_blends.get(&m.node) {
                m.prev_animation = prev.clone();
                m.prev_time = *t;
                m.blend = k.clamp(0.0, 1.0);
            }
        }
        list.mouse_locked = st.mouse_locked;
        list.fullscreen = st.fullscreen;
        list.window_title = st.window_title.clone();
        let jolt = Self::shake_offset(&st);
        if let Some(cam) = &mut list.camera2d {
            if let Some(c) = st.cam2_center {
                cam.center = c;
            }
            cam.center += jolt;
        }
        if let Some(cam) = &mut list.camera3d {
            cam.world = glam::Mat4::from_translation(glam::Vec3::new(jolt.x, jolt.y, 0.0) * 0.01) * cam.world;
        }
        if let Some(f) = &st.fade {
            let mut c = f.color;
            c[3] *= f.amount();
            list.fade = c;
        }
        crate::commands::append_draws(&st.draws, &mut list);
        list.sprites.sort_by(|a, b| a.layer.total_cmp(&b.layer));
        list
    }

    fn timers(&mut self, dt: f64) {
        let mut fire = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let frozen: HashSet<NodeId> = if st.paused { st.timers.iter().map(|t| t.node).filter(|i| st.frozen(*i)).collect() } else { HashSet::new() };
            let tree = &st.tree;
            st.timers.retain_mut(|t| {
                if !tree.contains(t.node) {
                    return false;
                }
                if frozen.contains(&t.node) {
                    return true;
                }
                t.left -= dt;
                if t.left > 0.0 {
                    return true;
                }
                fire.push((t.node, t.name.clone()));
                match t.every {
                    Some(e) => {
                        t.left += e.max(0.001);
                        true
                    }
                    None => false,
                }
            });
        }
        for (n, name) in fire {
            self.run(n, "on_timer", vec![Value::text(&name)]);
            self.run(n, &format!("on_timer_{}", name), vec![]);
        }
    }

    /// `DEF when_x()` turning true runs `on_x()`.
    fn conditions(&mut self) {
        let mut checks = Vec::new();
        for (id, list) in &self.scripts {
            for (i, s) in list.iter().enumerate() {
                for h in &s.handlers {
                    if h.starts_with("when_") && !s.dead && !self.st.borrow().frozen(*id) {
                        checks.push((*id, i, h.clone()));
                    }
                }
            }
        }
        checks.sort_by(|a, b| a.2.cmp(&b.2));
        for (id, i, h) in checks {
            let now = self.run_one(id, i, &h, vec![]).is_some_and(|v| jlb::ops::truth(&v).unwrap_or(false));
            let Some(s) = self.scripts.get_mut(&id).and_then(|v| v.get_mut(i)) else { continue };
            let was = s.when_was.insert(h.clone(), now).unwrap_or(false);
            if now && !was {
                let on = format!("on_{}", &h[5..]);
                self.run_one(id, i, &on, vec![]);
            }
        }
    }

    fn collisions(&mut self) {
        let (now, before) = {
            let st = self.st.borrow();
            let tree = &st.tree;
            let areas = collect_areas(tree, &st.physics.layers, |id| self.scripts.contains_key(&id));
            let mut now = HashSet::new();
            // Physics bodies touching each other, reported for their owners.
            let owner_of = |id: NodeId| -> NodeId {
                match tree.get(id) {
                    Some(n) if !self.scripts.contains_key(&id) => match n.parent {
                        Some(p) if self.scripts.contains_key(&p) && n.kind != NodeKind::Body2D && n.kind != NodeKind::Body3D => p,
                        _ => id,
                    },
                    _ => id,
                }
            };
            for (a, b) in st.physics.touching() {
                let (a, b) = (owner_of(a), owner_of(b));
                if a != b {
                    now.insert((a, b));
                    now.insert((b, a));
                }
            }
            for i in 0..areas.len() {
                for j in i + 1..areas.len() {
                    let (a, b) = (&areas[i], &areas[j]);
                    if a.owner != b.owner && !(a.body && b.body) && crate::physics::Layers::meet(a.groups, b.groups) && a.shape.overlaps(&b.shape) {
                        now.insert((a.owner, b.owner));
                        now.insert((b.owner, a.owner));
                    }
                }
            }
            (now, st.touching.clone())
        };
        let mut started: Vec<_> = now.difference(&before).copied().collect();
        let mut ended: Vec<_> = before.difference(&now).copied().collect();
        let mut during: Vec<_> = now.iter().copied().collect();
        started.sort();
        ended.sort();
        during.sort();
        self.st.borrow_mut().touching = now;
        for (a, b) in started {
            self.run(a, "on_collide_start", vec![id_value(b)]);
        }
        for (a, b) in during {
            self.run(a, "on_collide", vec![id_value(b)]);
        }
        for (a, b) in ended {
            self.run(a, "on_collide_end", vec![id_value(b)]);
        }
    }

    /// Handles queued events, spawns and destroys (scripts' requests), in order.
    fn drain(&mut self) {
        for _ in 0..10_000 {
            let Some(p) = self.st.borrow_mut().queue.pop_front() else { return };
            match p {
                Pending::Event { target: Some(t), handler, args } => self.run(t, &handler, args),
                Pending::Event { target: None, handler, args } => self.broadcast(&handler, args),
                Pending::Created(id) => {
                    let mut ids = Vec::new();
                    self.st.borrow().tree.walk_from(id, &mut ids);
                    self.attach_all(&ids);
                    self.autoplay(&ids);
                    for i in ids {
                        self.run(i, "on_create", vec![]);
                    }
                }
                Pending::Destroy(id) => {
                    let mut ids = Vec::new();
                    self.st.borrow().tree.walk_from(id, &mut ids);
                    for i in ids.iter().rev() {
                        self.run(*i, "on_destroy", vec![]);
                    }
                    let mut st = self.st.borrow_mut();
                    let gone = st.tree.remove(id);
                    for g in gone {
                        self.it.cancel_tagged(node_tag(g));
                        self.scripts.remove(&g);
                        if let Some(s) = st.audio.by_node.remove(&g) {
                            st.audio.stop(s);
                        }
                    }
                }
            }
        }
        self.st.borrow_mut().log(LogKind::Error, "too many events in one frame (do two scripts keep SENDing to each other?)");
        self.st.borrow_mut().queue.clear();
    }
}

/// A collider's box in world space: [left, top, right, bottom].
/// Something that reports touches without being a physics body (Collider2D / Collider3D), or a
/// body seen as an area so colliders notice it.
struct Area {
    owner: NodeId,
    body: bool,
    groups: (u32, u32),
    shape: AreaShape,
}

enum AreaShape {
    Rect([f32; 4]),
    Box(glam::Vec3, glam::Vec3),
    Ball(glam::Vec3, f32),
}

impl AreaShape {
    fn overlaps(&self, o: &AreaShape) -> bool {
        use AreaShape::*;
        let box_ball = |lo: glam::Vec3, hi: glam::Vec3, c: glam::Vec3, r: f32| c.clamp(lo, hi).distance_squared(c) < r * r;
        match (self, o) {
            (Rect(a), Rect(b)) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3],
            (Box(al, ah), Box(bl, bh)) => al.cmplt(*bh).all() && bl.cmplt(*ah).all(),
            (Ball(a, ra), Ball(b, rb)) => a.distance(*b) < ra + rb,
            (Box(l, h), Ball(c, r)) | (Ball(c, r), Box(l, h)) => box_ball(*l, *h, *c, *r),
            _ => false,
        }
    }
}

fn collect_areas(tree: &SceneTree, layers: &crate::physics::Layers, has_scripts: impl Fn(NodeId) -> bool) -> Vec<Area> {
    let mut out = Vec::new();
    for id in tree.walk() {
        let n = &tree.nodes[id];
        if !matches!(n.kind, NodeKind::Collider2D | NodeKind::Collider3D | NodeKind::Body2D | NodeKind::Body3D) || !tree.visible_in_tree(id) {
            continue;
        }
        let body = matches!(n.kind, NodeKind::Body2D | NodeKind::Body3D);
        // A collider speaks for its parent, unless it has scripts itself.
        let owner = if body || has_scripts(id) || n.parent.is_none() { id } else { n.parent.unwrap() };
        let groups = layers.of(tree, id);
        let shape = match n.kind {
            NodeKind::Collider2D => AreaShape::Rect(aabb(tree, id)),
            NodeKind::Body2D => {
                let mut r = aabb(tree, id);
                if matches!(n.text("shape"), "circle" | "ball") {
                    let c = tree.global_matrix(id).w_axis;
                    let rad = n.num("radius", 16.0) as f32;
                    r = [c.x - rad, c.y - rad, c.x + rad, c.y + rad];
                }
                AreaShape::Rect(r)
            }
            _ => {
                let m = tree.global_matrix(id);
                let (scale, _, pos) = m.to_scale_rotation_translation();
                if matches!(n.text("shape"), "sphere" | "ball") {
                    AreaShape::Ball(pos, n.num("radius", 0.5) as f32 * scale.abs().max_element())
                } else {
                    let half = glam::Vec3::new(n.num("width", 1.0) as f32, n.num("height", 1.0) as f32, n.num("depth", 1.0) as f32) / 2.0;
                    let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
                    for k in 0..8 {
                        let c = glam::Vec3::new(if k & 1 == 0 { -half.x } else { half.x }, if k & 2 == 0 { -half.y } else { half.y }, if k & 4 == 0 { -half.z } else { half.z });
                        let p = m.transform_point3(c);
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                    AreaShape::Box(lo, hi)
                }
            }
        };
        out.push(Area { owner, body, groups, shape });
    }
    out
}

pub fn aabb(tree: &SceneTree, id: NodeId) -> [f32; 4] {
    let n = &tree.nodes[id];
    let m = tree.global_matrix(id);
    let (w, h) = (n.num("width", 32.0) as f32 / 2.0, n.num("height", 32.0) as f32 / 2.0);
    let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for (x, y) in [(-w, -h), (w, -h), (w, h), (-w, h)] {
        let p = m.transform_point3(gex_core::glam::Vec3::new(x, y, 0.0));
        r[0] = r[0].min(p.x);
        r[1] = r[1].min(p.y);
        r[2] = r[2].max(p.x);
        r[3] = r[3].max(p.y);
    }
    r
}

/// The `on_...` and `when_...` functions a script file defines (top-level DEFs).
pub fn scan_handlers(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path).map(|t| scan_handlers_text(&t)).unwrap_or_default()
}

pub fn scan_handlers_text(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for line in text.lines() {
        let l = line.trim_start();
        let rest = l.strip_prefix("DEF ").or_else(|| l.strip_prefix("FUNC ")).or_else(|| l.strip_prefix("GLOBAL DEF "));
        let Some(rest) = rest else { continue };
        let name: String = rest.trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if name.starts_with("on_") || name.starts_with("when_") {
            out.insert(name);
        }
    }
    out
}

/// Where project files live, for tests and tools.
pub fn project_at(dir: PathBuf) -> Result<Project, String> {
    Project::open(&dir)
}
