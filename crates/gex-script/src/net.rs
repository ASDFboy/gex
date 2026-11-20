//! Multiplayer games. One player hosts (HOST_GAME) and the others join (JOIN_GAME); everything
//! goes through the host, which passes each message on. Shared objects (NET_SPAWN, NET_SYNC)
//! belong to one player's game, which sends their position, rotation, size, visibility and
//! properties 20 times a second when they change; the other games glide their copies there.
//! Events (NET_EMIT, NET_SEND) run scripts in other players' games.

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::Receiver;

use gex_core::{blueprint, ObjectId, ObjectKind, Variant};
use jlb::Value;
use serde::{Deserialize, Serialize};

use crate::wire::{Conn, Frame};
use crate::world::WorldState;

pub const DEFAULT_PORT: u16 = 7777;
const PROTOCOL: u32 = 1;
const SEND_HZ: f64 = 20.0;
/// Objects inside a shared object that are kept in step (the rest are left alone).
const MAX_PARTS: usize = 64;
/// Properties each game keeps for itself.
const LOCAL_PROPS: &[&str] = &["body", "playing", "current"];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t")]
enum Msg {
    Hello { v: u32, game: String },
    Refused { reason: String },
    Welcome { id: u32, players: Vec<u32>, objects: Vec<NetObj> },
    Joined { id: u32 },
    Left { id: u32 },
    Event { from: u32, to: Option<u32>, name: String, args: Vec<serde_json::Value> },
    Spawn { obj: NetObj },
    State { net: String, parts: Vec<Part> },
    Destroy { net: String },
    Owner { net: String, owner: u32 },
}

/// A shared object, as every game knows it.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct NetObj {
    net: String,
    /// The blueprint file it lives in (it only appears for players in that blueprint).
    level: String,
    /// The blueprint file it's an instance of; None for an object placed in the level.
    res: Option<String>,
    /// Its parent's path ("" for the top of the blueprint).
    parent: String,
    owner: u32,
    /// A placed object that was destroyed.
    #[serde(default)]
    gone: bool,
    /// Its latest state.
    parts: Vec<Part>,
}

/// The state of one object in a shared object.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct Part {
    /// Path from the shared object ("" for itself).
    p: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tf: Option<[f32; 9]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vis: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    props: Vec<(String, Variant)>,
}

fn merge(into: &mut Vec<Part>, new: &[Part]) {
    for n in new {
        match into.iter_mut().find(|p| p.p == n.p) {
            Some(p) => {
                if n.tf.is_some() {
                    p.tf = n.tf;
                }
                if n.vis.is_some() {
                    p.vis = n.vis;
                }
                for (k, v) in &n.props {
                    match p.props.iter_mut().find(|(pk, _)| pk == k) {
                        Some(e) => e.1 = v.clone(),
                        None => p.props.push((k.clone(), v.clone())),
                    }
                }
            }
            None => into.push(n.clone()),
        }
    }
}

/// What the game does after the network is read: run events, start or remove objects.
pub(crate) enum Action {
    Event { handler: String, args: Vec<Value>, from: u32 },
    Created(ObjectId),
    Destroy(ObjectId),
}

pub struct Net {
    /// This player's number: 1 hosts, 2, 3... joined; 0 while still joining.
    pub me: u32,
    pub host: bool,
    listener: Option<TcpListener>,
    /// The host's players (0 until they say hello), or, when joined, just the host.
    peers: Vec<(u32, Conn)>,
    joining: Option<Receiver<Result<TcpStream, String>>>,
    next_id: u32,
    pub players: Vec<u32>,
    /// Every shared object, in every blueprint.
    objects: HashMap<String, NetObj>,
    /// Shared objects in this game's blueprint, both ways.
    local: HashMap<String, ObjectId>,
    by_object: HashMap<ObjectId, String>,
    /// Where other players' objects are heading (position, rotation, scale).
    targets: HashMap<ObjectId, [f32; 9]>,
    /// What was last sent for this game's objects.
    sent: HashMap<String, Vec<Part>>,
    /// Properties changed on another player's copy (bodies follow, cameras stay off), to put back
    /// if it becomes ours.
    orig: HashMap<ObjectId, Vec<(String, Option<Variant>)>>,
    acc: f64,
    counter: u32,
    /// Who sent the event running now.
    pub sender: Option<u32>,
    game: String,
    level_loaded: bool,
}

impl Net {
    fn new(game: &str) -> Net {
        Net {
            me: 0,
            host: false,
            listener: None,
            peers: Vec::new(),
            joining: None,
            next_id: 2,
            players: Vec::new(),
            objects: HashMap::new(),
            local: HashMap::new(),
            by_object: HashMap::new(),
            targets: HashMap::new(),
            sent: HashMap::new(),
            orig: HashMap::new(),
            acc: 0.0,
            counter: 0,
            sender: None,
            game: game.to_string(),
            level_loaded: true,
        }
    }

    pub fn joining(&self) -> bool {
        self.me == 0
    }

    /// Sends to everyone else (a joined game sends to the host, which passes it on).
    fn send_all(&mut self, m: &Msg) {
        let Ok(b) = serde_json::to_vec(m) else { return };
        for (id, c) in &mut self.peers {
            if *id != 0 {
                c.send_json(&b);
            }
        }
    }

    fn send_except(&mut self, except: u32, b: &[u8]) {
        for (id, c) in &mut self.peers {
            if *id != 0 && *id != except {
                c.send_json(b);
            }
        }
    }

    fn send_to(&mut self, to: u32, m: &Msg) {
        if let Some((_, c)) = self.peers.iter_mut().find(|(id, _)| *id == to || (!self.host && *id == 1)) {
            c.send(m);
        }
    }

    fn flush(&mut self) {
        for (_, c) in &mut self.peers {
            c.flush();
        }
    }

    pub fn owner_of(&self, net: &str) -> Option<u32> {
        self.objects.get(net).map(|o| o.owner)
    }

    /// Moving to another blueprint: this game's objects in the old one go for everyone.
    pub fn change_level(&mut self, old: &str) {
        let mine: Vec<String> = self.objects.values().filter(|o| o.level == old && o.owner == self.me && o.res.is_some()).map(|o| o.net.clone()).collect();
        for n in mine {
            self.objects.remove(&n);
            self.send_all(&Msg::Destroy { net: n });
        }
        self.objects.retain(|_, o| !(o.level == old && o.res.is_none()));
        self.flush();
        self.local.clear();
        self.by_object.clear();
        self.targets.clear();
        self.sent.clear();
        self.orig.clear();
        self.level_loaded = false;
    }
}

fn game_name(st: &WorldState) -> String {
    st.project.config.name.clone()
}

pub fn host(st: &mut WorldState, port: u16) -> Result<(), String> {
    if st.net.is_some() {
        return Err("already in a multiplayer game (LEAVE_GAME first)".into());
    }
    let l = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("can't host on port {}: {}", port, e))?;
    l.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut n = Net::new(&game_name(st));
    n.me = 1;
    n.host = true;
    n.players = vec![1];
    n.listener = Some(l);
    st.net = Some(n);
    Ok(())
}

pub fn join(st: &mut WorldState, address: &str, port: u16) -> Result<(), String> {
    if st.net.is_some() {
        return Err("already in a multiplayer game (LEAVE_GAME first)".into());
    }
    let mut n = Net::new(&game_name(st));
    n.joining = Some(crate::wire::connect(address, port));
    st.net = Some(n);
    Ok(())
}

/// Leaving: other players' objects go from this game; this game's own stay.
pub fn leave(st: &mut WorldState) -> Vec<ObjectId> {
    let Some(mut n) = st.net.take() else { return Vec::new() };
    n.flush();
    let mut gone = Vec::new();
    for (net, id) in &n.local {
        if n.objects.get(net).is_some_and(|o| o.owner != n.me && o.res.is_some()) {
            gone.push(*id);
        }
    }
    for id in n.orig.keys().copied().collect::<Vec<_>>() {
        restore(&mut n, st, id);
    }
    gone
}

fn tf_of(st: &WorldState, id: ObjectId) -> [f32; 9] {
    let t = &st.tree.objects[id].transform;
    [t.position.x, t.position.y, t.position.z, t.rotation.x, t.rotation.y, t.rotation.z, t.scale.x, t.scale.y, t.scale.z]
}

fn set_tf(st: &mut WorldState, id: ObjectId, tf: &[f32; 9]) {
    let t = &mut st.tree.objects[id].transform;
    t.position = glam::Vec3::new(tf[0], tf[1], tf[2]);
    t.rotation = glam::Vec3::new(tf[3], tf[4], tf[5]);
    t.scale = glam::Vec3::new(tf[6], tf[7], tf[8]);
}

/// The objects kept in step inside a shared object, with their paths from it.
fn parts_of(st: &WorldState, root: ObjectId) -> Vec<(String, ObjectId)> {
    let mut ids = Vec::new();
    st.tree.walk_from(root, &mut ids);
    ids.truncate(MAX_PARTS);
    let base = st.tree.path_of(root);
    ids.into_iter()
        .map(|i| {
            let full = st.tree.path_of(i);
            let rel = full.strip_prefix(base.as_str()).unwrap_or("").trim_start_matches('/').to_string();
            (rel, i)
        })
        .collect()
}

fn find_part(st: &WorldState, root: ObjectId, p: &str) -> Option<ObjectId> {
    if p.is_empty() {
        Some(root)
    } else {
        st.tree.find_path(Some(root), p)
    }
}

fn snapshot(st: &WorldState, root: ObjectId) -> Vec<Part> {
    parts_of(st, root)
        .into_iter()
        .map(|(p, i)| {
            let n = &st.tree.objects[i];
            let mut props: Vec<(String, Variant)> = n.props.iter().filter(|(k, _)| !LOCAL_PROPS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
            props.sort_by(|a, b| a.0.cmp(&b.0));
            Part { p, tf: Some(tf_of(st, i)), vis: Some(n.visible), props }
        })
        .collect()
}

fn close(a: &[f32; 9], b: &[f32; 9]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-4 * (1.0 + x.abs()))
}

/// What changed since `old`.
fn diff(old: &[Part], new: &[Part]) -> Vec<Part> {
    let mut out = Vec::new();
    for n in new {
        let o = old.iter().find(|o| o.p == n.p);
        let mut d = Part { p: n.p.clone(), ..Default::default() };
        if let Some(tf) = n.tf {
            if o.and_then(|o| o.tf).is_none_or(|ot| !close(&ot, &tf)) {
                d.tf = Some(tf);
            }
        }
        if n.vis.is_some() && o.and_then(|o| o.vis) != n.vis {
            d.vis = n.vis;
        }
        for (k, v) in &n.props {
            if o.and_then(|o| o.props.iter().find(|(ok, _)| ok == k)).is_none_or(|(_, ov)| ov != v) {
                d.props.push((k.clone(), v.clone()));
            }
        }
        if d.tf.is_some() || d.vis.is_some() || !d.props.is_empty() {
            out.push(d);
        }
    }
    out
}

/// Another player's copy: bodies follow instead of falling, cameras don't take over the view.
fn make_remote(n: &mut Net, st: &mut WorldState, root: ObjectId) {
    let mut ids = Vec::new();
    st.tree.walk_from(root, &mut ids);
    for i in ids {
        if n.orig.contains_key(&i) {
            continue;
        }
        let o = &mut st.tree.objects[i];
        let mut saved = Vec::new();
        match o.kind {
            ObjectKind::Body2D | ObjectKind::Body3D if !matches!(o.text("body"), "static" | "kinematic") => {
                saved.push(("body".to_string(), o.props.get("body").cloned()));
                o.props.insert("body".into(), Variant::Text("kinematic".into()));
            }
            ObjectKind::Camera2D | ObjectKind::Camera3D => {
                saved.push(("current".to_string(), o.props.get("current").cloned()));
                o.props.insert("current".into(), Variant::Bool(false));
            }
            _ => {}
        }
        if !saved.is_empty() {
            n.orig.insert(i, saved);
        }
    }
}

fn restore(n: &mut Net, st: &mut WorldState, id: ObjectId) {
    let mut ids = Vec::new();
    if st.tree.contains(id) {
        st.tree.walk_from(id, &mut ids);
    }
    for i in ids {
        n.targets.remove(&i);
        if let Some(saved) = n.orig.remove(&i) {
            let o = &mut st.tree.objects[i];
            for (k, v) in saved {
                match v {
                    Some(v) => o.props.insert(k, v),
                    None => o.props.remove(&k),
                };
            }
        }
    }
}

fn apply_parts(n: &mut Net, st: &mut WorldState, root: ObjectId, parts: &[Part], exact: bool) {
    for p in parts {
        let Some(i) = find_part(st, root, &p.p) else { continue };
        if let Some(tf) = &p.tf {
            if exact {
                set_tf(st, i, tf);
            }
            n.targets.insert(i, *tf);
        }
        let o = &mut st.tree.objects[i];
        if let Some(v) = p.vis {
            o.visible = v;
        }
        for (k, v) in &p.props {
            if !LOCAL_PROPS.contains(&k.as_str()) {
                o.props.insert(k.clone(), v.clone());
            }
        }
    }
}

fn link(n: &mut Net, net: &str, id: ObjectId) {
    n.local.insert(net.to_string(), id);
    n.by_object.insert(id, net.to_string());
}

/// Puts a shared object into this game.
fn instantiate(n: &mut Net, st: &mut WorldState, obj: &NetObj, actions: &mut Vec<Action>) {
    let Some(res) = &obj.res else { return };
    if n.local.contains_key(&obj.net) {
        return;
    }
    // The parent: another shared object ("@" and its name), or a path in the blueprint.
    let parent = match obj.parent.strip_prefix('@') {
        Some(net) => n.local.get(net).copied().filter(|i| st.tree.contains(*i)).or(st.tree.root),
        None if obj.parent.is_empty() => st.tree.root,
        None => st.tree.find_path(None, &obj.parent).or(st.tree.root),
    };
    let project = st.project.clone();
    match blueprint::instance(&mut st.tree, parent, res, &|p: &str| project.read_text(p)) {
        Ok(id) => {
            link(n, &obj.net, id);
            apply_parts(n, st, id, &obj.parts, true);
            if obj.owner != n.me {
                make_remote(n, st, id);
            }
            actions.push(Action::Created(id));
        }
        Err(e) => st.log(crate::LogKind::Error, format!("another player's {}: {}", res, e.0)),
    }
}

/// An object placed in the blueprint, by its shared name ("level#path").
fn placed(st: &WorldState, net: &str) -> Option<ObjectId> {
    let (level, path) = net.split_once('#')?;
    (level == st.blueprint).then(|| st.tree.find_path(None, path)).flatten()
}

fn apply(n: &mut Net, st: &mut WorldState, m: Msg, actions: &mut Vec<Action>) {
    match m {
        Msg::Event { from, name, args, .. } => {
            let args = args.iter().map(crate::extras::from_json).collect();
            actions.push(Action::Event { handler: format!("on_{}", name), args, from });
        }
        Msg::Spawn { obj } => {
            if obj.level == st.blueprint {
                instantiate(n, st, &obj, actions);
            }
            n.objects.insert(obj.net.clone(), obj);
        }
        Msg::State { net, parts } => {
            let owner = match n.objects.get_mut(&net) {
                Some(o) if o.gone => return,
                Some(o) => {
                    merge(&mut o.parts, &parts);
                    o.owner
                }
                None => {
                    // A placed object another game shares that this one hasn't yet.
                    let Some((level, path)) = net.split_once('#') else { return };
                    let obj = NetObj { net: net.clone(), level: level.to_string(), res: None, parent: path.to_string(), owner: 1, gone: false, parts: parts.clone() };
                    n.objects.insert(net.clone(), obj);
                    1
                }
            };
            if owner == n.me {
                return;
            }
            let id = match n.local.get(&net) {
                Some(i) => Some(*i),
                None => placed(st, &net).inspect(|i| {
                    link(n, &net, *i);
                    make_remote(n, st, *i);
                }),
            };
            if let Some(id) = id.filter(|i| st.tree.contains(*i)) {
                apply_parts(n, st, id, &parts, false);
            }
        }
        Msg::Destroy { net } => {
            match n.objects.get_mut(&net) {
                Some(o) if o.res.is_none() => o.gone = true,
                Some(_) => {
                    n.objects.remove(&net);
                }
                None => {
                    if let Some((level, path)) = net.split_once('#') {
                        n.objects.insert(net.clone(), NetObj { net: net.clone(), level: level.to_string(), res: None, parent: path.to_string(), owner: 1, gone: true, parts: Vec::new() });
                    }
                }
            }
            let net = net;
            if n.local.get(&net).is_none() {
                if let Some(i) = placed(st, &net) {
                    actions.push(Action::Destroy(i));
                    return;
                }
            }
            n.sent.remove(&net);
            if let Some(id) = n.local.remove(&net) {
                n.by_object.remove(&id);
                if st.tree.contains(id) {
                    actions.push(Action::Destroy(id));
                }
            }
        }
        Msg::Owner { net, owner } => {
            match n.objects.get_mut(&net) {
                Some(o) => o.owner = owner,
                None => {
                    let Some((level, path)) = net.split_once('#') else { return };
                    n.objects.insert(net.clone(), NetObj { net: net.clone(), level: level.to_string(), res: None, parent: path.to_string(), owner, gone: false, parts: Vec::new() });
                }
            }
            if let Some(id) = n.local.get(&net).copied().filter(|i| st.tree.contains(*i)) {
                if owner == n.me {
                    restore(n, st, id);
                    n.sent.insert(net, snapshot(st, id));
                } else {
                    n.sent.remove(&net);
                    make_remote(n, st, id);
                }
            }
        }
        Msg::Joined { id } => {
            if !n.players.contains(&id) {
                n.players.push(id);
            }
            actions.push(Action::Event { handler: "on_player_joined".into(), args: vec![jlb::embed::num(id as f64)], from: 0 });
        }
        Msg::Left { id } => player_left(n, st, id, actions),
        Msg::Hello { .. } | Msg::Refused { .. } | Msg::Welcome { .. } => {}
    }
}

/// A player is gone: so are the objects their game made.
fn player_left(n: &mut Net, st: &mut WorldState, id: u32, actions: &mut Vec<Action>) {
    n.players.retain(|p| *p != id);
    let theirs: Vec<String> = n.objects.values().filter(|o| o.owner == id).map(|o| o.net.clone()).collect();
    for net in theirs {
        let spawned = n.objects.get(&net).is_some_and(|o| o.res.is_some());
        if spawned {
            apply(n, st, Msg::Destroy { net }, actions);
        } else if n.host {
            // Placed objects go back to the host.
            apply(n, st, Msg::Owner { net: net.clone(), owner: 1 }, actions);
            n.send_all(&Msg::Owner { net, owner: 1 });
        }
    }
    actions.push(Action::Event { handler: "on_player_left".into(), args: vec![jlb::embed::num(id as f64)], from: 0 });
}

fn event(handler: &str, args: Vec<Value>) -> Action {
    Action::Event { handler: handler.into(), args, from: 0 }
}

/// Reads the network: once a frame, before the scripts run.
pub(crate) fn poll(st: &mut WorldState, dt: f64) -> Vec<Action> {
    let Some(mut n) = st.net.take() else { return Vec::new() };
    let mut actions = Vec::new();
    if let Some(rx) = &n.joining {
        match rx.try_recv() {
            Ok(Ok(stream)) => {
                n.joining = None;
                match Conn::new(stream) {
                    Ok(mut c) => {
                        c.send(&Msg::Hello { v: PROTOCOL, game: n.game.clone() });
                        n.peers.push((1, c));
                    }
                    Err(e) => {
                        actions.push(event("on_join_failed", vec![Value::text(&e.to_string())]));
                        return actions;
                    }
                }
            }
            Ok(Err(e)) => {
                actions.push(event("on_join_failed", vec![Value::text(&e)]));
                return actions;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => {
                actions.push(event("on_join_failed", vec![Value::text("couldn't connect")]));
                return actions;
            }
        }
    }
    if let Some(l) = &n.listener {
        for _ in 0..16 {
            match l.accept() {
                Ok((s, _)) => {
                    if let Ok(c) = Conn::new(s) {
                        n.peers.push((0, c));
                    }
                }
                Err(_) => break,
            }
        }
    }
    let mut inbox: Vec<(usize, Msg, Vec<u8>)> = Vec::new();
    for (i, (_, c)) in n.peers.iter_mut().enumerate() {
        for f in c.recv() {
            if let Frame::Json(b) = f {
                if let Ok(m) = serde_json::from_slice::<Msg>(&b) {
                    inbox.push((i, m, b));
                }
            }
        }
    }
    let mut gone_host: Option<String> = None;
    for (i, m, raw) in inbox {
        let pid = n.peers[i].0;
        if n.host {
            match m {
                Msg::Hello { v, game } => {
                    if pid != 0 {
                        continue;
                    }
                    let c = &mut n.peers[i].1;
                    if v != PROTOCOL {
                        c.send(&Msg::Refused { reason: "the host is running a different version of GEX".into() });
                        c.flush();
                        c.closed = true;
                        continue;
                    }
                    if game != n.game {
                        c.send(&Msg::Refused { reason: format!("the host is playing {}, not {}", n.game, game) });
                        c.flush();
                        c.closed = true;
                        continue;
                    }
                    let id = n.next_id;
                    n.next_id += 1;
                    n.players.push(id);
                    let objects = n.objects.values().cloned().collect();
                    let welcome = Msg::Welcome { id, players: n.players.clone(), objects };
                    n.peers[i].0 = id;
                    n.peers[i].1.send(&welcome);
                    if let Ok(b) = serde_json::to_vec(&Msg::Joined { id }) {
                        n.send_except(id, &b);
                    }
                    actions.push(event("on_player_joined", vec![jlb::embed::num(id as f64)]));
                }
                _ if pid == 0 => {}
                Msg::Event { to, name, args, .. } => {
                    let m = Msg::Event { from: pid, to, name, args };
                    match to {
                        None => {
                            if let Ok(b) = serde_json::to_vec(&m) {
                                n.send_except(pid, &b);
                            }
                            apply(&mut n, st, m, &mut actions);
                        }
                        Some(t) if t == n.me => apply(&mut n, st, m, &mut actions),
                        Some(t) => n.send_to(t, &m),
                    }
                }
                m @ (Msg::Spawn { .. } | Msg::State { .. } | Msg::Destroy { .. } | Msg::Owner { .. }) => {
                    n.send_except(pid, &raw);
                    apply(&mut n, st, m, &mut actions);
                }
                _ => {}
            }
        } else {
            match m {
                Msg::Refused { reason } => {
                    actions.push(event("on_join_failed", vec![Value::text(&reason)]));
                    return actions;
                }
                Msg::Welcome { id, players, objects } => {
                    n.me = id;
                    n.players = players;
                    n.objects = objects.into_iter().map(|o| (o.net.clone(), o)).collect();
                    n.level_loaded = false;
                    actions.push(event("on_connected", vec![jlb::embed::num(id as f64)]));
                }
                m => apply(&mut n, st, m, &mut actions),
            }
        }
    }
    // Players who left.
    let mut k = 0;
    while k < n.peers.len() {
        if n.peers[k].1.closed {
            let (pid, _) = n.peers.remove(k);
            if n.host {
                if pid != 0 {
                    if let Ok(b) = serde_json::to_vec(&Msg::Left { id: pid }) {
                        n.send_except(pid, &b);
                    }
                    player_left(&mut n, st, pid, &mut actions);
                }
            } else {
                gone_host = Some(if n.me == 0 { "the host closed the connection".into() } else { "the host left the game".into() });
            }
        } else {
            k += 1;
        }
    }
    if let Some(reason) = gone_host {
        st.net = Some(n);
        for id in leave(st) {
            actions.push(Action::Destroy(id));
        }
        actions.push(event("on_disconnected", vec![Value::text(&reason)]));
        return actions;
    }
    // Shared objects already in this blueprint (on joining, or after changing blueprint).
    if !n.level_loaded && n.me != 0 {
        n.level_loaded = true;
        let here: Vec<NetObj> = n.objects.values().filter(|o| o.level == st.blueprint).cloned().collect();
        for o in here {
            if o.res.is_some() {
                instantiate(&mut n, st, &o, &mut actions);
            } else if let Some(id) = placed(st, &o.net) {
                if o.gone {
                    actions.push(Action::Destroy(id));
                    continue;
                }
                link(&mut n, &o.net, id);
                if o.owner != n.me {
                    make_remote(&mut n, st, id);
                    apply_parts(&mut n, st, id, &o.parts, true);
                }
            }
        }
    }
    // Other players' objects glide to where they were last seen.
    let k = 1.0 - (-dt as f32 * 15.0).exp();
    n.targets.retain(|id, _| st.tree.contains(*id));
    for (id, to) in &n.targets {
        let t = &mut st.tree.objects[*id].transform;
        let mut cur = [t.position.x, t.position.y, t.position.z, t.rotation.x, t.rotation.y, t.rotation.z, t.scale.x, t.scale.y, t.scale.z];
        for j in 0..9 {
            let mut d = to[j] - cur[j];
            if (3..6).contains(&j) {
                d = (d + 180.0).rem_euclid(360.0) - 180.0;
            }
            cur[j] = if d.abs() < 1e-4 { to[j] } else { cur[j] + d * k };
        }
        t.position = glam::Vec3::new(cur[0], cur[1], cur[2]);
        t.rotation = glam::Vec3::new(cur[3], cur[4], cur[5]);
        t.scale = glam::Vec3::new(cur[6], cur[7], cur[8]);
    }
    n.flush();
    st.net = Some(n);
    actions
}

/// Sends this game's shared objects' changes (20 times a second): after the scripts run.
pub(crate) fn send_states(st: &mut WorldState, dt: f64) {
    let Some(mut n) = st.net.take() else { return };
    n.acc += dt;
    if n.acc >= 1.0 / SEND_HZ && n.me != 0 {
        n.acc = (n.acc - 1.0 / SEND_HZ).min(1.0 / SEND_HZ);
        let mine: Vec<(String, ObjectId)> = n.local.iter().filter(|(k, i)| n.objects.get(*k).is_some_and(|o| o.owner == n.me) && st.tree.contains(**i)).map(|(k, i)| (k.clone(), *i)).collect();
        for (net, id) in mine {
            let now = snapshot(st, id);
            let d = diff(n.sent.get(&net).map(|v| v.as_slice()).unwrap_or(&[]), &now);
            if d.is_empty() {
                continue;
            }
            if let Some(o) = n.objects.get_mut(&net) {
                merge(&mut o.parts, &d);
            }
            n.sent.insert(net.clone(), now);
            n.send_all(&Msg::State { net, parts: d });
        }
    }
    n.flush();
    st.net = Some(n);
}

/// NET_SPAWN: the new instance (already in the tree) appears for everyone.
pub fn spawned(st: &mut WorldState, id: ObjectId, res: &str) {
    let Some(mut n) = st.net.take() else { return };
    n.counter += 1;
    let net = format!("{}:{}", n.me, n.counter);
    let parent = match st.tree.objects[id].parent.filter(|p| Some(*p) != st.tree.root) {
        Some(p) => match n.by_object.get(&p) {
            Some(net) => format!("@{}", net),
            None => st.tree.path_of(p),
        },
        None => String::new(),
    };
    let parts = snapshot(st, id);
    let obj = NetObj { net: net.clone(), level: st.blueprint.clone(), res: Some(res.to_string()), parent, owner: n.me, gone: false, parts: parts.clone() };
    link(&mut n, &net, id);
    n.sent.insert(net.clone(), parts);
    if n.me != 0 {
        n.send_all(&Msg::Spawn { obj: obj.clone() });
    }
    n.objects.insert(net, obj);
    st.net = Some(n);
}

/// NET_SYNC: an object placed in the blueprint is shared.
pub fn sync(st: &mut WorldState, id: ObjectId) -> Result<String, String> {
    let Some(mut n) = st.net.take() else { return Ok(String::new()) };
    if let Some(net) = n.by_object.get(&id).cloned() {
        st.net = Some(n);
        return Ok(net);
    }
    let net = format!("{}#{}", st.blueprint, st.tree.path_of(id));
    let owner = match n.objects.get(&net) {
        Some(o) => o.owner,
        None => {
            n.objects.insert(net.clone(), NetObj { net: net.clone(), level: st.blueprint.clone(), res: None, parent: st.tree.path_of(id), owner: 1, gone: false, parts: Vec::new() });
            1
        }
    };
    link(&mut n, &net, id);
    if owner == n.me {
        n.sent.insert(net.clone(), Vec::new());
    } else {
        make_remote(&mut n, st, id);
        let parts = n.objects[&net].parts.clone();
        apply_parts(&mut n, st, id, &parts, true);
    }
    st.net = Some(n);
    Ok(net)
}

/// The shared object an object is (or is inside).
pub fn shared_root(st: &WorldState, id: ObjectId) -> Option<(ObjectId, String)> {
    let n = st.net.as_ref()?;
    let mut cur = Some(id);
    while let Some(c) = cur {
        if let Some(net) = n.by_object.get(&c) {
            return Some((c, net.clone()));
        }
        cur = st.tree.get(c)?.parent;
    }
    None
}

pub fn set_owner(st: &mut WorldState, id: ObjectId, owner: u32) -> Result<(), String> {
    let Some((root, net)) = shared_root(st, id) else { return Err("that object isn't shared (NET_SPAWN or NET_SYNC it first)".into()) };
    let mut n = st.net.take().unwrap();
    if !n.players.contains(&owner) {
        st.net = Some(n);
        return Err(format!("there's no player {}", owner));
    }
    let mut actions = Vec::new();
    apply(&mut n, st, Msg::Owner { net: net.clone(), owner }, &mut actions);
    if owner == n.me {
        n.sent.insert(net.clone(), snapshot(st, root));
    }
    n.send_all(&Msg::Owner { net, owner });
    st.net = Some(n);
    Ok(())
}

/// NET_EMIT (to = None) and NET_SEND.
pub fn send_event(st: &mut WorldState, to: Option<u32>, name: &str, args: &[Value]) -> Result<(), String> {
    let Some(n) = st.net.as_mut() else { return Ok(()) };
    if n.me == 0 {
        return Err("still joining (wait for on_connected)".into());
    }
    let args = args.iter().map(|a| crate::extras::to_json(a, 0)).collect::<Result<Vec<_>, _>>()?;
    let m = Msg::Event { from: n.me, to, name: name.to_string(), args };
    match to {
        None => n.send_all(&m),
        Some(t) => {
            if !n.players.contains(&t) {
                return Err(format!("there's no player {}", t));
            }
            n.send_to(t, &m)
        }
    }
    Ok(())
}

/// Objects removed from the tree: shared ones go for everyone.
pub(crate) fn forget(st: &mut WorldState, gone: &[ObjectId]) {
    let Some(n) = st.net.as_mut() else { return };
    for g in gone {
        n.targets.remove(g);
        n.orig.remove(g);
        if let Some(net) = n.by_object.remove(g) {
            n.local.remove(&net);
            n.sent.remove(&net);
            match n.objects.get_mut(&net) {
                Some(o) if o.res.is_none() => o.gone = true,
                Some(_) => {
                    n.objects.remove(&net);
                }
                None => continue,
            }
            if n.me != 0 {
                n.send_all(&Msg::Destroy { net });
            }
        }
    }
}

pub fn net_object(st: &WorldState, net: &str) -> Option<ObjectId> {
    st.net.as_ref()?.local.get(net).copied().filter(|i| st.tree.contains(*i))
}
