//! Grid pathfinding over tilemaps, and save data that lasts between runs.

use std::collections::{BinaryHeap, HashMap};
use std::path::PathBuf;

use gex_core::{ObjectId, BlueprintTree};
use jlb::embed::{as_f64, list_table, named_table, num};
use jlb::Value;

/// A* over a TileMap2D's grid: solid tiles block, everything else (empty cells too) is
/// walkable. Gives world-space cell centers from start to goal, or None. Diagonal steps
/// are allowed when they don't cut a corner. The search stays within the map's bounds
/// plus a margin of a few cells.
pub fn find_path(tree: &BlueprintTree, map: ObjectId, from: glam::Vec2, to: glam::Vec2) -> Option<Vec<glam::Vec2>> {
    let n = &tree.objects[map];
    let grid = gex_core::tiles::parse(n.text("tiles"));
    let only = gex_core::tiles::solid_set(n.text("solid_tiles"));
    let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
    let m = tree.global_matrix(map);
    let inv = m.inverse();
    let cell = |p: glam::Vec2| {
        let l = inv.transform_point3(p.extend(0.0));
        ((l.x / tw).floor() as i32, (l.y / th).floor() as i32)
    };
    let rows = grid.len() as i32;
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0) as i32;
    let margin = 4;
    let blocked = |c: i32, r: i32| {
        if c < -margin || r < -margin || c >= cols + margin || r >= rows + margin {
            return true;
        }
        let t = gex_core::tiles::get(&grid, c, r);
        t >= 0 && only.as_ref().is_none_or(|o| o.contains(&t))
    };
    let (start, goal) = (cell(from), cell(to));
    if blocked(goal.0, goal.1) {
        return None;
    }
    let h = |a: (i32, i32)| {
        let (dx, dy) = ((a.0 - goal.0).abs(), (a.1 - goal.1).abs());
        (dx.max(dy) * 10 + dx.min(dy) * 4) as i64
    };
    let mut open = BinaryHeap::new();
    let mut came: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut cost: HashMap<(i32, i32), i64> = HashMap::new();
    cost.insert(start, 0);
    open.push(std::cmp::Reverse((h(start), start)));
    let mut steps = 0;
    while let Some(std::cmp::Reverse((_, cur))) = open.pop() {
        steps += 1;
        if cur == goal || steps > 200_000 {
            break;
        }
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let nb = (cur.0 + dx, cur.1 + dy);
            if blocked(nb.0, nb.1) || (dx != 0 && dy != 0 && (blocked(cur.0 + dx, cur.1) || blocked(cur.0, cur.1 + dy))) {
                continue;
            }
            let c = cost[&cur] + if dx != 0 && dy != 0 { 14 } else { 10 };
            if cost.get(&nb).is_none_or(|old| c < *old) {
                cost.insert(nb, c);
                came.insert(nb, cur);
                open.push(std::cmp::Reverse((c + h(nb), nb)));
            }
        }
    }
    if !came.contains_key(&goal) && start != goal {
        return None;
    }
    let mut path = vec![goal];
    let mut cur = goal;
    while cur != start {
        cur = came[&cur];
        path.push(cur);
    }
    path.reverse();
    Some(path.into_iter().map(|(c, r)| m.transform_point3(glam::Vec3::new((c as f32 + 0.5) * tw, (r as f32 + 0.5) * th, 0.0)).truncate()).collect())
}

pub fn path_value(points: Option<Vec<glam::Vec2>>) -> Value {
    match points {
        None => Value::Null,
        Some(p) => list_table(p.into_iter().map(|v| named_table(vec![("x", num(v.x as f64)), ("y", num(v.y as f64))])).collect()),
    }
}

// ---------------------------------------------------------------- save data

/// Where a game's saves live: <user data>/GEX/saves/<game name>/.
pub fn save_dir(game: &str) -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."));
    let safe: String = game.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' }).collect();
    base.join("GEX").join("saves").join(if safe.trim().is_empty() { "game".to_string() } else { safe })
}

fn slot_path(game: &str, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Err(format!("save names are letters, digits, - and _ (not {:?})", name));
    }
    Ok(save_dir(game).join(format!("{}.json", name)))
}

pub(crate) fn to_json(v: &Value, depth: usize) -> Result<serde_json::Value, String> {
    if depth > 64 {
        return Err("that table is nested too deeply to save".into());
    }
    Ok(match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Num(_) => serde_json::Number::from_f64(as_f64(v).unwrap_or(0.0)).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
        Value::Text(t) => serde_json::Value::String(t.to_string()),
        Value::Table(rc, _) => {
            let mut items = Vec::new();
            for s in &rc.items {
                items.push(to_json(&s.borrow(), depth + 1)?);
            }
            let mut named = serde_json::Map::new();
            for (k, s) in rc.named.iter() {
                named.insert(k.to_string(), to_json(&s.borrow(), depth + 1)?);
            }
            serde_json::json!({ "items": items, "named": named })
        }
        o => return Err(format!("a {} can't be saved", o.type_name())),
    })
}

pub(crate) fn from_json(j: &serde_json::Value) -> Value {
    match j {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => num(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Value::text(s),
        serde_json::Value::Array(a) => list_table(a.iter().map(from_json).collect()),
        serde_json::Value::Object(o) => {
            let mut t = jlb::value::Table::new();
            if let Some(serde_json::Value::Array(items)) = o.get("items") {
                for i in items {
                    t.items.push(jlb::value::new_slot(from_json(i)));
                }
            }
            if let Some(serde_json::Value::Object(named)) = o.get("named") {
                for (k, v) in named {
                    t.named.insert(std::rc::Rc::from(k.as_str()), jlb::value::new_slot(from_json(v)));
                }
            }
            Value::new_table(t)
        }
    }
}

/// Saves written so far but maybe not on disk yet, by file: the newest text of each
/// (None: deleted). Reading a save checks here first, so a load right after a save sees it.
static PENDING: std::sync::Mutex<Option<std::collections::HashMap<PathBuf, Option<String>>>> = std::sync::Mutex::new(None);

/// The thread that writes saves, so a game never waits on the disk (or a virus scanner
/// looking at each new file) in the middle of a frame.
fn writer() -> &'static std::sync::mpsc::Sender<PathBuf> {
    static TX: std::sync::OnceLock<std::sync::mpsc::Sender<PathBuf>> = std::sync::OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
        let _ = std::thread::Builder::new().name("saves".into()).spawn(move || {
            while let Ok(p) = rx.recv() {
                // The newest text for this file (later saves of it replace earlier ones).
                let text = PENDING.lock().unwrap().as_ref().and_then(|m| m.get(&p).cloned());
                let Some(text) = text else { continue };
                match &text {
                    Some(t) => {
                        if let Some(dir) = p.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        // Written beside it, then swapped in: a crash mid-write can't damage a save.
                        let tmp = p.with_extension("json.tmp");
                        if std::fs::write(&tmp, t).is_ok() && std::fs::rename(&tmp, &p).is_err() {
                            let _ = std::fs::write(&p, t);
                        }
                    }
                    None => {
                        let _ = std::fs::remove_file(&p);
                    }
                }
                let mut g = PENDING.lock().unwrap();
                if let Some(m) = g.as_mut() {
                    if m.get(&p) == Some(&text) {
                        m.remove(&p);
                    }
                }
            }
        });
        tx
    })
}

fn queue_write(p: PathBuf, text: Option<String>) {
    PENDING.lock().unwrap().get_or_insert_with(Default::default).insert(p.clone(), text);
    let _ = writer().send(p);
}

/// Waits (up to a few seconds) until every save is on disk: when a game stops.
pub fn flush_saves() {
    for _ in 0..300 {
        if PENDING.lock().unwrap().as_ref().is_none_or(|m| m.is_empty()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub fn save(game: &str, name: &str, v: &Value) -> Result<(), String> {
    let p = slot_path(game, name)?;
    let j = to_json(v, 0)?;
    queue_write(p, Some(serde_json::to_string_pretty(&j).unwrap()));
    Ok(())
}

pub fn load(game: &str, name: &str) -> Result<Value, String> {
    let p = slot_path(game, name)?;
    let pending = PENDING.lock().unwrap().as_ref().and_then(|m| m.get(&p).cloned());
    let text = match pending {
        Some(Some(t)) => Ok(t),
        Some(None) => return Ok(Value::Null),
        None => std::fs::read_to_string(&p),
    };
    match text {
        Ok(t) => serde_json::from_str(&t).map(|j| from_json(&j)).map_err(|e| format!("save {} is damaged: {}", name, e)),
        Err(_) => Ok(Value::Null),
    }
}

pub fn delete(game: &str, name: &str) -> Result<(), String> {
    let p = slot_path(game, name)?;
    queue_write(p, None);
    Ok(())
}

/// The display settings a game kept with SAVE_DISPLAY (a name no save slot can have).
fn display_path(game: &str) -> PathBuf {
    save_dir(game).join("display.settings.json")
}

/// Keeps display settings for the next time the game starts (None forgets them).
pub fn save_display(game: &str, d: Option<&gex_core::settings::Display>) -> Result<(), String> {
    let p = display_path(game);
    match d {
        None => {
            let _ = std::fs::remove_file(p);
            Ok(())
        }
        Some(d) => {
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&p, serde_json::to_string_pretty(d).unwrap()).map_err(|e| format!("couldn't keep the display settings: {}", e))
        }
    }
}

/// The display settings kept with SAVE_DISPLAY, if any.
pub fn saved_display(game: &str) -> Option<gex_core::settings::Display> {
    serde_json::from_str(&std::fs::read_to_string(display_path(game)).ok()?).ok()
}
