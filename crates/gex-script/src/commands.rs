//! Engine commands available to every script, ALL CAPS like the rest of .jlb's built-ins.
//! Wherever a command takes a node, it accepts a node id (a number), a path text
//! ("Gun", "../Enemy", "/Level/Door"; relative to the running node), or NULL for the
//! running node itself.

use std::cell::RefCell;
use std::rc::Rc;

use gex_core::{scene, NodeId};
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
    ("TWEEN", "(node, property, to, seconds, ease)", "slides a number, position or color to a new value; ease: linear, in, out (default), in_out, back, bounce, elastic; then on_tween_end(property)"),
    ("TWEEN_STOP", "(node, property)", "stops a node's tweens (just that property's, if given), leaving the value where it is"),
    ("TIMER_STOP", "(name)", "cancels this node's timer"),
    ("COLLIDING", "(node)", "the nodes touching this one now, as a table of ids"),
    ("MOVE", "(node, dx, dy, dz)", "moves a body, sliding along walls; gives TRUE when it's on the floor after"),
    ("IMPULSE", "(node, x, y, z)", "adds to a dynamic body's velocity at once (a kick or a jump), whatever its mass"),
    ("RAYCAST", "(x, y, dx, dy, length, ignore)", "the first 2D body along a line: [node:, x:, y:, nx:, ny:, distance:] or NULL"),
    ("RAYCAST_3D", "(x, y, z, dx, dy, dz, length, ignore)", "the same in 3D"),
    ("DISTANCE", "(a, b)", "how far apart two nodes are"),
    ("PLAY_SOUND", "(file, volume, pitch)", "plays a sound file once (res:// or hl2://...); gives its id"),
    ("STOP_SOUND", "(id)", "stops a sound from PLAY_SOUND (NULL stops every sound)"),
    ("PLAY", "(node)", "starts a Sound node"),
    ("STOP", "(node)", "stops a Sound node"),
    ("MUSIC", "(file, volume)", "plays a looping music track, replacing the last one (\"\" stops it)"),
    ("VOLUME", "(level)", "the master volume, 0 to 1"),
    ("BURST", "(node, count)", "a Particles2D throws out this many particles at once (an explosion, a puff)"),
    ("ANIMATE", "(node, name)", "plays an animation: a Sprite2D's (from its animations property) or a model's"),
    ("ANIMATIONS", "(node)", "the names of a Mesh3D model's animations"),
    ("TILE", "(node, col, row)", "a TileMap2D's tile id at a cell (-1 = empty)"),
    ("SET_TILE", "(node, col, row, id)", "changes a tile (-1 clears it)"),
    ("TILE_AT", "(node, x, y)", "the cell under a world position: [col:, row:]"),
    ("FIND_PATH", "(tilemap, x1, y1, x2, y2)", "a walkable path through a TileMap2D: a table of [x:, y:] points, or NULL"),
    ("SAVE_DATA", "(name, value)", "keeps a value (a table, number, text...) on disk between runs"),
    ("LOAD_DATA", "(name)", "a value kept with SAVE_DATA, or NULL"),
    ("DELETE_DATA", "(name)", "forgets a value kept with SAVE_DATA"),
    ("DRAW_RECT", "(x, y, w, h, color, filled)", "draws a rectangle this frame (2D world)"),
    ("DRAW_LINE", "(x1, y1, x2, y2, color)", "draws a line this frame"),
    ("DRAW_CIRCLE", "(x, y, radius, color, filled)", "draws a circle this frame"),
    ("DRAW_TEXT", "(x, y, text, size, color)", "draws text this frame"),
    ("DRAW_IMAGE", "(file, x, y, w, h)", "draws an image this frame"),
    ("KEY_DOWN", "(key)", "TRUE while a key is held (mouse buttons: \"mouse_left\", \"mouse_right\", \"mouse_middle\")"),
    ("KEY_PRESSED", "(key)", "TRUE on the frame a key went down"),
    ("KEY_RELEASED", "(key)", "TRUE on the frame a key went up"),
    ("MOUSE_X", "()", "mouse x in game coordinates"),
    ("MOUSE_Y", "()", "mouse y in game coordinates"),
    ("MOUSE_WHEEL", "()", "how far the wheel turned this frame"),
    ("MOUSE_LOCK", "(on)", "hides the mouse and keeps it in the window, for mouse-look (Escape frees it in the editor)"),
    ("MOUSE_DX", "()", "how far the mouse moved sideways this frame, in screen pixels (works while locked)"),
    ("MOUSE_DY", "()", "how far the mouse moved up or down this frame (down is positive)"),
    ("AXIS", "(name)", "a gamepad stick or trigger, -1 to 1: left_x, left_y, right_x, right_y, lt, rt"),
    ("DELTA_TIME", "()", "seconds since the last frame"),
    ("GAME_TIME", "()", "seconds since the game started"),
    ("CHANGE_SCENE", "(scene, fade, color)", "switches to another scene file; with fade (seconds) the screen fades to color (default black) and back"),
    ("REMEMBER", "(name, value)", "keeps a value while the game runs, across CHANGE_SCENE (lives, score, inventory)"),
    ("RECALL", "(name)", "a value kept with REMEMBER, or NULL"),
    ("SHAKE", "(strength, seconds)", "shakes the camera: strength in pixels (2D), dying away over seconds"),
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

/// Something a script drew this frame (2D world coordinates).
#[derive(Clone, Debug)]
pub enum Draw {
    Rect { x: f32, y: f32, w: f32, h: f32, color: [f32; 4], filled: bool },
    Line { a: [f32; 2], b: [f32; 2], color: [f32; 4] },
    Circle { x: f32, y: f32, r: f32, color: [f32; 4], filled: bool },
    Text { x: f32, y: f32, text: String, size: f32, color: [f32; 4] },
    Image { file: String, x: f32, y: f32, w: f32, h: f32 },
}

/// Turns DRAW_* commands into sprites, lines and labels on top of the scene.
pub fn append_draws(draws: &[Draw], list: &mut gex_core::render::RenderList) {
    use gex_core::render::{Label, Region, Sprite};
    let node = NodeId::default();
    let sprite = |x: f32, y: f32, w: f32, h: f32, color, texture: &str| Sprite {
        node,
        model: glam::Mat4::from_translation(glam::Vec3::new(x, y, 0.0)),
        size: glam::Vec2::new(w, h),
        centered: false,
        color,
        texture: texture.to_string(),
        layer: 1000.0,
        region: Region::Whole,
        flip: [false; 2],
    };
    for d in draws {
        match d {
            Draw::Rect { x, y, w, h, color, filled: true } => list.sprites.push(sprite(*x, *y, *w, *h, *color, "")),
            Draw::Rect { x, y, w, h, color, filled: false } => {
                let p = [[*x, *y], [x + w, *y], [x + w, y + h], [*x, y + h]];
                for k in 0..4 {
                    list.lines_2d.push((p[k], p[(k + 1) % 4], *color));
                }
            }
            Draw::Line { a, b, color } => list.lines_2d.push((*a, *b, *color)),
            Draw::Circle { x, y, r, color, filled: true } => list.sprites.push(sprite(x - r, y - r, r * 2.0, r * 2.0, *color, "@circle")),
            Draw::Circle { x, y, r, color, filled: false } => {
                let n = 32;
                for k in 0..n {
                    let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
                    let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
                    list.lines_2d.push(([x + r * a0.cos(), y + r * a0.sin()], [x + r * a1.cos(), y + r * a1.sin()], *color));
                }
            }
            Draw::Text { x, y, text, size, color } => list.labels.push(Label { node, pos: glam::Vec2::new(*x, *y), text: text.clone(), size: *size, color: *color, layer: 1000.0 }),
            Draw::Image { file, x, y, w, h } => list.sprites.push(sprite(*x, *y, *w, *h, [1.0; 4], file)),
        }
    }
}

/// A color given as a table ([r:, g:, b:, a:] or [r, g, b, a], 0 to 1), as text ("#ff8800",
/// "red"...), or NULL for white.
fn color_arg(v: Option<&Value>) -> [f32; 4] {
    match v {
        None | Some(Value::Null) => [1.0; 4],
        Some(Value::Text(t)) => {
            let t = t.trim().to_ascii_lowercase();
            let named = match t.as_str() {
                "white" => Some([1.0, 1.0, 1.0]),
                "black" => Some([0.0, 0.0, 0.0]),
                "red" => Some([0.9, 0.2, 0.2]),
                "green" => Some([0.3, 0.8, 0.3]),
                "blue" => Some([0.25, 0.45, 0.95]),
                "yellow" => Some([0.95, 0.85, 0.2]),
                "orange" => Some([0.95, 0.55, 0.15]),
                "purple" => Some([0.6, 0.3, 0.85]),
                "gray" | "grey" => Some([0.5, 0.5, 0.5]),
                _ => None,
            };
            if let Some(c) = named {
                return [c[0], c[1], c[2], 1.0];
            }
            let h = t.trim_start_matches('#');
            let p = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255) as f32 / 255.0;
            if h.len() >= 6 {
                [p(0), p(2), p(4), if h.len() >= 8 { p(6) } else { 1.0 }]
            } else {
                [1.0; 4]
            }
        }
        Some(t @ Value::Table(..)) => match to_variant(t, Some(&gex_core::Variant::Color([1.0; 4]))) {
            Some(gex_core::Variant::Color(c)) if jlb::embed::table_get(t, "r").is_some() => c,
            _ => match t {
                Value::Table(rc, _) => {
                    let g = |i: usize, d: f32| rc.items.get(i).and_then(|s| as_f64(&s.borrow())).map(|v| v as f32).unwrap_or(d);
                    [g(0, 1.0), g(1, 1.0), g(2, 1.0), g(3, 1.0)]
                }
                _ => [1.0; 4],
            },
        },
        Some(_) => [1.0; 4],
    }
}

fn truthy(v: Option<&Value>, default: bool) -> bool {
    match v {
        None | Some(Value::Null) => default,
        Some(v) => jlb::ops::truth(v).unwrap_or(default),
    }
}

/// Starts a Sound node's stream (restarting it if it's playing).
pub fn play_node(st: &mut WorldState, id: NodeId) -> Result<(), String> {
    let n = &st.tree.nodes[id];
    if n.kind != gex_core::NodeKind::Sound {
        return Err(format!("{} isn't a Sound node", n.name));
    }
    let file = n.text("stream").to_string();
    if file.is_empty() {
        return Err(format!("the Sound node {} has no stream file", n.name));
    }
    let (vol, pitch, looped) = (n.num("volume", 1.0) as f32, n.num("pitch", 1.0) as f32, n.prop("loop").is_some_and(|v| v.as_bool()));
    if let Some(old) = st.audio.by_node.remove(&id) {
        st.audio.stop(old);
    }
    let r = st.audio.play(&st.vfs, &file, vol, pitch, looped);
    let s = r?;
    if s != 0 {
        st.audio.by_node.insert(id, s);
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
            "BURST" => {
                let id = target(st, a.first())?;
                if st.tree.nodes[id].kind != gex_core::NodeKind::Particles2D {
                    return err(format!("{} isn't a Particles2D", st.tree.nodes[id].name));
                }
                let n = num_arg(&a, 1, 20.0).max(0.0) as usize;
                let st = &mut *st;
                st.particles.entry(id).or_default().spawn(&st.tree, id, n);
                Ok(Value::Null)
            }
            "TWEEN" => {
                let id = target(st, a.first())?;
                let prop = text_arg(name, &a, 1)?;
                let Some(from) = crate::tween::read(&st.tree, id, &prop) else {
                    return err(format!("{} has no number or color property {:?} to tween", st.tree.nodes[id].name, prop));
                };
                let to = match (from, a.get(2)) {
                    (crate::tween::Val::Color(_), Some(v)) => crate::tween::Val::Color(color_arg(Some(v))),
                    (_, Some(v)) => match as_f64(v) {
                        Some(x) => crate::tween::Val::Num(x),
                        None => return err("TWEEN needs a number to go to"),
                    },
                    _ => return err("TWEEN needs a value to go to"),
                };
                let ease = match a.get(4) {
                    Some(Value::Null) | None => "out".to_string(),
                    Some(_) => text_arg(name, &a, 4)?,
                };
                if !crate::tween::EASES.contains(&ease.as_str()) {
                    return err(format!("no ease called {:?}; try {}", ease, crate::tween::EASES.join(", ")));
                }
                st.tweens.retain(|t| !(t.node == id && t.prop == prop));
                st.tweens.push(crate::tween::Tween { node: id, prop, from, to, time: 0.0, length: num_arg(&a, 3, 0.5).max(0.0), ease });
                Ok(Value::Null)
            }
            "TWEEN_STOP" => {
                let id = target(st, a.first())?;
                let prop = match a.get(1) {
                    Some(Value::Null) | None => None,
                    Some(_) => Some(text_arg(name, &a, 1)?),
                };
                st.tweens.retain(|t| !(t.node == id && prop.as_ref().is_none_or(|p| *p == t.prop)));
                Ok(Value::Null)
            }
            "MOVE" => {
                let id = target(st, a.first())?;
                let d = glam::Vec3::new(num_arg(&a, 1, 0.0) as f32, num_arg(&a, 2, 0.0) as f32, num_arg(&a, 3, 0.0) as f32);
                let g = st.physics.move_node(&mut st.tree, id, d).map_err(jlb::value::fault)?;
                Ok(Value::Bool(g))
            }
            "IMPULSE" => {
                let id = target(st, a.first())?;
                let v = glam::Vec3::new(num_arg(&a, 1, 0.0) as f32, num_arg(&a, 2, 0.0) as f32, num_arg(&a, 3, 0.0) as f32);
                st.physics.impulse(&st.tree, id, v);
                Ok(Value::Null)
            }
            "RAYCAST" | "RAYCAST_3D" => {
                let three = name == "RAYCAST_3D";
                let n = |i| num_arg(&a, i, 0.0) as f32;
                let (len_i, ign_i) = if three { (6, 7) } else { (4, 5) };
                let max = num_arg(&a, len_i, if three { 1000.0 } else { 100000.0 }) as f32;
                let ignore = match a.get(ign_i) {
                    None | Some(Value::Null) => None,
                    v => Some(target(st, v)?),
                };
                let hit = if three {
                    st.physics.raycast_3d(glam::Vec3::new(n(0), n(1), n(2)), glam::Vec3::new(n(3), n(4), n(5)), max, ignore)
                } else {
                    st.physics.raycast_2d(glam::Vec2::new(n(0), n(1)), glam::Vec2::new(n(2), n(3)), max, ignore)
                };
                Ok(match hit {
                    None => Value::Null,
                    Some(h) => jlb::embed::named_table(vec![
                        ("node", id_value(h.node)),
                        ("x", num(h.point.x as f64)),
                        ("y", num(h.point.y as f64)),
                        ("z", num(h.point.z as f64)),
                        ("nx", num(h.normal.x as f64)),
                        ("ny", num(h.normal.y as f64)),
                        ("nz", num(h.normal.z as f64)),
                        ("distance", num(h.distance as f64)),
                    ]),
                })
            }
            "DISTANCE" => {
                let x = target(st, a.first())?;
                let y = target(st, a.get(1))?;
                let p = |i| st.tree.global_matrix(i).w_axis.truncate();
                Ok(num(p(x).distance(p(y)) as f64))
            }
            "PLAY_SOUND" => {
                let f = text_arg(name, &a, 0)?;
                let r = st.audio.play(&st.vfs, &f, num_arg(&a, 1, 1.0) as f32, num_arg(&a, 2, 1.0) as f32, false);
                Ok(num(r.map_err(jlb::value::fault)? as f64))
            }
            "STOP_SOUND" => {
                match a.first() {
                    None | Some(Value::Null) => st.audio.stop_all(),
                    Some(v) => st.audio.stop(as_f64(v).unwrap_or(0.0) as u64),
                }
                Ok(Value::Null)
            }
            "PLAY" => {
                let id = target(st, a.first())?;
                play_node(st, id).map_err(jlb::value::fault)?;
                Ok(Value::Null)
            }
            "STOP" => {
                let id = target(st, a.first())?;
                if let Some(s) = st.audio.by_node.remove(&id) {
                    st.audio.stop(s);
                }
                Ok(Value::Null)
            }
            "MUSIC" => {
                let f = text_arg(name, &a, 0)?;
                let r = st.audio.music(&st.vfs, &f, num_arg(&a, 1, 1.0) as f32);
                r.map_err(jlb::value::fault)?;
                Ok(Value::Null)
            }
            "VOLUME" => {
                st.audio.master = num_arg(&a, 0, 1.0).clamp(0.0, 4.0) as f32;
                Ok(Value::Null)
            }
            "ANIMATE" => {
                let id = target(st, a.first())?;
                let anim = text_arg(name, &a, 1)?;
                st.anims.remove(&id);
                let n = &mut st.tree.nodes[id];
                n.props.insert("animation".into(), gex_core::Variant::Text(anim));
                if n.kind == gex_core::NodeKind::Mesh3D {
                    n.props.insert("anim_time".into(), gex_core::Variant::Num(0.0));
                }
                Ok(Value::Null)
            }
            "ANIMATIONS" => {
                let id = target(st, a.first())?;
                let mesh = st.tree.nodes[id].text("mesh").to_string();
                let names = match gex_assets::model::cached(&st.vfs, &mesh) {
                    Ok(m) => m.animations.iter().map(|a| Value::text(&a.name)).collect(),
                    Err(_) => Vec::new(),
                };
                Ok(list_table(names))
            }
            "TILE" | "SET_TILE" => {
                let id = target(st, a.first())?;
                if st.tree.nodes[id].kind != gex_core::NodeKind::TileMap2D {
                    return err(format!("{} isn't a TileMap2D", st.tree.nodes[id].name));
                }
                let (c, r) = (num_arg(&a, 1, 0.0).floor() as i32, num_arg(&a, 2, 0.0).floor() as i32);
                let mut grid = gex_core::tiles::parse(st.tree.nodes[id].text("tiles"));
                if name == "TILE" {
                    return Ok(num(gex_core::tiles::get(&grid, c, r) as f64));
                }
                gex_core::tiles::set(&mut grid, c, r, num_arg(&a, 3, -1.0) as i32);
                st.tree.nodes[id].props.insert("tiles".into(), gex_core::Variant::Text(gex_core::tiles::to_text(&grid)));
                Ok(Value::Null)
            }
            "TILE_AT" => {
                let id = target(st, a.first())?;
                let n = &st.tree.nodes[id];
                let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
                let local = st.tree.global_matrix(id).inverse().transform_point3(glam::Vec3::new(num_arg(&a, 1, 0.0) as f32, num_arg(&a, 2, 0.0) as f32, 0.0));
                Ok(jlb::embed::named_table(vec![("col", num((local.x / tw).floor() as f64)), ("row", num((local.y / th).floor() as f64))]))
            }
            "DRAW_RECT" | "DRAW_CIRCLE" | "DRAW_LINE" | "DRAW_TEXT" | "DRAW_IMAGE" => {
                let n = |i| num_arg(&a, i, 0.0) as f32;
                let d = match name {
                    "DRAW_RECT" => Draw::Rect { x: n(0), y: n(1), w: n(2), h: n(3), color: color_arg(a.get(4)), filled: truthy(a.get(5), true) },
                    "DRAW_CIRCLE" => Draw::Circle { x: n(0), y: n(1), r: n(2), color: color_arg(a.get(3)), filled: truthy(a.get(4), true) },
                    "DRAW_LINE" => Draw::Line { a: [n(0), n(1)], b: [n(2), n(3)], color: color_arg(a.get(4)) },
                    "DRAW_TEXT" => Draw::Text { x: n(0), y: n(1), text: a.get(2).map(display).unwrap_or_default(), size: num_arg(&a, 3, 16.0) as f32, color: color_arg(a.get(4)) },
                    _ => Draw::Image { file: text_arg(name, &a, 0)?, x: n(1), y: n(2), w: num_arg(&a, 3, 64.0) as f32, h: num_arg(&a, 4, 64.0) as f32 },
                };
                if st.draws.len() < 100_000 {
                    st.draws.push(d);
                }
                Ok(Value::Null)
            }
            "FIND_PATH" => {
                let id = target(st, a.first())?;
                if st.tree.nodes[id].kind != gex_core::NodeKind::TileMap2D {
                    return err("FIND_PATH needs a TileMap2D to walk through");
                }
                let n = |i| num_arg(&a, i, 0.0) as f32;
                let p = crate::extras::find_path(&st.tree, id, glam::Vec2::new(n(1), n(2)), glam::Vec2::new(n(3), n(4)));
                Ok(crate::extras::path_value(p))
            }
            "SAVE_DATA" => {
                let key = text_arg(name, &a, 0)?;
                crate::extras::save(&st.project.config.name, &key, a.get(1).unwrap_or(&Value::Null)).map_err(jlb::value::fault)?;
                Ok(Value::Null)
            }
            "LOAD_DATA" => crate::extras::load(&st.project.config.name, &text_arg(name, &a, 0)?).map_err(jlb::value::fault),
            "DELETE_DATA" => {
                crate::extras::delete(&st.project.config.name, &text_arg(name, &a, 0)?).map_err(jlb::value::fault)?;
                Ok(Value::Null)
            }
            "MOUSE_WHEEL" => Ok(num(st.wheel as f64)),
            "MOUSE_LOCK" => {
                st.mouse_locked = a.first().is_none_or(|v| !matches!(v, Value::Bool(false) | Value::Null));
                Ok(Value::Null)
            }
            "MOUSE_DX" => Ok(num(st.mouse_delta[0] as f64)),
            "MOUSE_DY" => Ok(num(st.mouse_delta[1] as f64)),
            "AXIS" => {
                let k = text_arg(name, &a, 0)?.to_ascii_lowercase();
                Ok(num(st.axes.get(&k).copied().unwrap_or(0.0) as f64))
            }
            "KEY_DOWN" => Ok(Value::Bool(st.input.key_down(&text_arg(name, &a, 0)?.to_lowercase()))),
            "KEY_PRESSED" => Ok(Value::Bool(st.input.key_pressed(&text_arg(name, &a, 0)?.to_lowercase()))),
            "KEY_RELEASED" => Ok(Value::Bool(st.input.key_released(&text_arg(name, &a, 0)?.to_lowercase()))),
            "MOUSE_X" => Ok(num(st.input.mouse[0] as f64)),
            "MOUSE_Y" => Ok(num(st.input.mouse[1] as f64)),
            "DELTA_TIME" => Ok(num(st.dt)),
            "GAME_TIME" => Ok(num(st.time)),
            "CHANGE_SCENE" => {
                let scene = text_arg(name, &a, 0)?;
                let fade = num_arg(&a, 1, 0.0) as f32;
                if fade > 0.0 {
                    if st.fade.as_ref().is_none_or(|f| f.to_scene.is_none()) {
                        let color = match a.get(2) {
                            Some(v) if !matches!(v, Value::Null) => color_arg(Some(v)),
                            _ => [0.0, 0.0, 0.0, 1.0],
                        };
                        st.fade = Some(crate::world::Fade { to_scene: Some(scene), time: 0.0, length: fade / 2.0, color });
                    }
                } else {
                    st.change_scene = Some(scene);
                }
                Ok(Value::Null)
            }
            "REMEMBER" => {
                let key = text_arg(name, &a, 0)?;
                let v = crate::extras::to_json(a.get(1).unwrap_or(&Value::Null), 0).map_err(jlb::value::fault)?;
                st.shared.insert(key, v);
                Ok(Value::Null)
            }
            "RECALL" => Ok(st.shared.get(&text_arg(name, &a, 0)?).map(crate::extras::from_json).unwrap_or(Value::Null)),
            "SHAKE" => {
                let s = num_arg(&a, 0, 8.0).max(0.0) as f32;
                let t = num_arg(&a, 1, 0.4).max(0.01) as f32;
                if s >= st.shake.0 {
                    st.shake = (s, s / t);
                }
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
