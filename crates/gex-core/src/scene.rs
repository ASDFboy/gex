//! Scene files (`.jscene`, RON text). A scene is a saved node tree; any node in it can
//! be an instance of another scene file, with its own overrides on top. A scene whose
//! root is an instance is an inherited scene ("Goblin extends Enemy").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::node::{Node, NodeId, NodeKind, Transform};
use crate::tree::SceneTree;
use crate::variant::Variant;

pub const SCENE_EXT: &str = "jscene";
pub const FORMAT: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneFile {
    pub format: u32,
    pub root: NodeDesc,
}

/// One node as written in a scene file. Only what differs from the defaults (or, for
/// an instance, from the instanced scene) is written.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeDesc {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<NodeKind>,
    /// This node is an instance of that scene file.
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
    pub children: Vec<NodeDesc>,
}

/// Reads scene files by `res://` path, for instancing.
pub type SceneReader<'a> = &'a dyn Fn(&str) -> Result<String, String>;

#[derive(Debug)]
pub struct SceneError(pub String);
impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SceneError {}

pub fn parse(text: &str) -> Result<SceneFile, SceneError> {
    ron::from_str(text).map_err(|e| SceneError(format!("scene file: {}", e)))
}

pub fn to_text(file: &SceneFile) -> String {
    let cfg = ron::ser::PrettyConfig::new().struct_names(false).indentor("  ".to_string());
    ron::ser::to_string_pretty(file, cfg).expect("scenes always serialize")
}

/// Builds a whole tree from scene text.
pub fn load(text: &str, read: SceneReader) -> Result<SceneTree, SceneError> {
    let file = parse(text)?;
    let mut tree = SceneTree::new();
    let mut stack = Vec::new();
    build(&mut tree, None, &file.root, read, &mut stack)?;
    Ok(tree)
}

/// Builds the scene at `path` under `parent` in an existing tree (the scene is
/// instanced: its root remembers the path and its insides are saved by reference).
pub fn instance(tree: &mut SceneTree, parent: Option<NodeId>, path: &str, read: SceneReader) -> Result<NodeId, SceneError> {
    let desc = NodeDesc { instance: Some(path.to_string()), ..Default::default() };
    build(tree, parent, &desc, read, &mut Vec::new())
}

fn build(tree: &mut SceneTree, parent: Option<NodeId>, d: &NodeDesc, read: SceneReader, stack: &mut Vec<String>) -> Result<NodeId, SceneError> {
    let id = match &d.instance {
        Some(path) => {
            if stack.contains(path) {
                return Err(SceneError(format!("{} ends up containing itself ({})", path, stack.join(" -> "))));
            }
            let text = read(path).map_err(|e| SceneError(format!("couldn't read {}: {}", path, e)))?;
            let file = parse(&text).map_err(|e| SceneError(format!("{}: {}", path, e)))?;
            stack.push(path.clone());
            let id = build(tree, parent, &file.root, read, stack)?;
            stack.pop();
            // Everything inside comes from the instanced file.
            let mut inside = Vec::new();
            tree.walk_from(id, &mut inside);
            for n in inside.into_iter().skip(1) {
                tree.nodes[n].from_instance = true;
            }
            let n = &mut tree.nodes[id];
            n.instance_of = Some(path.clone());
            if !d.name.is_empty() {
                n.name = d.name.clone();
            }
            id
        }
        None => {
            let kind = d.kind.unwrap_or(NodeKind::Node);
            let name = if d.name.is_empty() { kind.name().to_string() } else { d.name.clone() };
            tree.add(parent, Node::new(name, kind))
        }
    };
    let n = &mut tree.nodes[id];
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
    Ok(id)
}

/// Turns a tree back into scene text. Instanced scenes are written as references with
/// only the overrides, so editing the instanced file updates every use of it.
pub fn save(tree: &SceneTree, read: SceneReader) -> Result<String, SceneError> {
    let root = tree.root.ok_or_else(|| SceneError("the scene is empty".into()))?;
    let file = SceneFile { format: FORMAT, root: describe(tree, root, read)? };
    Ok(to_text(&file))
}

pub fn describe(tree: &SceneTree, id: NodeId, read: SceneReader) -> Result<NodeDesc, SceneError> {
    let n = &tree.nodes[id];
    let mut d = NodeDesc { name: n.name.clone(), ..Default::default() };
    // What this node would be without anything written here.
    let base: Node = match &n.instance_of {
        Some(path) => {
            d.instance = Some(path.clone());
            let mut t = SceneTree::new();
            let r = instance(&mut t, None, path, read)?;
            t.nodes[r].clone()
        }
        None => {
            d.kind = Some(n.kind);
            Node::new(n.name.clone(), n.kind)
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
        if !tree.nodes[*c].from_instance {
            d.children.push(describe(tree, *c, read)?);
        }
    }
    Ok(d)
}

/// A scene file holding a single node of this kind (for "New scene").
pub fn new_scene_text(name: &str, kind: NodeKind) -> String {
    to_text(&SceneFile { format: FORMAT, root: NodeDesc { name: name.into(), kind: Some(kind), ..Default::default() } })
}
