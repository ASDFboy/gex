//! Engine commands available to every script, ALL CAPS like the rest of .jlb's built-ins.
//! Wherever a command takes an object, it accepts an object id (a number), a path text
//! ("Gun", "../Enemy", "/Level/Door"; relative to the running object), or NULL for the
//! running object itself.

use std::cell::RefCell;
use std::rc::Rc;

use gex_core::{blueprint, ObjectId, Variant};
use jlb::embed::{as_f64, list_table, num};
use jlb::host::Host;
use jlb::value::{display, err};
use jlb::{Interp, Value, R};

use crate::convert::{id_value, to_value, to_variant, value_id};
use crate::world::{Pending, Timer, WorldState};

/// (name, inputs, what it does) — the editor shows these as help and completions.
pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("OBJECT", "(path)", "the object at a path (relative to this one), or by bare name anywhere in the blueprint, or NULL"),
    ("GET", "(object, property)", "a property of another object"),
    ("SET", "(object, property, value)", "changes a property of another object"),
    ("OBJECTS", "(group)", "every object in a group, as a table of ids"),
    ("CHILDREN", "(object)", "an object's children, as a table of ids"),
    ("PARENT", "(object)", "an object's parent, or NULL"),
    ("NAME_OF", "(object)", "an object's name"),
    ("PATH_OF", "(object)", "an object's full path"),
    ("SPAWN", "(blueprint, x, y, z, parent)", "adds an instance of a blueprint file; gives its id"),
    ("DESTROY", "(object)", "removes an object (at the end of the frame); runs on_destroy"),
    ("SEND", "(object, event, inputs...)", "runs on_<event>(inputs...) on one object"),
    ("EMIT", "(event, inputs...)", "runs on_<event>(inputs...) on every object that has it"),
    ("BROADCAST", "(event, inputs...)", "the same as EMIT: every object with on_<event> receives it"),
    ("TIMER", "(name, seconds, repeat)", "runs on_timer(name) and on_timer_<name>() later"),
    ("TWEEN", "(object, property, to, seconds, ease)", "slides a number, position or color to a new value; ease: linear, in, out (default), in_out, back, bounce, elastic; then on_tween_end(property)"),
    ("TWEEN_STOP", "(object, property)", "stops an object's tweens (just that property's, if given), leaving the value where it is"),
    ("TIMER_STOP", "(name)", "cancels this object's timer"),
    ("COLLIDING", "(object)", "the objects touching this one now, as a table of ids"),
    ("MOVE", "(object, dx, dy, dz)", "moves a body, sliding along walls; gives TRUE when it's on the floor after"),
    ("IMPULSE", "(object, x, y, z)", "adds to a dynamic body's velocity at once (a kick or a jump), whatever its mass"),
    ("RAYCAST", "(x, y, dx, dy, length, ignore, layers)", "the first 2D body along a line: [object:, x:, y:, nx:, ny:, distance:] or NULL; layers (like \"world, enemies\") limits what it can hit"),
    ("RAYCAST_3D", "(x, y, z, dx, dy, dz, length, ignore, layers)", "the same in 3D"),
    ("DISTANCE", "(a, b)", "how far apart two objects are"),
    ("PLAY_SOUND", "(file, volume, pitch, pan, echo)", "plays a sound file once (res:// or hl2://...); pan -1 left to 1 right, echo 0 to 1; gives its id"),
    ("STOP_SOUND", "(id)", "stops a sound from PLAY_SOUND (NULL stops every sound)"),
    ("PLAY", "(object)", "starts a Sound object, or starts a Timer over"),
    ("STOP", "(object)", "stops a Sound object or a Timer"),
    ("MUSIC", "(file, volume)", "plays a looping music track, replacing the last one (\"\" stops it)"),
    ("VOLUME", "(level)", "the master volume, 0 to 1"),
    ("BURST", "(object, count)", "a Particles2D or Particles3D throws out this many particles at once (an explosion, a puff)"),
    ("RAGDOLL", "(object, vx, vy, vz)", "a Mesh3D with a skinned model (a character) goes limp and falls as a ragdoll, starting at that velocity; IMPULSE shoves it"),
    ("USE_DOOR", "(door, open)", "opens or closes one of a map's doors (in the group map_doors); with no second input, toggles it"),
    ("MAP_ENTITIES", "(map, classname)", "the entities in a map (a Mesh3D showing a .bsp): a table of [classname:, name:, x:, y:, z:, rotation_y:, keys:, bounds:] (keys holds every setting as written; bounds is a brush entity's box, like a ladder's); classname picks one kind, \"info_player*\" any that start so"),
    ("PLAY_CLIP", "(animator, clip)", "an Animator plays one of its clips (made in the Timeline) from the start; on_clip_end(clip) runs on its parent when it ends"),
    ("STOP_CLIP", "(animator)", "stops an Animator's clip where it is"),
    ("CLIP_PLAYING", "(animator)", "the name of the clip an Animator is playing, or NULL"),
    ("ANIMATE", "(object, name, restart)", "plays an animation: a Sprite2D's (from its animations property) or a model's; the one already playing carries on unless restart is TRUE"),
    ("ANIMATIONS", "(object)", "the names of a Mesh3D model's animations"),
    ("TILE", "(object, col, row)", "a TileMap2D's tile id at a cell (-1 = empty)"),
    ("SET_TILE", "(object, col, row, id)", "changes a tile (-1 clears it)"),
    ("TILE_AT", "(object, x, y)", "the cell under a world position: [col:, row:]"),
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
    ("AXIS", "(name) or (key_minus, key_plus)", "a gamepad stick or trigger, -1 to 1: left_x, left_y, right_x, right_y, lt, rt. With two keys: -1 while the first is held, 1 for the second, 0 for both or neither (x += AXIS(\"a\", \"d\") * speed * dt)"),
    ("DELTA_TIME", "()", "seconds since the last frame"),
    ("GAME_TIME", "()", "seconds since the game started"),
    ("CHANGE_BLUEPRINT", "(blueprint, fade, color)", "switches to another blueprint file; with fade (seconds) the screen fades to color (default black) and back"),
    ("REMEMBER", "(name, value)", "keeps a value while the game runs, across CHANGE_BLUEPRINT (lives, score, inventory)"),
    ("RECALL", "(name)", "a value kept with REMEMBER, or NULL"),
    ("SHAKE", "(strength, seconds)", "shakes the camera: strength in pixels (2D), dying away over seconds"),
    ("PAUSE", "(on)", "pauses the game (TRUE) or carries on (FALSE): only objects in the group while_paused keep running, for a pause menu"),
    ("PAUSED", "()", "TRUE while the game is paused"),
    ("TIME_SCALE", "(scale)", "how fast game time runs: 1 normal, 0.3 slow motion, 2 double; with no input, gives the current scale"),
    ("FULLSCREEN", "(on)", "switches the game window to full screen (TRUE) or back (FALSE); with no input, gives whether it is"),
    ("WINDOW_TITLE", "(text)", "changes the game window's title"),
    ("QUIT", "()", "stops the game"),
    ("SET_GRAPHICS", "(setting, value)", "changes a project graphics setting while the game runs (render_scale, shadows, bloom, tonemap, exposure, saturation, vignette...; see Project settings)"),
    ("GRAPHICS", "(setting)", "a graphics setting's value; with no input, a table of all of them"),
    ("SET_PHYSICS", "(setting, value)", "changes a project physics setting while the game runs (gravity_2d, gravity_3d, tick_rate, solver_iterations, max_slope...)"),
    ("PHYSICS", "(setting)", "a physics setting's value; with no input, a table of all of them"),
    ("SET_DISPLAY", "(setting, value)", "changes a display setting while the game runs: width, height (the resolution), window_scale, fullscreen, resizable, scale_mode, scale_filter, bar_color"),
    ("DISPLAY", "(setting)", "a display setting's value; with no input, a table of all of them"),
    ("SAVE_DISPLAY", "(keep)", "keeps the current display settings for the next time the game starts (an options menu's Apply); FALSE forgets them"),
    ("SET_ACTION", "(action, keys)", "gives an input action new keys (a list or a text like \"space, w\"), for key rebinding; no keys removes it"),
    ("ACTION", "(action)", "an input action's keys as a list; with no input, a table of every action"),
    ("HOST_GAME", "(port)", "starts a multiplayer game other players can join (port 7777 if left out); you're player 1"),
    ("JOIN_GAME", "(address, port)", "joins a multiplayer game someone hosts (\"192.168.1.20\", \"localhost\", \"host:port\"); then on_connected(id), or on_join_failed(reason)"),
    ("LEAVE_GAME", "()", "leaves the multiplayer game, or stops hosting it"),
    ("ONLINE", "()", "TRUE while hosting or in someone's game"),
    ("IS_HOST", "()", "TRUE while hosting"),
    ("MY_ID", "()", "your player number: 1 for the host, 2, 3... for players who join; 0 when not online"),
    ("PLAYERS", "()", "every player's number (yours too), as a table"),
    ("NET_EMIT", "(event, inputs...)", "runs on_<event>(inputs...) on every object in every other player's game"),
    ("NET_SEND", "(player, event, inputs...)", "runs on_<event>(inputs...) on every object in one player's game"),
    ("SENDER", "()", "in an event from another player's game: that player's number (NULL otherwise)"),
    ("NET_SPAWN", "(blueprint, x, y, z, parent)", "SPAWN for everyone: the instance appears in every player's game and follows yours (position, rotation, size, visibility and properties); gives its id"),
    ("NET_SYNC", "(object)", "shares an object placed in the blueprint so it's the same in every game; the host's game moves it until SET_OWNER"),
    ("OWNER", "(object)", "the number of the player whose game moves a shared object"),
    ("IS_MINE", "(object)", "TRUE when your game moves this object: one you shared, or any that isn't shared; FALSE for other players' copies"),
    ("SET_OWNER", "(object, player)", "hands a shared object to another player's game, which moves it from then on"),
    ("NET_ID", "(object)", "a shared object's name, the same in every game (ids aren't): send it, then NET_OBJECT it"),
    ("NET_OBJECT", "(net_id)", "the object in this game for a NET_ID, or NULL"),
    ("MY_ADDRESS", "()", "this computer's address for friends to join: ZeroTier (or Tailscale) when it has one, otherwise the local network one"),
];

/// Names from before nodes were called objects and scenes blueprints: scripts using them
/// keep working.
pub const OLD_NAMES: &[(&str, &str)] = &[("NODE", "OBJECT"), ("NODES", "OBJECTS"), ("CHANGE_SCENE", "CHANGE_BLUEPRINT")];

pub fn register() {
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.0).chain(OLD_NAMES.iter().map(|o| o.0)).collect();
    jlb::host::register(&names);
}

pub struct EngineHost {
    pub st: Rc<RefCell<WorldState>>,
}

fn target(st: &WorldState, v: Option<&Value>) -> R<ObjectId> {
    let cur = st.current;
    let id = match v {
        None | Some(Value::Null) => cur,
        Some(Value::Text(p)) => find(st, p),
        Some(v @ Value::Num(_)) => value_id(v),
        Some(o) => return err(format!("an object is given as an id, a path or NULL, not a {}", o.type_name())),
    };
    match id {
        Some(i) if st.tree.contains(i) => Ok(i),
        _ => err(match v {
            Some(Value::Text(p)) => format!("there's no object at {}", p),
            _ => "that object doesn't exist (was it destroyed?)".into(),
        }),
    }
}

/// An object by path from the current one; a bare name not found there is looked for in
/// the whole blueprint ("Player" finds the Player wherever it is).
fn find(st: &WorldState, p: &str) -> Option<ObjectId> {
    st.tree.find_path(st.current, p).or_else(|| (!p.contains('/') && !p.is_empty() && p != "..").then(|| st.tree.find_named(p)).flatten())
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

fn ids(list: Vec<ObjectId>) -> Value {
    list_table(list.into_iter().map(id_value).collect())
}

/// An object property by name, including the transform shortcuts that `self` has.
fn get_prop(st: &WorldState, id: ObjectId, k: &str) -> Value {
    let n = &st.tree.objects[id];
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

fn set_prop(st: &mut WorldState, id: ObjectId, k: &str, v: &Value) -> R<()> {
    let n = &mut st.tree.objects[id];
    let like = n.props.get(k).cloned();
    let Some(var) = to_variant(v, like.as_ref()) else {
        return err(format!("a {} can't be stored in an object property", v.type_name()));
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

/// Turns DRAW_* commands into sprites, lines and labels on top of the blueprint.
pub fn append_draws(draws: &[Draw], list: &mut gex_core::render::RenderList) {
    use gex_core::render::{Label, Region, Sprite};
    let object = ObjectId::default();
    let sprite = |x: f32, y: f32, w: f32, h: f32, color, texture: &str| Sprite {
        object,
        model: glam::Mat4::from_translation(glam::Vec3::new(x, y, 0.0)),
        size: glam::Vec2::new(w, h),
        centered: false,
        color,
        texture: texture.to_string(),
        layer: 1000.0,
        region: Region::Whole,
        flip: [false; 2],
        parallax: glam::Vec2::ONE,
        repeat: [false; 2],
        ..Default::default()
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
            Draw::Text { x, y, text, size, color } => list.labels.push(Label { object, pos: glam::Vec2::new(*x, *y), text: text.clone(), size: *size, color: *color, layer: 1000.0, font: String::new() }),
            Draw::Image { file, x, y, w, h } => list.sprites.push(sprite(*x, *y, *w, *h, [1.0; 4], file)),
        }
    }
}

/// A color given as a table ([r:, g:, b:, a:] or [r, g, b, a], 0 to 1), as text ("#ff8800",
/// "red"...), or NULL for white.
pub(crate) fn color_arg(v: Option<&Value>) -> [f32; 4] {
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
        Some(t @ Value::Table(rc, _)) if rc.named.len() > 0 => match to_variant(t, Some(&gex_core::Variant::Color([1.0; 4]))) {
            Some(gex_core::Variant::Color(c)) => c,
            _ => [1.0; 4],
        },
        Some(Value::Table(rc, _)) => {
            let g = |i: usize, d: f32| rc.items.get(i).and_then(|s| as_f64(&s.borrow())).map(|v| v as f32).unwrap_or(d);
            [g(0, 1.0), g(1, 1.0), g(2, 1.0), g(3, 1.0)]
        }
        Some(_) => [1.0; 4],
    }
}

fn truthy(v: Option<&Value>, default: bool) -> bool {
    match v {
        None | Some(Value::Null) => default,
        Some(v) => jlb::ops::truth(v).unwrap_or(default),
    }
}

/// Starts a Sound object's stream (restarting it if it's playing).
pub fn play_object(st: &mut WorldState, id: ObjectId) -> Result<(), String> {
    let n = &st.tree.objects[id];
    if n.kind != gex_core::ObjectKind::Sound {
        return Err(format!("{} isn't a Sound object", n.name));
    }
    let file = n.text("stream").to_string();
    if file.is_empty() {
        return Err(format!("the Sound object {} has no stream file", n.name));
    }
    let (vol, pitch, looped) = (n.num("volume", 1.0) as f32, n.num("pitch", 1.0) as f32, n.prop("loop").is_some_and(|v| v.as_bool()));
    if let Some(old) = st.audio.by_object.remove(&id) {
        st.audio.stop(old);
    }
    let echo = n.num("echo", 0.0) as f32;
    let fx = crate::audio::SoundFx { pan: n.num("pan", 0.0) as f32, echo: (echo > 0.0).then_some((n.num("echo_delay", 0.15) as f32, echo)) };
    let r = st.audio.play_fx(&st.vfs, &file, vol, pitch, looped, fx);
    let s = r?;
    if s != 0 {
        st.audio.by_object.insert(id, s);
    }
    Ok(())
}

impl Host for EngineHost {
    fn call(&mut self, _it: &mut Interp, name: &'static str, a: Vec<Value>) -> Result<R<Value>, Vec<Value>> {
        let name = OLD_NAMES.iter().find(|o| o.0 == name).map_or(name, |o| o.1);
        if !COMMANDS.iter().any(|c| c.0 == name) {
            return Err(a);
        }
        let mut st = self.st.borrow_mut();
        let st = &mut *st;
        // Inside a task (START), NULL means the object the task belongs to.
        if let Some(tag) = _it.running_tag() {
            st.current = Some(crate::world::tag_object(tag));
        }
        let r: R<Value> = (|| match name {
            "OBJECT" => {
                let p = text_arg(name, &a, 0)?;
                Ok(find(st, &p).map(id_value).unwrap_or(Value::Null))
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
            "OBJECTS" => Ok(ids(st.tree.in_group(&text_arg(name, &a, 0)?))),
            "CHILDREN" => {
                let id = target(st, a.first())?;
                Ok(ids(st.tree.objects[id].children.clone()))
            }
            "PARENT" => {
                let id = target(st, a.first())?;
                Ok(st.tree.objects[id].parent.map(id_value).unwrap_or(Value::Null))
            }
            "NAME_OF" => {
                let id = target(st, a.first())?;
                Ok(Value::text(&st.tree.objects[id].name))
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
                let id = blueprint::instance(&mut st.tree, parent, &res, &|p: &str| project.read_text(p)).map_err(|e| jlb::value::fault(e.0))?;
                let t = &mut st.tree.objects[id].transform;
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
            "EMIT" | "BROADCAST" => {
                let ev = text_arg(name, &a, 0)?;
                st.queue.push_back(Pending::Event { target: None, handler: format!("on_{}", ev), args: a[1..].to_vec() });
                Ok(Value::Null)
            }
            "TIMER" => {
                let object = target(st, None)?;
                let n = text_arg(name, &a, 0)?;
                let secs = num_arg(&a, 1, 1.0);
                let repeat = a.get(2).is_some_and(|v| jlb::ops::truth(v).unwrap_or(false));
                st.timers.retain(|t| !(t.object == object && t.name == n));
                st.timers.push(Timer { object, name: n, left: secs, every: repeat.then_some(secs) });
                Ok(Value::Null)
            }
            "TIMER_STOP" => {
                let object = target(st, None)?;
                let n = text_arg(name, &a, 0)?;
                st.timers.retain(|t| !(t.object == object && t.name == n));
                Ok(Value::Null)
            }
            "COLLIDING" => {
                let id = target(st, a.first())?;
                let mut v: Vec<ObjectId> = st.touching.iter().filter(|(x, _)| *x == id).map(|(_, y)| *y).collect();
                v.sort();
                Ok(ids(v))
            }
            "BURST" => {
                let id = target(st, a.first())?;
                let n = num_arg(&a, 1, 20.0).max(0.0) as usize;
                let st = &mut *st;
                match st.tree.objects[id].kind {
                    gex_core::ObjectKind::Particles2D => st.particles.entry(id).or_default().spawn(&st.tree, id, n),
                    gex_core::ObjectKind::Particles3D => st.particles_3d.entry(id).or_default().spawn(&st.tree, id, n),
                    _ => return err(format!("{} isn't a Particles2D or Particles3D", st.tree.objects[id].name)),
                }
                Ok(Value::Null)
            }
            "TWEEN" => {
                let id = target(st, a.first())?;
                let prop = text_arg(name, &a, 1)?;
                let Some(from) = crate::tween::read(&st.tree, id, &prop) else {
                    return err(format!("{} has no number or color property {:?} to tween", st.tree.objects[id].name, prop));
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
                st.tweens.retain(|t| !(t.object == id && t.prop == prop));
                st.tweens.push(crate::tween::Tween { object: id, prop, from, to, time: 0.0, length: num_arg(&a, 3, 0.5).max(0.0), ease });
                Ok(Value::Null)
            }
            "TWEEN_STOP" => {
                let id = target(st, a.first())?;
                let prop = match a.get(1) {
                    Some(Value::Null) | None => None,
                    Some(_) => Some(text_arg(name, &a, 1)?),
                };
                st.tweens.retain(|t| !(t.object == id && prop.as_ref().is_none_or(|p| *p == t.prop)));
                Ok(Value::Null)
            }
            "MOVE" => {
                let id = target(st, a.first())?;
                let d = glam::Vec3::new(num_arg(&a, 1, 0.0) as f32, num_arg(&a, 2, 0.0) as f32, num_arg(&a, 3, 0.0) as f32);
                let g = st.physics.move_object(&mut st.tree, id, d).map_err(jlb::value::fault)?;
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
                let mask = match a.get(ign_i + 1) {
                    Some(Value::Text(t)) => st.physics.layers.bits(t, u32::MAX),
                    _ => u32::MAX,
                };
                let hit = if three {
                    st.physics.raycast_3d(glam::Vec3::new(n(0), n(1), n(2)), glam::Vec3::new(n(3), n(4), n(5)), max, ignore, mask)
                } else {
                    st.physics.raycast_2d(glam::Vec2::new(n(0), n(1)), glam::Vec2::new(n(2), n(3)), max, ignore, mask)
                };
                Ok(match hit {
                    None => Value::Null,
                    Some(h) => jlb::embed::named_table(vec![
                        ("object", id_value(h.object)),
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
                let echo = num_arg(&a, 4, 0.0) as f32;
                let fx = crate::audio::SoundFx { pan: num_arg(&a, 3, 0.0) as f32, echo: (echo > 0.0).then_some((0.18, echo)) };
                let r = st.audio.play_fx(&st.vfs, &f, num_arg(&a, 1, 1.0) as f32, num_arg(&a, 2, 1.0) as f32, false, fx);
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
                if st.tree.objects[id].kind == gex_core::ObjectKind::Timer {
                    let n = &mut st.tree.objects[id];
                    let wait = n.num("wait_time", 1.0).max(0.001);
                    n.props.insert("running".into(), Variant::Bool(true));
                    n.props.insert("time_left".into(), Variant::Num(wait));
                    return Ok(Value::Null);
                }
                play_object(st, id).map_err(jlb::value::fault)?;
                Ok(Value::Null)
            }
            "STOP" => {
                let id = target(st, a.first())?;
                if st.tree.objects[id].kind == gex_core::ObjectKind::Timer {
                    let n = &mut st.tree.objects[id];
                    n.props.insert("running".into(), Variant::Bool(false));
                    n.props.insert("time_left".into(), Variant::Num(0.0));
                    return Ok(Value::Null);
                }
                if let Some(s) = st.audio.by_object.remove(&id) {
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
            "RAGDOLL" => {
                let id = target(st, a.first())?;
                let n = &st.tree.objects[id];
                let mesh = n.text("mesh").to_string();
                if n.kind != gex_core::ObjectKind::Mesh3D || !gex_assets::model::is_model(&mesh) {
                    return err(format!("{} isn't a Mesh3D with a model", n.name));
                }
                let model = gex_assets::model::cached(&st.vfs, &mesh).map_err(jlb::value::fault)?;
                if model.joints.is_empty() {
                    return err(format!("{} has no skeleton to make a ragdoll from", mesh));
                }
                // Where the bones are right now (as animated), in the world.
                let anim = model.animation(n.text("animation"));
                let t = st.anims.get(&id).map(|a| a.1 as f32).unwrap_or(n.num("anim_time", 0.0) as f32);
                let world = st.tree.global_matrix(id);
                let joints: Vec<glam::Mat4> = model.pose(anim, t).iter().zip(&model.joints).map(|(m, j)| world * *m * j.inverse_bind.inverse()).collect();
                let parents: Vec<Option<usize>> = model.joints.iter().map(|j| j.parent).collect();
                let ibs: Vec<glam::Mat4> = model.joints.iter().map(|j| j.inverse_bind).collect();
                let v = glam::Vec3::new(num_arg(&a, 1, 0.0) as f32, num_arg(&a, 2, 0.0) as f32, num_arg(&a, 3, 0.0) as f32);
                st.anims.remove(&id);
                let st = &mut *st;
                st.physics.ragdoll(&st.tree, id, &joints, &parents, &ibs, v);
                Ok(Value::Null)
            }
            "USE_DOOR" => {
                let id = target(st, a.first())?;
                let n = &st.tree.objects[id];
                if n.text("door").is_empty() {
                    return err(format!("{} isn't one of a map's doors", n.name));
                }
                let open_now = n.prop("door_open").is_some_and(|v| v.as_bool());
                let want = match a.get(1) {
                    Some(Value::Bool(b)) => *b,
                    _ => !open_now,
                };
                if want == open_now {
                    return Ok(Value::Null);
                }
                let time = n.num("door_time", 1.0);
                // Remember where it started, the first time.
                let n = &mut st.tree.objects[id];
                if n.prop("door_home_x").is_none() {
                    let t = n.transform;
                    for (k, v) in [("door_home_x", t.position.x), ("door_home_y", t.position.y), ("door_home_z", t.position.z), ("door_home_ry", t.rotation.y)] {
                        n.props.insert(k.into(), gex_core::Variant::Num(v as f64));
                    }
                }
                n.props.insert("door_open".into(), gex_core::Variant::Bool(want));
                let k = if want { 1.0 } else { 0.0 };
                let mut go: Vec<(&str, f64)> = Vec::new();
                if n.prop("door_turn").is_some() {
                    go.push(("rotation_y", n.num("door_home_ry", 0.0) + n.num("door_turn", 90.0) * k));
                } else {
                    for (axis, key) in [("x", "door_move_x"), ("y", "door_move_y"), ("z", "door_move_z")] {
                        go.push((axis, n.num(&format!("door_home_{}", axis), 0.0) + n.num(key, 0.0) * k));
                    }
                }
                for (prop, to) in go {
                    let from = crate::tween::read(&st.tree, id, prop).unwrap_or(crate::tween::Val::Num(0.0));
                    st.tweens.retain(|t| !(t.object == id && t.prop == prop));
                    st.tweens.push(crate::tween::Tween { object: id, prop: prop.to_string(), from, to: crate::tween::Val::Num(to), time: 0.0, length: time, ease: "in_out".into() });
                }
                Ok(Value::Null)
            }
            "MAP_ENTITIES" => {
                let id = target(st, a.first())?;
                let mesh = st.tree.objects[id].text("mesh").to_string();
                if !gex_assets::model::map_base(&mesh).to_ascii_lowercase().ends_with(".bsp") {
                    return err(format!("{} doesn't show a map (.bsp)", st.tree.objects[id].name));
                }
                let want = match a.get(1) {
                    Some(Value::Text(t)) => t.to_ascii_lowercase(),
                    _ => String::new(),
                };
                let model = gex_assets::model::cached(&st.vfs, &mesh).map_err(jlb::value::fault)?;
                let world = st.tree.global_matrix(id);
                let mut out = Vec::new();
                for e in &model.entities {
                    let get = |k: &str| e.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()).unwrap_or_default();
                    let class = get("classname").to_ascii_lowercase();
                    let hit = want.is_empty() || match want.strip_suffix('*') {
                        Some(p) => class.starts_with(p),
                        None => class == want,
                    };
                    if !hit {
                        continue;
                    }
                    let m = world * gex_assets::model::entity_transform(e);
                    let (_, r, t) = m.to_scale_rotation_translation();
                    let yaw = r.to_euler(glam::EulerRot::YXZ).0.to_degrees();
                    let keys = jlb::embed::named_table(e.iter().map(|(k, v)| (k.as_str(), Value::text(v))).collect());
                    // Brush entities (ladders, triggers): their box in the world.
                    let mut bounds = Value::Null;
                    if let Some(n) = get("model").strip_prefix('*') {
                        let sub = format!("{}#*{}", gex_assets::model::map_base(&mesh), n);
                        if let Ok(b) = gex_assets::model::cached(&st.vfs, &sub) {
                            let o = world * gex_assets::model::entity_transform(e);
                            let (lo, hi) = (o.transform_point3(b.min), o.transform_point3(b.max));
                            let (lo, hi) = (lo.min(hi), lo.max(hi));
                            bounds = jlb::embed::named_table(vec![("min_x", num(lo.x as f64)), ("min_y", num(lo.y as f64)), ("min_z", num(lo.z as f64)), ("max_x", num(hi.x as f64)), ("max_y", num(hi.y as f64)), ("max_z", num(hi.z as f64))]);
                        }
                    }
                    out.push(jlb::embed::named_table(vec![
                        ("classname", Value::text(&class)),
                        ("name", Value::text(&get("targetname"))),
                        ("x", num(t.x as f64)),
                        ("y", num(t.y as f64)),
                        ("z", num(t.z as f64)),
                        ("rotation_y", num(yaw as f64)),
                        ("keys", keys),
                        ("bounds", bounds),
                    ]));
                }
                Ok(list_table(out))
            }
            "PLAY_CLIP" => {
                let id = target(st, a.first())?;
                let name = text_arg(name, &a, 1)?;
                if st.tree.objects[id].kind != gex_core::ObjectKind::Animator {
                    return err(format!("{} isn't an Animator", st.tree.objects[id].name));
                }
                let file = st.anim_file(id).map_err(jlb::value::fault)?;
                if file.clip(&name).is_none() {
                    let names: Vec<&str> = file.clips.iter().map(|c| c.name.as_str()).collect();
                    return err(format!("no clip called {:?} (there's {})", name, names.join(", ")));
                }
                st.clips_playing.insert(id, (name, 0.0));
                Ok(Value::Null)
            }
            "STOP_CLIP" => {
                let id = target(st, a.first())?;
                st.clips_playing.remove(&id);
                Ok(Value::Null)
            }
            "CLIP_PLAYING" => {
                let id = target(st, a.first())?;
                Ok(st.clips_playing.get(&id).map(|c| Value::text(&c.0)).unwrap_or(Value::Null))
            }
            "ANIMATE" => {
                let id = target(st, a.first())?;
                let anim = text_arg(name, &a, 1)?;
                // Already playing it: carry on (so calling ANIMATE every frame is fine).
                let restart = a.get(2).is_some_and(|v| matches!(v, Value::Bool(true)));
                let same = st.tree.objects[id].text("animation") == anim && st.anims.get(&id).is_none_or(|(n, _, ended)| *n == anim && !ended);
                if same && !restart {
                    return Ok(Value::Null);
                }
                st.anims.remove(&id);
                let n = &mut st.tree.objects[id];
                n.props.insert("animation".into(), gex_core::Variant::Text(anim));
                if n.kind == gex_core::ObjectKind::Mesh3D {
                    n.props.insert("anim_time".into(), gex_core::Variant::Num(0.0));
                }
                Ok(Value::Null)
            }
            "ANIMATIONS" => {
                let id = target(st, a.first())?;
                let mesh = st.tree.objects[id].text("mesh").to_string();
                let names = match gex_assets::model::cached(&st.vfs, &mesh) {
                    Ok(m) => m.animations.iter().map(|a| Value::text(&a.name)).collect(),
                    Err(_) => Vec::new(),
                };
                Ok(list_table(names))
            }
            "TILE" | "SET_TILE" => {
                let id = target(st, a.first())?;
                if st.tree.objects[id].kind != gex_core::ObjectKind::TileMap2D {
                    return err(format!("{} isn't a TileMap2D", st.tree.objects[id].name));
                }
                let (c, r) = (num_arg(&a, 1, 0.0).floor() as i32, num_arg(&a, 2, 0.0).floor() as i32);
                let mut grid = gex_core::tiles::parse(st.tree.objects[id].text("tiles"));
                if name == "TILE" {
                    return Ok(num(gex_core::tiles::get(&grid, c, r) as f64));
                }
                gex_core::tiles::set(&mut grid, c, r, num_arg(&a, 3, -1.0) as i32);
                st.tree.objects[id].props.insert("tiles".into(), gex_core::Variant::Text(gex_core::tiles::to_text(&grid)));
                Ok(Value::Null)
            }
            "TILE_AT" => {
                let id = target(st, a.first())?;
                let n = &st.tree.objects[id];
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
                if st.tree.objects[id].kind != gex_core::ObjectKind::TileMap2D {
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
                // Two keys: -1 while the first is held, 1 for the second, 0 for both or neither.
                if a.len() >= 2 {
                    let k2 = text_arg(name, &a, 1)?.to_ascii_lowercase();
                    return Ok(num(st.input.key_down(&k2) as i32 as f64 - st.input.key_down(&k) as i32 as f64));
                }
                Ok(num(st.axes.get(&k).copied().unwrap_or(0.0) as f64))
            }
            "KEY_DOWN" => Ok(Value::Bool(st.input.key_down(&text_arg(name, &a, 0)?.to_lowercase()))),
            "KEY_PRESSED" => Ok(Value::Bool(st.input.key_pressed(&text_arg(name, &a, 0)?.to_lowercase()))),
            "KEY_RELEASED" => Ok(Value::Bool(st.input.key_released(&text_arg(name, &a, 0)?.to_lowercase()))),
            "MOUSE_X" => Ok(num(st.input.mouse[0] as f64)),
            "MOUSE_Y" => Ok(num(st.input.mouse[1] as f64)),
            "DELTA_TIME" => Ok(num(st.dt)),
            "GAME_TIME" => Ok(num(st.time)),
            "CHANGE_BLUEPRINT" => {
                let blueprint = text_arg(name, &a, 0)?;
                let fade = num_arg(&a, 1, 0.0) as f32;
                if fade > 0.0 {
                    if st.fade.as_ref().is_none_or(|f| f.to_blueprint.is_none()) {
                        let color = match a.get(2) {
                            Some(v) if !matches!(v, Value::Null) => color_arg(Some(v)),
                            _ => [0.0, 0.0, 0.0, 1.0],
                        };
                        st.fade = Some(crate::world::Fade { to_blueprint: Some(blueprint), time: 0.0, length: fade / 2.0, color });
                    }
                } else {
                    st.change_blueprint = Some(blueprint);
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
            "PAUSE" => {
                st.paused = a.first().is_none_or(|v| !matches!(v, Value::Bool(false) | Value::Null));
                Ok(Value::Null)
            }
            "PAUSED" => Ok(Value::Bool(st.paused)),
            "SET_GRAPHICS" | "SET_PHYSICS" | "SET_DISPLAY" => {
                let key = text_arg(name, &a, 0)?;
                let v = a.get(1).cloned().unwrap_or(Value::Null);
                let what = &name[4..];
                let c = &mut st.project.config;
                let like = match what {
                    "GRAPHICS" => c.graphics.get(&key),
                    "DISPLAY" => c.display.get(&key),
                    _ => st.physics.settings.get(&key),
                };
                let Some(like) = like else { return err(format!("there's no {} setting called {}", what.to_lowercase(), key)) };
                let Some(var) = to_variant(&v, Some(&like)) else { return err(format!("{} can't take a {}", key, v.type_name())) };
                let r = match what {
                    "GRAPHICS" => c.graphics.set(&key, &var),
                    "DISPLAY" => c.display.set(&key, &var),
                    _ => st.physics.settings.set(&key, &var),
                };
                r.or_else(err)?;
                Ok(Value::Null)
            }
            "GRAPHICS" | "PHYSICS" | "DISPLAY" => {
                use gex_core::settings::{Display, Graphics, PhysicsSettings};
                let c = &st.project.config;
                let all: Vec<(&str, gex_core::Variant)> = match name {
                    "GRAPHICS" => Graphics::FIELDS.iter().filter_map(|f| c.graphics.get(f.name).map(|v| (f.name, v))).collect(),
                    "DISPLAY" => Display::FIELDS.iter().filter_map(|f| c.display.get(f.name).map(|v| (f.name, v))).collect(),
                    _ => PhysicsSettings::FIELDS.iter().filter_map(|f| st.physics.settings.get(f.name).map(|v| (f.name, v))).collect(),
                };
                match a.first() {
                    None | Some(Value::Null) => Ok(jlb::embed::named_table(all.iter().map(|(k, v)| (*k, to_value(v))).collect())),
                    Some(_) => {
                        let key = text_arg(name, &a, 0)?;
                        match all.iter().find(|(k, _)| *k == key) {
                            Some((_, v)) => Ok(to_value(v)),
                            None => err(format!("there's no setting called {}", key)),
                        }
                    }
                }
            }
            "SAVE_DISPLAY" => {
                let keep = a.first().is_none_or(|v| !matches!(v, Value::Bool(false) | Value::Null));
                // Only a built game keeps them: in the editor, Project settings stay in charge.
                if !crate::world::STANDALONE.load(std::sync::atomic::Ordering::Relaxed) {
                    st.log(crate::LogKind::Info, "SAVE_DISPLAY: display settings are only kept by the built game (Build or an export), not when playing in the editor");
                    return Ok(Value::Null);
                }
                let c = &st.project.config;
                crate::extras::save_display(&c.name, keep.then_some(&c.display)).or_else(err)?;
                Ok(Value::Null)
            }
            "HOST_GAME" => {
                let port = num_arg(&a, 0, crate::net::DEFAULT_PORT as f64);
                if !(1.0..=65535.0).contains(&port) {
                    return err("HOST_GAME's port is a number from 1 to 65535");
                }
                crate::net::host(st, port as u16).or_else(err)?;
                Ok(Value::Null)
            }
            "JOIN_GAME" => {
                let addr = match a.first() {
                    None | Some(Value::Null) => "localhost".to_string(),
                    _ => text_arg(name, &a, 0)?,
                };
                let (host, port) = crate::wire::split_address(&addr, num_arg(&a, 1, crate::net::DEFAULT_PORT as f64) as u16);
                crate::net::join(st, &host, port).or_else(err)?;
                Ok(Value::Null)
            }
            "LEAVE_GAME" => {
                for id in crate::net::leave(st) {
                    st.queue.push_back(Pending::Destroy(id));
                }
                Ok(Value::Null)
            }
            "ONLINE" => Ok(Value::Bool(st.net.as_ref().is_some_and(|n| !n.joining()))),
            "IS_HOST" => Ok(Value::Bool(st.net.as_ref().is_some_and(|n| n.host))),
            "MY_ID" => Ok(num(st.net.as_ref().map(|n| n.me).unwrap_or(0) as f64)),
            "PLAYERS" => {
                let mut p: Vec<u32> = st.net.as_ref().map(|n| n.players.clone()).unwrap_or_default();
                p.sort();
                Ok(list_table(p.into_iter().map(|i| num(i as f64)).collect()))
            }
            "SENDER" => Ok(st.net.as_ref().and_then(|n| n.sender).map(|i| num(i as f64)).unwrap_or(Value::Null)),
            "NET_EMIT" => {
                let ev = text_arg(name, &a, 0)?;
                crate::net::send_event(st, None, &ev, &a[1..]).or_else(err)?;
                Ok(Value::Null)
            }
            "NET_SEND" => {
                let to = match a.first() {
                    Some(v @ Value::Num(_)) => as_f64(v).unwrap_or(0.0) as u32,
                    _ => return err("NET_SEND needs a player number first (from PLAYERS, SENDER or on_player_joined)"),
                };
                let ev = text_arg(name, &a, 1)?;
                let me = st.net.as_ref().map(|n| n.me);
                if me == Some(to) {
                    st.queue.push_back(Pending::Event { target: None, handler: format!("on_{}", ev), args: a[2..].to_vec() });
                } else {
                    crate::net::send_event(st, Some(to), &ev, &a[2..]).or_else(err)?;
                }
                Ok(Value::Null)
            }
            "NET_SPAWN" => {
                let res = text_arg(name, &a, 0)?;
                let parent = match a.get(4) {
                    None | Some(Value::Null) => st.tree.root,
                    v => Some(target(st, v)?),
                };
                let project = st.project.clone();
                let id = blueprint::instance(&mut st.tree, parent, &res, &|p: &str| project.read_text(p)).map_err(|e| jlb::value::fault(e.0))?;
                let t = &mut st.tree.objects[id].transform;
                t.position.x = num_arg(&a, 1, t.position.x as f64) as f32;
                t.position.y = num_arg(&a, 2, t.position.y as f64) as f32;
                t.position.z = num_arg(&a, 3, t.position.z as f64) as f32;
                crate::net::spawned(st, id, &res);
                st.queue.push_back(Pending::Created(id));
                Ok(id_value(id))
            }
            "NET_SYNC" => {
                let id = target(st, a.first())?;
                crate::net::sync(st, id).or_else(err)?;
                Ok(Value::Null)
            }
            "OWNER" => {
                let id = target(st, a.first())?;
                let me = st.net.as_ref().map(|n| n.me).unwrap_or(0);
                Ok(num(crate::net::shared_root(st, id).and_then(|(_, net)| st.net.as_ref()?.owner_of(&net)).unwrap_or(me) as f64))
            }
            "IS_MINE" => {
                let id = target(st, a.first())?;
                let Some(n) = st.net.as_ref() else { return Ok(Value::Bool(true)) };
                let owner = crate::net::shared_root(st, id).and_then(|(_, net)| n.owner_of(&net));
                Ok(Value::Bool(owner.is_none_or(|o| o == n.me)))
            }
            "SET_OWNER" => {
                let id = target(st, a.first())?;
                let p = match a.get(1) {
                    Some(v @ Value::Num(_)) => as_f64(v).unwrap_or(0.0) as u32,
                    _ => return err("SET_OWNER needs a player number"),
                };
                if st.net.is_none() {
                    return Ok(Value::Null);
                }
                crate::net::set_owner(st, id, p).or_else(err)?;
                Ok(Value::Null)
            }
            "NET_ID" => {
                let id = target(st, a.first())?;
                Ok(crate::net::shared_root(st, id).filter(|(r, _)| *r == id).map(|(_, n)| Value::text(&n)).unwrap_or(Value::Null))
            }
            "NET_OBJECT" => {
                let n = text_arg(name, &a, 0)?;
                Ok(crate::net::net_object(st, &n).map(id_value).unwrap_or(Value::Null))
            }
            "MY_ADDRESS" => Ok(crate::wire::local_ip().map(|s| Value::text(&s)).unwrap_or(Value::Null)),
            "SET_ACTION" => {
                let action = text_arg(name, &a, 0)?.trim().to_lowercase().replace(' ', "_");
                let keys: Vec<String> = match a.get(1) {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Text(t)) => t.split([',', ' ']).map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect(),
                    Some(Value::Table(rc, _)) => rc.items.iter().filter_map(|k| match &*k.borrow() {
                        Value::Text(t) => Some(t.trim().to_lowercase()),
                        _ => None,
                    }).filter(|k| !k.is_empty()).collect(),
                    Some(o) => return err(format!("SET_ACTION needs keys as a list or a text, not a {}", o.type_name())),
                };
                if keys.is_empty() {
                    st.input.actions.remove(&action);
                } else {
                    st.input.actions.insert(action, keys);
                }
                Ok(Value::Null)
            }
            "ACTION" => {
                let keys = |ks: &Vec<String>| jlb::embed::list_table(ks.iter().map(|k| Value::Text(k.as_str().into())).collect());
                match a.first() {
                    None | Some(Value::Null) => Ok(jlb::embed::named_table(st.input.actions.iter().map(|(k, v)| (k.as_str(), keys(v))).collect())),
                    Some(_) => {
                        let action = text_arg(name, &a, 0)?.to_lowercase();
                        Ok(st.input.actions.get(&action).map(keys).unwrap_or(Value::Null))
                    }
                }
            }
            "TIME_SCALE" => {
                if let Some(v) = a.first().and_then(as_f64) {
                    st.time_scale = v.clamp(0.0, 100.0);
                }
                Ok(num(st.time_scale))
            }
            "FULLSCREEN" => {
                if let Some(v) = a.first() {
                    st.project.config.display.fullscreen = !matches!(v, Value::Bool(false) | Value::Null);
                }
                Ok(Value::Bool(st.project.config.display.fullscreen))
            }
            "WINDOW_TITLE" => {
                st.window_title = Some(text_arg(name, &a, 0)?);
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
