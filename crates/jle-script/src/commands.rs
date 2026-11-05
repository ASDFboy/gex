//! Engine commands available to every script, ALL CAPS like the rest of .jlb's built-ins.
//! Wherever a command takes a node, it accepts a node id (a number), a path text
//! ("Gun", "../Enemy", "/Level/Door"; relative to the running node), or NULL for the
//! running node itself.

use std::cell::RefCell;
use std::rc::Rc;

use jle_core::{scene, NodeId};
use jlb::embed::{as_f64, list_table, num};
use jlb::host::Host;
use jlb::value::{display, err};
use jlb::{Interp, Value, R};

use crate::convert::{id_value, to_value, to_variant, value_id};
use crate::world::{Pending, Timer, WorldState};

/// (name, inputs, what it does) — the editor shows these as help and completions.
pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("NODE", "(path)", "the node at a path (relative to this one), or NULL"),
    ("GET", "(node, property)", "a property of another node"),
    ("SET", "(node, property, value)", "changes a property of another node"),
    ("NODES", "(group)", "every node in a group, as a table of ids"),
    ("CHILDREN", "(node)", "a node's children, as a table of ids"),
    ("PARENT", "(node)", "a node's parent, or NULL"),
    ("NAME_OF", "(node)", "a node's name"),
    ("PATH_OF", "(node)", "a node's full path"),
    ("SPAWN", "(scene, x, y, z, parent)", "adds an instance of a scene file; gives its id"),
    ("DESTROY", "(node)", "removes a node (at the end of the frame); runs on_destroy"),
    ("SEND", "(node, event, inputs...)", "runs on_<event>(inputs...) on one node"),
    ("EMIT", "(event, inputs...)", "runs on_<event>(inputs...) on every node that has it"),
    ("TIMER", "(name, seconds, repeat)", "runs on_timer(name) and on_timer_<name>() later"),
    ("TIMER_STOP", "(name)", "cancels this node's timer"),
    ("COLLIDING", "(node)", "the nodes touching this one now, as a table of ids"),
    ("DISTANCE", "(a, b)", "how far apart two nodes are"),
    ("KEY_DOWN", "(key)", "TRUE while a key (or mouse button) is held"),
    ("KEY_PRESSED", "(key)", "TRUE on the frame a key went down"),
    ("KEY_RELEASED", "(key)", "TRUE on the frame a key went up"),
    ("MOUSE_X", "()", "mouse x in game coordinates"),
    ("MOUSE_Y", "()", "mouse y in game coordinates"),
    ("DELTA_TIME", "()", "seconds since the last frame"),
    ("GAME_TIME", "()", "seconds since the game started"),
    ("CHANGE_SCENE", "(scene)", "switches to another scene file"),
    ("QUIT", "()", "stops the game"),
];

pub fn register() {
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.0).collect();
    jlb::host::register(&names);
}

pub struct EngineHost {
    pub st: Rc<RefCell<WorldState>>,
}

fn target(st: &WorldState, v: Option<&Value>) -> R<NodeId> {
    let cur = st.current;
    let id = match v {
        None | Some(Value::Null) => cur,
        Some(Value::Text(p)) => st.tree.find_path(cur, p),
        Some(v @ Value::Num(_)) => value_id(v),
        Some(o) => return err(format!("a node is given as an id, a path or NULL, not a {}", o.type_name())),
    };
    match id {
        Some(i) if st.tree.contains(i) => Ok(i),
        _ => err(match v {
            Some(Value::Text(p)) => format!("there's no node at {}", p),
            _ => "that node doesn't exist (was it destroyed?)".into(),
        }),
    }
}

fn text_arg(name: &str, a: &[Value], i: usize) -> R<String> {
    match a.get(i) {
        Some(Value::Text(s)) => Ok(s.to_string()),
        Some(o) => err(format!("{} needs text here, not a {}", name, o.type_name())),
        None => err(format!("{} needs more inputs", name)),
    }
}

fn num_arg(a: &[Value], i: usize, default: f64) -> f64 {
    a.get(i).and_then(as_f64).unwrap_or(default)
}

fn ids(list: Vec<NodeId>) -> Value {
    list_table(list.into_iter().map(id_value).collect())
}

/// A node property by name, including the transform shortcuts that `self` has.
fn get_prop(st: &WorldState, id: NodeId, k: &str) -> Value {
    let n = &st.tree.nodes[id];
    let t = &n.transform;
    match k {
        "name" => Value::text(&n.name),
        "kind" => Value::text(n.kind.name()),
        "id" => id_value(id),
        "x" => num(t.position.x as f64),
        "y" => num(t.position.y as f64),
        "z" => num(t.position.z as f64),
        "rotation" | "rotation_z" => num(t.rotation.z as f64),
        "rotation_x" => num(t.rotation.x as f64),
        "rotation_y" => num(t.rotation.y as f64),
        "scale_x" => num(t.scale.x as f64),
        "scale_y" => num(t.scale.y as f64),
        "scale_z" => num(t.scale.z as f64),
        "visible" => Value::Bool(n.visible),
        _ => n.props.get(k).map(to_value).unwrap_or(Value::Null),
    }
}

fn set_prop(st: &mut WorldState, id: NodeId, k: &str, v: &Value) -> R<()> {
    let n = &mut st.tree.nodes[id];
    let like = n.props.get(k).cloned();
    let Some(var) = to_variant(v, like.as_ref()) else {
        return err(format!("a {} can't be stored in a node property", v.type_name()));
    };
    let f = var.as_f64().unwrap_or(0.0) as f32;
    let t = &mut n.transform;
    match k {
        "id" | "kind" => return err(format!("{} can't be changed", k)),
        "name" => n.name = display(v),
        "x" => t.position.x = f,
        "y" => t.position.y = f,
        "z" => t.position.z = f,
        "rotation" | "rotation_z" => t.rotation.z = f,
        "rotation_x" => t.rotation.x = f,
        "rotation_y" => t.rotation.y = f,
        "scale_x" => t.scale.x = f,
        "scale_y" => t.scale.y = f,
        "scale_z" => t.scale.z = f,
        "visible" => n.visible = var.as_bool(),
        _ => {
            n.props.insert(k.to_string(), var);
        }
    }
    Ok(())
}

impl Host for EngineHost {
    fn call(&mut self, _it: &mut Interp, name: &'static str, a: Vec<Value>) -> Result<R<Value>, Vec<Value>> {
        if !COMMANDS.iter().any(|c| c.0 == name) {
            return Err(a);
        }
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        let r: R<Value> = (|| match name {
            "NODE" => {
                let p = text_arg(name, &a, 0)?;
                Ok(st.tree.find_path(st.current, &p).map(id_value).unwrap_or(Value::Null))
            }
            "GET" => {
                let id = target(st, a.first())?;
                Ok(get_prop(st, id, &text_arg(name, &a, 1)?))
            }
            "SET" => {
                let id = target(st, a.first())?;
                let k = text_arg(name, &a, 1)?;
                set_prop(st, id, &k, a.get(2).unwrap_or(&Value::Null))?;
                Ok(Value::Null)
            }
            "NODES" => Ok(ids(st.tree.in_group(&text_arg(name, &a, 0)?))),
            "CHILDREN" => {
                let id = target(st, a.first())?;
                Ok(ids(st.tree.nodes[id].children.clone()))
            }
            "PARENT" => {
                let id = target(st, a.first())?;
                Ok(st.tree.nodes[id].parent.map(id_value).unwrap_or(Value::Null))
            }
            "NAME_OF" => {
                let id = target(st, a.first())?;
                Ok(Value::text(&st.tree.nodes[id].name))
            }
            "PATH_OF" => {
                let id = target(st, a.first())?;
                Ok(Value::text(&st.tree.path_of(id)))
            }
            "SPAWN" => {
                let res = text_arg(name, &a, 0)?;
                let parent = match a.get(4) {
                    None | Some(Value::Null) => st.tree.root,
                    v => Some(target(st, v)?),
                };
                let project = st.project.clone();
                let id = scene::instance(&mut st.tree, parent, &res, &|p: &str| project.read_text(p)).map_err(|e| jlb::value::fault(e.0))?;
                let t = &mut st.tree.nodes[id].transform;
                t.position.x = num_arg(&a, 1, t.position.x as f64) as f32;
                t.position.y = num_arg(&a, 2, t.position.y as f64) as f32;
                t.position.z = num_arg(&a, 3, t.position.z as f64) as f32;
                st.queue.push_back(Pending::Created(id));
                Ok(id_value(id))
            }
            "DESTROY" => {
                let id = target(st, a.first())?;
                st.queue.push_back(Pending::Destroy(id));
                Ok(Value::Null)
            }
            "SEND" => {
                let id = target(st, a.first())?;
                let ev = text_arg(name, &a, 1)?;
                st.queue.push_back(Pending::Event { target: Some(id), handler: format!("on_{}", ev), args: a[2..].to_vec() });
                Ok(Value::Null)
            }
            "EMIT" => {
                let ev = text_arg(name, &a, 0)?;
                st.queue.push_back(Pending::Event { target: None, handler: format!("on_{}", ev), args: a[1..].to_vec() });
                Ok(Value::Null)
            }
            "TIMER" => {
                let node = target(st, None)?;
                let n = text_arg(name, &a, 0)?;
                let secs = num_arg(&a, 1, 1.0);
                let repeat = a.get(2).is_some_and(|v| jlb::ops::truth(v).unwrap_or(false));
                st.timers.retain(|t| !(t.node == node && t.name == n));
                st.timers.push(Timer { node, name: n, left: secs, every: repeat.then_some(secs) });
                Ok(Value::Null)
            }
            "TIMER_STOP" => {
                let node = target(st, None)?;
                let n = text_arg(name, &a, 0)?;
                st.timers.retain(|t| !(t.node == node && t.name == n));
                Ok(Value::Null)
            }
            "COLLIDING" => {
                let id = target(st, a.first())?;
                let mut v: Vec<NodeId> = st.touching.iter().filter(|(x, _)| *x == id).map(|(_, y)| *y).collect();
                v.sort();
                Ok(ids(v))
            }
            "DISTANCE" => {
                let x = target(st, a.first())?;
                let y = target(st, a.get(1))?;
                let p = |i| st.tree.global_matrix(i).w_axis.truncate();
                Ok(num(p(x).distance(p(y)) as f64))
            }
            "KEY_DOWN" => Ok(Value::Bool(st.input.key_down(&text_arg(name, &a, 0)?.to_lowercase()))),
            "KEY_PRESSED" => Ok(Value::Bool(st.input.key_pressed(&text_arg(name, &a, 0)?.to_lowercase()))),
            "KEY_RELEASED" => Ok(Value::Bool(st.input.key_released(&text_arg(name, &a, 0)?.to_lowercase()))),
            "MOUSE_X" => Ok(num(st.input.mouse[0] as f64)),
            "MOUSE_Y" => Ok(num(st.input.mouse[1] as f64)),
            "DELTA_TIME" => Ok(num(st.dt)),
            "GAME_TIME" => Ok(num(st.time)),
            "CHANGE_SCENE" => {
                st.change_scene = Some(text_arg(name, &a, 0)?);
                Ok(Value::Null)
            }
            "QUIT" => {
                st.quit = true;
                Ok(Value::Null)
            }
            _ => err(format!("{} isn't an engine command", name)),
        })();
        Ok(r)
    }
}
