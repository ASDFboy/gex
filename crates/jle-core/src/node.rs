//! Nodes: the one building block. A node has a kind (what the engine does with it),
//! a transform, properties, scripts and children. A "scene" is just a node tree saved
//! to a file, and any node can be an instance of another scene.

use std::collections::BTreeMap;

use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::variant::Variant;

/// What the engine does with a node. 2D kinds use x/y in pixels (y grows downward,
/// like GameMaker) and `rotation.z` in degrees; 3D kinds use metres, y up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    /// A plain grouping node (no transform effect beyond passing it on).
    Node,
    // ---- 2D ----
    Node2D,
    /// A textured or plain-colored rectangle.
    Sprite2D,
    Label2D,
    Camera2D,
    /// A rectangle that reports collisions (on_collide) with other colliders.
    Collider2D,
    // ---- 3D ----
    Node3D,
    /// A primitive (cube, sphere, plane) or a model resource.
    Mesh3D,
    Camera3D,
    Light3D,
}

pub const ALL_KINDS: &[NodeKind] = &[
    NodeKind::Node,
    NodeKind::Node2D,
    NodeKind::Sprite2D,
    NodeKind::Label2D,
    NodeKind::Camera2D,
    NodeKind::Collider2D,
    NodeKind::Node3D,
    NodeKind::Mesh3D,
    NodeKind::Camera3D,
    NodeKind::Light3D,
];

impl NodeKind {
    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Node => "Node",
            NodeKind::Node2D => "Node2D",
            NodeKind::Sprite2D => "Sprite2D",
            NodeKind::Label2D => "Label2D",
            NodeKind::Camera2D => "Camera2D",
            NodeKind::Collider2D => "Collider2D",
            NodeKind::Node3D => "Node3D",
            NodeKind::Mesh3D => "Mesh3D",
            NodeKind::Camera3D => "Camera3D",
            NodeKind::Light3D => "Light3D",
        }
    }
    pub fn from_name(s: &str) -> Option<NodeKind> {
        ALL_KINDS.iter().copied().find(|k| k.name().eq_ignore_ascii_case(s))
    }
    pub fn is_2d(self) -> bool {
        matches!(self, NodeKind::Node2D | NodeKind::Sprite2D | NodeKind::Label2D | NodeKind::Camera2D | NodeKind::Collider2D)
    }
    pub fn is_3d(self) -> bool {
        matches!(self, NodeKind::Node3D | NodeKind::Mesh3D | NodeKind::Camera3D | NodeKind::Light3D)
    }
    /// The built-in properties this kind has, with their defaults. Scene files only
    /// store values that differ from these.
    pub fn default_props(self) -> Vec<(&'static str, Variant)> {
        use Variant::*;
        match self {
            NodeKind::Sprite2D => vec![
                ("texture", Res(String::new())),
                ("width", Num(32.0)),
                ("height", Num(32.0)),
                ("color", Color([1.0, 1.0, 1.0, 1.0])),
                ("centered", Bool(true)),
                ("layer", Num(0.0)),
            ],
            NodeKind::Label2D => vec![("text", Text("Label".into())), ("size", Num(16.0)), ("color", Color([1.0; 4])), ("layer", Num(10.0))],
            NodeKind::Camera2D => vec![("zoom", Num(1.0)), ("current", Bool(true))],
            NodeKind::Collider2D => vec![("width", Num(32.0)), ("height", Num(32.0)), ("group", Text(String::new()))],
            NodeKind::Mesh3D => vec![("mesh", Text("cube".into())), ("color", Color([0.8, 0.8, 0.8, 1.0])), ("texture", Res(String::new()))],
            NodeKind::Camera3D => vec![("fov", Num(70.0)), ("current", Bool(true)), ("near", Num(0.05)), ("far", Num(500.0))],
            NodeKind::Light3D => vec![("color", Color([1.0, 1.0, 1.0, 1.0])), ("energy", Num(1.0))],
            _ => vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub position: Vec3,
    /// Euler angles in degrees (applied Y, then X, then Z). 2D nodes use only `z`.
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { position: Vec3::ZERO, rotation: Vec3::ZERO, scale: Vec3::ONE }
    }
}

impl Transform {
    pub fn quat(&self) -> Quat {
        let r = self.rotation * (std::f32::consts::PI / 180.0);
        Quat::from_euler(EulerRot::YXZ, r.y, r.x, r.z)
    }
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.quat(), self.position)
    }
    pub fn is_default(&self) -> bool {
        *self == Transform::default()
    }
}

slotmap::new_key_type! {
    /// A node's handle in its tree. Stays valid until the node is removed.
    pub struct NodeId;
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub transform: Transform,
    pub visible: bool,
    /// Built-in properties of the kind, plus any custom ones you add.
    pub props: BTreeMap<String, Variant>,
    /// `res://` paths of the .jlb scripts attached to this node, run in this order.
    pub scripts: Vec<String>,
    /// Groups let scripts find sets of nodes ("enemies") and colliders filter hits.
    pub groups: Vec<String>,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    /// Set on the root of an instanced scene: the scene file it came from.
    pub instance_of: Option<String>,
    /// True for nodes that came from inside an instanced scene (saved by reference, not
    /// one by one).
    pub from_instance: bool,
}

impl Node {
    pub fn new(name: impl Into<String>, kind: NodeKind) -> Node {
        let props = kind.default_props().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        Node {
            name: name.into(),
            kind,
            transform: Transform::default(),
            visible: true,
            props,
            scripts: Vec::new(),
            groups: Vec::new(),
            parent: None,
            children: Vec::new(),
            instance_of: None,
            from_instance: false,
        }
    }
    pub fn prop(&self, k: &str) -> Option<&Variant> {
        self.props.get(k)
    }
    pub fn num(&self, k: &str, default: f64) -> f64 {
        self.props.get(k).and_then(Variant::as_f64).unwrap_or(default)
    }
    pub fn text(&self, k: &str) -> &str {
        self.props.get(k).and_then(Variant::as_str).unwrap_or("")
    }
    pub fn color(&self, k: &str) -> [f32; 4] {
        self.props.get(k).map(Variant::as_color).unwrap_or([1.0; 4])
    }
}
