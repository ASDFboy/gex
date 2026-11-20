//! Blueprint files (`.jblueprint`, RON text). A blueprint is a saved object tree; any object in it can
//! be an instance of another blueprint file, with its own overrides on top. A blueprint whose
//! root is an instance is an inherited blueprint ("Goblin extends Enemy").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::object::{Object, ObjectId, ObjectKind, Transform};
use crate::tree::BlueprintTree;
use crate::variant::Variant;

pub const BLUEPRINT_EXT: &str = "jblueprint";
pub const FORMAT: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BlueprintFile {
    pub format: u32,
    pub root: ObjectDesc,
}

/// One object as written in a blueprint file. Only what differs from the defaults (or, for
/// an instance, from the instanced blueprint) is written.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectDesc {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ObjectKind>,
    /// This object is an instance of that blueprint file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, Variant>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ObjectDesc>,
    /// For instances: changes to objects inside the instanced blueprint, by path from this object
    /// ("Hitbox", "Arm/Hand"), including children added under them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, ObjectOverride>,
}

/// Changes to one object inside an instanced blueprint.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, Variant>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ObjectDesc>,
}

impl ObjectOverride {
    fn is_empty(&self) -> bool {
        self.transform.is_none() && self.visible.is_none() && self.props.is_empty() && self.scripts.is_empty() && self.groups.is_empty() && self.children.is_empty()
    }
}

/// Reads blueprint files by `res://` path, for instancing.
pub type BlueprintReader<'a> = &'a dyn Fn(&str) -> Result<String, String>;

#[derive(Debug)]
pub struct BlueprintError(pub String);
impl std::fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for BlueprintError {}

pub fn parse(text: &str) -> Result<BlueprintFile, BlueprintError> {
    ron::from_str(text).map_err(|e| BlueprintError(format!("blueprint file: {}", e)))
}

pub fn to_text(file: &BlueprintFile) -> String {
    let cfg = ron::ser::PrettyConfig::new().struct_names(false).indentor("  ".to_string());
    ron::ser::to_string_pretty(file, cfg).expect("blueprints always serialize")
}

/// Takes out everything in a blueprint file that uses files that are gone: instances of a deleted
/// blueprint, attached scripts, and properties pointing at them. True if anything changed.
pub fn remove_refs(file: &mut BlueprintFile, gone: &dyn Fn(&str) -> bool) -> bool {
    fn props(p: &mut BTreeMap<String, Variant>, gone: &dyn Fn(&str) -> bool) -> bool {
        let n = p.len();
        p.retain(|_, v| !matches!(v, Variant::Res(r) | Variant::Text(r) if r.contains("://") && gone(r)));
        p.len() != n
    }
    fn kids(c: &mut Vec<ObjectDesc>, gone: &dyn Fn(&str) -> bool) -> bool {
        let n = c.len();
        c.retain(|d| !d.instance.as_deref().is_some_and(gone));
        let mut changed = c.len() != n;
        for d in c {
            changed |= object(d, gone);
        }
        changed
    }
    fn scripts(s: &mut Vec<String>, gone: &dyn Fn(&str) -> bool) -> bool {
        let n = s.len();
        s.retain(|x| !gone(x));
        s.len() != n
    }
    fn object(d: &mut ObjectDesc, gone: &dyn Fn(&str) -> bool) -> bool {
        let mut changed = props(&mut d.props, gone) | scripts(&mut d.scripts, gone) | kids(&mut d.children, gone);
        for o in d.overrides.values_mut() {
            changed |= props(&mut o.props, gone) | scripts(&mut o.scripts, gone) | kids(&mut o.children, gone);
        }
        changed
    }
    object(&mut file.root, gone)
}

/// Builds a whole tree from blueprint text.
pub fn load(text: &str, read: BlueprintReader) -> Result<BlueprintTree, BlueprintError> {
    let file = parse(text)?;
    let mut tree = BlueprintTree::new();
    let mut stack = Vec::new();
    build(&mut tree, None, &file.root, read, &mut stack)?;
    // Uids from paths, so every computer that loads this file names its objects alike.
    for id in tree.walk() {
        let p = tree.path_of(id);
        tree.objects[id].uid = crate::object::path_uid(&p);
    }
    Ok(tree)
}

/// Builds the blueprint at `path` under `parent` in an existing tree (the blueprint is
/// instanced: its root remembers the path and its insides are saved by reference).
pub fn instance(tree: &mut BlueprintTree, parent: Option<ObjectId>, path: &str, read: BlueprintReader) -> Result<ObjectId, BlueprintError> {
    let desc = ObjectDesc { instance: Some(path.to_string()), ..Default::default() };
    build(tree, parent, &desc, read, &mut Vec::new())
}

fn build(tree: &mut BlueprintTree, parent: Option<ObjectId>, d: &ObjectDesc, read: BlueprintReader, stack: &mut Vec<String>) -> Result<ObjectId, BlueprintError> {
    let id = match &d.instance {
        Some(path) => {
            if stack.contains(path) {
                return Err(BlueprintError(format!("{} ends up containing itself ({})", path, stack.join(" -> "))));
            }
            let text = read(path).map_err(|e| BlueprintError(format!("couldn't read {}: {}", path, e)))?;
            let file = parse(&text).map_err(|e| BlueprintError(format!("{}: {}", path, e)))?;
            stack.push(path.clone());
            let id = build(tree, parent, &file.root, read, stack)?;
            stack.pop();
            // Everything inside comes from the instanced file.
            let mut inside = Vec::new();
            tree.walk_from(id, &mut inside);
            for n in inside.into_iter().skip(1) {
                tree.objects[n].from_instance = true;
            }
            let n = &mut tree.objects[id];
            n.instance_of = Some(path.clone());
            if !d.name.is_empty() {
                n.name = d.name.clone();
            }
            id
        }
        None => {
            let kind = d.kind.unwrap_or(ObjectKind::Object);
            let name = if d.name.is_empty() { kind.name().to_string() } else { d.name.clone() };
            tree.add(parent, Object::new(name, kind))
        }
    };
    let n = &mut tree.objects[id];
    if let Some(t) = d.transform {
        n.transform = t;
    }
    if let Some(v) = d.visible {
        n.visible = v;
    }
    for (k, v) in &d.props {
        n.props.insert(k.clone(), v.clone());
    }
    for s in &d.scripts {
        if !n.scripts.contains(s) {
            n.scripts.push(s.clone());
        }
    }
    for g in &d.groups {
        if !n.groups.contains(g) {
            n.groups.push(g.clone());
        }
    }
    for c in &d.children {
        build(tree, Some(id), c, read, stack)?;
    }
    for (path, ov) in &d.overrides {
        // An object the instanced blueprint no longer has: its changes are dropped.
        let Some(target) = tree.find_path(Some(id), path) else { continue };
        let n = &mut tree.objects[target];
        if let Some(t) = ov.transform {
            n.transform = t;
        }
        if let Some(v) = ov.visible {
            n.visible = v;
        }
        for (k, v) in &ov.props {
            n.props.insert(k.clone(), v.clone());
        }
        for s in &ov.scripts {
            if !n.scripts.contains(s) {
                n.scripts.push(s.clone());
            }
        }
        for g in &ov.groups {
            if !n.groups.contains(g) {
                n.groups.push(g.clone());
            }
        }
        for c in &ov.children {
            build(tree, Some(target), c, read, stack)?;
        }
    }
    Ok(id)
}

/// The path from `root` down to `id` ("Arm/Hand").
fn rel_path(tree: &BlueprintTree, root: ObjectId, id: ObjectId) -> String {
    let mut parts = Vec::new();
    let mut cur = id;
    while cur != root {
        parts.push(tree.objects[cur].name.clone());
        match tree.objects[cur].parent {
            Some(p) => cur = p,
            None => break,
        }
    }
    parts.reverse();
    parts.join("/")
}

/// Differences between an object and the same object in the instanced blueprint.
fn diff(n: &Object, base: &Object) -> ObjectOverride {
    ObjectOverride {
        transform: (n.transform != base.transform).then_some(n.transform),
        visible: (n.visible != base.visible).then_some(n.visible),
        props: n.props.iter().filter(|(k, v)| base.props.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect(),
        scripts: n.scripts.iter().filter(|s| !base.scripts.contains(s)).cloned().collect(),
        groups: n.groups.iter().filter(|g| !base.groups.contains(g)).cloned().collect(),
        children: Vec::new(),
    }
}

/// Turns a tree back into blueprint text. Instanced blueprints are written as references with
/// only the overrides, so editing the instanced file updates every use of it.
pub fn save(tree: &BlueprintTree, read: BlueprintReader) -> Result<String, BlueprintError> {
    let root = tree.root.ok_or_else(|| BlueprintError("the blueprint is empty".into()))?;
    let file = BlueprintFile { format: FORMAT, root: describe(tree, root, read)? };
    Ok(to_text(&file))
}

pub fn describe(tree: &BlueprintTree, id: ObjectId, read: BlueprintReader) -> Result<ObjectDesc, BlueprintError> {
    let n = &tree.objects[id];
    let mut d = ObjectDesc { name: n.name.clone(), ..Default::default() };
    // What this object would be without anything written here.
    let base: Object = match &n.instance_of {
        Some(path) => {
            d.instance = Some(path.clone());
            let mut t = BlueprintTree::new();
            let r = instance(&mut t, None, path, read)?;
            t.objects[r].clone()
        }
        None => {
            d.kind = Some(n.kind);
            Object::new(n.name.clone(), n.kind)
        }
    };
    if n.transform != base.transform {
        d.transform = Some(n.transform);
    }
    if n.visible != base.visible {
        d.visible = Some(n.visible);
    }
    for (k, v) in &n.props {
        if base.props.get(k) != Some(v) {
            d.props.insert(k.clone(), v.clone());
        }
    }
    d.scripts = n.scripts.iter().filter(|s| !base.scripts.contains(s)).cloned().collect();
    d.groups = n.groups.iter().filter(|g| !base.groups.contains(g)).cloned().collect();
    for c in &n.children {
        if !tree.objects[*c].from_instance {
            d.children.push(describe(tree, *c, read)?);
        }
    }
    if let Some(path) = &n.instance_of {
        // Changes inside the instance, and children added under its objects.
        let mut bt = BlueprintTree::new();
        let br = instance(&mut bt, None, path, read)?;
        let mut stack: Vec<ObjectId> = n.children.iter().copied().filter(|c| tree.objects[*c].from_instance).collect();
        while let Some(x) = stack.pop() {
            let xn = &tree.objects[x];
            let rel = rel_path(tree, id, x);
            let mut ov = match bt.find_path(Some(br), &rel) {
                Some(b) => diff(xn, &bt.objects[b]),
                None => ObjectOverride::default(),
            };
            for c in &xn.children {
                if tree.objects[*c].from_instance {
                    stack.push(*c);
                } else {
                    ov.children.push(describe(tree, *c, read)?);
                }
            }
            if !ov.is_empty() {
                d.overrides.insert(rel, ov);
            }
        }
    }
    Ok(d)
}

/// A blueprint file holding a single object of this kind (for "New blueprint").
pub fn new_blueprint_text(name: &str, kind: ObjectKind) -> String {
    to_text(&BlueprintFile { format: FORMAT, root: ObjectDesc { name: name.into(), kind: Some(kind), ..Default::default() } })
}
