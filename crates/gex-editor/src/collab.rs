//! Working on one project together, live. One person shares it (File → Work together) and
//! gives the others the address and invite code; they get a copy of the project, and then
//!
//! - blueprints stay in step property by property (two people changing different properties
//!   of one object both keep their change; the same property, the last change wins), and
//!   objects stay the same objects through renames and moves,
//! - scripts stay in step letter by letter, even with two people typing in one place,
//! - files saved, added, renamed or deleted on one computer change on everyone's,
//! - everyone sees the others: their tab, mouse, selection and text cursor; they can chat,
//!   follow someone around, and play the game together.
//!
//! Security: after the first hello everything is encrypted and authenticated with
//! ChaCha20-Poly1305 under keys derived (HKDF-SHA256) from the invite code and both sides'
//! random nonces, so only people with the code get in, nobody on the network can read or change
//! the traffic, and messages can't be replayed. Codes are 80 random bits; a computer that gets
//! one wrong five times is shut out for the session. The host can make people view-only or
//! remove them. Paths from others are checked to stay inside the project.
//!
//! Everything goes through the host. Connections sleep on threads of their own and wake the
//! window when something arrives (gex_script::wire::Link), so a quiet session costs nothing.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use eframe::egui::{self, Color32, RichText};
use gex_core::{BlueprintTree, Object, ObjectId, ObjectKind, Variant};
use gex_script::wire::{Frame, Link, Listener, Waker};
use serde::{Deserialize, Serialize};

use crate::docs::Tip as _;
use crate::state::EditorState;
use crate::theme;

pub const DEFAULT_PORT: u16 = 7778;
/// Where games played together host.
const GAME_PORT: u16 = 7777;
const PROTOCOL: u32 = 2;
/// Files bigger than this aren't shared.
const MAX_FILE: u64 = 256 << 20;
const MAX_PEOPLE: usize = 16;
/// Wrong invite codes from one computer before it's shut out.
const MAX_FAILS: u32 = 5;
/// Joined: how long to keep trying to get back in after the connection drops.
const RECONNECT_FOR: Duration = Duration::from_secs(60);

// ------------------------------------------------------------------ security

/// One direction of an encrypted connection: its key and message counter (the nonce; a message
/// replayed or reordered fails to open).
struct Cipher {
    aead: ChaCha20Poly1305,
    n: u64,
}

impl Cipher {
    fn new(key: &[u8]) -> Cipher {
        Cipher { aead: ChaCha20Poly1305::new(Key::from_slice(key)), n: 0 }
    }
    fn nonce(&mut self) -> Nonce {
        let mut b = [0u8; 12];
        b[4..].copy_from_slice(&self.n.to_le_bytes());
        self.n += 1;
        *Nonce::from_slice(&b)
    }
    fn seal(&mut self, kind: u8, body: &[u8]) -> Vec<u8> {
        let mut plain = Vec::with_capacity(body.len() + 1);
        plain.push(kind);
        plain.extend_from_slice(body);
        let n = self.nonce();
        self.aead.encrypt(&n, plain.as_slice()).unwrap_or_default()
    }
    fn open(&mut self, data: &[u8]) -> Option<(u8, Vec<u8>)> {
        let n = self.nonce();
        let mut p = self.aead.decrypt(&n, data).ok()?;
        if p.is_empty() {
            return None;
        }
        let kind = p.remove(0);
        Some((kind, p))
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    // Without system randomness keys would be guessable: better to stop.
    getrandom::getrandom(&mut b).expect("no secure random numbers on this system");
    b
}

const CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A new invite code: 80 random bits as 16 characters, XXXX-XXXX-XXXX-XXXX.
fn new_code() -> String {
    let b: [u8; 10] = random();
    let mut bits: u128 = 0;
    for x in b {
        bits = (bits << 8) | x as u128;
    }
    let chars: Vec<char> = (0..16).map(|i| CODE_ALPHABET[((bits >> (75 - i * 5)) & 31) as usize] as char).collect();
    chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}

/// A code as typed: case, dashes and spaces don't matter, and O/I/L read as 0/1/1.
fn normal_code(c: &str) -> String {
    c.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            x => x,
        })
        .collect()
}

/// (joiner → host key, host → joiner key) from the code and both sides' nonces.
fn keys(code: &str, joiner: &[u8], host: &[u8]) -> ([u8; 32], [u8; 32]) {
    let mut salt = joiner.to_vec();
    salt.extend_from_slice(host);
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(&salt), normal_code(code).as_bytes());
    let mut okm = [0u8; 64];
    hk.expand(b"gex work together v2", &mut okm).expect("64 bytes is a valid length");
    let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
    a.copy_from_slice(&okm[..32]);
    b.copy_from_slice(&okm[32..]);
    (a, b)
}

// ------------------------------------------------------------------ messages

/// Before the keys: in the clear, and only these.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t")]
enum Hi {
    Hi { v: u32, nonce: Vec<u8> },
    Challenge { nonce: Vec<u8> },
    Refused { reason: String },
}

/// Everything else, encrypted.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "t")]
enum M {
    Hello { name: String },
    Refused { reason: String },
    Welcome { id: u32, project: String, people: Vec<Person>, files: Vec<(String, u64, u64)>, token: String },
    Need { paths: Vec<String> },
    /// A file; its bytes come in the next message.
    File { path: String },
    Gone { path: String },
    Synced,
    Joined { person: Person },
    Left { id: u32 },
    Presence { id: u32, p: Presence },
    Objects { res: String, from: u32, patches: Vec<Patch> },
    OpenBp { res: String },
    BpState { res: String, patches: Vec<Patch> },
    Edit { res: String, from: u32, rev: u64, op: Op },
    OpenScript { res: String },
    ScriptState { res: String, rev: u64, text: String },
    Chat { from: u32, text: String },
    Role { id: u32, editor: bool },
    Kicked,
    Play { bp: String, port: u16 },
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Presence {
    /// The blueprint they have open.
    pub bp: String,
    /// The script they're typing in, and where (a character index).
    pub script: String,
    pub caret: Option<u32>,
    /// Their mouse in the blueprint view (a world position) and whether that view is 3D.
    pub cursor: Option<[f32; 3]>,
    pub mode3d: bool,
    /// What they've selected (object uids).
    pub sel: Vec<u64>,
    /// Their view: 2D centre and zoom; 3D target, yaw, pitch, distance.
    pub cam2: [f32; 3],
    pub cam3: [f32; 6],
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Person {
    pub id: u32,
    pub name: String,
    pub editor: bool,
    pub p: Presence,
}

// ------------------------------------------------------------------ blueprints, property by property

/// One object as shared: where it is in the tree, and its own data (not its children).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Entry {
    parent: u64,
    depth: u32,
    /// Its children, in order.
    kids: Vec<u64>,
    name: String,
    kind: ObjectKind,
    pos: [f32; 3],
    rot: [f32; 3],
    scale: [f32; 3],
    visible: bool,
    props: BTreeMap<String, Variant>,
    scripts: Vec<String>,
    groups: Vec<String>,
    inst: (Option<String>, bool),
}

impl Entry {
    fn blank(kind: ObjectKind) -> Entry {
        Entry { parent: 0, depth: 0, kids: Vec::new(), name: String::new(), kind, pos: [0.0; 3], rot: [0.0; 3], scale: [1.0; 3], visible: true, props: BTreeMap::new(), scripts: Vec::new(), groups: Vec::new(), inst: (None, false) }
    }
}

type Flat = HashMap<u64, Entry>;

/// What changed about one object: only the parts that changed (None = unchanged). A new
/// object has every part; `gone` removes it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct Patch {
    uid: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    gone: bool,
    /// Its parent and depth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    place: Option<(u64, u32)>,
    /// Its children's order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kids: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<ObjectKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pos: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rot: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
    /// Properties set (Some) or removed (None).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    props: Vec<(String, Option<Variant>)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scripts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    groups: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inst: Option<(Option<String>, bool)>,
}

impl Patch {
    fn full(uid: u64, e: &Entry) -> Patch {
        Patch {
            uid,
            gone: false,
            place: Some((e.parent, e.depth)),
            kids: Some(e.kids.clone()),
            name: Some(e.name.clone()),
            kind: Some(e.kind),
            pos: Some(e.pos),
            rot: Some(e.rot),
            scale: Some(e.scale),
            visible: Some(e.visible),
            props: e.props.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect(),
            scripts: Some(e.scripts.clone()),
            groups: Some(e.groups.clone()),
            inst: Some(e.inst.clone()),
        }
    }

    fn between(uid: u64, old: Option<&Entry>, new: &Entry) -> Option<Patch> {
        let Some(o) = old else { return Some(Patch::full(uid, new)) };
        if o == new {
            return None;
        }
        let mut p = Patch { uid, ..Default::default() };
        if o.parent != new.parent {
            p.place = Some((new.parent, new.depth));
        }
        if o.kids != new.kids {
            p.kids = Some(new.kids.clone());
        }
        if o.name != new.name {
            p.name = Some(new.name.clone());
        }
        if o.kind != new.kind {
            p.kind = Some(new.kind);
        }
        if o.pos != new.pos {
            p.pos = Some(new.pos);
        }
        if o.rot != new.rot {
            p.rot = Some(new.rot);
        }
        if o.scale != new.scale {
            p.scale = Some(new.scale);
        }
        if o.visible != new.visible {
            p.visible = Some(new.visible);
        }
        if o.props != new.props {
            for (k, v) in &new.props {
                if o.props.get(k) != Some(v) {
                    p.props.push((k.clone(), Some(v.clone())));
                }
            }
            for k in o.props.keys().filter(|k| !new.props.contains_key(*k)) {
                p.props.push((k.clone(), None));
            }
        }
        if o.scripts != new.scripts {
            p.scripts = Some(new.scripts.clone());
        }
        if o.groups != new.groups {
            p.groups = Some(new.groups.clone());
        }
        if o.inst != new.inst {
            p.inst = Some(new.inst.clone());
        }
        Some(p)
    }

    /// The parts it changes, by name (to know which incoming changes are older than ours).
    fn fields(&self) -> Vec<String> {
        let mut f = Vec::new();
        for (on, n) in [
            (self.place.is_some(), "place"),
            (self.kids.is_some(), "kids"),
            (self.name.is_some(), "name"),
            (self.kind.is_some(), "kind"),
            (self.pos.is_some(), "pos"),
            (self.rot.is_some(), "rot"),
            (self.scale.is_some(), "scale"),
            (self.visible.is_some(), "visible"),
            (self.scripts.is_some(), "scripts"),
            (self.groups.is_some(), "groups"),
            (self.inst.is_some(), "inst"),
        ] {
            if on {
                f.push(n.to_string());
            }
        }
        f.extend(self.props.iter().map(|(k, _)| format!("p:{}", k)));
        f
    }

    /// Without the parts `skip` names.
    fn without(mut self, skip: &dyn Fn(&str) -> bool) -> Patch {
        if skip("place") {
            self.place = None;
        }
        if skip("kids") {
            self.kids = None;
        }
        if skip("name") {
            self.name = None;
        }
        if skip("kind") {
            self.kind = None;
        }
        if skip("pos") {
            self.pos = None;
        }
        if skip("rot") {
            self.rot = None;
        }
        if skip("scale") {
            self.scale = None;
        }
        if skip("visible") {
            self.visible = None;
        }
        if skip("scripts") {
            self.scripts = None;
        }
        if skip("groups") {
            self.groups = None;
        }
        if skip("inst") {
            self.inst = None;
        }
        self.props.retain(|(k, _)| !skip(&format!("p:{}", k)));
        self
    }

    fn is_new(&self) -> bool {
        self.place.is_some() && self.name.is_some() && self.kind.is_some()
    }

    fn is_empty(&self) -> bool {
        !self.gone && self.fields().is_empty()
    }

    fn to_entry(&self, e: &mut Entry) {
        if let Some((p, d)) = self.place {
            (e.parent, e.depth) = (p, d);
        }
        if let Some(k) = &self.kids {
            e.kids = k.clone();
        }
        if let Some(v) = &self.name {
            e.name = v.clone();
        }
        if let Some(v) = self.kind {
            e.kind = v;
        }
        if let Some(v) = self.pos {
            e.pos = v;
        }
        if let Some(v) = self.rot {
            e.rot = v;
        }
        if let Some(v) = self.scale {
            e.scale = v;
        }
        if let Some(v) = self.visible {
            e.visible = v;
        }
        for (k, v) in &self.props {
            match v {
                Some(v) => e.props.insert(k.clone(), v.clone()),
                None => e.props.remove(k),
            };
        }
        if let Some(v) = &self.scripts {
            e.scripts = v.clone();
        }
        if let Some(v) = &self.groups {
            e.groups = v.clone();
        }
        if let Some(v) = &self.inst {
            e.inst = v.clone();
        }
    }

    fn to_object(&self, o: &mut Object) {
        if let Some(v) = self.pos {
            o.transform.position = v.into();
        }
        if let Some(v) = self.rot {
            o.transform.rotation = v.into();
        }
        if let Some(v) = self.scale {
            o.transform.scale = v.into();
        }
        if let Some(v) = &self.name {
            o.name = v.clone();
        }
        if let Some(v) = self.kind {
            o.kind = v;
        }
        if let Some(v) = self.visible {
            o.visible = v;
        }
        for (k, v) in &self.props {
            match v {
                Some(v) => o.props.insert(k.clone(), v.clone()),
                None => o.props.remove(k),
            };
        }
        if let Some(v) = &self.scripts {
            o.scripts = v.clone();
        }
        if let Some(v) = &self.groups {
            o.groups = v.clone();
        }
        if let Some((i, f)) = &self.inst {
            o.instance_of = i.clone();
            o.from_instance = *f;
        }
    }
}

/// Copies (duplicate, paste) start with the uid of what they copied: later ones get their own,
/// the same way on every computer.
fn fix_uids(tree: &mut BlueprintTree) {
    let mut seen = HashSet::with_capacity(tree.objects.len());
    for id in tree.walk() {
        let mut u = tree.objects[id].uid;
        if u == 0 {
            u = gex_core::object::path_uid(&tree.path_of(id));
        }
        let mut k = 0u64;
        while !seen.insert(u) {
            k += 1;
            u = gex_core::object::splitmix(u.wrapping_add(k)) | 1;
        }
        tree.objects[id].uid = u;
    }
}

fn entry_of(tree: &BlueprintTree, id: ObjectId, depth: u32) -> Entry {
    let o = &tree.objects[id];
    let parent = o.parent.map(|p| tree.objects[p].uid).unwrap_or(0);
    let t = &o.transform;
    Entry {
        parent,
        depth,
        kids: o.children.iter().map(|c| tree.objects[*c].uid).collect(),
        name: o.name.clone(),
        kind: o.kind,
        pos: t.position.into(),
        rot: t.rotation.into(),
        scale: t.scale.into(),
        visible: o.visible,
        props: o.props.clone(),
        scripts: o.scripts.clone(),
        groups: o.groups.clone(),
        inst: (o.instance_of.clone(), o.from_instance),
    }
}

fn flat(tree: &BlueprintTree) -> Flat {
    let mut out = HashMap::with_capacity(tree.objects.len());
    let Some(root) = tree.root else { return out };
    let mut stack = vec![(root, 0u32)];
    while let Some((id, d)) = stack.pop() {
        out.insert(tree.objects[id].uid, entry_of(tree, id, d));
        for c in tree.objects[id].children.iter().rev() {
            stack.push((*c, d + 1));
        }
    }
    out
}

fn diff(old: &Flat, new: &Flat) -> Vec<Patch> {
    let mut out: Vec<Patch> = new.iter().filter_map(|(u, e)| Patch::between(*u, old.get(u), e)).collect();
    out.extend(old.keys().filter(|u| !new.contains_key(*u)).map(|u| Patch { uid: *u, gone: true, ..Default::default() }));
    out
}

/// A patch on the shared state; false when it can't apply (a change to an object that's gone,
/// or a new one whose parent is).
fn patch_flat(f: &mut Flat, p: &Patch) -> bool {
    if p.gone {
        return f.remove(&p.uid).is_some();
    }
    if let Some(e) = f.get_mut(&p.uid) {
        p.to_entry(e);
        return true;
    }
    let Some((parent, _)) = p.place else { return false };
    if !p.is_new() || (parent != 0 && !f.contains_key(&parent)) {
        return false;
    }
    let mut e = Entry::blank(p.kind.unwrap());
    p.to_entry(&mut e);
    f.insert(p.uid, e);
    true
}

/// Makes a tree follow patches: removals, then additions and moves, parents first.
fn patch_tree(tree: &mut BlueprintTree, patches: &[Patch]) {
    if patches.is_empty() {
        return;
    }
    let mut map: HashMap<u64, ObjectId> = tree.objects.iter().map(|(id, o)| (o.uid, id)).collect();
    for p in patches.iter().filter(|p| p.gone) {
        if let Some(id) = map.get(&p.uid).copied().filter(|i| tree.contains(*i) && Some(*i) != tree.root) {
            tree.remove(id);
        }
    }
    let mut up: Vec<&Patch> = patches.iter().filter(|p| !p.gone).collect();
    up.sort_by_key(|p| p.place.map(|(_, d)| d).unwrap_or(0));
    for p in &up {
        let id = match map.get(&p.uid).copied().filter(|i| tree.contains(*i)) {
            Some(id) => id,
            None if !p.is_new() => continue,
            None => {
                let (parent, _) = p.place.unwrap();
                let (name, kind) = (p.name.clone().unwrap(), p.kind.unwrap());
                let id = if parent == 0 {
                    match tree.root {
                        Some(r) => r,
                        None => tree.add(None, Object::new(name, kind)),
                    }
                } else {
                    let Some(par) = map.get(&parent).copied().filter(|i| tree.contains(*i)) else { continue };
                    tree.add(Some(par), Object::new(name, kind))
                };
                tree.objects[id].uid = p.uid;
                map.insert(p.uid, id);
                id
            }
        };
        // Moved under another parent (where among its children comes from the parent's order).
        if let Some((parent, _)) = p.place {
            if let Some(par) = map.get(&parent).copied().filter(|i| parent != 0 && tree.contains(*i)) {
                if tree.objects[id].parent != Some(par) {
                    let name = tree.objects[id].name.clone();
                    tree.reparent(id, par, None);
                    tree.objects[id].name = name;
                }
            }
        }
        p.to_object(&mut tree.objects[id]);
    }
    // Children's order: the ones listed in the listed order, then any others as they were.
    for p in up {
        let (Some(kids), Some(id)) = (&p.kids, map.get(&p.uid).copied().filter(|i| tree.contains(*i))) else { continue };
        let now = tree.objects[id].children.clone();
        let mut order: Vec<ObjectId> = kids.iter().filter_map(|u| map.get(u).copied()).filter(|c| now.contains(c)).collect();
        let listed: HashSet<ObjectId> = order.iter().copied().collect();
        order.extend(now.iter().copied().filter(|c| !listed.contains(c)));
        tree.objects[id].children = order;
    }
}

/// The shared state's objects by path (to recognise objects loaded from disk on both sides).
fn paths(f: &Flat) -> HashMap<String, u64> {
    fn path(u: u64, f: &Flat, memo: &mut HashMap<u64, String>, depth: u32) -> String {
        if let Some(p) = memo.get(&u) {
            return p.clone();
        }
        let Some(e) = f.get(&u) else { return String::new() };
        let p = if e.parent == 0 || depth > 512 { e.name.clone() } else { format!("{}/{}", path(e.parent, f, memo, depth + 1), e.name) };
        memo.insert(u, p.clone());
        p
    }
    let mut memo = HashMap::with_capacity(f.len());
    f.keys().map(|u| (path(*u, f, &mut memo, 0), *u)).collect()
}

/// Each listed object's children's order, as it is in the tree.
fn kids_now(tree: &BlueprintTree, uids: &HashSet<u64>) -> Vec<(u64, Vec<u64>)> {
    tree.objects.iter().filter(|(_, o)| uids.contains(&o.uid)).map(|(_, o)| (o.uid, o.children.iter().map(|c| tree.objects[*c].uid).collect())).collect()
}

/// The objects whose children a set of patches may have changed: themselves, their parents
/// (new, moved), and the parents of removed ones.
fn touched_parents(patches: &[Patch], flat: &Flat) -> HashSet<u64> {
    let mut out = HashSet::new();
    for p in patches {
        if p.kids.is_some() {
            out.insert(p.uid);
        }
        if let Some((par, _)) = p.place {
            out.insert(par);
        }
        if let Some(e) = flat.get(&p.uid) {
            out.insert(e.parent);
        }
    }
    out.remove(&0);
    out
}

struct Bp {
    /// The shared state, as last sent or received.
    flat: Flat,
    /// Joined: parts of objects changed here that the host hasn't passed back yet (others'
    /// changes to them are older, and are skipped).
    pending: HashMap<(u64, String), u32>,
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

impl Doc {
    fn new(text: String, ready: bool) -> Doc {
        Doc { rev: 0, text, log: VecDeque::new(), inflight: None, ready }
    }
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

/// A path someone sent, made safe: inside the project, no "..", no drive letters, nothing hidden.
fn safe_path(dir: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.len() > 1024 || rel.starts_with('/') || rel.starts_with('\\') || rel.contains(':') {
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

/// A res:// path someone sent that stays inside the project.
fn safe_res(res: &str) -> bool {
    res.strip_prefix("res://").is_some_and(|r| safe_path(Path::new("/"), r).is_some())
}

// ------------------------------------------------------------------ the session

#[derive(PartialEq)]
enum Stage {
    /// Hosting: waiting for their hello.
    Hi,
    /// Keys made; waiting for their first encrypted message.
    Keyed,
    Ready,
}

struct Peer {
    id: u32,
    link: Link,
    ip: Option<IpAddr>,
    stage: Stage,
    out: Option<Cipher>,
    inn: Option<Cipher>,
    /// Joining: our nonce, until the host's arrives.
    nonce: Vec<u8>,
    /// Files still to send them (joining).
    queue: VecDeque<String>,
    synced_sent: bool,
    asked: bool,
    /// A file whose bytes come next.
    incoming: Option<String>,
    editor: bool,
}

impl Peer {
    fn new(link: Link) -> Peer {
        let ip = link.peer_ip();
        Peer { id: 0, link, ip, stage: Stage::Hi, out: None, inn: None, nonce: Vec::new(), queue: VecDeque::new(), synced_sent: false, asked: false, incoming: None, editor: true }
    }
    fn hi(&mut self, m: &Hi) {
        if let Ok(b) = serde_json::to_vec(m) {
            self.link.send_json(&b);
        }
    }
    fn send(&mut self, m: &M) {
        if let (Ok(b), Some(c)) = (serde_json::to_vec(m), self.out.as_mut()) {
            let sealed = c.seal(0, &b);
            self.link.send_bytes(&sealed);
        }
    }
    fn send_file(&mut self, path: &str, bytes: &[u8]) {
        self.send(&M::File { path: path.to_string() });
        if let Some(c) = self.out.as_mut() {
            let sealed = c.seal(1, bytes);
            self.link.send_bytes(&sealed);
        }
    }
}

pub struct ChatLine {
    /// 0 for notes like "Bob joined".
    pub from: u32,
    pub name: String,
    pub text: String,
}

pub struct Session {
    pub me: u32,
    pub host: bool,
    pub name: String,
    /// Everyone else.
    pub people: Vec<Person>,
    pub status: String,
    /// Hosting: the invite code; joining: the code given.
    pub code: String,
    /// This person may change things (the host decides).
    pub editor: bool,
    listener: Option<Listener>,
    pub port: u16,
    waker: Waker,
    peers: Vec<Peer>,
    connecting: Option<Receiver<Result<TcpStream, String>>>,
    /// Joined: where the host is, to get back in if the connection drops.
    address: (String, u16),
    /// Getting back in: (give up at, next try at).
    reconnect: Option<(Instant, Instant)>,
    next_id: u32,
    fails: HashMap<IpAddr, u32>,
    banned: HashSet<IpAddr>,
    /// The project folder (joining: where the copy goes).
    pub dir: PathBuf,
    base_dir: PathBuf,
    pub project: String,
    /// Joining: every file has arrived; the project can open.
    pub ready: bool,
    pub opened: bool,
    /// Joining: bytes to get and got.
    pub total: u64,
    pub got: u64,
    /// Hosting: a mark in the project's .gex folder, so joining from this same computer doesn't
    /// copy the project onto itself.
    token: String,
    files: HashMap<String, FileInfo>,
    scan_at: Instant,
    bps: HashMap<String, Bp>,
    scripts: HashMap<String, Doc>,
    open_scripts: HashSet<String>,
    open_bps: HashSet<String>,
    objects_at: Instant,
    others_at: Instant,
    pub local: Presence,
    sent: Option<Presence>,
    presence_at: Instant,
    pub cursor_frame: u64,
    pub caret_frame: u64,
    pub frame: u64,
    /// The session ended (the host left, a wrong code, removed): why.
    pub ended: Option<String>,
    /// Uids to object ids in the blueprint open here, for drawing others' selections.
    ids: (String, HashMap<u64, ObjectId>),
    busy: bool,
    /// Hosting: this computer's addresses, labelled by network (ZeroTier first).
    pub addrs: Vec<(String, &'static str)>,
    pub chat: Vec<ChatLine>,
    pub unread: usize,
    /// Following someone: their blueprint, view and script.
    pub following: Option<u32>,
    followed_script: String,
    /// The host started playing together: (blueprint, port).
    play: Option<(String, u16)>,
    /// Joined: the host's address as this computer sees it (for playing together).
    host_ip: Option<IpAddr>,
}

pub fn default_name() -> String {
    std::env::var("GEX_NAME").or_else(|_| std::env::var("USERNAME")).or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Someone".into())
}

/// Where joined projects go by default: where new projects are made.
pub fn default_folder() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    std::fs::read_to_string(crate::state::config_dir().join("projects_dir.txt")).ok().map(|s| PathBuf::from(s.trim())).filter(|p| p.is_dir()).unwrap_or_else(|| home.join("GEX Projects"))
}

pub fn color(id: u32) -> Color32 {
    const C: [Color32; 6] = [theme::MK_PINK, theme::MK_GREEN, theme::MK_ORANGE, theme::MK_PURPLE, theme::MK_YELLOW, theme::MK_CYAN];
    C[(id as usize).wrapping_sub(1) % C.len()]
}

fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).take(64).collect();
    let s = s.trim().to_string();
    if s.is_empty() { "project".into() } else { s }
}

fn clean_name(name: &str, id: u32) -> String {
    let n: String = name.chars().filter(|c| !c.is_control()).take(32).collect();
    let n = n.trim().to_string();
    if n.is_empty() { format!("Guest {}", id) } else { n }
}

fn clean_text(t: &str) -> String {
    t.chars().filter(|c| !c.is_control()).take(500).collect()
}

impl Session {
    fn new(name: &str, dir: PathBuf, ctx: &egui::Context) -> Session {
        let now = Instant::now();
        let c = ctx.clone();
        Session {
            me: 0,
            host: false,
            name: clean_name(name, 0),
            people: Vec::new(),
            status: String::new(),
            code: String::new(),
            editor: true,
            listener: None,
            port: DEFAULT_PORT,
            waker: std::sync::Arc::new(move || c.request_repaint()),
            peers: Vec::new(),
            connecting: None,
            address: (String::new(), DEFAULT_PORT),
            reconnect: None,
            next_id: 2,
            fails: HashMap::new(),
            banned: HashSet::new(),
            dir: dir.clone(),
            base_dir: dir,
            project: String::new(),
            ready: false,
            opened: false,
            total: 0,
            got: 0,
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
            addrs: Vec::new(),
            chat: Vec::new(),
            unread: 0,
            following: None,
            followed_script: String::new(),
            play: None,
            host_ip: None,
        }
    }

    /// Shares the open project.
    pub fn host(ctx: &egui::Context, st: &EditorState, name: &str, port: u16) -> Result<Session, String> {
        let l = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("can't use port {}: {}", port, e))?;
        let mut s = Session::new(name, st.project.dir.clone(), ctx);
        s.listener = Some(Listener::new(l, s.waker.clone()).map_err(|e| e.to_string())?);
        s.me = 1;
        s.host = true;
        s.ready = true;
        s.opened = true;
        s.port = port;
        s.code = new_code();
        s.project = st.project.config.name.clone();
        s.token = random::<8>().iter().map(|b| format!("{:02x}", b)).collect();
        let _ = std::fs::create_dir_all(st.project.dir.join(".gex"));
        let _ = std::fs::write(st.project.dir.join(".gex").join("sharing"), &s.token);
        s.addrs = gex_script::wire::addresses();
        s.scan(true);
        s.status = format!("Sharing {}", s.project);
        Ok(s)
    }

    /// Joins someone's session; the project is copied into `folder`.
    pub fn join(ctx: &egui::Context, name: &str, address: &str, code: &str, folder: &Path) -> Session {
        let (h, p) = gex_script::wire::split_address(address, DEFAULT_PORT);
        let mut s = Session::new(name, folder.to_path_buf(), ctx);
        s.code = code.to_string();
        s.connecting = Some(gex_script::wire::connect(&h, p));
        s.status = format!("Connecting to {}:{}…", h, p);
        s.address = (h, p);
        s
    }

    pub fn reconnecting(&self) -> bool {
        self.reconnect.is_some()
    }

    fn send_all(&mut self, m: &M) {
        for p in &mut self.peers {
            if p.stage == Stage::Ready {
                p.send(m);
            }
        }
    }

    fn send_except(&mut self, except: u32, m: &M) {
        for p in &mut self.peers {
            if p.stage == Stage::Ready && p.id != except {
                p.send(m);
            }
        }
    }

    /// Looks for files changed on this computer and sends them (`quiet`: only take note).
    fn scan(&mut self, quiet: bool) {
        self.scan_at = Instant::now();
        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        for (rel, len, m) in walk(&self.dir) {
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
        // View-only: changes here aren't shared.
        if quiet || self.me == 0 || !self.editor {
            return;
        }
        for (rel, bytes) in changed {
            for p in &mut self.peers {
                if p.stage == Stage::Ready {
                    p.send_file(&rel, &bytes);
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
    if rel == gex_core::project::PROJECT_FILE {
        if let Ok(p) = gex_core::project::Project::open(&st.project.dir) {
            st.project.config = p.config;
        }
    }
}

/// Undo steps that take in others' changes (so undo takes back only your own). Going through
/// every step of a long history for every change would cost too much on big blueprints.
const HISTORY_SYNCED: usize = 64;

/// Every tree that shows a blueprint: open ones and their recent undo steps.
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

fn open_tree_mut<'a>(st: &'a mut EditorState, res: &str) -> Option<&'a mut BlueprintTree> {
    if st.blueprint_res.as_deref() == Some(res) {
        return Some(&mut st.tree);
    }
    st.docs.iter_mut().find(|d| d.blueprint_res.as_deref() == Some(res)).map(|d| &mut d.tree)
}

fn open_bps(st: &EditorState) -> HashSet<String> {
    st.blueprint_res.iter().cloned().chain(st.docs.iter().filter_map(|d| d.blueprint_res.clone())).collect()
}

fn apply_to_open(st: &mut EditorState, res: &str, patches: &[Patch]) {
    if patches.is_empty() {
        return;
    }
    for t in trees_of(st, res) {
        patch_tree(t, patches);
    }
    // Objects others removed can't stay selected.
    let tree = &st.tree;
    st.selection.retain(|i| tree.contains(*i));
    if st.selected.is_some_and(|i| !tree.contains(i)) {
        st.selected = None;
    }
}

/// Gives objects the uids the host knows them by, where the paths match: objects made during
/// the session got new uids there, but were loaded from disk here. Gives back old → new.
fn adopt_uids(st: &mut EditorState, res: &str, host: &Flat) -> HashMap<u64, u64> {
    let mut remap = HashMap::new();
    let Some(t) = open_tree(st, res) else { return remap };
    let hp = paths(host);
    for id in t.walk() {
        let u = t.objects[id].uid;
        if host.contains_key(&u) {
            continue;
        }
        if let Some(h) = hp.get(&t.path_of(id)).copied() {
            remap.insert(u, h);
        }
    }
    if !remap.is_empty() {
        for t in trees_of(st, res) {
            for (_, o) in t.objects.iter_mut() {
                if let Some(n) = remap.get(&o.uid) {
                    o.uid = *n;
                }
            }
        }
    }
    remap
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

/// Once a frame: the network, files, blueprints, scripts, where everyone is, following.
pub fn tick(ctx: &egui::Context, st: &mut EditorState) {
    let Some(mut s) = st.collab.take() else { return };
    s.frame = ctx.cumulative_frame_nr();
    s.pump(ctx, Some(st));
    if s.ended.is_none() && s.reconnect.is_none() {
        s.local_changes(st);
        s.presence(st);
        s.follow(st);
        s.start_play(st);
    }
    s.wake(ctx);
    st.collab = Some(s);
}

/// Before the project opens (joining): just the network and files.
pub fn tick_joining(ctx: &egui::Context, s: &mut Session) {
    s.frame = ctx.cumulative_frame_nr();
    s.pump(ctx, None);
    s.wake(ctx);
}

impl Session {
    /// The next look: right away while things arrive or files go out; every 2 seconds when
    /// quiet (for files changed outside GEX). Arrivals wake the window themselves.
    fn wake(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.busy) || self.peers.iter().any(|p| !p.queue.is_empty()) {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.connecting.is_some() || self.reconnect.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }

    /// Joined, and the connection dropped: keep trying for a minute.
    fn lost_host(&mut self, why: &str) {
        if !self.opened || self.address.0.is_empty() {
            self.ended = Some(why.to_string());
            return;
        }
        let now = Instant::now();
        self.peers.clear();
        self.people.clear();
        self.following = None;
        self.reconnect = Some((now + RECONNECT_FOR, now));
        self.status = format!("{}: getting back in…", why);
        for bp in self.bps.values_mut() {
            bp.ready = false;
            bp.pending.clear();
        }
        for d in self.scripts.values_mut() {
            d.ready = false;
        }
    }

    fn pump(&mut self, ctx: &egui::Context, mut st: Option<&mut EditorState>) {
        if self.ended.is_some() {
            return;
        }
        if let Some((deadline, next)) = self.reconnect {
            let now = Instant::now();
            if now > deadline && self.connecting.is_none() {
                self.ended = Some("lost the connection to the host".into());
                return;
            }
            if self.connecting.is_none() && now >= next && self.peers.is_empty() {
                self.connecting = Some(gex_script::wire::connect(&self.address.0, self.address.1));
                self.reconnect = Some((deadline, now + Duration::from_secs(3)));
            }
        }
        // Joining: the connection.
        if let Some(rx) = &self.connecting {
            match rx.try_recv() {
                Ok(Ok(stream)) => {
                    self.connecting = None;
                    match Link::new(stream, self.waker.clone()) {
                        Ok(link) => {
                            let mut p = Peer::new(link);
                            p.id = 1;
                            p.synced_sent = true;
                            p.asked = true;
                            p.nonce = random::<16>().to_vec();
                            p.hi(&Hi::Hi { v: PROTOCOL, nonce: p.nonce.clone() });
                            self.host_ip = p.ip;
                            self.peers.push(p);
                            if self.reconnect.is_none() {
                                self.status = "Connected, checking the invite code…".into();
                            }
                        }
                        Err(e) => self.lost_host(&e.to_string()),
                    }
                }
                Ok(Err(e)) => {
                    self.connecting = None;
                    if self.reconnect.is_none() {
                        self.ended = Some(e);
                        return;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(_) => {
                    self.connecting = None;
                    if self.reconnect.is_none() {
                        self.ended = Some("couldn't connect".into());
                        return;
                    }
                }
            }
        }
        if let Some(l) = &self.listener {
            while let Ok(sock) = l.rx.try_recv() {
                let Ok(link) = Link::new(sock, self.waker.clone()) else { continue };
                let mut p = Peer::new(link);
                if p.ip.is_some_and(|ip| self.banned.contains(&ip)) {
                    p.hi(&Hi::Refused { reason: "you can't join this session".into() });
                    p.link.close_after_sending();
                    continue;
                }
                self.peers.push(p);
            }
        }
        // Hosting: send joiners their files, a few at a time.
        if self.host {
            let dir = self.dir.clone();
            for p in &mut self.peers {
                while p.link.backlog() < (8 << 20) {
                    let Some(rel) = p.queue.pop_front() else { break };
                    if let Ok(bytes) = std::fs::read(dir.join(&rel)) {
                        p.send_file(&rel, &bytes);
                    }
                }
                if p.asked && p.queue.is_empty() && !p.synced_sent && p.stage == Stage::Ready {
                    p.send(&M::Synced);
                    p.synced_sent = true;
                }
            }
        }
        // What arrived, in the order each peer sent it; a file's bytes follow its header.
        enum Item {
            Msg(M),
            File(String, Vec<u8>),
        }
        let mut items: Vec<(usize, Item)> = Vec::new();
        let mut bad: Vec<usize> = Vec::new();
        for (i, p) in self.peers.iter_mut().enumerate() {
            for f in p.link.recv() {
                if bad.contains(&i) {
                    break;
                }
                match f {
                    // In the clear: only the handshake (and a refusal).
                    Frame::Json(b) => {
                        let Ok(h) = serde_json::from_slice::<Hi>(&b) else {
                            bad.push(i);
                            continue;
                        };
                        match h {
                            Hi::Hi { v, nonce } if self.host && p.stage == Stage::Hi && nonce.len() == 16 => {
                                if v != PROTOCOL {
                                    p.hi(&Hi::Refused { reason: "the host has a different version of GEX (both need the newest)".into() });
                                    p.link.close_after_sending();
                                    continue;
                                }
                                let mine = random::<16>().to_vec();
                                let (j2h, h2j) = keys(&self.code, &nonce, &mine);
                                p.hi(&Hi::Challenge { nonce: mine });
                                p.inn = Some(Cipher::new(&j2h));
                                p.out = Some(Cipher::new(&h2j));
                                p.stage = Stage::Keyed;
                            }
                            Hi::Challenge { nonce } if !self.host && p.inn.is_none() && nonce.len() == 16 => {
                                let (j2h, h2j) = keys(&self.code, &p.nonce, &nonce);
                                p.out = Some(Cipher::new(&j2h));
                                p.inn = Some(Cipher::new(&h2j));
                                p.stage = Stage::Ready;
                                let hello = M::Hello { name: self.name.clone() };
                                p.send(&hello);
                            }
                            Hi::Refused { reason } if !self.host => {
                                self.reconnect = None;
                                self.ended = Some(reason);
                                return;
                            }
                            _ => bad.push(i),
                        }
                    }
                    Frame::Bytes(b) => {
                        let opened = p.inn.as_mut().and_then(|c| c.open(&b));
                        match opened {
                            Some((0, json)) => match serde_json::from_slice::<M>(&json) {
                                Ok(M::File { path }) => p.incoming = Some(path),
                                Ok(m) => items.push((i, Item::Msg(m))),
                                Err(_) => {}
                            },
                            Some((1, bytes)) => {
                                if let Some(path) = p.incoming.take() {
                                    items.push((i, Item::File(path, bytes)));
                                }
                            }
                            _ => {
                                // Doesn't open: the wrong code, or tampering.
                                if self.host && p.stage == Stage::Keyed {
                                    if let Some(ip) = p.ip {
                                        let n = self.fails.entry(ip).or_default();
                                        *n += 1;
                                        if *n >= MAX_FAILS {
                                            self.banned.insert(ip);
                                        }
                                    }
                                    p.hi(&Hi::Refused { reason: "that invite code isn't right".into() });
                                    p.link.close_after_sending();
                                }
                                bad.push(i);
                            }
                        }
                    }
                }
            }
        }
        for i in bad {
            let p = &mut self.peers[i];
            if p.stage != Stage::Keyed {
                p.link.close();
            }
        }
        if !items.is_empty() {
            self.busy = true;
        }
        for (i, item) in items {
            // Hosting: nothing more from someone refused or removed. (Joined: the host's last
            // words, like being removed, still count after it closes.)
            if self.host && self.peers[i].link.is_closed() {
                continue;
            }
            let m = match item {
                Item::Msg(m) => m,
                Item::File(path, bytes) => {
                    let from = self.peers[i].id;
                    if self.host && (self.peers[i].stage != Stage::Ready || !self.peers[i].editor) {
                        continue;
                    }
                    self.got += bytes.len() as u64;
                    let wrote = self.write_file(&path, &bytes, st.as_deref_mut());
                    if self.host && wrote {
                        for p in &mut self.peers {
                            if p.stage == Stage::Ready && p.id != from {
                                p.send_file(&path, &bytes);
                            }
                        }
                    }
                    continue;
                }
            };
            if self.host {
                self.host_message(ctx, i, m, st.as_deref_mut());
            } else {
                self.joined_message(ctx, m, st.as_deref_mut());
            }
            if self.ended.is_some() {
                return;
            }
        }
        // People who left.
        let mut k = 0;
        while k < self.peers.len() {
            if !self.peers[k].link.is_closed() {
                k += 1;
                continue;
            }
            let p = self.peers.remove(k);
            if self.host {
                if p.stage == Stage::Ready {
                    if let Some(x) = self.people.iter().find(|x| x.id == p.id) {
                        let line = format!("{} left", x.name);
                        self.chat.push(ChatLine { from: 0, name: String::new(), text: line });
                    }
                    self.people.retain(|x| x.id != p.id);
                    self.send_all(&M::Left { id: p.id });
                    if self.following == Some(p.id) {
                        self.following = None;
                    }
                }
            } else if self.ended.is_none() {
                self.lost_host(if self.ready { "the connection to the host broke" } else { "the host closed the connection" });
                return;
            }
        }
        // Files changed here.
        if self.ready && self.opened && self.scan_at.elapsed() > Duration::from_millis(1500) {
            self.scan(false);
        }
    }

    fn host_message(&mut self, ctx: &egui::Context, i: usize, m: M, st: Option<&mut EditorState>) {
        let pid = self.peers[i].id;
        let editor = self.peers[i].editor;
        if self.peers[i].stage == Stage::Keyed {
            // The first message that opens proves the code; it must be the hello.
            let M::Hello { name } = m else {
                self.peers[i].link.close();
                return;
            };
            if self.people.len() + 1 >= MAX_PEOPLE {
                let p = &mut self.peers[i];
                p.send(&M::Refused { reason: "the session is full".into() });
                p.link.close_after_sending();
                return;
            }
            let id = self.next_id;
            self.next_id += 1;
            let person = Person { id, name: clean_name(&name, id), editor: true, p: Presence::default() };
            let mut people = vec![Person { id: self.me, name: self.name.clone(), editor: true, p: self.local.clone() }];
            people.extend(self.people.iter().cloned());
            let files = self.files.iter().map(|(k, f)| (k.clone(), f.len, f.hash)).collect();
            let welcome = M::Welcome { id, project: self.project.clone(), people, files, token: self.token.clone() };
            let p = &mut self.peers[i];
            p.id = id;
            p.stage = Stage::Ready;
            p.send(&welcome);
            self.send_except(id, &M::Joined { person: person.clone() });
            self.chat.push(ChatLine { from: 0, name: String::new(), text: format!("{} joined", person.name) });
            self.people.push(person);
            return;
        }
        match m {
            M::Need { paths } => {
                let p = &mut self.peers[i];
                p.queue.extend(paths.into_iter().filter(|x| self.files.contains_key(x)));
                p.asked = true;
                p.synced_sent = false;
            }
            M::Gone { path } if editor => {
                if safe_path(&self.dir, &path).is_some() && self.files.contains_key(&path) {
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
            M::Chat { text, .. } => {
                let text = clean_text(&text);
                if text.trim().is_empty() {
                    return;
                }
                let name = self.people.iter().find(|x| x.id == pid).map(|x| x.name.clone()).unwrap_or_default();
                self.chat_line(pid, name, text.clone());
                self.send_except(pid, &M::Chat { from: pid, text });
            }
            M::Objects { res, patches, .. } => {
                let Some(st) = st else { return };
                if !safe_res(&res) {
                    return;
                }
                if !self.bps.contains_key(&res) {
                    let f = read_bp_flat(st, &res);
                    self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: true });
                }
                // Changes made here since the last look go first.
                self.blueprint_changes(st, &res);
                if !editor {
                    // View-only: they get the shared state back.
                    let patches = self.bps[&res].flat.iter().map(|(u, e)| Patch::full(*u, e)).collect();
                    self.peers[i].send(&M::BpState { res, patches });
                    return;
                }
                let bp = self.bps.get_mut(&res).unwrap();
                let touched = touched_parents(&patches, &bp.flat);
                let mut took: Vec<Patch> = patches.into_iter().filter(|p| patch_flat(&mut bp.flat, p)).collect();
                apply_to_open(st, &res, &took);
                // Children's order as it came out here is what everyone gets (two people adding
                // to one parent at once would otherwise end up in different orders).
                if let Some(t) = open_tree(st, &res) {
                    for (u, kids) in kids_now(t, &touched) {
                        if let Some(e) = bp.flat.get_mut(&u) {
                            e.kids = kids.clone();
                        }
                        match took.iter_mut().find(|p| p.uid == u && !p.gone) {
                            Some(p) => p.kids = Some(kids),
                            None => took.push(Patch { uid: u, kids: Some(kids), ..Default::default() }),
                        }
                    }
                }
                // Everyone, the sender too: that's how it knows its change got here.
                self.send_all(&M::Objects { res, from: pid, patches: took });
            }
            M::OpenBp { res } => {
                let Some(st) = st else { return };
                if !safe_res(&res) {
                    return;
                }
                if !self.bps.contains_key(&res) {
                    let f = read_bp_flat(st, &res);
                    self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: true });
                } else {
                    self.blueprint_changes(st, &res);
                }
                let patches = self.bps[&res].flat.iter().map(|(u, e)| Patch::full(*u, e)).collect();
                self.peers[i].send(&M::BpState { res, patches });
            }
            M::OpenScript { res } => {
                let Some(st) = st else { return };
                if !safe_res(&res) {
                    return;
                }
                self.script_changes(st, &res);
                if !self.scripts.contains_key(&res) {
                    self.scripts.insert(res.clone(), Doc::new(read_script(st, &res), true));
                }
                let d = &self.scripts[&res];
                let m = M::ScriptState { res: res.clone(), rev: d.rev, text: d.text.clone() };
                self.peers[i].send(&m);
            }
            M::Edit { res, rev: base, op, .. } => {
                let Some(st) = st else { return };
                if !safe_res(&res) || op.t.len() > (8 << 20) {
                    return;
                }
                self.script_changes(st, &res);
                if !self.scripts.contains_key(&res) {
                    self.scripts.insert(res.clone(), Doc::new(read_script(st, &res), true));
                }
                let d = self.scripts.get_mut(&res).unwrap();
                if !editor || base > d.rev || base + (d.log.len() as u64) < d.rev {
                    // View-only, or too far behind to move past everything since: the text again.
                    let m = M::ScriptState { res: res.clone(), rev: d.rev, text: d.text.clone() };
                    self.peers[i].send(&m);
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
    }

    fn chat_line(&mut self, from: u32, name: String, text: String) {
        self.chat.push(ChatLine { from, name, text });
        if self.chat.len() > 200 {
            self.chat.remove(0);
        }
        self.unread += 1;
    }

    fn joined_message(&mut self, ctx: &egui::Context, m: M, st: Option<&mut EditorState>) {
        match m {
            M::Refused { reason } => {
                self.reconnect = None;
                self.ended = Some(reason);
            }
            M::Kicked => {
                self.reconnect = None;
                self.ended = Some("the host removed you from the session".into());
            }
            M::Welcome { id, project, people, files, token } => {
                self.me = id;
                self.editor = true;
                self.people = people;
                if !self.opened {
                    self.project = project.clone();
                    self.dir = self.base_dir.join(sanitize(&project));
                    // Joining from the computer that shares it, into its own folder: a copy beside it.
                    let mark = |d: &Path| std::fs::read_to_string(d.join(".gex").join("sharing")).is_ok_and(|t| !token.is_empty() && t == token);
                    if mark(&self.dir) {
                        self.dir = self.base_dir.join(format!("{} (joined)", sanitize(&project)));
                    }
                    let _ = std::fs::create_dir_all(&self.dir);
                }
                let mut need = Vec::new();
                self.total = 0;
                self.got = 0;
                for (rel, len, hash) in files {
                    let Some(p) = safe_path(&self.dir, &rel) else { continue };
                    let same = std::fs::metadata(&p).is_ok_and(|m| m.len() == len) && std::fs::read(&p).is_ok_and(|b| fnv(&b) == hash);
                    if same {
                        if let Ok(m) = std::fs::metadata(&p) {
                            self.files.insert(rel.clone(), FileInfo { len, modified: modified(&m), hash });
                        }
                    } else {
                        self.total += len;
                        need.push(rel);
                    }
                }
                if self.reconnect.is_none() {
                    self.status = format!("Getting {} ({} files)…", project, need.len());
                }
                if let Some(h) = self.peers.first_mut() {
                    h.send(&M::Need { paths: need });
                }
            }
            M::Synced => {
                let back = self.reconnect.take().is_some();
                self.ready = true;
                self.status = format!("Working on {} together", self.project);
                // Files that were already here count as seen.
                self.scan(true);
                if back {
                    // Back in: ask for everything open again (changes made meanwhile are kept
                    // and sent on top).
                    self.open_bps.clear();
                    self.open_scripts.clear();
                    self.sent = None;
                    self.chat.push(ChatLine { from: 0, name: String::new(), text: "Back in.".into() });
                }
            }
            M::Gone { path } => {
                if self.ready {
                    self.remove_file(&path);
                }
            }
            M::Joined { person } => {
                self.chat.push(ChatLine { from: 0, name: String::new(), text: format!("{} joined", person.name) });
                self.people.retain(|x| x.id != person.id);
                self.people.push(person);
            }
            M::Left { id } => {
                if let Some(p) = self.people.iter().find(|x| x.id == id) {
                    let line = format!("{} left", p.name);
                    self.chat.push(ChatLine { from: 0, name: String::new(), text: line });
                }
                self.people.retain(|x| x.id != id);
                if self.following == Some(id) {
                    self.following = None;
                }
            }
            M::Presence { id, p } => {
                if let Some(x) = self.people.iter_mut().find(|x| x.id == id) {
                    x.p = p;
                }
            }
            M::Chat { from, text } => {
                let name = self.people.iter().find(|x| x.id == from).map(|x| x.name.clone()).unwrap_or_default();
                self.chat_line(from, name, clean_text(&text));
            }
            M::Role { id, editor } => {
                if id == self.me {
                    self.editor = editor;
                    let t = if editor { "You can make changes again." } else { "You're view-only now: your changes aren't shared." };
                    self.chat.push(ChatLine { from: 0, name: String::new(), text: t.into() });
                } else if let Some(x) = self.people.iter_mut().find(|x| x.id == id) {
                    x.editor = editor;
                }
            }
            M::Play { bp, port } => {
                if safe_res(&bp) {
                    self.play = Some((bp, port));
                }
            }
            M::Objects { res, from, patches } => {
                let Some(st) = st else { return };
                let me = self.me;
                let Some(bp) = self.bps.get_mut(&res) else { return };
                if !bp.ready {
                    return;
                }
                if from == me {
                    for p in &patches {
                        for f in p.fields() {
                            let k = (p.uid, f);
                            if let Some(n) = bp.pending.get_mut(&k) {
                                *n -= 1;
                                if *n == 0 {
                                    bp.pending.remove(&k);
                                }
                            }
                        }
                    }
                    // The order children came out in at the host is the one that counts.
                    let order: Vec<Patch> = patches
                        .iter()
                        .filter(|p| p.kids.is_some() && !bp.pending.contains_key(&(p.uid, "kids".to_string())))
                        .map(|p| Patch { uid: p.uid, kids: p.kids.clone(), ..Default::default() })
                        .collect();
                    for p in &order {
                        patch_flat(&mut bp.flat, p);
                    }
                    apply_to_open(st, &res, &order);
                    return;
                }
                // Changes made here first, so they aren't mistaken for these.
                self.blueprint_changes(st, &res);
                let bp = self.bps.get_mut(&res).unwrap();
                let take: Vec<Patch> = {
                    let pending = &bp.pending;
                    patches
                        .into_iter()
                        .map(|p| {
                            let u = p.uid;
                            if p.gone { p } else { p.without(&|f| pending.contains_key(&(u, f.to_string()))) }
                        })
                        .filter(|p| !p.is_empty())
                        .collect()
                };
                let take: Vec<Patch> = take.into_iter().filter(|p| patch_flat(&mut bp.flat, p)).collect();
                if take.iter().any(|p| p.gone) {
                    let gone: HashSet<u64> = take.iter().filter(|p| p.gone).map(|p| p.uid).collect();
                    bp.pending.retain(|(u, _), _| !gone.contains(u));
                }
                apply_to_open(st, &res, &take);
            }
            M::BpState { res, patches } => {
                let Some(st) = st else { return };
                if !self.bps.contains_key(&res) {
                    return;
                }
                let mut host: Flat = HashMap::with_capacity(patches.len());
                for p in patches.iter().filter(|p| p.is_new()) {
                    let mut e = Entry::blank(p.kind.unwrap());
                    p.to_entry(&mut e);
                    host.insert(p.uid, e);
                }
                if let Some(t) = open_tree_mut(st, &res) {
                    fix_uids(t);
                }
                let remap = adopt_uids(st, &res, &host);
                // Changes made here before the state came (or while away): kept, on top of it.
                let mut base = std::mem::take(&mut self.bps.get_mut(&res).unwrap().flat);
                if !remap.is_empty() {
                    base = base
                        .into_iter()
                        .map(|(u, mut e)| {
                            if let Some(n) = remap.get(&e.parent) {
                                e.parent = *n;
                            }
                            (remap.get(&u).copied().unwrap_or(u), e)
                        })
                        .collect();
                }
                let here = open_tree(st, &res).map(flat).unwrap_or_default();
                let mine: Vec<Patch> = if self.editor { diff(&base, &here) } else { Vec::new() };
                apply_to_open(st, &res, &diff(&here, &host));
                let mine: Vec<Patch> = mine.into_iter().filter(|p| p.gone || p.is_new() || host.contains_key(&p.uid)).collect();
                apply_to_open(st, &res, &mine);
                let bp = self.bps.get_mut(&res).unwrap();
                bp.flat = host;
                bp.ready = true;
                bp.pending.clear();
            }
            M::ScriptState { res, rev, text } => {
                let Some(st) = st else { return };
                let editor = self.editor;
                let Some(d) = self.scripts.get_mut(&res) else { return };
                let here = st.scripts.get(&res).map(|s| s.text.clone());
                // Typed here before the state came (or while away): kept, moved past the host's changes.
                let mut want = text.clone();
                if let (true, Some(h)) = (editor && !d.ready, here.as_deref()) {
                    if let Some(mine) = text_diff(&d.text, h) {
                        let mine = match text_diff(&d.text, &text) {
                            Some(theirs) => xf(&mine, &theirs, false),
                            None => mine,
                        };
                        apply_op(&mut want, &mine);
                    }
                }
                d.rev = rev;
                d.text = text;
                d.inflight = None;
                d.ready = true;
                if let Some(h) = here {
                    if let Some(op) = text_diff(&h, &want) {
                        edit_open_script(ctx, st, &res, &op);
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

    /// Objects changed here in one blueprint since the last look: sent on (or, view-only,
    /// put back).
    fn blueprint_changes(&mut self, st: &mut EditorState, res: &str) {
        if !self.bps.get(res).is_some_and(|b| b.ready) {
            return;
        }
        let Some(tree) = open_tree_mut(st, res) else { return };
        fix_uids(tree);
        let now = flat(tree);
        let bp = self.bps.get_mut(res).unwrap();
        let patches = diff(&bp.flat, &now);
        if patches.is_empty() {
            return;
        }
        if !self.editor {
            let back = diff(&now, &bp.flat);
            if let Some(t) = open_tree_mut(st, res) {
                patch_tree(t, &back);
            }
            return;
        }
        bp.flat = now;
        if !self.host {
            for p in &patches {
                for f in p.fields() {
                    *bp.pending.entry((p.uid, f)).or_default() += 1;
                }
            }
        }
        let m = M::Objects { res: res.to_string(), from: self.me, patches };
        self.send_all(&m);
    }

    /// Typing here in one script since the last look: sent on (or, view-only, put back).
    fn script_changes(&mut self, st: &mut EditorState, res: &str) {
        let Some(s) = st.scripts.get_mut(res) else { return };
        let Some(d) = self.scripts.get_mut(res) else { return };
        if !d.ready || s.text == d.text {
            return;
        }
        if !self.editor {
            s.text = d.text.clone();
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

    fn local_changes(&mut self, st: &mut EditorState) {
        // Scripts and blueprints opened or closed here.
        if st.scripts.len() != self.open_scripts.len() || st.scripts.keys().any(|k| !self.open_scripts.contains(k)) {
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
                            self.scripts.insert(res.clone(), Doc::new(text, true));
                        }
                    }
                } else {
                    // Back in after a drop: keep the base, so what was typed meanwhile is kept.
                    match self.scripts.get_mut(res) {
                        Some(d) => d.ready = false,
                        None => {
                            let text = st.scripts[res].text.clone();
                            self.scripts.insert(res.clone(), Doc::new(text, false));
                        }
                    }
                    self.send_all(&M::OpenScript { res: res.clone() });
                }
            }
            if !self.host {
                self.scripts.retain(|r, _| scripts.contains(r));
            }
            self.open_scripts = scripts;
        }
        let bps = open_bps(st);
        if bps != self.open_bps {
            for res in bps.difference(&self.open_bps.clone()) {
                if self.host {
                    match self.bps.get(res) {
                        Some(bp) => {
                            let host = bp.flat.clone();
                            adopt_uids(st, res, &host);
                            let here = open_tree(st, res).map(flat).unwrap_or_default();
                            apply_to_open(st, res, &diff(&here, &host));
                        }
                        None => {
                            if let Some(t) = open_tree_mut(st, res) {
                                fix_uids(t);
                            }
                            let f = open_tree(st, res).map(flat).unwrap_or_default();
                            self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: true });
                        }
                    }
                } else {
                    if !self.bps.contains_key(res) {
                        if let Some(t) = open_tree_mut(st, res) {
                            fix_uids(t);
                        }
                        let f = open_tree(st, res).map(flat).unwrap_or_default();
                        self.bps.insert(res.clone(), Bp { flat: f, pending: HashMap::new(), ready: false });
                    }
                    self.send_all(&M::OpenBp { res: res.clone() });
                }
            }
            if !self.host {
                self.bps.retain(|r, _| bps.contains(r));
            }
            self.open_bps = bps;
        }
        // Typing: every frame (one comparison per open script when nothing changed).
        let tracked: Vec<String> = self.scripts.keys().filter(|r| st.scripts.contains_key(*r)).cloned().collect();
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
                    self.ids = (res, st.tree.objects.iter().map(|(id, o)| (o.uid, id)).collect());
                } else if !self.ids.1.is_empty() {
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
        if self.presence_at.elapsed() <= Duration::from_millis(66) {
            return;
        }
        self.local.bp = st.blueprint_res.clone().unwrap_or_default();
        self.local.mode3d = st.mode_3d;
        self.local.cam2 = [st.cam2.center.x, st.cam2.center.y, st.cam2.zoom];
        let c = &st.cam3;
        self.local.cam3 = [c.target.x, c.target.y, c.target.z, c.yaw, c.pitch, c.distance];
        let tree = &st.tree;
        let mut sel: Vec<u64> = st.selection.iter().chain(st.selected.iter()).filter(|i| tree.contains(**i)).map(|i| tree.objects[*i].uid).collect();
        sel.sort_unstable();
        sel.dedup();
        self.local.sel = sel;
        if self.sent.as_ref() != Some(&self.local) {
            self.presence_at = Instant::now();
            self.sent = Some(self.local.clone());
            let m = M::Presence { id: self.me, p: self.local.clone() };
            self.send_all(&m);
        }
    }

    /// Following someone: their blueprint, their view, the script they type in.
    fn follow(&mut self, st: &mut EditorState) {
        let Some(id) = self.following else { return };
        let Some(p) = self.people.iter().find(|x| x.id == id).map(|x| x.p.clone()) else {
            self.following = None;
            return;
        };
        if !p.bp.is_empty() && st.blueprint_res.as_deref() != Some(p.bp.as_str()) && st.playing.is_none() && safe_res(&p.bp) {
            st.open_blueprint(&p.bp);
        }
        st.mode_3d = p.mode3d;
        st.cam2.center = gex_core::glam::Vec2::new(p.cam2[0], p.cam2[1]);
        st.cam2.zoom = p.cam2[2].max(0.001);
        st.cam3.target = gex_core::glam::Vec3::new(p.cam3[0], p.cam3[1], p.cam3[2]);
        (st.cam3.yaw, st.cam3.pitch, st.cam3.distance) = (p.cam3[3], p.cam3[4], p.cam3[5].max(0.01));
        st.cam_fitted = true;
        if !p.script.is_empty() && p.script != self.followed_script && safe_res(&p.script) {
            let text = st.script_text(&p.script);
            let line = p.caret.map(|c| text.chars().take(c as usize).filter(|ch| *ch == '\n').count() + 1);
            st.open_script(&p.script, line);
        }
        self.followed_script = p.script;
    }

    /// The host started playing together: play too, and join the host's game.
    fn start_play(&mut self, st: &mut EditorState) {
        let Some((bp, port)) = self.play.take() else { return };
        if st.playing.is_some() {
            return;
        }
        if st.blueprint_res.as_deref() != Some(bp.as_str()) {
            st.open_blueprint(&bp);
        }
        st.play();
        let ip = self.host_ip.map(|ip| ip.to_string()).unwrap_or_else(|| self.address.0.clone());
        if let Some(p) = &st.playing {
            p.handle.go_online(gex_script::thread::NetStart::Join(ip, port));
            self.chat.push(ChatLine { from: 0, name: String::new(), text: "Playing together: your game joined the host's.".into() });
        }
    }

    /// Hosting: play the blueprint open here, and everyone else's GEX joins the game.
    pub fn play_together(&mut self, st: &mut EditorState) {
        if !self.host {
            return;
        }
        if st.playing.is_none() {
            st.play();
        }
        let Some(p) = &st.playing else { return };
        p.handle.go_online(gex_script::thread::NetStart::Host(GAME_PORT));
        let bp = st.blueprint_res.clone().unwrap_or_default();
        self.send_all(&M::Play { bp, port: GAME_PORT });
        self.chat.push(ChatLine { from: 0, name: String::new(), text: "Playing together: everyone's game joins this one.".into() });
    }

    pub fn say(&mut self, text: &str) {
        let text = clean_text(text);
        if text.trim().is_empty() {
            return;
        }
        let (me, name) = (self.me, self.name.clone());
        self.chat.push(ChatLine { from: me, name, text: text.clone() });
        self.send_all(&M::Chat { from: me, text });
    }

    /// Hosting: let someone change things, or not.
    pub fn set_editor(&mut self, id: u32, editor: bool) {
        if !self.host {
            return;
        }
        if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
            p.editor = editor;
        }
        if let Some(x) = self.people.iter_mut().find(|x| x.id == id) {
            x.editor = editor;
        }
        self.send_all(&M::Role { id, editor });
    }

    /// Hosting: remove someone (and keep their computer out for this session).
    pub fn kick(&mut self, id: u32) {
        if !self.host {
            return;
        }
        if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
            p.send(&M::Kicked);
            p.link.close_after_sending();
            if let Some(ip) = p.ip {
                self.banned.insert(ip);
            }
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

impl Drop for Session {
    fn drop(&mut self) {
        if self.host {
            let _ = std::fs::remove_file(self.dir.join(".gex").join("sharing"));
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

/// Others' mice and selections in the blueprint view; this person's mouse, for them. Moving
/// the view here stops following someone.
pub fn viewport(painter: &egui::Painter, st: &mut EditorState, to_screen: &dyn Fn(gex_core::glam::Vec3) -> Option<egui::Pos2>, pointer: Option<gex_core::glam::Vec3>, moved_view: bool) {
    let Some(s) = &mut st.collab else { return };
    if pointer.is_some() {
        s.local.cursor = pointer.map(|p| p.to_array());
        s.cursor_frame = s.frame;
    }
    if moved_view {
        s.following = None;
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
            for u in &p.p.sel {
                let Some(id) = s.ids.1.get(u).copied().filter(|i| st.tree.contains(*i)) else { continue };
                let n = &st.tree.objects[id];
                let m = st.tree.global_matrix(id);
                if st.mode_3d || !matches!(n.kind, ObjectKind::Sprite2D | ObjectKind::Collider2D | ObjectKind::Label | ObjectKind::Button | ObjectKind::Panel) {
                    if let Some(sp) = to_screen(m.w_axis.truncate()) {
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
    let mut n: Option<usize> = None;
    for p in &s.people {
        let (true, Some(c)) = (p.p.script == res, p.p.caret) else { continue };
        let len = *n.get_or_insert_with(|| text.chars().count());
        let cur = egui::text::CCursor::new((c as usize).min(len));
        let r = galley.pos_from_cursor(cur).translate(origin.to_vec2());
        let col = color(p.id);
        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(2.0, r.height())), 0.0, col);
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(ui.clip_rect().left(), r.top()), egui::pos2(ui.clip_rect().right(), r.bottom())), 0.0, theme::alpha(col, 14));
        label(ui.painter(), r.min + egui::vec2(2.0, -12.0), &p.name, col);
    }
}

/// Coloured squares with initials in the menu bar: who's here (hover for what they're doing),
/// and unread chat. Gives true when one was clicked.
pub fn people_bar(ui: &mut egui::Ui, st: &EditorState) -> bool {
    let Some(s) = &st.collab else { return false };
    let mut clicked = false;
    if s.reconnecting() {
        ui.label(RichText::new("reconnecting…").small().color(theme::WARN));
    }
    if s.unread > 0 {
        clicked |= ui.add(egui::Button::new(RichText::new(format!("{} new", s.unread)).small()).corner_radius(0.0)).tip("Chat in Work together").clicked();
    }
    for p in s.people.iter().rev() {
        let initial: String = p.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
        let fill = if s.following == Some(p.id) { Color32::WHITE } else { color(p.id) };
        let r = ui.add(egui::Button::new(RichText::new(initial).color(Color32::BLACK).strong()).fill(fill).corner_radius(0.0).min_size(egui::vec2(18.0, 18.0)));
        let follow = if s.following == Some(p.id) { "following (move the view to stop)" } else { "click for Work together" };
        if r.on_hover_text(format!("{}: {}\n{}", p.name, Session::doing(&p.p), follow)).clicked() {
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
    pub code: String,
    pub folder: String,
    pub port: u16,
    pub message: String,
    pub say: String,
    /// Windows Firewall: whether GEX's rule is there (checked once), and the rule being added.
    firewall: Option<Option<bool>>,
    firewall_job: Option<Receiver<Result<(), String>>>,
}

impl Default for Panel {
    fn default() -> Panel {
        Panel { open: false, name: default_name(), address: String::new(), code: String::new(), folder: default_folder().to_string_lossy().into_owned(), port: DEFAULT_PORT, message: String::new(), say: String::new(), firewall: None, firewall_job: None }
    }
}

fn swatch(ui: &mut egui::Ui, c: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 0.0, c);
}

/// File → Work together. Gives back a session being joined (the app opens its project when
/// every file is in).
pub fn window(ctx: &egui::Context, panel: &mut Panel, mut st: Option<&mut EditorState>, joining: &mut Option<Session>) -> Option<Session> {
    if !panel.open {
        return None;
    }
    let mut out = None;
    let mut open = true;
    egui::Window::new("Work together").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.set_width(440.0);
        if let Some(j) = joining {
            ui.label(&j.status);
            if j.total > 0 {
                ui.add(egui::ProgressBar::new((j.got as f32 / j.total as f32).clamp(0.0, 1.0)).show_percentage());
            } else {
                ui.spinner();
            }
            if ui.button("Cancel").clicked() {
                *joining = None;
            }
            return;
        }
        let in_session = st.as_ref().is_some_and(|s| s.collab.is_some());
        if let (true, Some(st)) = (in_session, st.as_deref_mut()) {
            session_ui(ui, panel, st);
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
                    if ui.button("Start sharing").tip("Others can join and work on this project with you: they get a copy of it. You get an invite code to give them.").clicked() {
                        match Session::host(ctx, st, &panel.name, panel.port) {
                            Ok(s) => {
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
        firewall_ui(ui, panel);
        ui.separator();
        theme::title(ui, "Join someone");
        egui::Grid::new("join").num_columns(2).show(ui, |ui| {
            ui.label("Address");
            let r = ui.add(egui::TextEdit::singleline(&mut panel.address).hint_text("192.168.1.20:7778").desired_width(280.0));
            // A pasted invite ("address  code") fills both.
            if r.changed() {
                let parts: Vec<String> = panel.address.split_whitespace().map(str::to_string).collect();
                if parts.len() == 2 && normal_code(&parts[1]).len() == 16 {
                    panel.address = parts[0].clone();
                    panel.code = parts[1].clone();
                }
            }
            ui.end_row();
            ui.label("Invite code");
            ui.add(egui::TextEdit::singleline(&mut panel.code).hint_text("XXXX-XXXX-XXXX-XXXX").desired_width(280.0));
            ui.end_row();
            ui.label("Put it in");
            ui.add(egui::TextEdit::singleline(&mut panel.folder).desired_width(280.0)).tip("The project is copied into a folder here, named after it.");
            ui.end_row();
        });
        ui.label(RichText::new("Only join people you trust: their scripts run on your computer when you play.").small().color(theme::TEXT_DIM));
        let ok = !panel.address.trim().is_empty() && normal_code(&panel.code).len() == 16;
        if ui.add_enabled(ok, egui::Button::new("Join")).clicked() {
            out = Some(Session::join(ctx, &panel.name, &panel.address, &panel.code, Path::new(panel.folder.trim())));
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

/// Windows: whether others can get in through the firewall, and a button to let them.
fn firewall_ui(ui: &mut egui::Ui, panel: &mut Panel) {
    if !cfg!(windows) {
        return;
    }
    if let Some(rx) = &panel.firewall_job {
        match rx.try_recv() {
            Ok(Ok(())) => {
                panel.firewall = Some(Some(true));
                panel.firewall_job = None;
            }
            Ok(Err(e)) => {
                panel.message = e;
                panel.firewall_job = None;
            }
            Err(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Waiting for Windows' admin prompt…");
                });
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                return;
            }
        }
    }
    let state = *panel.firewall.get_or_insert_with(crate::firewall::allowed);
    if state == Some(true) {
        ui.label(RichText::new("Windows Firewall lets others in on every network (ZeroTier too).").small().color(theme::MK_GREEN));
        return;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Windows Firewall may block friends on ZeroTier.").small().color(theme::WARN));
        if ui.small_button("Allow GEX through the firewall").tip("Windows treats ZeroTier, Tailscale and Hamachi networks as Public and blocks GEX on them unless it's allowed there. This lets GEX in on every network type, after a Windows admin prompt (once per computer, and again if GEX moves).").clicked() {
            panel.firewall_job = Some(crate::firewall::allow());
        }
    });
}

fn session_ui(ui: &mut egui::Ui, panel: &mut Panel, st: &mut EditorState) {
    let (mut stop, mut play) = (false, false);
    let s = st.collab.as_mut().unwrap();
    s.unread = 0;
    ui.label(RichText::new(&s.status).color(theme::TEXT));
    if s.host {
        ui.horizontal(|ui| {
            ui.label("Invite code");
            ui.label(RichText::new(&s.code).color(theme::ACCENT).monospace());
        });
        // Every address, by network: friends on ZeroTier use the ZeroTier one, and so on.
        egui::Grid::new("invite").num_columns(3).show(ui, |ui| {
            for (ip, label) in &s.addrs {
                ui.label(RichText::new(*label).color(if *label == "ZeroTier" { theme::MK_ORANGE } else { theme::TEXT_DIM }));
                let a = format!("{}:{}", ip, s.port);
                ui.label(RichText::new(&a).color(theme::ACCENT).monospace());
                if ui.small_button("Copy invite").tip(format!("{} and the invite code together, for someone on your {} network: they paste it into Address", a, label)).clicked() {
                    ui.ctx().copy_text(format!("{}  {}", a, s.code));
                }
                ui.end_row();
            }
        });
        if s.addrs.is_empty() {
            ui.label(RichText::new("This computer isn't on a network.").color(theme::WARN));
        }
        if ui.small_button("Look again").tip("Joined ZeroTier or another network just now? This finds its address").clicked() {
            s.addrs = gex_script::wire::addresses();
        }
        ui.label(RichText::new("Give these only to people you trust. Friends on your ZeroTier (or Tailscale, Hamachi) network use that address; on the same Wi-Fi, the LAN one; otherwise forward this TCP port on your router.").small().color(theme::TEXT_DIM));
        firewall_ui(ui, panel);
    } else if !s.editor {
        ui.label(RichText::new("View only: the host decides who can make changes.").color(theme::WARN));
    }
    ui.separator();
    ui.horizontal(|ui| {
        swatch(ui, color(s.me.max(1)));
        ui.label(format!("{} (you)", s.name));
    });
    let (mut follow, mut role, mut kick) = (None, None, None);
    for p in &s.people {
        ui.horizontal(|ui| {
            swatch(ui, color(p.id));
            ui.label(&p.name);
            ui.label(RichText::new(Session::doing(&p.p)).small().color(theme::TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if s.host {
                    if ui.small_button("Remove").tip("Takes them out of the session; their computer can't come back in this time").clicked() {
                        kick = Some(p.id);
                    }
                    let mut ed = p.editor;
                    if ui.checkbox(&mut ed, "Can edit").changed() {
                        role = Some((p.id, ed));
                    }
                } else if !p.editor {
                    ui.label(RichText::new("view only").small());
                }
                let on = s.following == Some(p.id);
                if ui.selectable_label(on, "Follow").tip("Go where they are: their blueprint, view and script (move the view to stop)").clicked() {
                    follow = Some(if on { None } else { Some(p.id) });
                }
            });
        });
    }
    if s.people.is_empty() {
        ui.label(RichText::new(if s.host { "No one has joined yet." } else { "The host is the only other one here." }).small().color(theme::TEXT_DIM));
    }
    if let Some(f) = follow {
        s.following = f;
    }
    if let Some((id, ed)) = role {
        s.set_editor(id, ed);
    }
    if let Some(id) = kick {
        s.kick(id);
    }
    ui.separator();
    egui::ScrollArea::vertical().max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
        for l in &s.chat {
            if l.from == 0 {
                ui.label(RichText::new(&l.text).small().italics().color(theme::TEXT_DIM));
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{}:", l.name)).color(color(l.from)).strong());
                    ui.label(&l.text);
                });
            }
        }
    });
    let r = ui.add(egui::TextEdit::singleline(&mut panel.say).hint_text("Say something (Enter)").desired_width(f32::INFINITY).char_limit(500));
    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let t = std::mem::take(&mut panel.say);
        s.say(&t);
        r.request_focus();
    }
    ui.separator();
    ui.horizontal(|ui| {
        if s.host && ui.button("Play together").tip("Plays this blueprint, and everyone's GEX joins the game (hosted on port 7777; scripts see IS_HOST, on_player_joined...)").clicked() {
            play = true;
        }
        if ui.button(if s.host { "Stop sharing" } else { "Leave" }).clicked() {
            stop = true;
        }
    });
    if play {
        let mut s = st.collab.take().unwrap();
        s.play_together(st);
        st.collab = Some(s);
    }
    if stop {
        st.collab = None;
        panel.message = "Stopped.".into();
    }
}

#[cfg(test)]
mod tests;
