//! A running game: the object tree plus every script attached to it, driven one frame at
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
use gex_core::{blueprint, ObjectId, ObjectKind, BlueprintTree, Variant};
use jlb::embed::{fault_location, fault_report, num};
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

/// An object as a task tag, and back.
pub fn object_tag(id: ObjectId) -> u64 {
    use slotmap::Key;
    id.data().as_ffi()
}
pub fn tag_object(tag: u64) -> ObjectId {
    ObjectId::from(slotmap::KeyData::from_ffi(tag))
}

/// Objects in this group keep running while the game is paused (a pause menu).
pub const PAUSE_GROUP: &str = "while_paused";

/// Events that stop for paused objects. Custom events (SEND / EMIT), on_create and on_destroy
/// still run.
fn pausable(handler: &str) -> bool {
    ["on_step", "on_physics", "on_draw", "on_draw_gui", "on_timer", "on_key_", "on_press_", "on_release_", "on_mouse_", "on_collide", "on_pressed", "on_text_", "on_animation_end", "on_tween_end", "when_"]
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
    pub objects: usize,
    pub sprites: usize,
    pub meshes: usize,
    /// Every event handler's time this frame, when profiling: (script, handler, ms, calls).
    pub handlers: Vec<(String, String, f32, u32)>,
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
    /// The player switched full screen itself (F11, Alt+Enter): the window's new state.
    pub fullscreen: Option<bool>,
    /// The editor's profiler is open: time every event handler (FrameStats::handlers).
    pub profile: bool,
}

/// True in a built or exported game (`gex --play`), false when playing in the editor.
pub static STANDALONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Debug)]
pub enum UiEvent {
    Pressed(ObjectId),
    Changed(ObjectId, String),
    Submitted(ObjectId, String),
    /// A Checkbox clicked: whether it's ticked now.
    Toggled(ObjectId, bool),
    /// A Slider's handle moved to a value.
    Slid(ObjectId, f64),
    /// A ScrollBox scrolled to a point (pixels from the top).
    Scrolled(ObjectId, f64),
}

fn jle_vec(x: f32, y: f32) -> glam::Vec3 {
    glam::Vec3::new(x, y, 0.0)
}

/// A screen fade: out (to `color`, then the blueprint changes) or in (from it).
#[derive(Clone, Debug)]
pub struct Fade {
    pub to_blueprint: Option<String>,
    pub time: f32,
    pub length: f32,
    pub color: [f32; 4],
}

impl Fade {
    /// How much of the screen the color covers now, 0 to 1.
    pub fn amount(&self) -> f32 {
        let t = (self.time / self.length.max(0.001)).clamp(0.0, 1.0);
        if self.to_blueprint.is_some() {
            t
        } else {
            1.0 - t
        }
    }
}

pub(crate) enum Pending {
    /// Run `handler` on one object's scripts, or on every object's when `target` is None.
    Event { target: Option<ObjectId>, handler: String, args: Vec<Value> },
    /// An object (and its subtree) just appeared: attach scripts, then on_create. The
    /// values SPAWN was given go to the object first.
    Created(ObjectId, Vec<(String, Value)>),
    /// Objects SPAWN made and started at once (see WorldState::adopt): their scripts join.
    Adopt(ObjectId),
    Destroy(ObjectId),
}

pub(crate) struct Timer {
    pub object: ObjectId,
    pub name: String,
    pub left: f64,
    pub every: Option<f64>,
}

/// What scripts can reach through engine commands, while the interpreter runs.
pub struct WorldState {
    pub tree: BlueprintTree,
    pub input: Input,
    pub time: f64,
    pub dt: f64,
    pub project: Project,
    pub logs: Vec<LogLine>,
    pub quit: bool,
    pub change_blueprint: Option<String>,
    /// Object whose script is running now (what NULL / no target means in commands).
    pub current: Option<ObjectId>,
    /// Collision owners overlapping right now.
    pub touching: HashSet<(ObjectId, ObjectId)>,
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
    pub window_title: Option<String>,
    /// DRAW_* commands from this frame.
    pub draws: Vec<crate::commands::Draw>,
    /// This frame's DRAW_* in screen space (screen: TRUE, or inside on_draw_gui).
    pub draws_screen: Vec<crate::commands::Draw>,
    /// This frame's DRAW_* into canvases (target:), by canvas.
    pub canvas_draws: HashMap<String, Vec<crate::commands::Draw>>,
    /// Canvases to clear before this frame's drawing into them (with a color, or their own).
    pub canvas_clear: HashMap<String, Option<[f32; 4]>>,
    /// Surfaces made by SURFACE_CREATE: name → size.
    pub surfaces: HashMap<String, [u32; 2]>,
    pub(crate) surface_count: u64,
    pub(crate) surfaces_freed: RefCell<Vec<String>>,
    /// Running on_draw_gui: DRAW_* are in screen space unless they say otherwise.
    pub(crate) gui: bool,
    /// From screen space (the game window's pixels, (0, 0) its top-left, as with no camera)
    /// to the 2D world, as of the last frame drawn.
    pub(crate) screen_fix: std::cell::Cell<glam::Mat4>,
    /// SET_CURSOR: a picture drawn as the mouse pointer (its size), or the pointer hidden.
    pub(crate) cursor: Option<(String, [f32; 2], [f32; 2])>,
    pub(crate) cursor_hidden: bool,
    /// The full-screen shader's values (POST_SHADER).
    pub(crate) post_values: [f32; 4],
    /// Sprite animations: object → (animation, time, ended).
    pub(crate) anims: HashMap<ObjectId, (String, f64, bool)>,
    /// Model crossfades: object → (from animation, its time, progress 0..1, length).
    pub(crate) anim_blends: HashMap<ObjectId, (String, f32, f32, f32)>,
    /// Tilemap collision boxes: object → (what they were made from, boxes).
    pub(crate) tile_boxes: HashMap<ObjectId, (String, Vec<crate::physics::StaticBox2>)>,
    /// Collision meshes made from Mesh3D models: object → (what they were made from, mesh).
    pub(crate) mesh_colliders: HashMap<ObjectId, (String, crate::physics::StaticMesh)>,
    /// REMEMBER'd values: they outlive CHANGE_BLUEPRINT (but not the game).
    pub shared: HashMap<String, serde_json::Value>,
    /// A fade to a color (before a CHANGE_BLUEPRINT) or from it (after one).
    pub fade: Option<Fade>,
    /// Where the 2D camera looks after smoothing and limits.
    pub cam2_center: Option<glam::Vec2>,
    /// SHAKE: strength left (pixels in 2D, centimetres in 3D) and how fast it fades per second.
    pub shake: (f32, f32),
    shake_seed: u32,
    pub(crate) tweens: Vec<crate::tween::Tween>,
    /// Animators playing a clip: object → (clip, seconds in).
    pub(crate) clips_playing: HashMap<ObjectId, (String, f32)>,
    /// Loaded .anim files.
    pub(crate) anim_files: HashMap<String, Arc<gex_core::anim::AnimFile>>,
    /// PAUSE: only objects in the "while_paused" group keep getting frame, input and timer events.
    pub paused: bool,
    /// TIME_SCALE: 0.5 is slow motion, 2 is double speed.
    pub time_scale: f64,
    /// Particles2D emitters and their live particles.
    pub(crate) particles: HashMap<ObjectId, crate::particles::Emitter>,
    pub(crate) particles_3d: HashMap<ObjectId, crate::particles::Emitter3>,
    /// The blueprint file being played ("" when the tree didn't come from one).
    pub blueprint: String,
    /// A multiplayer game (HOST_GAME / JOIN_GAME).
    pub net: Option<crate::net::Net>,
    /// Pictures' sizes and see-through parts (DRAW_SPRITE, precise mouse hits).
    pub(crate) images: crate::hit::Images,
    /// What each script file has (events, $variables), read once per file.
    pub(crate) script_info: HashMap<PathBuf, ScriptInfoEntry>,
    /// Scripts SPAWN attached at once (and ran on_create on): handed to the World with
    /// Pending::Adopt.
    pub(crate) adopt: Vec<(ObjectId, Vec<ScriptInst>)>,
    /// Pictures to get ready in the background (handed to the next render list).
    pub(crate) preload: Vec<String>,
    /// Goes up when an object loses a property (objects otherwise only gain them), so a
    /// script's `self` only needs its names checked one by one after that.
    pub(crate) props_gen: u64,
}

impl WorldState {
    /// An Animator's clips (loaded once).
    pub fn anim_file(&mut self, animator: ObjectId) -> Result<Arc<gex_core::anim::AnimFile>, String> {
        let res = self.tree.objects[animator].text("clips").to_string();
        if res.is_empty() {
            return Err(format!("the Animator {} has no clips file yet (make clips in the Timeline)", self.tree.objects[animator].name));
        }
        if let Some(f) = self.anim_files.get(&res) {
            return Ok(f.clone());
        }
        let f = Arc::new(gex_core::anim::AnimFile::parse(&self.project.read_text(&res)?).map_err(|e| format!("{}: {}", res, e))?);
        self.anim_files.insert(res, f.clone());
        Ok(f)
    }

    /// Paused, and this object isn't one that runs while paused.
    pub fn frozen(&self, id: ObjectId) -> bool {
        self.paused && !self.tree.get(id).is_some_and(|n| n.groups.iter().any(|g| g == PAUSE_GROUP))
    }

    pub fn log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.logs.push(LogLine { kind, text: text.into(), file: None, line: 0 });
    }
}

pub(crate) struct ScriptInst {
    pub(crate) res: String,
    pub(crate) env: Env,
    /// The on_/when_ functions this script defines.
    pub(crate) handlers: HashSet<String>,
    pub(crate) when_was: HashMap<String, bool>,
    pub(crate) dead: bool,
    /// What this script's `self` holds, entry by entry, as of the last event: kept so the
    /// next event only updates what changed on the object meanwhile.
    pub(crate) sync: SelfSnap,
    /// props_gen when `sync` was last checked name by name.
    pub(crate) sync_gen: u64,
}

pub struct Game {
    /// Timings gathered during the frame.
    pub stats: FrameStats,
    /// Profiling this frame: handler time by (script, handler).
    profile: Option<HashMap<(String, String), (f32, u32)>>,
    pub(crate) it: Interp,
    pub st: Rc<RefCell<WorldState>>,
    scripts: HashMap<ObjectId, Vec<ScriptInst>>,
    physics_acc: f64,
    started: bool,
    /// Persistent objects brought over from the last blueprint: already running (their
    /// scripts keep their variables; on_create doesn't run again).
    carried: HashSet<ObjectId>,
    /// Per script file: its change stamp, its events and its $variables.
    /// Objects the mouse is over (for on_mouse_enter / on_mouse_leave).
    hovered: HashSet<ObjectId>,
    /// Which objects have each event, in tree order (made when first asked, again after the
    /// tree or the scripts change: see `with_handler`).
    handler_index: HashMap<String, Rc<Vec<ObjectId>>>,
    index_key: (u64, u64),
    /// Goes up whenever a script is attached, reloaded or turned off.
    scripts_rev: u64,
}

type ScriptInfoEntry = ((Option<std::time::SystemTime>, u64), Rc<(HashSet<String>, Vec<String>)>);

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
    /// A game over `tree` (usually a copy of the blueprint being edited, so object ids match).
    pub fn new(project: Project, tree: BlueprintTree, cancel: Option<Arc<AtomicBool>>) -> Game {
        commands::register();
        let st = Rc::new(RefCell::new(WorldState {
            tree,
            input: Input { actions: project.config.actions.clone(), ..Default::default() },
            time: 0.0,
            dt: 0.0,
            project: project.clone(),
            logs: Vec::new(),
            quit: false,
            change_blueprint: None,
            current: None,
            touching: HashSet::new(),
            queue: VecDeque::new(),
            timers: Vec::new(),
            physics: crate::physics::Physics::new(project.config.physics.clone()),
            audio: Default::default(),
            vfs: gex_assets::Vfs::new(&project.dir, &project.config),
            axes: HashMap::new(),
            wheel: 0.0,
            mouse_delta: [0.0; 2],
            mouse_locked: false,
            window_title: None,
            draws: Vec::new(),
            draws_screen: Vec::new(),
            canvas_draws: HashMap::new(),
            canvas_clear: HashMap::new(),
            surfaces: HashMap::new(),
            surface_count: 0,
            surfaces_freed: RefCell::new(Vec::new()),
            gui: false,
            screen_fix: std::cell::Cell::new(glam::Mat4::IDENTITY),
            cursor: None,
            cursor_hidden: false,
            post_values: [0.0; 4],
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
            blueprint: String::new(),
            net: None,
            images: Default::default(),
            script_info: HashMap::new(),
            adopt: Vec::new(),
            preload: Vec::new(),
            props_gen: 0,
        }));
        let mut it = Interp::embedded(project.dir.clone());
        it.set_output(Box::new(LogWriter(st.clone(), Vec::new())));
        it.host = Some(Box::new(EngineHost { st: st.clone() }));
        it.cancel = cancel;
        Game { stats: FrameStats::default(), profile: None, it, st, scripts: HashMap::new(), physics_acc: 0.0,
            started: false, carried: HashSet::new(), hovered: HashSet::new(), handler_index: HashMap::new(), index_key: (u64::MAX, 0), scripts_rev: 0 }
    }

    /// Moving to the next blueprint (CHANGE_BLUEPRINT): what lasts for the whole run comes
    /// along from the game that's ending: REMEMBER values and GLOBAL variables. GLOBAL
    /// functions are left behind; the new blueprint's scripts make their own.
    pub fn carry_over(&mut self, old: &mut Game) {
        let shared = std::mem::take(&mut old.st.borrow_mut().shared);
        self.st.borrow_mut().shared = shared;
        self.it.take_parsed_from(&mut old.it);
        // Pictures' sizes, scripts' events: the same files, already looked at.
        self.st.borrow_mut().images = std::mem::take(&mut old.st.borrow_mut().images);
        let info = std::mem::take(&mut old.st.borrow_mut().script_info);
        self.st.borrow_mut().script_info = info;
        for (k, s) in old.it.shared.drain() {
            if !matches!(&*s.borrow(), jlb::Value::Func(_) | jlb::Value::Builtin(_) | jlb::Value::Task(_)) {
                self.it.shared.insert(k, s);
            }
        }
        self.carry_persistent(old);
    }

    /// Objects marked `persistent` (like GameMaker's) come along into the new blueprint, under
    /// its root, with their children: their scripts keep their variables and their timers keep
    /// running; on_create doesn't run again.
    fn carry_persistent(&mut self, old: &mut Game) {
        let tops: Vec<ObjectId> = {
            let ost = old.st.borrow();
            let t = &ost.tree;
            let marked = |i: ObjectId| t.objects[i].prop("persistent").is_some_and(|v| v.as_bool());
            t.walk()
                .into_iter()
                .filter(|&i| marked(i) && {
                    let mut p = t.objects[i].parent;
                    let mut under_marked = false;
                    while let Some(x) = p {
                        under_marked |= marked(x);
                        p = t.objects[x].parent;
                    }
                    !under_marked
                })
                .collect()
        };
        if tops.is_empty() {
            return;
        }
        let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
        {
            let ost = old.st.borrow();
            let mut st = self.st.borrow_mut();
            let root = st.tree.root;
            for top in tops {
                let new_top = st.tree.copy_subtree(&ost.tree, top, root);
                let (mut a, mut b) = (Vec::new(), Vec::new());
                ost.tree.walk_from(top, &mut a);
                st.tree.walk_from(new_top, &mut b);
                map.extend(a.into_iter().zip(b));
            }
            st.timers.extend(ost.timers.iter().filter_map(|t| map.get(&t.object).map(|&o| Timer { object: o, name: t.name.clone(), left: t.left, every: t.every })));
        }
        for (o, n) in &map {
            if let Some(mut list) = old.scripts.remove(o) {
                for s in &mut list {
                    s.sync = Default::default();
                    s.sync_gen = u64::MAX;
                }
                self.scripts.insert(*n, list);
            }
            self.carried.insert(*n);
        }
        self.scripts_rev += 1;
    }

    /// Loads a blueprint file as a fresh game.
    pub fn from_blueprint(project: Project, blueprint_res: &str, cancel: Option<Arc<AtomicBool>>) -> Result<Game, String> {
        let text = project.read_text(blueprint_res)?;
        let p2 = project.clone();
        let tree = blueprint::load(&text, &|p: &str| p2.read_text(p)).map_err(|e| e.0)?;
        let g = Game::new(project, tree, cancel);
        g.st.borrow_mut().blueprint = blueprint_res.to_string();
        Ok(g)
    }

    pub fn tree(&self) -> std::cell::Ref<'_, BlueprintTree> {
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
            let auto: Vec<(ObjectId, String)> = st.tree.walk().into_iter().filter(|i| st.tree.objects[*i].kind == ObjectKind::Animator && !st.tree.objects[*i].text("autoplay").is_empty()).map(|i| (i, st.tree.objects[i].text("autoplay").to_string())).collect();
            for (id, clip) in auto {
                st.clips_playing.insert(id, (clip, 0.0));
            }
        }
        let ids = self.st.borrow().tree.walk();
        {
            // Every picture placed in the blueprint, shown yet or not, is got ready.
            let mut st = self.st.borrow_mut();
            let mut seen = HashSet::new();
            let mut pre = Vec::new();
            for id in &ids {
                let n = &st.tree.objects[*id];
                let t = match n.kind {
                    ObjectKind::Sprite2D | ObjectKind::Particles2D => n.text("texture"),
                    ObjectKind::TileMap2D => n.text("tileset"),
                    _ => "",
                };
                if !t.is_empty() && seen.insert(t.to_string()) {
                    pre.push(t.to_string());
                }
            }
            st.preload.extend(pre);
        }
        let fresh: Vec<ObjectId> = ids.iter().copied().filter(|i| !self.carried.contains(i)).collect();
        self.attach_all(&fresh);
        self.autoplay(&fresh);
        for id in &fresh {
            self.run(*id, "on_create", vec![]);
        }
        self.drain();
    }

    /// Maps (a Mesh3D showing a .bsp) bring their physics props to life: each becomes a
    /// Body3D (its outline as the shape) holding the model, under the map's object.
    fn spawn_map_props(&mut self) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let maps: Vec<ObjectId> = st
            .tree
            .walk()
            .into_iter()
            .filter(|i| {
                let n = &st.tree.objects[*i];
                n.kind == ObjectKind::Mesh3D && n.text("mesh").to_ascii_lowercase().ends_with(".bsp") && n.prop("map_props").is_none_or(|v| v.as_bool())
            })
            .collect();
        for map in maps {
            let mesh = st.tree.objects[map].text("mesh").to_string();
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
                let mut body = gex_core::Object::new(name, ObjectKind::Body3D);
                body.transform.position = t;
                body.transform.rotation = glam::Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
                body.props.insert("shape".into(), Variant::Text("hull".into()));
                body.groups.push("map_props".into());
                let b = st.tree.add(Some(map), body);
                let mut look = gex_core::Object::new("Model", ObjectKind::Mesh3D);
                look.props.insert("mesh".into(), Variant::Text(format!("{}://{}", scheme, file.to_ascii_lowercase())));
                st.tree.add(Some(b), look);
            }
            // Doors and other moving brushes become objects of their own (USE_DOOR opens them),
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
                let mut door = gex_core::Object::new(name, ObjectKind::Mesh3D);
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
            st.tree.objects[map].props.insert("mesh".into(), Variant::Text(format!("{}?static", base)));
        }
    }

    /// Starts Sound objects that play by themselves.
    fn autoplay(&mut self, ids: &[ObjectId]) {
        let mut st = self.st.borrow_mut();
        for id in ids {
            let auto = st.tree.get(*id).is_some_and(|n| n.kind == ObjectKind::Sound && n.prop("autoplay").is_some_and(|v| v.as_bool()));
            if auto {
                if let Err(e) = crate::commands::play_object(&mut st, *id) {
                    st.log(LogKind::Error, e);
                }
            }
        }
    }

    fn attach_all(&mut self, ids: &[ObjectId]) {
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

    fn attach(&mut self, id: ObjectId, res: &str) {
        let st = self.st.clone();
        if let Some(inst) = load_script(&st, &mut self.it, id, res) {
            self.scripts.entry(id).or_default().push(inst);
            self.scripts_rev += 1;
        }
    }

    /// This frame's handler times (when profiling), into the stats.
    pub fn take_profile(&mut self) {
        if let Some(p) = self.profile.take() {
            self.stats.handlers = p.into_iter().map(|((r, h), (ms, n))| (r, h, ms, n)).collect();
        }
    }

    /// Gives the interpreter a debugger hook (breakpoints and stepping).
    pub fn set_pause_hook(&mut self, hook: jlb::host::PauseHook) {
        self.it.pause = Some(hook);
    }

    /// Loads a changed script again on every object that uses it, keeping the values of
    /// outside variables it still has (so a running game keeps its state).
    pub fn reload_script(&mut self, res: &str) -> usize {
        let Some(path) = self.st.borrow().project.res_to_path(res) else { return 0 };
        let handlers = scan_handlers(&path);
        let users: Vec<(ObjectId, usize)> = self
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
                    // (The script's own code may have destroyed its object meanwhile.)
                    if let Some(s) = self.scripts.get_mut(&id).and_then(|l| l.get_mut(i)) {
                        s.handlers = with_imported_handlers(&env, handlers.clone());
                        s.env = env;
                        self.scripts_rev += 1;
                        s.dead = false;
                        done += 1;
                    }
                }
                Err(f) => self.report(&f, res),
            }
        }
        if done > 0 {
            self.st.borrow_mut().log(LogKind::Info, format!("reloaded {} on {} object(s)", res, done));
        }
        done
    }

    /// Gives every running task (START) its turn: up to its next WAIT, AWAIT or a slice of
    /// statements. Tasks see their object's `self` like events do.
    fn run_tasks(&mut self) {
        let tags = self.it.live_task_tags();
        if tags.is_empty() {
            return;
        }
        let mut objects: Vec<ObjectId> = tags.into_iter().map(tag_object).collect();
        objects.sort();
        objects.dedup();
        let mut envs = Vec::new();
        for id in objects {
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
        report(&self.st, f, res)
    }

    /// The `self` table a script sees, and what it held (to tell what the script changed).
    fn self_table(&self, id: ObjectId) -> (Value, SelfSnap) {
        self_table(&self.st, id)
    }

    /// Before an event: the script's `self` brought up to date with the object. Only entries
    /// whose property changed since the last event (moved by physics, SET by another
    /// object...) are written; with nothing changed this makes nothing. (A new property, or
    /// a table that isn't this script's any more, makes `self` afresh.)
    fn self_in(&self, id: ObjectId, env: &Env, sync: &mut SelfSnap, sync_gen: &mut u64) {
        let st = self.st.borrow();
        let Some(n) = st.tree.get(id) else { return };
        let t = &n.transform;
        let base: [(&str, f64); 9] = [
            ("x", t.position.x as f64),
            ("y", t.position.y as f64),
            ("z", t.position.z as f64),
            ("rotation", t.rotation.z as f64),
            ("rotation_x", t.rotation.x as f64),
            ("rotation_y", t.rotation.y as f64),
            ("scale_x", t.scale.x as f64),
            ("scale_y", t.scale.y as f64),
            ("scale_z", t.scale.z as f64),
        ];
        let count = 2 + base.len() + n.props.len();
        let table = Interp::env_get(env, "self");
        let fits = match &table {
            Some(Value::Table(rc, _)) if rc.named.len() == count + 2 && sync.len() == count => {
                let e = rc.named.iter().as_slice();
                // The same entries in the same order (names are shared, so pointers compare).
                // Objects only gain properties (which changes the count) unless props_gen
                // moved, so the names themselves only need comparing then.
                e[2..].iter().zip(sync.iter()).all(|(a, b)| Rc::ptr_eq(&a.0, &b.0))
                    && (*sync_gen == st.props_gen || &*sync[0].0 == "name"
                    && sync[1..1 + base.len()].iter().zip(&base).all(|(a, b)| &*a.0 == b.0)
                    && &*sync[1 + base.len()].0 == "visible"
                    && sync[2 + base.len()..].iter().zip(n.props.keys()).all(|(a, k)| &*a.0 == k.as_str()))
            }
            _ => false,
        };
        let gen = st.props_gen;
        if !fits {
            drop(table);
            drop(st);
            let (me, snap) = self.self_table(id);
            Interp::env_set(env, "self", me);
            *sync = snap;
            *sync_gen = gen;
            return;
        }
        *sync_gen = gen;
        let Some(Value::Table(rc, _)) = &table else { return };
        let e = rc.named.iter().as_slice();
        fn put(e: &[(Rc<str>, jlb::value::Slot)], sync: &mut SelfSnap, i: usize, v: Value, last: Option<Variant>) {
            *e[i + 2].1.borrow_mut() = v.clone();
            sync[i].1 = v;
            sync[i].2 = last;
        }
        // name, the transform, visible, then the properties (the order PRINT(self) shows).
        if !matches!(&sync[0].1, Value::Text(s) if **s == *n.name) {
            put(e, sync, 0, Value::text(&n.name), None);
        }
        let vis = 1 + base.len();
        if !matches!(&sync[vis].1, Value::Bool(b) if *b == n.visible) {
            put(e, sync, vis, Value::Bool(n.visible), None);
        }
        for (j, (_, f)) in base.iter().enumerate() {
            let i = 1 + j;
            if !matches!(&sync[i].1, Value::Num(x) if x.to_f64() == *f) {
                put(e, sync, i, num(*f), None);
            }
        }
        for (j, v) in n.props.values().enumerate() {
            let i = 2 + base.len() + j;
            let same = match (&sync[i].2, v) {
                (Some(last), v) => last == v,
                (None, Variant::Num(f)) => matches!(&sync[i].1, Value::Num(x) if x.to_f64() == *f),
                (None, Variant::Bool(b)) => matches!(&sync[i].1, Value::Bool(x) if x == b),
                (None, Variant::Text(t) | Variant::Res(t)) => matches!(&sync[i].1, Value::Text(x) if **x == **t),
                (None, Variant::Null) => matches!(&sync[i].1, Value::Null),
                _ => false,
            };
            if !same {
                let val = to_value(v);
                let last = matches!(val, Value::Table(..)).then(|| v.clone());
                put(e, sync, i, val, last);
            }
        }
    }

    /// After an event: what the script changed in `self` goes to the object, and the record
    /// of what `self` holds is brought up to date.
    fn self_out(&mut self, id: ObjectId, env: &Env, sync: &mut SelfSnap) {
        // The usual case, in one pass: `self` has the entries the record has, in the same
        // order (the names are the same shared pieces of text), so each entry is checked
        // against what went in, and only those the script changed go to the object.
        if let Some(Value::Table(rc, _)) = Interp::env_get(env, "self") {
            let e = rc.named.iter().as_slice();
            if e.len() == sync.len() + 2 && e[2..].iter().zip(sync.iter()).all(|(a, b)| Rc::ptr_eq(&a.0, &b.0)) {
                let mut st = self.st.borrow_mut();
                let Some(n) = st.tree.get_mut(id) else { return };
                for (i, s) in sync.iter_mut().enumerate() {
                    let now = e[i + 2].1.borrow();
                    let v = match &s.2 {
                        None if same_value(&now, &s.1) => continue,
                        Some(keep) => match to_variant(&now, Some(keep)) {
                            Some(v) if v == *keep => continue,
                            v => v,
                        },
                        None => to_variant(&now, n.props.get(&*s.0)),
                    };
                    drop(now);
                    // (A value an object can't hold isn't put on it; `self` is still made afresh.)
                    if let Some(v) = v {
                        apply_prop(n, &s.0, v);
                    }
                    // The next event takes it afresh from the object.
                    s.1 = Value::Builtin(STALE);
                    s.2 = None;
                }
                return;
            }
        }
        self.read_self_back(id, env, sync);
        // Entries the script changed now hold what the object has: remember them as such.
        let Some(Value::Table(rc, _)) = Interp::env_get(env, "self") else { return };
        let e = rc.named.iter().as_slice();
        if e.len() != sync.len() + 2 {
            sync.clear(); // new entries: made afresh next time
            return;
        }
        for (i, s) in sync.iter_mut().enumerate() {
            let now = e[i + 2].1.borrow();
            let changed = match &s.2 {
                Some(keep) => !matches!(&*now, Value::Table(..)) || to_variant(&now, Some(keep)).as_ref() != Some(keep),
                None => !same_value(&now, &s.1),
            };
            if changed {
                // Written by the script: the next event takes it afresh from the object (which
                // has it now, in the property's own shape).
                s.1 = Value::Builtin(STALE);
                s.2 = None;
            }
        }
    }

    /// Applies what a script changed in `self` back to the object.
    fn read_self_back(&mut self, id: ObjectId, env: &Env, before: &SelfSnap) {
        read_self_back(&self.st, id, env, before)
    }

    /// Runs `handler` in each of the object's scripts that defines it.
    pub fn run(&mut self, id: ObjectId, handler: &str, args: Vec<Value>) {
        let count = self.scripts.get(&id).map_or(0, |s| s.len());
        for i in 0..count {
            self.run_one(id, i, handler, args.clone());
        }
    }

    fn run_one(&mut self, id: ObjectId, i: usize, handler: &str, args: Vec<Value>) -> Option<Value> {
        let env = {
            let s = self.scripts.get(&id)?.get(i)?;
            if s.dead || !s.handlers.contains(handler) {
                return None;
            }
            s.env.clone()
        };
        let res = |g: &Self| g.scripts.get(&id).and_then(|v| v.get(i)).map(|s| s.res.clone()).unwrap_or_default();
        if pausable(handler) && self.st.borrow().frozen(id) {
            return None;
        }
        let f = Interp::env_get(&env, handler)?;
        if !f.is_func() {
            return None;
        }
        let (mut sync, mut sync_gen) = self.scripts.get_mut(&id).and_then(|v| v.get_mut(i)).map(|s| (std::mem::take(&mut s.sync), s.sync_gen)).unwrap_or_default();
        self.self_in(id, &env, &mut sync, &mut sync_gen);
        self.st.borrow_mut().current = Some(id);
        // Tasks this event STARTs belong to this object.
        self.it.event_tag = object_tag(id);
        let t0 = std::time::Instant::now();
        let r = self.it.call_value(&f, args);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.stats.scripts_ms += ms;
        if self.profile.is_some() {
            let r = res(self);
            let e = self.profile.as_mut().unwrap().entry((r, handler.to_string())).or_default();
            e.0 += ms;
            e.1 += 1;
        }
        if ms > 0.05 {
            let r = res(self);
            self.stats.slowest.push((r, handler.to_string(), ms));
            self.stats.slowest.sort_by(|a, b| b.2.total_cmp(&a.2));
            self.stats.slowest.truncate(5);
        }
        self.self_out(id, &env, &mut sync);
        if let Some(s) = self.scripts.get_mut(&id).and_then(|v| v.get_mut(i)) {
            s.sync = sync;
            s.sync_gen = sync_gen;
        }
        match r {
            Ok(v) => Some(v),
            Err(f) => {
                let res = res(self);
                self.report(&f, &res);
                if !matches!(f, Fault::End) {
                    if let Some(s) = self.scripts.get_mut(&id).and_then(|v| v.get_mut(i)) {
                        s.dead = true;
                        self.scripts_rev += 1;
                    }
                }
                None
            }
        }
    }

    /// Runs `handler` on every object that has it, parents first.
    pub fn broadcast(&mut self, handler: &str, args: Vec<Value>) {
        let ids = self.with_handler(handler);
        for id in ids.iter() {
            self.run(*id, handler, args.clone());
        }
    }

    /// The objects with a script that has this event, in tree order (parents first). Kept
    /// from frame to frame until the tree or the scripts change, so an event that most
    /// objects don't have costs nothing.
    fn with_handler(&mut self, handler: &str) -> Rc<Vec<ObjectId>> {
        let key = (self.st.borrow().tree.rev, self.scripts_rev);
        if key != self.index_key {
            self.handler_index.clear();
            self.index_key = key;
        }
        if let Some(v) = self.handler_index.get(handler) {
            return v.clone();
        }
        let ids: Vec<ObjectId> = self
            .st
            .borrow()
            .tree
            .walk()
            .into_iter()
            .filter(|id| self.scripts.get(id).is_some_and(|s| s.iter().any(|s| !s.dead && s.handlers.contains(handler))))
            .collect();
        let v = Rc::new(ids);
        self.handler_index.insert(handler.to_string(), v.clone());
        v
    }

    /// One frame of the game (the simple form, for tools and tests).
    pub fn frame(&mut self, dt: f64, keys_down: HashSet<String>, mouse: [f32; 2], mouse_down: HashSet<String>) {
        self.step(FrameInput { dt, keys: keys_down, mouse, buttons: mouse_down, ..Default::default() });
    }

    /// One frame of the game.
    pub fn step(&mut self, input: FrameInput) {
        self.profile = input.profile.then(HashMap::new);
        self.start();
        let real_dt = input.dt.clamp(0.0, 0.25);
        let moved;
        let paused;
        let dt;
        {
            let mut st = self.st.borrow_mut();
            moved = st.input.mouse != input.mouse;
            st.input.next_frame(input.keys, input.mouse, input.buttons);
            if let Some(f) = input.fullscreen {
                st.project.config.display.fullscreen = f;
            }
            st.axes = input.axes;
            st.wheel = input.wheel;
            st.mouse_delta = input.mouse_delta;
            paused = st.paused;
            // While paused, the objects that still run see real time; the rest is frozen.
            dt = if paused { real_dt } else { real_dt * st.time_scale.max(0.0) };
            st.dt = dt;
            if !paused {
                st.time += dt;
            }
            st.draws.clear();
            st.draws_screen.clear();
            st.canvas_draws.clear();
            st.canvas_clear.clear();
        }
        self.net_poll(real_dt);
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
        self.object_mouse();
        self.tile_statics();
        // Fixed-rate physics and collisions.
        if !paused {
            self.physics_acc += dt;
        }
        let (step, max_steps) = {
            let st = self.st.borrow();
            let s = &st.physics.settings;
            (1.0 / s.tick_rate.clamp(10.0, 480.0) as f64, s.max_steps.max(1))
        };
        let mut steps = 0;
        while self.physics_acc >= step && steps < max_steps {
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
        // Too far behind to catch up: drop the backlog instead of fast-forwarding later.
        if steps == max_steps && self.physics_acc > step {
            self.physics_acc = step;
        }
        {
            let alpha = (self.physics_acc / step) as f32;
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            st.physics.present(&mut st.tree, alpha);
        }
        self.broadcast("on_step", vec![num(dt)]);
        self.broadcast("on_draw", vec![]);
        // Drawing over everything, in the window's pixels (GameMaker's Draw GUI).
        self.st.borrow_mut().gui = true;
        self.broadcast("on_draw_gui", vec![]);
        self.st.borrow_mut().gui = false;
        self.animate(dt);
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let frozen: Vec<ObjectId> = if st.paused { st.particles.keys().copied().filter(|i| st.frozen(*i)).collect() } else { Vec::new() };
            let amount = st.project.config.graphics.particles.clamp(0.0, 1.0) as f64;
            crate::particles::update(&st.tree, &mut st.particles, dt as f32, &frozen, amount);
            let frozen3: Vec<ObjectId> = if st.paused { st.particles_3d.keys().copied().filter(|i| st.frozen(*i)).collect() } else { Vec::new() };
            crate::particles::update_3d(&st.tree, &mut st.particles_3d, dt as f32, &frozen3, amount);
        }
        self.timers(dt);
        let finished = {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let frozen: Vec<ObjectId> = if st.paused { st.tweens.iter().map(|t| t.object).filter(|i| st.frozen(*i)).collect() } else { Vec::new() };
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
            // Sound objects report whether they're still playing.
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            st.audio.tidy();
            let playing: Vec<(ObjectId, bool)> = st.tree.of_kinds(&[ObjectKind::Sound]).iter().map(|&i| (i, st.audio.by_object.contains_key(&i))).collect();
            // Where the "ears" are: the 3D camera, else the 2D camera (or the screen's middle).
            let cam3 = st.tree.of_kinds(&[ObjectKind::Camera3D]).iter().copied().find(|i| st.tree.objects[*i].prop("current").is_none_or(|v| v.as_bool()));
            // ...and which way is right, for the balance.
            let (ear, right) = match cam3 {
                Some(c) => {
                    let m = st.tree.global_matrix(c);
                    (Some((m.w_axis.truncate(), true)), m.x_axis.truncate().normalize_or(glam::Vec3::X))
                }
                None => {
                    let [w, h] = st.project.config.display.size();
                    let c = st.cam2_center.unwrap_or(glam::Vec2::new(w as f32 / 2.0, h as f32 / 2.0));
                    (Some((c.extend(0.0), false)), glam::Vec3::X)
                }
            };
            for (i, p) in playing {
                let n = &st.tree.objects[i];
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
                        if let Some(id) = st.audio.by_object.get(&i).copied() {
                            st.audio.set_pan(id, (side * 0.9).clamp(-1.0, 1.0));
                        }
                    } else if let Some(id) = st.audio.by_object.get(&i).copied() {
                        st.audio.set_pan(id, n.num("pan", 0.0) as f32);
                    }
                    if let Some(id) = st.audio.by_object.get(&i).copied() {
                        st.audio.set_volume(id, vol);
                    }
                }
                st.tree.objects[i].props.insert("playing".into(), Variant::Bool(p));
            }
        }
        self.conditions();
        self.drain();
        crate::net::send_states(&mut self.st.borrow_mut(), real_dt);
    }

    /// Multiplayer: what other players' games sent since the last frame.
    fn net_poll(&mut self, dt: f64) {
        if self.st.borrow().net.is_none() {
            return;
        }
        let actions = crate::net::poll(&mut self.st.borrow_mut(), dt);
        for a in actions {
            match a {
                crate::net::Action::Event { handler, args, from } => {
                    if let Some(n) = self.st.borrow_mut().net.as_mut() {
                        n.sender = (from > 0).then_some(from);
                    }
                    self.broadcast(&handler, args);
                    if let Some(n) = self.st.borrow_mut().net.as_mut() {
                        n.sender = None;
                    }
                }
                crate::net::Action::Created(id) => self.st.borrow_mut().queue.push_back(Pending::Created(id, Vec::new())),
                crate::net::Action::Destroy(id) => self.st.borrow_mut().queue.push_back(Pending::Destroy(id)),
            }
        }
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
            UiEvent::Toggled(id, on) => {
                let Some(slug) = self.set_ui(id, "checked", Variant::Bool(on)) else { return };
                self.run(id, "on_toggled", vec![Value::Bool(on)]);
                self.broadcast(&format!("on_toggled_{}", slug), vec![Value::Bool(on)]);
            }
            UiEvent::Slid(id, v) => {
                let Some(slug) = self.set_ui(id, "value", Variant::Num(v)) else { return };
                self.run(id, "on_value_changed", vec![num(v)]);
                self.broadcast(&format!("on_value_changed_{}", slug), vec![num(v)]);
            }
            UiEvent::Scrolled(id, v) => {
                self.set_ui(id, "scroll", Variant::Num(v.max(0.0)));
            }
        }
    }

    /// Sets a property a UI object's use changed; gives its name for on_..._<name> events.
    fn set_ui(&mut self, id: ObjectId, prop: &str, v: Variant) -> Option<String> {
        let mut st = self.st.borrow_mut();
        let n = st.tree.get_mut(id)?;
        n.props.insert(prop.into(), v);
        Some(n.name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect())
    }

    /// Sprite sheet animations: moves each animated Sprite2D to its current frame.
    fn animate(&mut self, dt: f64) {
        let mut ended = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let ids = st.tree.of_kinds(&[ObjectKind::Sprite2D]);
            let any_meta = st.images.any_meta(&st.vfs);
            for &id in ids.iter() {
                if st.frozen(id) {
                    continue;
                }
                let n = &st.tree.objects[id];
                // Most sprites have no animation: skip them without making anything.
                if n.text("animation").is_empty() {
                    if !st.anims.is_empty() {
                        st.anims.remove(&id);
                    }
                    // A picture with its own settings plays through its frames by itself (at
                    // its fps, times the sprite's picture_speed).
                    if let Some((p, _)) = any_meta.then(|| st.images.meta(&st.vfs, n.text("texture"))).flatten() {
                        if p.fps > 0.0 && p.count() > 1 {
                            let count = p.count() as f64;
                            let f = (n.num("frame", 0.0) + p.fps as f64 * n.num("picture_speed", 1.0) * dt).rem_euclid(count);
                            st.tree.objects[id].props.insert("frame".into(), Variant::Num(f));
                        }
                    }
                    continue;
                }
                let name = n.text("animation").to_string();
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
                st.tree.objects[id].props.insert("frame".into(), Variant::Num(frames[k] as f64));
            }
        }
        // Model (Mesh3D) animations.
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let ids: Vec<ObjectId> = st.tree.of_kinds(&[ObjectKind::Mesh3D]).iter().copied().filter(|i| !st.frozen(*i)).collect();
            for id in ids {
                let n = &st.tree.objects[id];
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
                st.tree.objects[id].props.insert("anim_time".into(), Variant::Num(t));
            }
        }
        for (id, name) in ended {
            self.run(id, "on_animation_end", vec![Value::text(&name)]);
        }
    }

    /// Solid tiles (rows of tiles merged into one box) and solid sprites become static physics
    /// boxes.
    fn tile_statics(&mut self) {
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let mut boxes = Vec::new();
        let mut keep = HashSet::new();
        for &id in st.tree.of_kinds(&[ObjectKind::TileMap2D]).iter() {
            let n = &st.tree.objects[id];
            if n.kind != ObjectKind::TileMap2D || !n.prop("solid").is_none_or(|v| v.as_bool()) {
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
                    boxes.push(crate::physics::StaticBox2 { owner: id, center: (lo + hi) / 2.0, half: (hi - lo) / 2.0, angle: 0.0, circle: false });
                }
            }
            st.tile_boxes.insert(id, (key, boxes[first..].to_vec()));
        }
        st.tile_boxes.retain(|k, _| keep.contains(k));
        // Sprites switched to solid are walls their own size.
        for &id in st.tree.of_kinds(&[ObjectKind::Sprite2D]).iter() {
            let n = &st.tree.objects[id];
            if n.kind != ObjectKind::Sprite2D || !n.prop("solid").is_some_and(|v| v.as_bool()) || !st.tree.visible_in_tree(id) {
                continue;
            }
            // The sprite's own box (or circle), turned and scaled the way it is.
            let m = st.tree.global_matrix(id);
            let (w, h) = (n.num("width", 32.0) as f32, n.num("height", 32.0) as f32);
            let mid = (glam::Vec2::splat(0.5) - gex_core::render::pivot_of(n)) * glam::Vec2::new(w, h);
            let center = m.transform_point3(mid.extend(0.0)).truncate();
            let (sx, sy) = (m.x_axis.truncate().length(), m.y_axis.truncate().length());
            let half = glam::Vec2::new(w * sx / 2.0, h * sy / 2.0);
            let angle = m.x_axis.y.atan2(m.x_axis.x);
            let circle = n.text("solid_shape") == "circle";
            let half = if circle { glam::Vec2::splat(half.max_element()) } else { half };
            if half.min_element() > 0.0 {
                boxes.push(crate::physics::StaticBox2 { owner: id, center, half, angle, circle });
            }
        }
        st.physics.statics_2d = boxes;
        // Mesh3D objects with collision on become static triangle meshes.
        let mut meshes = Vec::new();
        let mut keep = HashSet::new();
        for &id in st.tree.of_kinds(&[ObjectKind::Mesh3D]).iter() {
            let n = &st.tree.objects[id];
            if n.kind != ObjectKind::Mesh3D || !n.prop("collision").is_some_and(|v| v.as_bool()) {
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
        for &id in st.tree.of_kinds(&[ObjectKind::Body3D]).iter() {
            let n = &st.tree.objects[id];
            if n.kind != ObjectKind::Body3D || !matches!(n.text("shape"), "hull" | "model") {
                continue;
            }
            let Some(child) = n.children.iter().copied().find(|c| st.tree.objects[*c].kind == ObjectKind::Mesh3D) else { continue };
            let c = &st.tree.objects[child];
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
            let ids: Vec<ObjectId> = st.clips_playing.keys().copied().collect();
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
                t += dt * st.tree.objects[id].num("speed", 1.0) as f32;
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
                    ended.push((st.tree.objects[id].parent.unwrap_or(id), id, name));
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
                match f.to_blueprint.clone() {
                    Some(blueprint) => {
                        if st.change_blueprint.is_none() {
                            st.change_blueprint = Some(blueprint);
                        }
                    }
                    None => st.fade = None,
                }
            }
        }
        st.shake.0 = (st.shake.0 - st.shake.1 * dt).max(0.0);
        let cam = st.tree.of_kinds(&[ObjectKind::Camera2D]).iter().copied().find(|i| {
            let n = &st.tree.objects[*i];
            n.kind == ObjectKind::Camera2D && n.prop("current").is_none_or(|v| v.as_bool())
        });
        let Some(cam) = cam else {
            st.cam2_center = None;
            return;
        };
        let n = &st.tree.objects[cam];
        let target = st.tree.global_matrix(cam).w_axis.truncate().truncate();
        let speed = n.num("smoothing", 0.0) as f32;
        let mut c = match st.cam2_center {
            Some(prev) if speed > 0.0 => prev + (target - prev) * (1.0 - (-speed * dt).exp()),
            _ => target,
        };
        if n.prop("limits").is_some_and(|v| v.as_bool()) {
            let zoom = n.num("zoom", 1.0).max(0.01) as f32;
            let [w, h] = st.project.config.display.size();
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
        // Sprites in layer order so far (particles and drawings are added after).
        let drawn = list.sprites.len();
        list.graphics = st.project.config.graphics.clone();
        crate::particles::draw(&st.tree, &st.particles, &mut list);
        crate::particles::draw_3d(&st.tree, &st.particles_3d, &mut list);
        let ragdolls: HashMap<ObjectId, std::sync::Arc<Vec<glam::Mat4>>> = st.physics.ragdoll_poses(&st.tree).into_iter().map(|(id, p)| (id, std::sync::Arc::new(p))).collect();
        for m in &mut list.meshes {
            if let Some(p) = ragdolls.get(&m.object) {
                m.pose = Some(p.clone());
            }
            if let Some((prev, t, k, _)) = st.anim_blends.get(&m.object) {
                m.prev_animation = prev.clone();
                m.prev_time = *t;
                m.blend = k.clamp(0.0, 1.0);
            }
        }
        list.mouse_locked = st.mouse_locked;
        list.display = st.project.config.display.clone();
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
        // Pictures with their own sprite settings: their frames and origin.
        if st.images.any_meta(&st.vfs) {
            let tree = &st.tree;
            let frames = |id: ObjectId| tree.get(id).map_or(0.0, |n| n.num("frame", 0.0));
            gex_core::render::apply_picture_settings(&mut list.sprites[..drawn], &frames, &mut |t: &str| st.images.meta(&st.vfs, t));
        }
        crate::commands::append_draws(&st.draws, &mut list);
        // Screen space: (0, 0) the top-left of the game window, in its pixels, whatever the
        // camera does (as with no camera).
        let design = glam::Vec2::new(st.project.config.display.width.max(1) as f32, st.project.config.display.height.max(1) as f32);
        let fix = match list.camera2d {
            Some(c) => glam::Mat4::from_translation(c.center.extend(0.0)) * glam::Mat4::from_rotation_z(c.rotation) * glam::Mat4::from_scale(glam::Vec3::new(1.0 / c.zoom.max(1e-4), 1.0 / c.zoom.max(1e-4), 1.0)) * glam::Mat4::from_translation((-design / 2.0).extend(0.0)),
            None => glam::Mat4::IDENTITY,
        };
        st.screen_fix.set(fix);
        crate::commands::append_screen_draws(&st.draws_screen, fix, &mut list);
        if let Some((file, size, pivot)) = &st.cursor {
            let unscale = glam::Mat4::from_cols(fix.x_axis, fix.y_axis, fix.z_axis, glam::Vec4::W);
            list.sprites.push(gex_core::render::Sprite {
                model: glam::Mat4::from_translation(glam::Vec3::new(st.input.mouse[0], st.input.mouse[1], 0.0)) * unscale,
                size: glam::Vec2::from(*size),
                pivot: glam::Vec2::from(*pivot),
                texture: file.clone(),
                layer: 3.0e6,
                lit: false,
                ..Default::default()
            });
        }
        list.hide_cursor = st.cursor.is_some() || st.cursor_hidden;
        list.post_values = st.post_values;
        // Canvases: the Canvas2Ds (from extract) and surfaces, with what's drawn into them.
        for (k, size) in &st.surfaces {
            list.canvases.push(gex_core::render::CanvasDraw { key: k.clone(), size: *size, ..Default::default() });
        }
        for c in &mut list.canvases {
            if let Some(clear) = st.canvas_clear.get(&c.key) {
                c.clear = true;
                if let Some(col) = clear {
                    c.clear_color = *col;
                }
            }
            if let Some(d) = st.canvas_draws.get(&c.key) {
                crate::commands::append_canvas_draws(d, c);
            }
        }
        list.canvases_freed = std::mem::take(&mut *st.surfaces_freed.borrow_mut());
        // (Stable: the same layer keeps tree order, then drawing order.)
        if list.sprites.len() != drawn {
            gex_core::render::sort_by_layer(&mut list.sprites);
        }
        drop(st);
        list.warm = true;
        // (Someone may be looking at the tree right now: then the preloads go with a later list.)
        if let Ok(mut st) = self.st.try_borrow_mut() {
            list.preload = std::mem::take(&mut st.preload);
        }
        list
    }

    fn timers(&mut self, dt: f64) {
        let mut fire = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let frozen: HashSet<ObjectId> = if st.paused { st.timers.iter().map(|t| t.object).filter(|i| st.frozen(*i)).collect() } else { HashSet::new() };
            let tree = &st.tree;
            st.timers.retain_mut(|t| {
                if !tree.contains(t.object) {
                    return false;
                }
                if frozen.contains(&t.object) {
                    return true;
                }
                t.left -= dt;
                if t.left > 0.0 {
                    return true;
                }
                fire.push((t.object, t.name.clone()));
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
        // Timer objects: on_timeout(name) on the Timer and its parent, on_timeout_<name>() everywhere.
        let mut done = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let paused = st.paused;
            for (id, n) in st.tree.objects.iter_mut() {
                if n.kind != ObjectKind::Timer || !n.prop("running").is_none_or(|v| v.as_bool()) {
                    continue;
                }
                if paused && !n.groups.iter().any(|g| g == PAUSE_GROUP) {
                    continue;
                }
                let wait = n.num("wait_time", 1.0).max(0.001);
                // Nothing left (new, or started again after it ran out): the whole wait.
                let mut left = n.prop("time_left").and_then(|v| v.as_f64()).filter(|l| *l > 0.0).unwrap_or(wait);
                left -= dt;
                if left <= 0.0 {
                    done.push((id, n.parent, n.name.clone()));
                    if n.prop("repeat").is_none_or(|v| v.as_bool()) {
                        left = (left + wait).max(0.001);
                    } else {
                        left = 0.0;
                        n.props.insert("running".into(), Variant::Bool(false));
                    }
                }
                n.props.insert("time_left".into(), Variant::Num(left.max(0.0)));
            }
        }
        for (id, parent, name) in done {
            self.run(id, "on_timeout", vec![Value::text(&name)]);
            if let Some(p) = parent {
                self.run(p, "on_timeout", vec![Value::text(&name)]);
            }
            let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
            self.broadcast(&format!("on_timeout_{}", slug), vec![]);
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
            let owner_of = |id: ObjectId| -> ObjectId {
                match tree.get(id) {
                    Some(n) if !self.scripts.contains_key(&id) => match n.parent {
                        Some(p) if self.scripts.contains_key(&p) && n.kind != ObjectKind::Body2D && n.kind != ObjectKind::Body3D => p,
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

    /// SPAWN(..., [hp: 5, speed: 300]): each value goes to the variable of that name in
    /// the new object's scripts (after their outside code ran, before on_create), or, when
    /// no script has one, to the object's property of that name.
    fn give_vars(&mut self, id: ObjectId, vars: Vec<(String, Value)>) {
        for (k, v) in vars {
            let mut found = false;
            if let Some(list) = self.scripts.get(&id) {
                for s in list {
                    if !s.dead && Interp::env_get(&s.env, &k).is_some_and(|old| !old.is_func()) {
                        Interp::env_set(&s.env, &k, v.clone());
                        found = true;
                    }
                }
            }
            if !found {
                let mut st = self.st.borrow_mut();
                if st.tree.contains(id) {
                    if let Err(f) = crate::commands::set_prop(&mut st, id, &k, &v) {
                        let msg = match f { jlb::Fault::Error(e) | jlb::Fault::TaskError(e) => e.msg.clone(), _ => String::new() };
                        st.log(LogKind::Error, format!("SPAWN's {}: {}", k, msg));
                    }
                }
            }
        }
    }

    /// The mouse on 2D objects: on_mouse_enter() and on_mouse_leave() as it goes over and off
    /// one, and while it's over one, on_clicked(button) when a button goes down,
    /// on_click_held(button) each frame it's held and on_click_released(button) when it's let
    /// go. Every object under the mouse gets them (not just the top one), like GameMaker.
    fn object_mouse(&mut self) {
        const EVENTS: [&str; 5] = ["on_mouse_enter", "on_mouse_leave", "on_clicked", "on_click_held", "on_click_released"];
        let mut ids: HashSet<ObjectId> = HashSet::new();
        for e in EVENTS {
            ids.extend(self.with_handler(e).iter().copied());
        }
        if ids.is_empty() && self.hovered.is_empty() {
            return;
        }
        let was_hovered = self.hovered.clone();
        let (order, now, pressed, down, released) = {
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let p = glam::Vec2::new(st.input.mouse[0], st.input.mouse[1]);
            let mut now = HashSet::new();
            for id in &ids {
                let precise = st.tree.get(*id).is_some_and(|n| n.prop("mouse_precise").is_some_and(|v| v.as_bool()));
                if crate::hit::point_on(st, *id, p, precise) {
                    now.insert(*id);
                }
            }
            let sorted = |h: &HashSet<String>| {
                let mut v: Vec<String> = h.iter().cloned().collect();
                v.sort();
                v
            };
            // The objects that can have the mouse on them, in tree order.
            let order: Vec<ObjectId> = st.tree.of_kinds(&[ObjectKind::Sprite2D, ObjectKind::Collider2D, ObjectKind::Body2D, ObjectKind::Label2D]).iter().copied().filter(|i| ids.contains(i) || was_hovered.contains(i)).collect();
            (order, now, sorted(&st.input.mouse_pressed), sorted(&st.input.mouse_down), sorted(&st.input.mouse_released))
        };
        let was = std::mem::take(&mut self.hovered);
        for id in order {
            let (on, before) = (now.contains(&id), was.contains(&id));
            if on && !before {
                self.run(id, "on_mouse_enter", vec![]);
            }
            if before && !on {
                self.run(id, "on_mouse_leave", vec![]);
            }
            if on {
                for b in &pressed {
                    self.run(id, "on_clicked", vec![Value::text(b)]);
                }
                for b in &down {
                    self.run(id, "on_click_held", vec![Value::text(b)]);
                }
                for b in &released {
                    self.run(id, "on_click_released", vec![Value::text(b)]);
                }
            }
        }
        self.hovered = now;
    }

    /// Handles queued events, spawns and destroys (scripts' requests), in order.
    fn drain(&mut self) {
        for _ in 0..10_000 {
            let Some(p) = self.st.borrow_mut().queue.pop_front() else { return };
            match p {
                Pending::Event { target: Some(t), handler, args } => self.run(t, &handler, args),
                Pending::Event { target: None, handler, args } => self.broadcast(&handler, args),
                Pending::Adopt(id) => {
                    let mut ids = Vec::new();
                    self.st.borrow().tree.walk_from(id, &mut ids);
                    let mine: Vec<(ObjectId, Vec<ScriptInst>)> = {
                        let mut st = self.st.borrow_mut();
                        let (mine, rest) = std::mem::take(&mut st.adopt).into_iter().partition(|(o, _)| ids.contains(o));
                        st.adopt = rest;
                        mine
                    };
                    for (o, list) in mine {
                        if self.st.borrow().tree.contains(o) {
                            self.scripts.entry(o).or_default().extend(list);
                        }
                    }
                    self.scripts_rev += 1;
                    self.autoplay(&ids);
                }
                Pending::Created(id, vars) => {
                    let mut ids = Vec::new();
                    self.st.borrow().tree.walk_from(id, &mut ids);
                    self.attach_all(&ids);
                    self.give_vars(id, vars);
                    self.autoplay(&ids);
                    for i in ids {
                        self.run(i, "on_create", vec![]);
                    }
                }
                Pending::Destroy(id) => {
                    if !self.st.borrow().tree.contains(id) {
                        continue;
                    }
                    let mut ids = Vec::new();
                    self.st.borrow().tree.walk_from(id, &mut ids);
                    for i in ids.iter().rev() {
                        self.run(*i, "on_destroy", vec![]);
                    }
                    let mut st = self.st.borrow_mut();
                    if !st.tree.contains(id) {
                        continue;
                    }
                    let gone = st.tree.remove(id);
                    crate::net::forget(&mut st, &gone);
                    for g in gone {
                        self.it.cancel_tagged(object_tag(g));
                        self.scripts.remove(&g);
                        if let Some(s) = st.audio.by_object.remove(&g) {
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
    owner: ObjectId,
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

fn collect_areas(tree: &BlueprintTree, layers: &crate::physics::Layers, has_scripts: impl Fn(ObjectId) -> bool) -> Vec<Area> {
    let mut out = Vec::new();
    for &id in tree.of_kinds(&[ObjectKind::Collider2D, ObjectKind::Collider3D, ObjectKind::Body2D, ObjectKind::Body3D]).iter() {
        let n = &tree.objects[id];
        if !matches!(n.kind, ObjectKind::Collider2D | ObjectKind::Collider3D | ObjectKind::Body2D | ObjectKind::Body3D) || !tree.visible_in_tree(id) {
            continue;
        }
        let body = matches!(n.kind, ObjectKind::Body2D | ObjectKind::Body3D);
        // A collider speaks for its parent, unless it has scripts itself.
        let owner = if body || has_scripts(id) || n.parent.is_none() { id } else { n.parent.unwrap() };
        let groups = layers.of(tree, id);
        let shape = match n.kind {
            ObjectKind::Collider2D => AreaShape::Rect(aabb(tree, id)),
            ObjectKind::Body2D => {
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

pub fn aabb(tree: &BlueprintTree, id: ObjectId) -> [f32; 4] {
    let n = &tree.objects[id];
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

/// A loaded script's events: the ones its own file defines, plus those that came in with
/// IMPORT (a shared file of events, used by many objects' scripts).
fn with_imported_handlers(env: &jlb::Env, mut own: HashSet<String>) -> HashSet<String> {
    for k in Interp::env_funcs(env) {
        if (k.starts_with("on_") || k.starts_with("when_")) && !own.contains(&*k) {
            own.insert(k.to_string());
        }
    }
    own
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

/// A value from a script's `self` put on the object: the transform shortcuts, name and
/// visible, or a property.
fn apply_prop(n: &mut gex_core::Object, k: &str, v: Variant) {
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
        _ => match n.props.get_mut(k) {
            Some(slot) => *slot = v,
            None => {
                n.props.insert(k.to_string(), v);
            }
        },
    }
}

/// What went into an object's `self` table: each name, the value put in, and for tables
/// (which a script can change in place) the property they came from.
pub(crate) type SelfSnap = Vec<(Rc<str>, Value, Option<Variant>)>;

/// In a SelfSnap: this entry is to be taken from the object again.
const STALE: &str = "<stale>";

/// The same value as went in (a number equal, a switch equal, the very same text).
fn same_value(now: &Value, put: &Value) -> bool {
    match (now, put) {
        (Value::Num(a), Value::Num(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) => true,
        (Value::Text(a), Value::Text(b)) => Rc::ptr_eq(a, b) || a == b,
        _ => false,
    }
}

thread_local! {
    static KEYS: RefCell<HashMap<String, Rc<str>>> = RefCell::new(HashMap::new());
}

/// A property name as a shared piece of text (made once per name).
fn key(k: &str) -> Rc<str> {
    KEYS.with(|m| {
        if let Some(r) = m.borrow().get(k) {
            return r.clone();
        }
        let r: Rc<str> = Rc::from(k);
        m.borrow_mut().insert(k.to_string(), r.clone());
        r
    })
}

/// Loads one of an object's scripts (its outside code runs, with `self`).
pub(crate) fn load_script(rc: &Rc<RefCell<WorldState>>, it: &mut Interp, id: ObjectId, res: &str) -> Option<ScriptInst> {
    let Some(path) = rc.borrow().project.res_to_path(res) else {
        rc.borrow_mut().log(LogKind::Error, format!("{} isn't a project path (scripts live in res://)", res));
        return None;
    };
    // What the file has (events, $variables): read once per file, not per object (SPAWN
    // of a bullet happens often).
    let meta = std::fs::metadata(&path).ok();
    let stamp = (meta.as_ref().and_then(|m| m.modified().ok()), meta.as_ref().map_or(0, |m| m.len()));
    let cached = rc.borrow().script_info.get(&path).cloned();
let info = match cached {
        Some((s, info)) if meta.is_some() && s == stamp => info,
        _ => {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let info = Rc::new((scan_handlers_text(&text), gex_core::object::script_vars(&text)));
            rc.borrow_mut().script_info.insert(path.clone(), (stamp, info.clone()));
            info
        }
    };
    let handlers = info.0.clone();
    // $variables nobody gave this object a value for start at 0.
    {
        let mut st = rc.borrow_mut();
        if let Some(n) = st.tree.objects.get_mut(id) {
            for v in &info.1 {
                n.props.entry(v.clone()).or_insert(Variant::Num(0.0));
            }
        }
    }
    let (me, before) = self_table(rc, id);
    rc.borrow_mut().current = Some(id);
    match it.load_module(&path, &[("self", me)]) {
        Ok(env) => {
            read_self_back(rc, id, &env, &before);
            let handlers = with_imported_handlers(&env, handlers);
            Some(ScriptInst {
                res: res.to_string(),
                env,
                handlers,
                when_was: HashMap::new(),
                dead: false,
                sync: Vec::new(),
                sync_gen: 0,
            })
        }
        Err(f) => {
            report(rc, &f, res);
            None
        }
    }
}

pub(crate) fn report(rc: &Rc<RefCell<WorldState>>, f: &Fault, res: &str) {
    let mut st = rc.borrow_mut();
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

pub(crate) fn self_table(rc: &Rc<RefCell<WorldState>>, id: ObjectId) -> (Value, SelfSnap) {
    let st = rc.borrow();
    let Some(n) = st.tree.get(id) else { return (Value::Null, Vec::new()) };
    let t = &n.transform;
    let base: [(&str, Variant); 11] = [
        ("name", Variant::Text(n.name.clone())),
        ("x", Variant::Num(t.position.x as f64)),
        ("y", Variant::Num(t.position.y as f64)),
        ("z", Variant::Num(t.position.z as f64)),
        ("rotation", Variant::Num(t.rotation.z as f64)),
        ("rotation_x", Variant::Num(t.rotation.x as f64)),
        ("rotation_y", Variant::Num(t.rotation.y as f64)),
        ("scale_x", Variant::Num(t.scale.x as f64)),
        ("scale_y", Variant::Num(t.scale.y as f64)),
        ("scale_z", Variant::Num(t.scale.z as f64)),
        ("visible", Variant::Bool(n.visible)),
    ];
    let mut snap: SelfSnap = Vec::with_capacity(base.len() + n.props.len());
    for (k, v) in base.iter() {
        snap.push((key(k), to_value(v), None));
    }
    for (k, v) in &n.props {
        let val = to_value(v);
        let keep = matches!(val, Value::Table(..)).then(|| v.clone());
        snap.push((key(k), val, keep));
    }
    let mut entries: Vec<(Rc<str>, Value)> = Vec::with_capacity(snap.len() + 2);
    entries.push((key("id"), id_value(id)));
    entries.push((key("kind"), Value::text(n.kind.name())));
    entries.extend(snap.iter().map(|(k, v, _)| (k.clone(), v.clone())));
    (jlb::embed::named_table_rc(entries), snap)
}

pub(crate) fn read_self_back(rc: &Rc<RefCell<WorldState>>, id: ObjectId, env: &Env, before: &SelfSnap) {
    let Some(Value::Table(t, _)) = Interp::env_get(env, "self") else { return };
    let mut st = rc.borrow_mut();
    let Some(n) = st.tree.get_mut(id) else { return };
    for (i, (k, slot)) in t.named.iter().enumerate() {
        let k: &str = k;
        if k == "id" || k == "kind" {
            continue;
        }
        // The table keeps the order it was made in, so the entry is usually at the same place.
        let was = before.get(i.wrapping_sub(2)).filter(|b| &*b.0 == k).or_else(|| before.iter().find(|b| &*b.0 == k));
        let now = slot.borrow();
        let v = match was {
            // Untouched: the same number, switch or piece of text that went in.
            Some((_, put, None)) if same_value(&now, put) => continue,
            Some((_, _, Some(keep))) => match to_variant(&now, Some(keep)) {
                Some(v) if v == *keep => continue,
                Some(v) => v,
                None => continue,
            },
            _ => match to_variant(&now, n.props.get(k)) {
                Some(v) => v,
                None => continue,
            },
        };
        drop(now);
        apply_prop(n, k, v);
    }
}
