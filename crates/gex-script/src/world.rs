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
}

impl WorldState {
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
    it: Interp,
    pub st: Rc<RefCell<WorldState>>,
    scripts: HashMap<NodeId, Vec<ScriptInst>>,
    physics_acc: f64,
    started: bool,
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
            input: Input::default(),
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
        }));
        let mut it = Interp::embedded(project.dir.clone());
        it.set_output(Box::new(LogWriter(st.clone(), Vec::new())));
        it.host = Some(Box::new(EngineHost { st: st.clone() }));
        it.cancel = cancel;
        Game { it, st, scripts: HashMap::new(), physics_acc: 0.0, started: false }
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
        let ids = self.st.borrow().tree.walk();
        self.attach_all(&ids);
        for id in &ids {
            self.run(*id, "on_create", vec![]);
        }
        self.drain();
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
        let f = Interp::env_get(&env, handler)?;
        if !f.is_func() {
            return None;
        }
        let (me, before) = self.self_table(id);
        Interp::env_set(&env, "self", me);
        self.st.borrow_mut().current = Some(id);
        let r = self.it.call_value(&f, args);
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

    /// One frame of the game.
    pub fn frame(&mut self, dt: f64, keys_down: HashSet<String>, mouse: [f32; 2], mouse_down: HashSet<String>) {
        self.start();
        let dt = dt.clamp(0.0, 0.25);
        {
            let mut st = self.st.borrow_mut();
            st.input.next_frame(keys_down, mouse, mouse_down);
            st.dt = dt;
            st.time += dt;
        }
        // Input events.
        let (pressed, released, clicks) = {
            let st = self.st.borrow();
            let mut p: Vec<String> = st.input.pressed.iter().cloned().collect();
            let mut r: Vec<String> = st.input.released.iter().cloned().collect();
            let mut c: Vec<String> = st.input.mouse_pressed.iter().cloned().collect();
            p.sort();
            r.sort();
            c.sort();
            (p, r, c)
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
            let m = self.st.borrow().input.mouse;
            self.broadcast("on_mouse_pressed", vec![Value::text(&b), num(m[0] as f64), num(m[1] as f64)]);
        }
        // Fixed-rate physics and collisions.
        self.physics_acc += dt;
        let step = 1.0 / PHYSICS_HZ;
        let mut steps = 0;
        while self.physics_acc >= step && steps < 5 {
            self.physics_acc -= step;
            steps += 1;
            self.broadcast("on_physics", vec![num(step)]);
            self.collisions();
        }
        self.broadcast("on_step", vec![num(dt)]);
        self.timers(dt);
        self.conditions();
        self.drain();
    }

    fn timers(&mut self, dt: f64) {
        let mut fire = Vec::new();
        {
            let mut st = self.st.borrow_mut();
            let tree = st.tree.clone();
            st.timers.retain_mut(|t| {
                if !tree.contains(t.node) {
                    return false;
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
                    if h.starts_with("when_") && !s.dead {
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
            let mut boxes: Vec<(NodeId, [f32; 4])> = Vec::new();
            for id in tree.walk() {
                let n = &tree.nodes[id];
                if n.kind != NodeKind::Collider2D || !tree.visible_in_tree(id) {
                    continue;
                }
                let owner = if self.scripts.contains_key(&id) || n.parent.is_none() { id } else { n.parent.unwrap() };
                boxes.push((owner, aabb(tree, id)));
            }
            let mut now = HashSet::new();
            for i in 0..boxes.len() {
                for j in i + 1..boxes.len() {
                    let (a, ra) = boxes[i];
                    let (b, rb) = boxes[j];
                    if a != b && ra[0] < rb[2] && rb[0] < ra[2] && ra[1] < rb[3] && rb[1] < ra[3] {
                        now.insert((a, b));
                        now.insert((b, a));
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
                    let gone = self.st.borrow_mut().tree.remove(id);
                    for g in gone {
                        self.scripts.remove(&g);
                    }
                }
            }
        }
        self.st.borrow_mut().log(LogKind::Error, "too many events in one frame (do two scripts keep SENDing to each other?)");
        self.st.borrow_mut().queue.clear();
    }
}

/// A collider's box in world space: [left, top, right, bottom].
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
