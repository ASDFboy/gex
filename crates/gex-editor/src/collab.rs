//! Working on one project together, live. One person hosts (File → Work together → Host) and
//! the others join with the host's address: they get a copy of the project, and from then on
//!
//! - blueprints stay in step object by object (two people moving different objects both win;
//!   the same object, the last change wins),
//! - scripts stay in step letter by letter, even with two people typing in the same one
//!   (edits are shifted past each other through the host),
//! - files saved, added, renamed or deleted on one computer change on everyone's,
//! - everyone sees where the others are: their tab, their mouse in the blueprint view, what
//!   they've selected, and their text cursor in scripts.
//!
//! Everything goes through the host, over TCP (see gex_script::wire). The network is read once
//! a frame; while a session is on the editor redraws 20 times a second to keep up.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use gex_core::{BlueprintTree, Object, ObjectId, ObjectKind, Transform, Variant};
use gex_script::wire::{Frame, Link, Listener, Waker};
use serde::{Deserialize, Serialize};

use crate::docs::Tip as _;
use crate::state::EditorState;
use crate::theme;

pub const DEFAULT_PORT: u16 = 7778;
const PROTOCOL: u32 = 1;
/// Files bigger than this aren't shared.
const MAX_FILE: u64 = 256 << 20;

// ------------------------------------------------------------------ messages

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t")]
enum M {
    Hello { v: u32, name: String },
    Refused { reason: String },
    Welcome { id: u32, project: String, people: Vec<Person>, files: Vec<(String, u64, u64)>, #[serde(default)] token: String },
    Need { paths: Vec<String> },
    /// A file; its bytes come in the next frame.
    File { path: String },
    Gone { path: String },
    Synced,
    Joined { person: Person },
    Left { id: u32 },
    Presence { id: u32, p: Presence },
    Objects { res: String, from: u32, changes: Vec<Change> },
    OpenBp { res: String },
    BpState { res: String, entries: Vec<Change> },
    Edit { res: String, from: u32, rev: u64, op: Op },
    OpenScript { res: String },
    ScriptState { res: String, rev: u64, text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Presence {
    /// The blueprint they have open.
    pub bp: String,
    /// The script they're typing in, if any.
    pub script: String,
    /// Their mouse in the blueprint view (world position) and whether that view is 3D.
    pub cursor: Option<[f32; 3]>,
    pub mode3d: bool,
    /// What they've selected (object paths).
    pub sel: Vec<String>,
    /// Their text cursor (a character index) in `script`.
    pub caret: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Person {
    pub id: u32,
    pub name: String,
    pub p: Presence,
}

// ------------------------------------------------------------------ blueprints, object by object

/// One object's own data (not its children), as shared.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct ObjData {
    name: String,
    kind: ObjectKind,
    tf: Transform,
    visible: bool,
    props: BTreeMap<String, Variant>,
    scripts: Vec<String>,
    groups: Vec<String>,
    instance_of: Option<String>,
    from_instance: bool,
}

impl ObjData {
    fn of(o: &Object) -> ObjData {
        ObjData {
            name: o.name.clone(),
            kind: o.kind,
            tf: o.transform,
            visible: o.visible,
            props: o.props.clone(),
            scripts: o.scripts.clone(),
            groups: o.groups.clone(),
            instance_of: o.instance_of.clone(),
            from_instance: o.from_instance,
        }
    }
    fn put(&self, o: &mut Object) {
        o.name = self.name.clone();
        o.kind = self.kind;
        o.transform = self.tf;
        o.visible = self.visible;
        o.props = self.props.clone();
        o.scripts = self.scripts.clone();
        o.groups = self.groups.clone();
        o.instance_of = self.instance_of.clone();
        o.from_instance = self.from_instance;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Entry {
    parent: String,
    index: u32,
    data: ObjData,
}

/// An object (by path) added or changed, or removed (`e` is None).
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Change {
    key: String,
    e: Option<Entry>,
}

type Flat = HashMap<String, Entry>;

/// Every object's path and id, parents first.
fn keys(tree: &BlueprintTree) -> Vec<(String, ObjectId)> {
    let mut out = Vec::with_capacity(tree.objects.len());
    let Some(root) = tree.root else { return out };
    out.push((tree.objects[root].name.clone(), root));
    let mut i = 0;
    while i < out.len() {
        let (k, id) = out[i].clone();
        for c in &tree.objects[id].children {
            out.push((format!("{}/{}", k, tree.objects[*c].name), *c));
        }
        i += 1;
    }
    out
}

fn flat(tree: &BlueprintTree) -> Flat {
    let ks = keys(tree);
    let by_id: HashMap<ObjectId, &str> = ks.iter().map(|(k, i)| (*i, k.as_str())).collect();
    ks.iter()
        .map(|(k, id)| {
            let o = &tree.objects[*id];
            let (parent, index) = match o.parent {
                Some(p) => (by_id.get(&p).copied().unwrap_or("").to_string(), tree.objects[p].children.iter().position(|c| c == id).unwrap_or(0) as u32),
                None => (String::new(), 0),
            };
            (k.clone(), Entry { parent, index, data: ObjData::of(o) })
        })
        .collect()
}

fn diff(old: &Flat, new: &Flat) -> Vec<Change> {
    let mut out: Vec<Change> = new.iter().filter(|(k, e)| old.get(*k) != Some(*e)).map(|(k, e)| Change { key: k.clone(), e: Some(e.clone()) }).collect();
    out.extend(old.keys().filter(|k| !new.contains_key(*k)).map(|k| Change { key: k.clone(), e: None }));
    out
}

fn depth(k: &str) -> usize {
    k.bytes().filter(|b| *b == b'/').count()
}

/// Makes a tree follow changes: removals deepest first, then additions and changes parents first.
fn apply_changes(tree: &mut BlueprintTree, changes: &[Change]) {
    let mut map: HashMap<String, ObjectId> = keys(tree).into_iter().collect();
    let mut gone: Vec<&Change> = changes.iter().filter(|c| c.e.is_none()).collect();
    gone.sort_by_key(|c| std::cmp::Reverse(depth(&c.key)));
    for c in gone {
        if let Some(id) = map.remove(&c.key) {
            if tree.contains(id) && Some(id) != tree.root {
                tree.remove(id);
            }
        }
    }
    let mut up: Vec<(&String, &Entry)> = changes.iter().filter_map(|c| c.e.as_ref().map(|e| (&c.key, e))).collect();
    up.sort_by_key(|(k, e)| (depth(k), e.index));
    for (key, e) in up {
        let existing = map.get(key).copied().filter(|i| tree.contains(*i));
        let id = match existing {
            Some(id) => id,
            None if e.parent.is_empty() => {
                // The top object renamed (or a first one).
                match tree.root {
                    Some(r) => r,
                    None => tree.add(None, Object::new(&e.data.name, e.data.kind)),
                }
            }
            None => {
                let Some(parent) = map.get(&e.parent).copied().filter(|i| tree.contains(*i)) else { continue };
                tree.add(Some(parent), Object::new(&e.data.name, e.data.kind))
            }
        };
        e.data.put(&mut tree.objects[id]);
        if let Some(p) = tree.objects[id].parent {
            let kids = &tree.objects[p].children;
            let want = (e.index as usize).min(kids.len() - 1);
            if kids.iter().position(|c| *c == id) != Some(want) {
                let kids = &mut tree.objects[p].children;
                kids.retain(|c| *c != id);
                kids.insert(want.min(kids.len()), id);
            }
        }
        map.insert(key.clone(), id);
    }
}

struct Bp {
    flat: Flat,
    /// Joined: objects changed here whose change the host hasn't passed back yet (their
    /// changes from others are older, and are skipped).
    pending: HashMap<String, u32>,
    /// Joined: the host's state arrived.
    ready: bool,
}

// ------------------------------------------------------------------ scripts, letter by letter

/// Replace the characters from s to e (character indices) with t.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Op {
    s: u32,
    e: u32,
    t: String,
}

fn char_len(s: &str) -> u32 {
    s.chars().count() as u32
}

fn byte_at(s: &str, c: u32) -> usize {
    s.char_indices().nth(c as usize).map(|(b, _)| b).unwrap_or(s.len())
}

fn apply_op(text: &mut String, op: &Op) {
    let n = char_len(text);
    let (s, e) = (op.s.min(n), op.e.min(n).max(op.s.min(n)));
    let (bs, be) = (byte_at(text, s), byte_at(text, e));
    text.replace_range(bs..be, &op.t);
}

/// The one replacement that turns `a` into `b` (what's between their common start and end).
fn text_diff(a: &str, b: &str) -> Option<Op> {
    if a == b {
        return None;
    }
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let pre = ac.iter().zip(&bc).take_while(|(x, y)| x == y).count();
    let max_suf = ac.len().min(bc.len()) - pre;
    let suf = ac.iter().rev().zip(bc.iter().rev()).take(max_suf).take_while(|(x, y)| x == y).count();
    Some(Op { s: pre as u32, e: (ac.len() - suf) as u32, t: bc[pre..bc.len() - suf].iter().collect() })
}

/// `a` moved past `b` (both made on the same text): what `a` becomes after `b` is applied.
/// Edits apart shift past each other; at the same spot `a_first` decides whose insert comes
/// first. Edits that overlap become one: everything either removed goes, and both texts stay,
/// the first one's first (the same result on both sides, whichever lands first).
fn xf(a: &Op, b: &Op, a_first: bool) -> Op {
    let blen = char_len(&b.t);
    let delta = blen as i64 - (b.e as i64 - b.s as i64);
    let shift = |x: u32| (x as i64 + delta).max(0) as u32;
    let (a_ins, b_ins) = (a.s == a.e, b.s == b.e);
    let overlap = match (a_ins, b_ins) {
        (true, true) => false,
        (true, false) => b.s < a.s && a.s < b.e,
        (false, true) => a.s < b.s && b.s < a.e,
        (false, false) => a.s < b.e && b.s < a.e,
    };
    if overlap {
        let (us, ue) = (a.s.min(b.s), a.e.max(b.e));
        let t = if a_first { format!("{}{}", a.t, b.t) } else { format!("{}{}", b.t, a.t) };
        return Op { s: us, e: shift(ue), t };
    }
    let start = |x: u32| -> u32 {
        if x < b.s {
            x
        } else if x == b.s {
            // At the spot b inserts: an insert of a's goes before it only when a is first; the
            // rest of a's range starts after it. At the start of b's range: before it.
            if b_ins && (!a_ins || !a_first) {
                x + blen
            } else {
                x
            }
        } else {
            shift(x)
        }
    };
    let s = start(a.s);
    let e = if a_ins {
        s
    } else if a.e <= b.s {
        a.e
    } else {
        shift(a.e)
    };
    Op { s, e: e.max(s), t: a.t.clone() }
}

/// Where a text cursor goes when an edit lands.
fn shift_caret(c: u32, op: &Op) -> u32 {
    if c <= op.s {
        c
    } else if c >= op.e {
        (c as i64 + char_len(&op.t) as i64 - (op.e as i64 - op.s as i64)).max(0) as u32
    } else {
        op.s + char_len(&op.t)
    }
}

struct Doc {
    rev: u64,
    /// Hosting: the shared text. Joined: the host's text plus the edit on its way.
    text: String,
    /// Hosting: recent edits, to move late ones past.
    log: VecDeque<(u64, Op)>,
    /// Joined: an edit sent and not yet passed back.
    inflight: Option<Op>,
    ready: bool,
}

// ------------------------------------------------------------------ files

#[derive(Clone, Copy, PartialEq)]
struct FileInfo {
    len: u64,
    modified: u128,
    hash: u64,
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn modified(m: &std::fs::Metadata) -> u128 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
}

/// Every shared file in the project: (path with '/', length, modified). Hidden folders (the
/// editor's own .gex) and build output stay on each computer.
fn walk(dir: &Path) -> Vec<(String, u64, u128)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, rel)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "target" || name.ends_with(".part") {
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            let r = if rel.is_empty() { name.clone() } else { format!("{}/{}", rel, name) };
            if m.is_dir() {
                stack.push((e.path(), r));
            } else if m.is_file() && m.len() <= MAX_FILE {
                out.push((r, m.len(), modified(&m)));
            }
        }
    }
    out
}

/// A path someone sent, made safe: inside the project, no "..", no drive letters.
fn safe_path(dir: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.starts_with('\\') || rel.contains(':') {
        return None;
    }
    let mut p = dir.to_path_buf();
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." || part.starts_with('.') {
            return None;
        }
        p.push(part);
    }
    Some(p)
}

// ------------------------------------------------------------------ the session

struct Peer {
    id: u32,
    conn: Link,
    /// Files still to send them (joining).
    queue: VecDeque<String>,
    synced_sent: bool,
    asked: bool,
    /// A file whose bytes are still coming.
    incoming: Option<String>,
}

pub struct Session {
    pub me: u32,
    pub host: bool,
    pub name: String,
    /// Everyone else.
    pub people: Vec<Person>,
    pub status: String,
    listener: Option<Listener>,
    pub port: u16,
    /// Wakes the window when something arrives.
    waker: Waker,
    peers: Vec<Peer>,
    connecting: Option<Receiver<Result<TcpStream, String>>>,
    next_id: u32,
    /// The project folder (joining: where the copy goes).
    pub dir: PathBuf,
    base_dir: PathBuf,
    pub project: String,
    /// Joining: every file has arrived; the project can open.
    pub ready: bool,
    pub opened: bool,
    /// Hosting: a mark left in the project's .gex folder, so joining from this same computer
    /// doesn't copy the project onto itself.
    token: String,
    files: HashMap<String, FileInfo>,
    scan_at: Instant,
    bps: HashMap<String, Bp>,
    scripts: HashMap<String, Doc>,
    /// Open scripts and blueprints seen last frame (to notice ones just opened).
    open_scripts: HashSet<String>,
    open_bps: HashSet<String>,
    objects_at: Instant,
    others_at: Instant,
    pub local: Presence,
    sent: Option<Presence>,
    presence_at: Instant,
    /// Frames when the view last saw the mouse and a script editor last had the keyboard.
    pub cursor_frame: u64,
    pub caret_frame: u64,
    pub frame: u64,
    /// The session ended (the host left, or the connection broke): why.
    pub ended: Option<String>,
    /// Paths to object ids in the blueprint open here, for drawing others' selections.
    ids: (String, HashMap<String, ObjectId>),
    /// Something arrived this frame.
    busy: bool,
}

pub fn default_name() -> String {
    std::env::var("GEX_NAME").or_else(|_| std::env::var("USERNAME")).or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Someone".into())
}

/// Where joined projects go by default.
pub fn default_folder() -> PathBuf {
    // Where new projects are made (the launcher's folder).
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    std::fs::read_to_string(crate::state::config_dir().join("projects_dir.txt")).ok().map(|s| PathBuf::from(s.trim())).filter(|p| p.is_dir()).unwrap_or_else(|| home.join("GEX Projects"))
}

pub fn color(id: u32) -> Color32 {
    const C: [Color32; 6] = [theme::MK_PINK, theme::MK_GREEN, theme::MK_ORANGE, theme::MK_PURPLE, theme::MK_YELLOW, theme::MK_CYAN];
    C[(id as usize).wrapping_sub(1) % C.len()]
}

fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "project".into() } else { s }
}

impl Session {
    fn new(name: &str, dir: PathBuf, ctx: &egui::Context) -> Session {
        let now = Instant::now();
        let c = ctx.clone();
        Session {
            me: 0,
            host: false,
            name: name.trim().chars().take(32).collect::<String>(),
            people: Vec::new(),
            status: String::new(),
            listener: None,
            port: DEFAULT_PORT,
            waker: std::sync::Arc::new(move || c.request_repaint()),
            peers: Vec::new(),
            connecting: None,
            next_id: 2,
            dir: dir.clone(),
            base_dir: dir,
            project: String::new(),
            ready: false,
            opened: false,
            token: String::new(),
            files: HashMap::new(),
            scan_at: now,
            bps: HashMap::new(),
            scripts: HashMap::new(),
            open_scripts: HashSet::new(),
            open_bps: HashSet::new(),
            objects_at: now,
            others_at: now,
            local: Presence::default(),
            sent: None,
            presence_at: now,
            cursor_frame: 0,
            caret_frame: 0,
            frame: 0,
            ended: None,
            ids: (String::new(), HashMap::new()),
            busy: false,
        }
    }

    /// Shares the open project.
    pub fn host(ctx: &egui::Context, st: &EditorState, name: &str, port: u16) -> Result<Session, String> {
        let l = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("can't use port {}: {}", port, e))?;
        let mut s = Session::new(name, st.project.dir.clone(), ctx);
        let l = Listener::new(l, s.waker.clone()).map_err(|e| e.to_string())?;
        s.me = 1;
        s.host = true;
        s.ready = true;
        s.opened = true;
        s.port = port;
        s.listener = Some(l);
        s.project = st.project.config.name.clone();
        s.token = format!("{:x}", fnv(format!("{:?}{}", Instant::now(), std::process::id()).as_bytes()) ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0));
        let _ = std::fs::create_dir_all(st.project.dir.join(".gex"));
        let _ = std::fs::write(st.project.dir.join(".gex").join("sharing"), &s.token);
        s.scan(true);
        s.status = format!("Sharing {}", s.project);
        Ok(s)
    }

    /// Joins someone's session; the project is copied into `folder`.
    pub fn join(ctx: &egui::Context, name: &str, address: &str, folder: &Path) -> Session {
        let (h, p) = gex_script::wire::split_address(address, DEFAULT_PORT);
        let mut s = Session::new(name, folder.to_path_buf(), ctx);
        s.connecting = Some(gex_script::wire::connect(&h, p));
        s.status = format!("Connecting to {}:{}…", h, p);
        s
    }

    /// What others type to join (this computer's network address and the port).
    pub fn address(&self) -> String {
        format!("{}:{}", gex_script::wire::local_ip().unwrap_or_else(|| "this computer's address".into()), self.port)
    }

    fn send_all(&mut self, m: &M) {
        let Ok(b) = serde_json::to_vec(m) else { return };
        for p in &mut self.peers {
            if p.id != 0 {
                p.conn.send_json(&b);
            }
        }
    }

    fn send_except(&mut self, except: u32, m: &M) {
        let Ok(b) = serde_json::to_vec(m) else { return };
        for p in &mut self.peers {
            if p.id != 0 && p.id != except {
                p.conn.send_json(&b);
            }
        }
    }

    fn send_file(conn: &mut Link, path: &str, bytes: &[u8]) {
        conn.send(&M::File { path: path.to_string() });
        conn.send_bytes(bytes);
    }

    /// Looks for files changed on this computer and sends them (`quiet`: only take note).
    fn scan(&mut self, quiet: bool) {
        self.scan_at = Instant::now();
        let now = walk(&self.dir);
        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        for (rel, len, m) in now {
            seen.insert(rel.clone());
            let known = self.files.get(&rel).copied();
            if known.is_some_and(|k| k.len == len && k.modified == m) {
                continue;
            }
            let Ok(bytes) = std::fs::read(self.dir.join(&rel)) else { continue };
            let hash = fnv(&bytes);
            self.files.insert(rel.clone(), FileInfo { len, modified: m, hash });
            if known.is_some_and(|k| k.hash == hash) {
                continue;
            }
            if !quiet {
                changed.push((rel, bytes));
            }
        }
        let gone: Vec<String> = self.files.keys().filter(|k| !seen.contains(*k)).cloned().collect();
        for g in &gone {
            self.files.remove(g);
        }
        if quiet || self.me == 0 {
            return;
        }
        for (rel, bytes) in changed {
            for p in &mut self.peers {
                if p.id != 0 {
                    Self::send_file(&mut p.conn, &rel, &bytes);
                }
            }
        }
        for g in gone {
            self.send_all(&M::Gone { path: g });
        }
    }

    /// Writes a file someone sent (and remembers it, so it isn't sent back).
    fn write_file(&mut self, rel: &str, bytes: &[u8], st: Option<&mut EditorState>) -> bool {
        let Some(p) = safe_path(&self.dir, rel) else { return false };
        if p.is_file() && std::fs::read(&p).map(|b| fnv(&b) == fnv(bytes)).unwrap_or(false) {
            return false;
        }
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if std::fs::write(&p, bytes).is_err() {
            return false;
        }
        if let Ok(m) = std::fs::metadata(&p) {
            self.files.insert(rel.to_string(), FileInfo { len: m.len(), modified: modified(&m), hash: fnv(bytes) });
        }
        if let Some(st) = st {
            file_arrived(st, rel, bytes);
        }
        true
    }

    fn remove_file(&mut self, rel: &str) {
        if let Some(p) = safe_path(&self.dir, rel) {
            let _ = std::fs::remove_file(&p);
            // Folders left empty go too.
            let mut d = p.parent().map(Path::to_path_buf);
            while let Some(x) = d.filter(|x| x.starts_with(&self.dir) && *x != self.dir) {
                if std::fs::remove_dir(&x).is_err() {
                    break;
                }
                d = x.parent().map(Path::to_path_buf);
            }
        }
        self.files.remove(rel);
    }
}

/// A file arrived for an open project: open scripts and blueprints now match the disk.
fn file_arrived(st: &mut EditorState, rel: &str, bytes: &[u8]) {
    let res = format!("res://{}", rel);
    let text = String::from_utf8_lossy(bytes).into_owned();
    if let Some(s) = st.scripts.get_mut(&res) {
        s.saved = text.clone();
    }
    if st.blueprint_res.as_deref() == Some(res.as_str()) {
        st.blueprint_saved = text.clone();
    }
    for d in &mut st.docs {
        if d.blueprint_res.as_deref() == Some(res.as_str()) {
            d.blueprint_saved = text.clone();
        }
    }
    if rel == "project.jproj" {
        if let Ok(p) = gex_core::project::Project::open(&st.project.dir) {
            st.project.config = p.config;
        }
    }
}

/// Undo steps that take others' changes too (deeper ones would undo theirs as well; going
/// through every step of a long history for each change would be slow).
const HISTORY_SYNCED: usize = 24;

/// Every tree that shows a blueprint: open ones and their recent undo steps (so undo takes back
/// your own changes, not the others').
fn trees_of<'a>(st: &'a mut EditorState, res: &str) -> Vec<&'a mut BlueprintTree> {
    let mut out: Vec<&mut BlueprintTree> = Vec::new();
    if st.blueprint_res.as_deref() == Some(res) {
        out.push(&mut st.tree);
        out.extend(st.undo.iter_mut().rev().take(HISTORY_SYNCED));
        out.extend(st.redo.iter_mut().rev().take(HISTORY_SYNCED));
    }
    for d in &mut st.docs {
        if d.blueprint_res.as_deref() == Some(res) {
            out.push(&mut d.tree);
            out.extend(d.undo.iter_mut().rev().take(HISTORY_SYNCED));
            out.extend(d.redo.iter_mut().rev().take(HISTORY_SYNCED));
        }
    }
    out
}

fn open_tree<'a>(st: &'a EditorState, res: &str) -> Option<&'a BlueprintTree> {
    if st.blueprint_res.as_deref() == Some(res) {
        return Some(&st.tree);
    }
    st.docs.iter().find(|d| d.blueprint_res.as_deref() == Some(res)).map(|d| &d.tree)
}

fn open_bps(st: &EditorState) -> HashSet<String> {
    st.blueprint_res.iter().cloned().chain(st.docs.iter().filter_map(|d| d.blueprint_res.clone())).collect()
}

fn apply_to_open(st: &mut EditorState, res: &str, changes: &[Change]) {
    if changes.is_empty() {
        return;
    }
    for t in trees_of(st, res) {
        apply_changes(t, changes);
    }
    // Objects others removed can't stay selected.
    let tree = &st.tree;
    st.selection.retain(|i| tree.contains(*i));
    if st.selected.is_some_and(|i| !tree.contains(i)) {
        st.selected = None;
    }
}

fn script_id(res: &str) -> egui::Id {
    egui::Id::new(("script", res))
}

/// An edit from someone else lands in an open script: the text changes, and the cursor here
/// stays on the same letter.
fn edit_open_script(ctx: &egui::Context, st: &mut EditorState, res: &str, op: &Op) {
    let Some(s) = st.scripts.get_mut(res) else { return };
    apply_op(&mut s.text, op);
    let id = script_id(res);
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        if let Some(r) = state.cursor.char_range() {
            let a = shift_caret(r.primary.index.0 as u32, op) as usize;
            let b = shift_caret(r.secondary.index.0 as u32, op) as usize;
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(b), egui::text::CCursor::new(a))));
            state.store(ctx, id);
        }
    }
}

fn read_script(st: &EditorState, res: &str) -> String {
    match st.scripts.get(res) {
        Some(s) => s.text.clone(),
        None => st.project.read_text(res).unwrap_or_default(),
    }
}

fn read_bp_flat(st: &EditorState, res: &str) -> Flat {
    if let Some(t) = open_tree(st, res) {
        return flat(t);
    }
    let read = st.reader();
    st.project.read_text(res).ok().and_then(|t| gex_core::blueprint::load(&t, &read).ok()).map(|t| flat(&t)).unwrap_or_default()
}

/// Once a frame: the network, files, blueprints, scripts and where everyone is.
pub fn tick(ctx: &egui::Context, st: &mut EditorState) {
    let Some(mut s) = st.collab.take() else { return };
    s.frame = ctx.cumulative_frame_nr();
    s.pump(ctx, Some(st));
    if s.ended.is_none() {
        s.local_changes(ctx, st);
        s.presence(st);
    }
    for p in &mut s.peers {
        p.conn.flush();
    }
    s.wake(ctx);
    st.collab = Some(s);
}

/// Before the project opens (joining): just the network and files.
pub fn tick_joining(ctx: &egui::Context, s: &mut Session) {
    s.frame = ctx.cumulative_frame_nr();
    s.pump(ctx, None);
    for p in &mut s.peers {
        p.conn.flush();
    }
    s.wake(ctx);
}

impl Session {
    /// The next look at the network: right away while things are arriving (others' pointers
    /// move smoothly); when it's quiet, only every two seconds (so an idle editor stays idle).
    fn wake(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.busy) || self.peers.iter().any(|p| !p.queue.is_empty()) {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.connecting.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            // Arrivals wake the window themselves; this is only for files changed outside GEX.
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }

    fn pump(&mut self, ctx: &egui::Context, mut st: Option<&mut EditorState>) {
        if self.ended.is_some() {
            return;
        }
        // Joining: the connection.
        if let Some(rx) = &self.connecting {
            match rx.try_recv() {
                Ok(Ok(stream)) => {
                    self.connecting = None;
                    match Link::new(stream, self.waker.clone()) {
                        Ok(mut c) => {
                            c.send(&M::Hello { v: PROTOCOL, name: self.name.clone() });
                            self.peers.push(Peer { id: 1, conn: c, queue: VecDeque::new(), synced_sent: true, asked: true, incoming: None });
                            self.status = "Connected, getting the project…".into();
                        }
                        Err(e) => self.ended = Some(e.to_string()),
                    }
                }
                Ok(Err(e)) => self.ended = Some(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => self.ended = Some("couldn't connect".into()),
            }
            if self.ended.is_some() {
                return;
            }
        }
        if let Some(l) = &self.listener {
            while let Ok(sock) = l.rx.try_recv() {
                if let Ok(c) = Link::new(sock, self.waker.clone()) {
                    self.peers.push(Peer { id: 0, conn: c, queue: VecDeque::new(), synced_sent: false, asked: false, incoming: None });
                }
            }
        }
        // Hosting: send joiners their files, a few at a time.
        if self.host {
            let dir = self.dir.clone();
            for p in &mut self.peers {
                while p.conn.backlog() < (8 << 20) {
                    let Some(rel) = p.queue.pop_front() else { break };
                    if let Ok(bytes) = std::fs::read(dir.join(&rel)) {
                        Self::send_file(&mut p.conn, &rel, &bytes);
                    }
                }
                if p.asked && p.queue.is_empty() && !p.synced_sent {
                    p.conn.send(&M::Synced);
                    p.synced_sent = true;
                }
            }
        }
        // Messages, in the order each peer sent them; a file's bytes follow its header (maybe
        // a frame or two later, for a big one).
        enum Item {
            Msg(M),
            File(String, Vec<u8>),
        }
        let mut items: Vec<(usize, Item)> = Vec::new();
        for (i, p) in self.peers.iter_mut().enumerate() {
            for f in p.conn.recv() {
                match f {
                    Frame::Json(b) => match serde_json::from_slice::<M>(&b) {
                        Ok(M::File { path }) => p.incoming = Some(path),
                        Ok(m) => items.push((i, Item::Msg(m))),
                        Err(_) => {}
                    },
                    Frame::Bytes(b) => {
                        if let Some(path) = p.incoming.take() {
                            items.push((i, Item::File(path, b)));
                        }
                    }
                }
            }
        }
        if !items.is_empty() {
            self.busy = true;
        }
        for (i, item) in items {
            let m = match item {
                Item::Msg(m) => m,
                Item::File(path, bytes) => {
                    let from = self.peers[i].id;
                    if self.host && from == 0 {
                        continue;
                    }
                    let wrote = self.write_file(&path, &bytes, st.as_deref_mut());
                    if self.host && wrote {
                        for p in &mut self.peers {
                            if p.id != 0 && p.id != from {
                                Self::send_file(&mut p.conn, &path, &bytes);
                            }
                        }
                    }
                    if !self.ready {
                        self.status = format!("Getting the project… {}", path);
                    }
                    continue;
                }
            };
            self.message(ctx, i, m, st.as_deref_mut());
            if self.ended.is_some() {
                return;
            }
        }
        // People who left.
        let mut k = 0;
        while k < self.peers.len() {
            if self.peers[k].conn.is_closed() {
                let p = self.peers.remove(k);
                if self.host {
                    if p.id != 0 {
                        self.people.retain(|x| x.id != p.id);
                        self.send_all(&M::Left { id: p.id });
                    }
                } else {
                    self.ended = Some(if self.ready { "the host stopped sharing".into() } else { "the host closed the connection".into() });
                    return;
                }
            } else {
                k += 1;
            }
        }
        // Files changed here.
        if self.ready && self.opened && self.scan_at.elapsed() > Duration::from_millis(1500) {
            self.scan(false);
        }
    }

    fn message(&mut self, ctx: &egui::Context, i: usize, m: M, st: Option<&mut EditorState>) {
        let pid = self.peers[i].id;
        if self.host {
            match m {
                M::Hello { v, name } if pid == 0 => {
                    let c = &mut self.peers[i].conn;
                    if v != PROTOCOL {
                        c.send(&M::Refused { reason: "the host has a different version of GEX".into() });
                        c.close_after_sending();
                        return;
                    }
                    let id = self.next_id;
                    self.next_id += 1;
                    let mut name: String = name.trim().chars().take(32).collect();
                    if name.is_empty() {
                        name = format!("Guest {}", id);
                    }
                    let person = Person { id, name, p: Presence::default() };
                    let mut people = vec![Person { id: self.me, name: self.name.clone(), p: self.local.clone() }];
                    people.extend(self.people.iter().cloned());
                    let files = self.files.iter().map(|(k, f)| (k.clone(), f.len, f.hash)).collect();
                    self.peers[i].id = id;
                    self.peers[i].conn.send(&M::Welcome { id, project: self.project.clone(), people, files, token: self.token.clone() });
                    self.send_except(id, &M::Joined { person: person.clone() });
                    self.people.push(person);
                }
                _ if pid == 0 => {}
                M::Need { paths } => {
                    let p = &mut self.peers[i];
                    p.queue.extend(paths.into_iter().filter(|x| self.files.contains_key(x)));
                    p.asked = true;
                }
                M::Gone { path } => {
                    if safe_path(&self.dir, &path).is_some() {
                        self.remove_file(&path);
                        self.send_except(pid, &M::Gone { path });
                    }
                }
                M::Presence { p, .. } => {
                    if let Some(x) = self.people.iter_mut().find(|x| x.id == pid) {
                        x.p = p.clone();
                    }
                    self.send_except(pid, &M::Presence { id: pid, p });
                }
                M::Objects { res, changes, .. } => {
                    let Some(st) = st else { return };
                    if !self.bps.contains_key(&res) {
                        let f = read_bp_flat(st, &res);
                        self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: true });
                    }
                    // Changes made here since the last look go first.
                    self.blueprint_changes(st, &res);
                    let bp = self.bps.get_mut(&res).unwrap();
                    for c in &changes {
                        match &c.e {
                            Some(e) => {
                                bp.flat.insert(c.key.clone(), e.clone());
                            }
                            None => {
                                let pre = format!("{}/", c.key);
                                bp.flat.retain(|k, _| *k != c.key && !k.starts_with(&pre));
                            }
                        }
                    }
                    apply_to_open(st, &res, &changes);
                    // Everyone, the sender too (that's how it knows its change got here).
                    self.send_all(&M::Objects { res, from: pid, changes });
                }
                M::OpenBp { res } => {
                    let Some(st) = st else { return };
                    if !self.bps.contains_key(&res) {
                        let f = read_bp_flat(st, &res);
                        self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: true });
                    } else {
                        self.blueprint_changes(st, &res);
                    }
                    let entries = self.bps[&res].flat.iter().map(|(k, e)| Change { key: k.clone(), e: Some(e.clone()) }).collect();
                    self.peers[i].conn.send(&M::BpState { res, entries });
                }
                M::OpenScript { res } => {
                    let Some(st) = st else { return };
                    self.script_changes(st, &res);
                    if !self.scripts.contains_key(&res) {
                        let text = read_script(st, &res);
                        self.scripts.insert(res.clone(), Doc { rev: 0, text, log: VecDeque::new(), inflight: None, ready: true });
                    }
                    let d = &self.scripts[&res];
                    let m = M::ScriptState { res: res.clone(), rev: d.rev, text: d.text.clone() };
                    self.peers[i].conn.send(&m);
                }
                M::Edit { res, rev: base, op, .. } => {
                    let Some(st) = st else { return };
                    self.script_changes(st, &res);
                    if !self.scripts.contains_key(&res) {
                        let text = read_script(st, &res);
                        self.scripts.insert(res.clone(), Doc { rev: 0, text, log: VecDeque::new(), inflight: None, ready: true });
                    }
                    let d = self.scripts.get_mut(&res).unwrap();
                    if base + (d.log.len() as u64) < d.rev {
                        // Too far behind to move past everything since: they get the text again.
                        let m = M::ScriptState { res: res.clone(), rev: d.rev, text: d.text.clone() };
                        self.peers[i].conn.send(&m);
                        return;
                    }
                    let mut op = op;
                    for (r, l) in &d.log {
                        if *r > base {
                            op = xf(&op, l, false);
                        }
                    }
                    apply_op(&mut d.text, &op);
                    d.rev += 1;
                    d.log.push_back((d.rev, op.clone()));
                    if d.log.len() > 2000 {
                        d.log.pop_front();
                    }
                    let rev = d.rev;
                    edit_open_script(ctx, st, &res, &op);
                    self.send_all(&M::Edit { res, from: pid, rev, op });
                }
                _ => {}
            }
        } else {
            match m {
                M::Refused { reason } => self.ended = Some(reason),
                M::Welcome { id, project, people, files, token } => {
                    self.me = id;
                    self.project = project.clone();
                    self.dir = self.base_dir.join(sanitize(&project));
                    // Joining from the computer that shares it, into its own folder: a copy beside it.
                    let mark = |d: &Path| std::fs::read_to_string(d.join(".gex").join("sharing")).is_ok_and(|t| !token.is_empty() && t == token);
                    if mark(&self.dir) {
                        self.dir = self.base_dir.join(format!("{} (joined)", sanitize(&project)));
                    }
                    let _ = std::fs::create_dir_all(&self.dir);
                    self.people = people;
                    let mut need = Vec::new();
                    for (rel, len, hash) in files {
                        let Some(p) = safe_path(&self.dir, &rel) else { continue };
                        let same = std::fs::metadata(&p).is_ok_and(|m| m.len() == len) && std::fs::read(&p).is_ok_and(|b| fnv(&b) == hash);
                        if same {
                            if let Ok(m) = std::fs::metadata(&p) {
                                self.files.insert(rel.clone(), FileInfo { len, modified: modified(&m), hash });
                            }
                        } else {
                            need.push(rel);
                        }
                    }
                    self.status = format!("Getting {} ({} files)…", project, need.len());
                    self.peers[i].conn.send(&M::Need { paths: need });
                }
                M::Synced => {
                    self.ready = true;
                    self.status = format!("Working on {} together", self.project);
                    // Files that were already here count as seen.
                    self.scan(true);
                }
                M::Gone { path } => {
                    if self.ready {
                        self.remove_file(&path);
                    }
                }
                M::Joined { person } => {
                    self.people.retain(|x| x.id != person.id);
                    self.people.push(person);
                }
                M::Left { id } => self.people.retain(|x| x.id != id),
                M::Presence { id, p } => {
                    if let Some(x) = self.people.iter_mut().find(|x| x.id == id) {
                        x.p = p;
                    }
                }
                M::Objects { res, from, changes } => {
                    let Some(st) = st else { return };
                    let me = self.me;
                    let Some(bp) = self.bps.get_mut(&res) else { return };
                    if !bp.ready {
                        return;
                    }
                    if from == me {
                        for c in &changes {
                            if let Some(n) = bp.pending.get_mut(&c.key) {
                                *n -= 1;
                                if *n == 0 {
                                    bp.pending.remove(&c.key);
                                }
                            }
                        }
                        return;
                    }
                    // Changes made here first, so they aren't mistaken for these.
                    self.blueprint_changes(st, &res);
                    let bp = self.bps.get_mut(&res).unwrap();
                    let take: Vec<Change> = changes.into_iter().filter(|c| !bp.pending.contains_key(&c.key)).collect();
                    for c in &take {
                        match &c.e {
                            Some(e) => {
                                bp.flat.insert(c.key.clone(), e.clone());
                            }
                            None => {
                                let pre = format!("{}/", c.key);
                                bp.flat.retain(|k, _| *k != c.key && !k.starts_with(&pre));
                            }
                        }
                    }
                    apply_to_open(st, &res, &take);
                }
                M::BpState { res, entries } => {
                    let Some(st) = st else { return };
                    let Some(bp) = self.bps.get_mut(&res) else { return };
                    let host: Flat = entries.into_iter().filter_map(|c| c.e.map(|e| (c.key, e))).collect();
                    let here = open_tree(st, &res).map(flat).unwrap_or_default();
                    let changes = diff(&here, &host);
                    bp.flat = host;
                    bp.ready = true;
                    bp.pending.clear();
                    apply_to_open(st, &res, &changes);
                }
                M::ScriptState { res, rev, text } => {
                    let Some(st) = st else { return };
                    let Some(d) = self.scripts.get_mut(&res) else { return };
                    d.rev = rev;
                    d.text = text.clone();
                    d.inflight = None;
                    d.ready = true;
                    if let Some(s) = st.scripts.get_mut(&res) {
                        if s.text != text {
                            if let Some(op) = text_diff(&s.text, &text) {
                                edit_open_script(ctx, st, &res, &op);
                            }
                        }
                    }
                }
                M::Edit { res, from, rev, op } => {
                    let Some(st) = st else { return };
                    let me = self.me;
                    let Some(d) = self.scripts.get_mut(&res) else { return };
                    if !d.ready {
                        return;
                    }
                    if from == me {
                        d.inflight = None;
                        d.rev = rev;
                        return;
                    }
                    let mut s1 = op;
                    if let Some(p) = d.inflight.take() {
                        let s_after = xf(&s1, &p, true);
                        d.inflight = Some(xf(&p, &s1, false));
                        s1 = s_after;
                    }
                    // Typed here and not sent yet: the edit moves past that too.
                    let editor = st.scripts.get(&res).map(|s| s.text.clone());
                    let s2 = match editor.as_deref().and_then(|e| text_diff(&d.text, e)) {
                        Some(b) => xf(&s1, &b, true),
                        None => s1.clone(),
                    };
                    apply_op(&mut d.text, &s1);
                    d.rev = rev;
                    edit_open_script(ctx, st, &res, &s2);
                }
                _ => {}
            }
        }
    }

    /// Objects changed here in one blueprint since the last look: sent on.
    fn blueprint_changes(&mut self, st: &EditorState, res: &str) {
        let Some(tree) = open_tree(st, res) else { return };
        let Some(bp) = self.bps.get_mut(res) else { return };
        if !bp.ready {
            return;
        }
        let now = flat(tree);
        let changes = diff(&bp.flat, &now);
        if changes.is_empty() {
            return;
        }
        bp.flat = now;
        if !self.host {
            for c in &changes {
                *bp.pending.entry(c.key.clone()).or_default() += 1;
            }
        }
        self.send_all(&M::Objects { res: res.to_string(), from: self.me, changes });
    }

    /// Typing here in one script since the last look: sent on.
    fn script_changes(&mut self, st: &EditorState, res: &str) {
        let Some(s) = st.scripts.get(res) else { return };
        let Some(d) = self.scripts.get_mut(res) else { return };
        if !d.ready {
            return;
        }
        if self.host {
            if let Some(op) = text_diff(&d.text, &s.text) {
                d.text = s.text.clone();
                d.rev += 1;
                d.log.push_back((d.rev, op.clone()));
                if d.log.len() > 2000 {
                    d.log.pop_front();
                }
                let m = M::Edit { res: res.to_string(), from: self.me, rev: d.rev, op };
                self.send_all(&m);
            }
        } else if d.inflight.is_none() {
            if let Some(op) = text_diff(&d.text, &s.text) {
                d.text = s.text.clone();
                d.inflight = Some(op.clone());
                let m = M::Edit { res: res.to_string(), from: self.me, rev: d.rev, op };
                self.send_all(&m);
            }
        }
    }

    fn local_changes(&mut self, ctx: &egui::Context, st: &mut EditorState) {
        let _ = ctx;
        // Scripts and blueprints opened or closed here.
        let scripts: HashSet<String> = st.scripts.keys().cloned().collect();
        for res in scripts.difference(&self.open_scripts.clone()) {
            if self.host {
                match self.scripts.get(res) {
                    // Changed by others while closed here: open it as it is now.
                    Some(d) => {
                        let text = d.text.clone();
                        if let Some(s) = st.scripts.get_mut(res) {
                            s.text = text;
                        }
                    }
                    None => {
                        let text = st.scripts[res].text.clone();
                        self.scripts.insert(res.clone(), Doc { rev: 0, text, log: VecDeque::new(), inflight: None, ready: true });
                    }
                }
            } else {
                let text = st.scripts[res].text.clone();
                self.scripts.insert(res.clone(), Doc { rev: 0, text, log: VecDeque::new(), inflight: None, ready: false });
                self.send_all(&M::OpenScript { res: res.clone() });
            }
        }
        if !self.host {
            self.scripts.retain(|r, _| scripts.contains(r));
        }
        self.open_scripts = scripts;
        let bps = open_bps(st);
        for res in bps.difference(&self.open_bps.clone()) {
            if self.host {
                match self.bps.get(res) {
                    Some(bp) => {
                        let here = open_tree(st, res).map(flat).unwrap_or_default();
                        let changes = diff(&here, &bp.flat);
                        apply_to_open(st, res, &changes);
                    }
                    None => {
                        let f = open_tree(st, res).map(flat).unwrap_or_default();
                        self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: true });
                    }
                }
            } else {
                let f = open_tree(st, res).map(flat).unwrap_or_default();
                self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: false });
                self.send_all(&M::OpenBp { res: res.clone() });
            }
        }
        if !self.host {
            self.bps.retain(|r, _| bps.contains(r));
        }
        self.open_bps = bps;
        // Typing: every frame (it's cheap, and others see letters as they come).
        let tracked: Vec<String> = self.scripts.keys().cloned().collect();
        for r in tracked {
            self.script_changes(st, &r);
        }
        // Objects: the blueprint being edited 10 times a second, the others once a second.
        let big = st.tree.objects.len() > 3000;
        if self.objects_at.elapsed() > Duration::from_millis(if big { 250 } else { 100 }) {
            self.objects_at = Instant::now();
            if let Some(res) = st.blueprint_res.clone() {
                self.blueprint_changes(st, &res);
                // Only needed to draw what others have selected here.
                if self.people.iter().any(|p| p.p.bp == res && !p.p.sel.is_empty()) {
                    self.ids = (res, keys(&st.tree).into_iter().collect());
                } else {
                    self.ids = (String::new(), HashMap::new());
                }
            }
        }
        if self.others_at.elapsed() > Duration::from_secs(1) {
            self.others_at = Instant::now();
            let others: Vec<String> = st.docs.iter().filter_map(|d| d.blueprint_res.clone()).collect();
            for r in others {
                self.blueprint_changes(st, &r);
            }
        }
    }

    fn presence(&mut self, st: &EditorState) {
        let recent = |f: u64| self.frame.saturating_sub(f) <= 2;
        if !recent(self.cursor_frame) {
            self.local.cursor = None;
        }
        if !recent(self.caret_frame) {
            self.local.caret = None;
            self.local.script.clear();
        }
        self.local.bp = st.blueprint_res.clone().unwrap_or_default();
        self.local.mode3d = st.mode_3d;
        if self.presence_at.elapsed() <= Duration::from_millis(66) {
            return;
        }
        // Paths, the same in every copy of the blueprint (ids aren't).
        let tree = &st.tree;
        self.local.sel = st.selection.iter().chain(st.selected.iter()).filter(|i| tree.contains(**i)).map(|i| tree.path_of(*i)).collect::<std::collections::BTreeSet<_>>().into_iter().collect();

        if self.sent.as_ref() != Some(&self.local) {
            self.presence_at = Instant::now();
            self.sent = Some(self.local.clone());
            let m = M::Presence { id: self.me, p: self.local.clone() };
            self.send_all(&m);
        }
    }

    /// What someone is doing, in words.
    pub fn doing(p: &Presence) -> String {
        if !p.script.is_empty() {
            return format!("typing in {}", p.script.trim_start_matches("res://"));
        }
        if p.bp.is_empty() {
            "looking around".into()
        } else {
            format!("in {}", p.bp.trim_start_matches("res://"))
        }
    }
}

// ------------------------------------------------------------------ drawing others

/// Small squares in their colors on a tab others are in.
pub fn tab_marks(ui: &egui::Ui, st: &EditorState, res: &str, rect: egui::Rect) {
    let Some(s) = &st.collab else { return };
    let mut x = rect.right() - 6.0;
    for p in &s.people {
        if p.p.bp == res || p.p.script == res {
            let r = egui::Rect::from_min_size(egui::pos2(x - 5.0, rect.top() + 2.0), egui::vec2(5.0, 5.0));
            ui.painter().rect_filled(r, 0.0, color(p.id));
            x -= 7.0;
        }
    }
}

fn label(painter: &egui::Painter, at: egui::Pos2, text: &str, c: Color32) {
    let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(11.0), Color32::BLACK);
    let r = egui::Rect::from_min_size(at, galley.size() + egui::vec2(6.0, 2.0));
    painter.rect_filled(r, 0.0, c);
    painter.galley(at + egui::vec2(3.0, 1.0), galley, Color32::BLACK);
}

/// Others' mice and selections in the blueprint view; and this person's mouse, for them.
pub fn viewport(painter: &egui::Painter, st: &mut EditorState, to_screen: &dyn Fn(gex_core::glam::Vec3) -> Option<egui::Pos2>, pointer: Option<gex_core::glam::Vec3>) {
    let Some(s) = &mut st.collab else { return };
    if pointer.is_some() {
        s.local.cursor = pointer.map(|p| p.to_array());
        s.cursor_frame = s.frame;
    }
    let res = st.blueprint_res.clone().unwrap_or_default();
    let Some(s) = &st.collab else { return };
    for p in &s.people {
        if p.p.bp != res {
            continue;
        }
        let c = color(p.id);
        // What they've selected: a box around each, in their color.
        if s.ids.0 == res {
            for k in &p.p.sel {
                let Some(id) = s.ids.1.get(k).copied().filter(|i| st.tree.contains(*i)) else { continue };
                let n = &st.tree.objects[id];
                let m = st.tree.global_matrix(id);
                let centre = m.w_axis.truncate();
                if st.mode_3d || !matches!(n.kind, ObjectKind::Sprite2D | ObjectKind::Collider2D | ObjectKind::Label | ObjectKind::Button | ObjectKind::Panel) {
                    if let Some(sp) = to_screen(centre) {
                        painter.rect_stroke(egui::Rect::from_center_size(sp, egui::vec2(14.0, 14.0)), 0.0, egui::Stroke::new(1.5, c), egui::StrokeKind::Outside);
                    }
                    continue;
                }
                let (w, h) = (n.num("width", 32.0) as f32, n.num("height", 32.0) as f32);
                let centered = n.kind == ObjectKind::Collider2D || n.prop("centered").is_none_or(|v| v.as_bool());
                let (x0, y0) = if centered { (-w / 2.0, -h / 2.0) } else { (0.0, 0.0) };
                let pts: Vec<egui::Pos2> = [(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)].iter().filter_map(|(x, y)| to_screen(m.transform_point3(gex_core::glam::Vec3::new(*x, *y, 0.0)))).collect();
                if pts.len() == 4 {
                    painter.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.5, c)));
                }
            }
        }
        // Their mouse: an arrow and their name.
        if let (Some(w), true) = (p.p.cursor, p.p.mode3d == st.mode_3d) {
            if let Some(sp) = to_screen(gex_core::glam::Vec3::from_array(w)) {
                if painter.clip_rect().contains(sp) {
                    let arrow = vec![sp, sp + egui::vec2(0.0, 14.0), sp + egui::vec2(4.0, 10.5), sp + egui::vec2(9.5, 10.0)];
                    painter.add(egui::Shape::convex_polygon(arrow, c, egui::Stroke::new(1.0, Color32::BLACK)));
                    label(painter, sp + egui::vec2(10.0, 12.0), &p.name, c);
                }
            }
        }
    }
}

/// Others' text cursors in a script: a bar in their color with their name.
pub fn carets(ui: &egui::Ui, s: Option<&Session>, res: &str, text: &str, galley: &egui::Galley, origin: egui::Pos2) {
    let Some(s) = s else { return };
    let n = text.chars().count();
    for p in &s.people {
        let (true, Some(c)) = (p.p.script == res, p.p.caret) else { continue };
        let cur = egui::text::CCursor::new((c as usize).min(n));
        let r = galley.pos_from_cursor(cur).translate(origin.to_vec2());
        let col = color(p.id);
        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(2.0, r.height())), 0.0, col);
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(ui.clip_rect().left(), r.top()), egui::pos2(ui.clip_rect().right(), r.bottom())), 0.0, theme::alpha(col, 14));
        label(ui.painter(), r.min + egui::vec2(2.0, -12.0), &p.name, col);
    }
}

/// Coloured squares with initials in the menu bar: who's here (hover for what they're doing).
pub fn people_bar(ui: &mut egui::Ui, st: &EditorState) -> bool {
    let Some(s) = &st.collab else { return false };
    let mut clicked = false;
    for p in s.people.iter().rev() {
        let initial: String = p.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
        let r = ui.add(egui::Button::new(RichText::new(initial).color(Color32::BLACK).strong()).fill(color(p.id)).corner_radius(0.0).min_size(egui::vec2(18.0, 18.0)));
        if r.on_hover_text(format!("{}: {}", p.name, Session::doing(&p.p))).clicked() {
            clicked = true;
        }
    }
    clicked
}

// ------------------------------------------------------------------ the window

pub struct Panel {
    pub open: bool,
    pub name: String,
    pub address: String,
    pub folder: String,
    pub port: u16,
    pub message: String,
}

impl Default for Panel {
    fn default() -> Panel {
        Panel { open: false, name: default_name(), address: String::new(), folder: default_folder().to_string_lossy().into_owned(), port: DEFAULT_PORT, message: String::new() }
    }
}

/// File → Work together. Gives back a session being joined (the app opens its project when
/// every file is in).
pub fn window(ctx: &egui::Context, panel: &mut Panel, mut st: Option<&mut EditorState>, joining: &Option<Session>) -> Option<Session> {
    if !panel.open {
        return None;
    }
    let mut out = None;
    let mut open = true;
    egui::Window::new("Work together").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.set_width(420.0);
        if let Some(j) = joining {
            ui.label(&j.status);
            ui.spinner();
            return;
        }
        let in_session = st.as_ref().is_some_and(|s| s.collab.is_some());
        if let (true, Some(st)) = (in_session, st.as_deref_mut()) {
            let mut stop = false;
            {
                let s = st.collab.as_ref().unwrap();
                ui.label(RichText::new(&s.status).color(theme::TEXT));
                if s.host {
                    ui.horizontal(|ui| {
                        ui.label("Others join with");
                        let a = s.address();
                        ui.label(RichText::new(&a).color(theme::ACCENT).monospace());
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(a);
                        }
                    });
                    ui.label(RichText::new("On the same network that's all. Over the internet, forward this TCP port on your router, or all join a free virtual network (ZeroTier, Tailscale) and use its address.").small().color(theme::TEXT_DIM));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.painter().rect_filled(egui::Rect::from_min_size(ui.cursor().min + egui::vec2(0.0, 3.0), egui::vec2(10.0, 10.0)), 0.0, color(s.me.max(1)));
                    ui.add_space(14.0);
                    ui.label(format!("{} (you)", s.name));
                });
                for p in &s.people {
                    ui.horizontal(|ui| {
                        ui.painter().rect_filled(egui::Rect::from_min_size(ui.cursor().min + egui::vec2(0.0, 3.0), egui::vec2(10.0, 10.0)), 0.0, color(p.id));
                        ui.add_space(14.0);
                        ui.label(&p.name);
                        ui.label(RichText::new(Session::doing(&p.p)).small().color(theme::TEXT_DIM));
                    });
                }
                if s.people.is_empty() {
                    ui.label(RichText::new(if s.host { "No one has joined yet." } else { "The host is the only other one here." }).small().color(theme::TEXT_DIM));
                }
                ui.separator();
                if ui.button(if s.host { "Stop sharing" } else { "Leave" }).clicked() {
                    stop = true;
                }
            }
            if stop {
                st.collab = None;
                panel.message = "Stopped.".into();
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Your name");
            ui.add(egui::TextEdit::singleline(&mut panel.name).desired_width(200.0).char_limit(32));
        });
        ui.separator();
        theme::title(ui, "Share this project");
        match st {
            Some(st) => {
                ui.horizontal(|ui| {
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut panel.port).range(1024..=65535));
                    if ui.button("Start sharing").tip("Others can join and work on this project with you. They get a copy of it.").clicked() {
                        match Session::host(ctx, st, &panel.name, panel.port) {
                            Ok(mut s) => {
                                s.name = panel.name.clone();
                                st.collab = Some(s);
                                panel.message.clear();
                            }
                            Err(e) => panel.message = e,
                        }
                    }
                });
            }
            None => {
                ui.label(RichText::new("Open a project to share it.").small().color(theme::TEXT_DIM));
            }
        }
        ui.separator();
        theme::title(ui, "Join someone");
        egui::Grid::new("join").num_columns(2).show(ui, |ui| {
            ui.label("Address");
            ui.add(egui::TextEdit::singleline(&mut panel.address).hint_text("192.168.1.20:7778").desired_width(260.0));
            ui.end_row();
            ui.label("Put it in");
            ui.add(egui::TextEdit::singleline(&mut panel.folder).desired_width(260.0)).tip("The project is copied into a folder here, named after it.");
            ui.end_row();
        });
        if ui.add_enabled(!panel.address.trim().is_empty(), egui::Button::new("Join")).clicked() {
            out = Some(Session::join(ctx, &panel.name, &panel.address, Path::new(panel.folder.trim())));
            panel.message.clear();
        }
        if !panel.message.is_empty() {
            ui.label(RichText::new(&panel.message).color(theme::WARN));
        }
    });
    if !open {
        panel.open = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_edits_cross_over() {
        // Two people type into "hello world" at once: both end up with the same text.
        let base = "hello world".to_string();
        let a = Op { s: 5, e: 5, t: ",".into() };
        let b = Op { s: 11, e: 11, t: "!".into() };
        let (mut x, mut y) = (base.clone(), base.clone());
        apply_op(&mut x, &a);
        apply_op(&mut x, &xf(&b, &a, false));
        apply_op(&mut y, &b);
        apply_op(&mut y, &xf(&a, &b, true));
        assert_eq!(x, "hello, world!");
        assert_eq!(x, y);
        // The same spot: the one ordered first goes first, the same on both sides.
        let a = Op { s: 0, e: 0, t: "A".into() };
        let b = Op { s: 0, e: 0, t: "B".into() };
        let (mut x, mut y) = (String::new(), String::new());
        apply_op(&mut x, &a);
        apply_op(&mut x, &xf(&b, &a, false));
        apply_op(&mut y, &b);
        apply_op(&mut y, &xf(&a, &b, true));
        assert_eq!(x, y);
        // A deletion over someone's insert.
        let a = Op { s: 2, e: 8, t: String::new() };
        let b = Op { s: 4, e: 4, t: "XYZ".into() };
        let (mut x, mut y) = (base.clone(), base.clone());
        apply_op(&mut x, &a);
        apply_op(&mut x, &xf(&b, &a, false));
        apply_op(&mut y, &b);
        apply_op(&mut y, &xf(&a, &b, true));
        assert_eq!(x, y);
        // Any two edits, wherever they are: both orders give the same text.
        let mut seed: u64 = 12345;
        let mut rnd = |n: u32| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) % n.max(1) as u64) as u32
        };
        for _ in 0..20000 {
            let base: String = (0..rnd(12)).map(|i| (b'a' + (i as u8 % 26)) as char).collect();
            let n = char_len(&base);
            let mut op = |tag: char| {
                let s = rnd(n + 1);
                let e = s + rnd(n - s + 1);
                let t: String = std::iter::repeat_n(tag, rnd(3) as usize).collect();
                Op { s, e, t }
            };
            let (a, b) = (op('A'), op('B'));
            let (mut x, mut y) = (base.clone(), base.clone());
            apply_op(&mut x, &a);
            apply_op(&mut x, &xf(&b, &a, false));
            apply_op(&mut y, &b);
            apply_op(&mut y, &xf(&a, &b, true));
            assert_eq!(x, y, "{:?} {:?} on {:?}", a, b, base);
        }
        assert_eq!(text_diff("abcdef", "abXYef"), Some(Op { s: 2, e: 4, t: "XY".into() }));
        assert_eq!(text_diff("aaa", "aaaa"), Some(Op { s: 3, e: 3, t: "a".into() }));
        let mut t = "héllo".to_string();
        apply_op(&mut t, &Op { s: 1, e: 2, t: "e".into() });
        assert_eq!(t, "hello");
    }

    #[test]
    fn blueprints_follow_object_changes() {
        let mut a = BlueprintTree::new();
        let root = a.add(None, Object::new("Root", ObjectKind::Object2D));
        let p = a.add(Some(root), Object::new("Player", ObjectKind::Sprite2D));
        a.add(Some(p), Object::new("Gun", ObjectKind::Sprite2D));
        a.add(Some(root), Object::new("Wall", ObjectKind::Sprite2D));
        let mut b = a.clone();
        let before = flat(&a);
        // Move one, rename one, add one, remove one.
        a.objects[p].transform.position.x = 50.0;
        let wall = a.find_path(None, "Root/Wall").unwrap();
        a.objects[wall].name = "Fence".into();
        a.add(Some(p), Object::new("Hat", ObjectKind::Sprite2D));
        let gun = a.find_path(None, "Root/Player/Gun").unwrap();
        a.remove(gun);
        let ch = diff(&before, &flat(&a));
        apply_changes(&mut b, &ch);
        assert_eq!(flat(&a), flat(&b));
        assert!(diff(&flat(&a), &flat(&b)).is_empty());
    }

    #[test]
    fn paths_from_others_stay_in_the_project() {
        let d = Path::new("/p");
        assert!(safe_path(d, "scripts/a.jlb").is_some());
        assert!(safe_path(d, "../x").is_none());
        assert!(safe_path(d, "/etc/passwd").is_none());
        assert!(safe_path(d, "C:/x").is_none());
        assert!(safe_path(d, "a/../../x").is_none());
        assert!(safe_path(d, ".gex/layout.ron").is_none());
    }
    /// Two editors on this computer: one shares the demo project, the other joins and gets a
    /// copy; then objects, typing (both at once), files and mouse pointers cross over.
    #[test]
    fn two_editors_work_together() {
        let tmp = std::env::temp_dir().join(format!("gex-collab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let a_dir = tmp.join("a");
        crate::templates::create_demo(&a_dir).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let ctx = egui::Context::default();
        let mut a = EditorState::new(gex_core::project::Project::open(&a_dir).unwrap());
        a.collab = Some(Session::host(&ctx, &a, "Ann", port).unwrap());
        let mut j = Session::join(&ctx, "Bob", &format!("localhost:{}", port), &tmp.join("b"));
        let t0 = Instant::now();
        while !j.ready {
            assert!(t0.elapsed() < Duration::from_secs(20), "joining took too long: {}", j.status);
            assert!(j.ended.is_none(), "{:?}", j.ended);
            tick(&ctx, &mut a);
            tick_joining(&ctx, &mut j);
            std::thread::sleep(Duration::from_millis(2));
        }
        // Every file arrived.
        for (rel, _, _) in walk(&a_dir) {
            assert_eq!(std::fs::read(a_dir.join(&rel)).unwrap(), std::fs::read(j.dir.join(&rel)).unwrap_or_default(), "{}", rel);
        }
        let mut b = EditorState::new(gex_core::project::Project::open(&j.dir).unwrap());
        j.opened = true;
        b.collab = Some(j);
        let run = |a: &mut EditorState, b: &mut EditorState, ms: u64| {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(ms) {
                tick(&ctx, a);
                tick(&ctx, b);
                std::thread::sleep(Duration::from_millis(3));
            }
        };
        run(&mut a, &mut b, 400);
        assert_eq!(a.blueprint_res, b.blueprint_res);
        assert!(b.collab.as_ref().unwrap().people.iter().any(|p| p.name == "Ann"));
        assert!(a.collab.as_ref().unwrap().people.iter().any(|p| p.name == "Bob"));
        // Ann moves one object while Bob adds one: both see both.
        let root = a.tree.root.unwrap();
        let first = a.tree.objects[root].children[0];
        a.tree.objects[first].transform.position.x = 1234.0;
        let broot = b.tree.root.unwrap();
        b.tree.add(Some(broot), Object::new("FromBob", ObjectKind::Sprite2D));
        run(&mut a, &mut b, 600);
        assert_eq!(flat(&a.tree), flat(&b.tree), "the blueprints match");
        assert!(a.tree.find_named("FromBob").is_some());
        let bf = b.tree.find_path(None, &a.tree.path_of(first)).unwrap();
        assert_eq!(b.tree.objects[bf].transform.position.x, 1234.0);
        // Both type into the same script at once.
        let res = "res://scripts/shared_test.jlb";
        std::fs::write(a_dir.join("scripts/shared_test.jlb"), "hello world\n").unwrap();
        run(&mut a, &mut b, 2200);
        assert!(j_has(&b, "scripts/shared_test.jlb"), "a new file arrives");
        a.open_script(res, None);
        b.open_script(res, None);
        run(&mut a, &mut b, 400);
        a.scripts.get_mut(res).unwrap().text = "hello, world\n".into();
        b.scripts.get_mut(res).unwrap().text = "hello world!\n".into();
        run(&mut a, &mut b, 600);
        assert_eq!(a.scripts[res].text, "hello, world!\n");
        assert_eq!(b.scripts[res].text, a.scripts[res].text);
        // Quick typing on both sides, letter by letter.
        for i in 0..20 {
            a.scripts.get_mut(res).unwrap().text.insert(0, 'a');
            let bt = &mut b.scripts.get_mut(res).unwrap().text;
            let at = bt.len() - 1;
            bt.insert(at, if i % 2 == 0 { 'x' } else { 'y' });
            tick(&ctx, &mut a);
            tick(&ctx, &mut b);
        }
        run(&mut a, &mut b, 600);
        assert_eq!(a.scripts[res].text, b.scripts[res].text);
        assert!(a.scripts[res].text.starts_with(&"a".repeat(20)) && a.scripts[res].text.ends_with("xyxyxyxyxyxyxyxyxyxy\n"), "{:?}", a.scripts[res].text);
        // Bob's mouse in the view, as Ann sees it.
        crate::collab::viewport(&egui::Painter::new(ctx.clone(), egui::LayerId::background(), egui::Rect::EVERYTHING), &mut b, &|_| None, Some(gex_core::glam::Vec3::new(5.0, 6.0, 0.0)));
        run(&mut a, &mut b, 300);
        let bob = a.collab.as_ref().unwrap().people.iter().find(|p| p.name == "Bob").unwrap().clone();
        assert_eq!(bob.p.cursor, Some([5.0, 6.0, 0.0]));
        // Ann stops: Bob is told, and keeps his copy.
        a.collab = None;
        run(&mut a, &mut b, 300);
        assert!(b.collab.as_ref().unwrap().ended.is_some());
        // The port is free again: sharing can start over.
        assert!(Session::host(&ctx, &a, "Ann", port).is_ok());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn j_has(st: &EditorState, rel: &str) -> bool {
        st.project.dir.join(rel).is_file()
    }
}
