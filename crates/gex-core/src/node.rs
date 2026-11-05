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
    /// Plays a sound file (with autoplay, or PLAY(node) from a script).
    Sound,
    // ---- 2D ----
    Node2D,
    /// A textured or plain-colored rectangle.
    Sprite2D,
    Label2D,
    Camera2D,
    /// A rectangle that reports collisions (on_collide) with other colliders.
    Collider2D,
    /// A physics body: dynamic (falls, bounces), static (walls, floors) or kinematic
    /// (moved by scripts with MOVE, like a platformer character).
    Body2D,
    /// A grid of tiles from a tileset image; its tiles can be solid for physics.
    TileMap2D,
    /// Sparks, smoke, dust: many small sprites that fly out, fade and shrink.
    Particles2D,
    /// A light in 2D: brightens the dark a Camera2D's `ambient` sets.
    Light2D,
    // ---- UI (screen space, in the game window's pixels) ----
    Panel,
    Button,
    Label,
    TextInput,
    ProgressBar,
    Image,
    // ---- 3D ----
    Node3D,
    /// A primitive (cube, sphere, plane) or a model resource.
    Mesh3D,
    Camera3D,
    Light3D,
    /// A 3D physics body (box, sphere or capsule).
    Body3D,
    /// A picture in 3D: faces the camera (billboard) or lies flat like a sign.
    Sprite3D,
    /// Text in the 3D world, always facing the screen.
    Label3D,
    /// Sparks, smoke and dust in 3D.
    Particles3D,
    /// A 3D area (box or sphere) that reports what enters it (on_collide events): triggers, pickups.
    Collider3D,
    /// Sky, ambient light and fog for 3D scenes (the first one in the tree is used).
    Environment,
}

pub const ALL_KINDS: &[NodeKind] = &[
    NodeKind::Node,
    NodeKind::Sound,
    NodeKind::Node2D,
    NodeKind::Sprite2D,
    NodeKind::Label2D,
    NodeKind::Camera2D,
    NodeKind::Collider2D,
    NodeKind::Body2D,
    NodeKind::TileMap2D,
    NodeKind::Particles2D,
    NodeKind::Light2D,
    NodeKind::Panel,
    NodeKind::Button,
    NodeKind::Label,
    NodeKind::TextInput,
    NodeKind::ProgressBar,
    NodeKind::Image,
    NodeKind::Node3D,
    NodeKind::Mesh3D,
    NodeKind::Camera3D,
    NodeKind::Light3D,
    NodeKind::Body3D,
    NodeKind::Collider3D,
    NodeKind::Sprite3D,
    NodeKind::Label3D,
    NodeKind::Particles3D,
    NodeKind::Environment,
];

impl NodeKind {
    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Node => "Node",
            NodeKind::Sound => "Sound",
            NodeKind::Node2D => "Node2D",
            NodeKind::Sprite2D => "Sprite2D",
            NodeKind::Label2D => "Label2D",
            NodeKind::Camera2D => "Camera2D",
            NodeKind::Collider2D => "Collider2D",
            NodeKind::Body2D => "Body2D",
            NodeKind::TileMap2D => "TileMap2D",
            NodeKind::Particles2D => "Particles2D",
            NodeKind::Light2D => "Light2D",
            NodeKind::Panel => "Panel",
            NodeKind::Button => "Button",
            NodeKind::Label => "Label",
            NodeKind::TextInput => "TextInput",
            NodeKind::ProgressBar => "ProgressBar",
            NodeKind::Image => "Image",
            NodeKind::Node3D => "Node3D",
            NodeKind::Mesh3D => "Mesh3D",
            NodeKind::Camera3D => "Camera3D",
            NodeKind::Light3D => "Light3D",
            NodeKind::Body3D => "Body3D",
            NodeKind::Collider3D => "Collider3D",
            NodeKind::Sprite3D => "Sprite3D",
            NodeKind::Label3D => "Label3D",
            NodeKind::Particles3D => "Particles3D",
            NodeKind::Environment => "Environment",
        }
    }
    pub fn from_name(s: &str) -> Option<NodeKind> {
        ALL_KINDS.iter().copied().find(|k| k.name().eq_ignore_ascii_case(s))
    }
    pub fn is_2d(self) -> bool {
        matches!(self, NodeKind::Node2D | NodeKind::Sprite2D | NodeKind::Label2D | NodeKind::Camera2D | NodeKind::Collider2D | NodeKind::Body2D | NodeKind::TileMap2D | NodeKind::Particles2D | NodeKind::Light2D)
    }
    pub fn is_ui(self) -> bool {
        matches!(self, NodeKind::Panel | NodeKind::Button | NodeKind::Label | NodeKind::TextInput | NodeKind::ProgressBar | NodeKind::Image)
    }
    pub fn is_3d(self) -> bool {
        matches!(self, NodeKind::Node3D | NodeKind::Mesh3D | NodeKind::Camera3D | NodeKind::Light3D | NodeKind::Body3D | NodeKind::Collider3D | NodeKind::Sprite3D | NodeKind::Label3D | NodeKind::Particles3D | NodeKind::Environment)
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
                ("hframes", Num(1.0)),
                ("vframes", Num(1.0)),
                ("frame", Num(0.0)),
                ("animations", Text(String::new())),
                ("animation", Text(String::new())),
                ("anim_fps", Num(10.0)),
                ("anim_loop", Bool(true)),
                ("flip_h", Bool(false)),
                ("flip_v", Bool(false)),
                ("parallax_x", Num(1.0)),
                ("parallax_y", Num(1.0)),
                ("repeat_x", Bool(false)),
                ("repeat_y", Bool(false)),
            ],
            NodeKind::Light2D => vec![("color", Color([1.0, 0.9, 0.7, 1.0])), ("energy", Num(1.0)), ("radius", Num(200.0)), ("texture", Res(String::new()))],
            NodeKind::TileMap2D => vec![
                ("tileset", Res(String::new())),
                ("tile_width", Num(32.0)),
                ("tile_height", Num(32.0)),
                ("tiles", Text(String::new())),
                ("solid", Bool(true)),
                ("solid_tiles", Text(String::new())),
                ("layer", Num(-5.0)),
                ("color", Color([1.0; 4])),
                ("parallax_x", Num(1.0)),
                ("parallax_y", Num(1.0)),
            ],
            NodeKind::Particles2D => vec![
                ("emitting", Bool(true)),
                ("rate", Num(30.0)),
                ("lifetime", Num(1.0)),
                ("texture", Res(String::new())),
                ("direction", Num(-90.0)),
                ("spread", Num(30.0)),
                ("speed", Num(120.0)),
                ("speed_random", Num(0.3)),
                ("gravity", Num(0.0)),
                ("size", Num(8.0)),
                ("size_end", Num(2.0)),
                ("color", Color([1.0, 0.8, 0.3, 1.0])),
                ("color_end", Color([1.0, 0.3, 0.1, 0.0])),
                ("spin", Num(0.0)),
                ("area", Num(0.0)),
                ("layer", Num(1.0)),
            ],
            NodeKind::Panel => vec![
                ("width", Num(200.0)),
                ("height", Num(100.0)),
                ("color", Color([0.1, 0.12, 0.16, 0.92])),
                ("border_color", Color([0.4, 0.75, 0.96, 1.0])),
                ("border", Num(1.0)),
                ("radius", Num(4.0)),
                ("texture", Res(String::new())),
                ("slice", Num(0.0)),
                ("anchor", Text("top_left".into())),
            ],
            NodeKind::Button => vec![
                ("text", Text("Button".into())),
                ("width", Num(140.0)),
                ("height", Num(36.0)),
                ("size", Num(16.0)),
                ("color", Color([0.14, 0.2, 0.28, 1.0])),
                ("text_color", Color([0.78, 0.84, 0.88, 1.0])),
                ("disabled", Bool(false)),
                ("font", Res(String::new())),
                ("texture", Res(String::new())),
                ("slice", Num(0.0)),
                ("anchor", Text("top_left".into())),
            ],
            NodeKind::Label => vec![
                ("text", Text("Label".into())),
                ("size", Num(18.0)),
                ("color", Color([1.0; 4])),
                ("align", Text("left".into())),
                ("font", Res(String::new())),
                ("anchor", Text("top_left".into())),
            ],
            NodeKind::TextInput => vec![
                ("text", Text(String::new())),
                ("placeholder", Text("type here".into())),
                ("width", Num(200.0)),
                ("height", Num(30.0)),
                ("size", Num(16.0)),
                ("font", Res(String::new())),
                ("anchor", Text("top_left".into())),
            ],
            NodeKind::ProgressBar => vec![
                ("value", Num(0.5)),
                ("max", Num(1.0)),
                ("width", Num(200.0)),
                ("height", Num(16.0)),
                ("color", Color([0.4, 0.75, 0.96, 1.0])),
                ("back_color", Color([0.1, 0.1, 0.12, 1.0])),
                ("anchor", Text("top_left".into())),
            ],
            NodeKind::Image => vec![
                ("texture", Res(String::new())),
                ("width", Num(64.0)),
                ("height", Num(64.0)),
                ("color", Color([1.0; 4])),
                ("slice", Num(0.0)),
                ("anchor", Text("top_left".into())),
            ],
            NodeKind::Label2D => vec![("text", Text("Label".into())), ("size", Num(16.0)), ("color", Color([1.0; 4])), ("layer", Num(10.0)), ("font", Res(String::new()))],
            NodeKind::Camera2D => vec![
                ("zoom", Num(1.0)),
                ("current", Bool(true)),
                ("smoothing", Num(0.0)),
                ("limits", Bool(false)),
                ("limit_left", Num(0.0)),
                ("limit_top", Num(0.0)),
                ("limit_right", Num(1280.0)),
                ("limit_bottom", Num(720.0)),
                ("ambient", Color([1.0; 4])),
            ],
            NodeKind::Collider2D => vec![("width", Num(32.0)), ("height", Num(32.0)), ("layer", Text(String::new())), ("mask", Text(String::new()))],
            NodeKind::Collider3D => vec![
                ("shape", Text("box".into())),
                ("width", Num(1.0)),
                ("height", Num(1.0)),
                ("depth", Num(1.0)),
                ("radius", Num(0.5)),
                ("layer", Text(String::new())),
                ("mask", Text(String::new())),
            ],
            NodeKind::Mesh3D => vec![
                ("mesh", Text("cube".into())),
                ("color", Color([0.8, 0.8, 0.8, 1.0])),
                ("texture", Res(String::new())),
                ("uv_scale_x", Num(1.0)),
                ("uv_scale_y", Num(1.0)),
                ("cast_shadows", Bool(true)),
                ("collision", Bool(false)),
                ("animation", Text(String::new())),
                ("anim_speed", Num(1.0)),
                ("anim_loop", Bool(true)),
                ("anim_time", Num(0.0)),
                ("anim_blend", Num(0.2)),
                ("normal_map", Res(String::new())),
                ("specular", Num(0.15)),
                ("emissive", Num(0.0)),
                ("unlit", Bool(false)),
                ("shader", Res(String::new())),
                ("map_props", Bool(true)),
            ],
            NodeKind::Sprite3D => vec![
                ("texture", Res(String::new())),
                ("width", Num(1.0)),
                ("height", Num(1.0)),
                ("color", Color([1.0; 4])),
                ("billboard", Bool(true)),
                ("unlit", Bool(true)),
                ("cast_shadows", Bool(false)),
            ],
            NodeKind::Label3D => vec![("text", Text("Label".into())), ("size", Num(24.0)), ("color", Color([1.0; 4])), ("font", Res(String::new()))],
            NodeKind::Particles3D => vec![
                ("emitting", Bool(true)),
                ("rate", Num(30.0)),
                ("lifetime", Num(1.5)),
                ("texture", Res(String::new())),
                ("direction_x", Num(0.0)),
                ("direction_y", Num(1.0)),
                ("direction_z", Num(0.0)),
                ("spread", Num(25.0)),
                ("speed", Num(2.0)),
                ("speed_random", Num(0.3)),
                ("gravity", Num(-2.0)),
                ("size", Num(0.3)),
                ("size_end", Num(0.05)),
                ("color", Color([1.0, 0.8, 0.3, 1.0])),
                ("color_end", Color([1.0, 0.3, 0.1, 0.0])),
                ("area", Num(0.0)),
                ("glow", Bool(true)),
            ],
            NodeKind::Camera3D => vec![("fov", Num(70.0)), ("current", Bool(true)), ("near", Num(0.05)), ("far", Num(500.0))],
            NodeKind::Light3D => vec![
                ("type", Text("directional".into())),
                ("color", Color([1.0, 1.0, 1.0, 1.0])),
                ("energy", Num(1.0)),
                ("range", Num(10.0)),
                ("angle", Num(45.0)),
                ("shadows", Bool(true)),
            ],
            NodeKind::Environment => vec![
                ("sky_top", Color([0.32, 0.5, 0.78, 1.0])),
                ("sky_horizon", Color([0.72, 0.8, 0.88, 1.0])),
                ("ground", Color([0.22, 0.22, 0.24, 1.0])),
                ("sky_texture", Res(String::new())),
                ("ambient", Color([0.55, 0.6, 0.7, 1.0])),
                ("ambient_energy", Num(0.35)),
                ("fog_color", Color([0.72, 0.8, 0.88, 1.0])),
                ("fog_start", Num(0.0)),
                ("fog_end", Num(0.0)),
                ("shadow_distance", Num(40.0)),
                ("bloom", Num(0.0)),
                ("bloom_threshold", Num(0.9)),
            ],
            NodeKind::Sound => vec![
                ("stream", Res(String::new())),
                ("volume", Num(1.0)),
                ("pitch", Num(1.0)),
                ("loop", Bool(false)),
                ("autoplay", Bool(false)),
                ("playing", Bool(false)),
                ("spatial", Bool(false)),
                ("range", Num(0.0)),
            ],
            NodeKind::Body2D => vec![
                ("body", Text("dynamic".into())),
                ("shape", Text("box".into())),
                ("width", Num(32.0)),
                ("height", Num(32.0)),
                ("radius", Num(16.0)),
                ("gravity_scale", Num(1.0)),
                ("friction", Num(0.5)),
                ("bounce", Num(0.0)),
                ("density", Num(1.0)),
                ("fixed_rotation", Bool(false)),
                ("vx", Num(0.0)),
                ("vy", Num(0.0)),
                ("on_floor", Bool(false)),
                ("layer", Text(String::new())),
                ("mask", Text(String::new())),
            ],
            NodeKind::Body3D => vec![
                ("body", Text("dynamic".into())),
                ("shape", Text("box".into())),
                ("width", Num(1.0)),
                ("height", Num(1.0)),
                ("depth", Num(1.0)),
                ("radius", Num(0.5)),
                ("gravity_scale", Num(1.0)),
                ("friction", Num(0.5)),
                ("bounce", Num(0.0)),
                ("density", Num(1.0)),
                ("fixed_rotation", Bool(false)),
                ("vx", Num(0.0)),
                ("vy", Num(0.0)),
                ("vz", Num(0.0)),
                ("on_floor", Bool(false)),
                ("layer", Text(String::new())),
                ("mask", Text(String::new())),
            ],
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
