//! Objects: the one building block. An object has a kind (what the engine does with it),
//! a transform, properties, scripts and children. A "blueprint" is just an object tree saved
//! to a file, and any object can be an instance of another blueprint.

use std::collections::BTreeMap;

use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::variant::Variant;

/// What the engine does with an object. 2D kinds use x/y in pixels (y grows downward,
/// like GameMaker) and `rotation.z` in degrees; 3D kinds use metres, y up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectKind {
    /// Plays animation clips (made in the Timeline) on the objects around it.
    Animator,
    /// A plain grouping object (no transform effect beyond passing it on).
    #[serde(alias = "Node")]
    Object,
    /// Plays a sound file (with autoplay, or PLAY(object) from a script).
    Sound,
    /// Counts down and runs on_timeout when it reaches zero (once, or over and over).
    Timer,
    // ---- 2D ----
    #[serde(alias = "Node2D")]
    Object2D,
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
    /// Places the UI objects under it side by side, left to right.
    Row,
    /// Places the UI objects under it one under another.
    Column,
    /// Places the UI objects under it in a grid, a set number across.
    Grid,
    /// A box the UI objects under it stack in and scroll through (mouse wheel, or dragging its bar).
    ScrollBox,
    /// A box that's ticked or not, with text beside it.
    Checkbox,
    /// A bar with a handle to drag between a lowest and highest value.
    Slider,
    // ---- 3D ----
    #[serde(alias = "Node3D")]
    Object3D,
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
    /// Sky, ambient light and fog for 3D blueprints (the first one in the tree is used).
    Environment,
}

pub const ALL_KINDS: &[ObjectKind] = &[
    ObjectKind::Object,
    ObjectKind::Animator,
    ObjectKind::Sound,
    ObjectKind::Timer,
    ObjectKind::Object2D,
    ObjectKind::Sprite2D,
    ObjectKind::Label2D,
    ObjectKind::Camera2D,
    ObjectKind::Collider2D,
    ObjectKind::Body2D,
    ObjectKind::TileMap2D,
    ObjectKind::Particles2D,
    ObjectKind::Light2D,
    ObjectKind::Panel,
    ObjectKind::Button,
    ObjectKind::Label,
    ObjectKind::TextInput,
    ObjectKind::ProgressBar,
    ObjectKind::Image,
    ObjectKind::Row,
    ObjectKind::Column,
    ObjectKind::Grid,
    ObjectKind::ScrollBox,
    ObjectKind::Checkbox,
    ObjectKind::Slider,
    ObjectKind::Object3D,
    ObjectKind::Mesh3D,
    ObjectKind::Camera3D,
    ObjectKind::Light3D,
    ObjectKind::Body3D,
    ObjectKind::Collider3D,
    ObjectKind::Sprite3D,
    ObjectKind::Label3D,
    ObjectKind::Particles3D,
    ObjectKind::Environment,
];

impl ObjectKind {
    pub fn name(self) -> &'static str {
        match self {
            ObjectKind::Object => "Object",
            ObjectKind::Animator => "Animator",
            ObjectKind::Sound => "Sound",
            ObjectKind::Timer => "Timer",
            ObjectKind::Object2D => "Object2D",
            ObjectKind::Sprite2D => "Sprite2D",
            ObjectKind::Label2D => "Label2D",
            ObjectKind::Camera2D => "Camera2D",
            ObjectKind::Collider2D => "Collider2D",
            ObjectKind::Body2D => "Body2D",
            ObjectKind::TileMap2D => "TileMap2D",
            ObjectKind::Particles2D => "Particles2D",
            ObjectKind::Light2D => "Light2D",
            ObjectKind::Panel => "Panel",
            ObjectKind::Button => "Button",
            ObjectKind::Label => "Label",
            ObjectKind::TextInput => "TextInput",
            ObjectKind::ProgressBar => "ProgressBar",
            ObjectKind::Image => "Image",
            ObjectKind::Row => "Row",
            ObjectKind::Column => "Column",
            ObjectKind::Grid => "Grid",
            ObjectKind::ScrollBox => "ScrollBox",
            ObjectKind::Checkbox => "Checkbox",
            ObjectKind::Slider => "Slider",
            ObjectKind::Object3D => "Object3D",
            ObjectKind::Mesh3D => "Mesh3D",
            ObjectKind::Camera3D => "Camera3D",
            ObjectKind::Light3D => "Light3D",
            ObjectKind::Body3D => "Body3D",
            ObjectKind::Collider3D => "Collider3D",
            ObjectKind::Sprite3D => "Sprite3D",
            ObjectKind::Label3D => "Label3D",
            ObjectKind::Particles3D => "Particles3D",
            ObjectKind::Environment => "Environment",
        }
    }
    pub fn from_name(s: &str) -> Option<ObjectKind> {
        // The names from before objects were called objects still work.
        let s = match s.to_ascii_lowercase().as_str() {
            "node" => "Object",
            "node2d" => "Object2D",
            "node3d" => "Object3D",
            _ => s,
        };
        ALL_KINDS.iter().copied().find(|k| k.name().eq_ignore_ascii_case(s))
    }
    pub fn is_2d(self) -> bool {
        matches!(self, ObjectKind::Object2D | ObjectKind::Sprite2D | ObjectKind::Label2D | ObjectKind::Camera2D | ObjectKind::Collider2D | ObjectKind::Body2D | ObjectKind::TileMap2D | ObjectKind::Particles2D | ObjectKind::Light2D)
    }
    pub fn is_ui(self) -> bool {
        matches!(
            self,
            ObjectKind::Panel
                | ObjectKind::Button
                | ObjectKind::Label
                | ObjectKind::TextInput
                | ObjectKind::ProgressBar
                | ObjectKind::Image
                | ObjectKind::Row
                | ObjectKind::Column
                | ObjectKind::Grid
                | ObjectKind::ScrollBox
                | ObjectKind::Checkbox
                | ObjectKind::Slider
        )
    }
    /// UI that places the UI objects under it (their own positions are ignored).
    pub fn is_layout(self) -> bool {
        matches!(self, ObjectKind::Row | ObjectKind::Column | ObjectKind::Grid | ObjectKind::ScrollBox)
    }
    pub fn is_3d(self) -> bool {
        matches!(self, ObjectKind::Object3D | ObjectKind::Mesh3D | ObjectKind::Camera3D | ObjectKind::Light3D | ObjectKind::Body3D | ObjectKind::Collider3D | ObjectKind::Sprite3D | ObjectKind::Label3D | ObjectKind::Particles3D | ObjectKind::Environment)
    }
    /// The built-in properties this kind has, with their defaults. Blueprint files only
    /// store values that differ from these.
    pub fn default_props(self) -> Vec<(&'static str, Variant)> {
        use Variant::*;
        match self {
            ObjectKind::Sprite2D => vec![
                ("texture", Res(String::new())),
                ("width", Num(32.0)),
                ("height", Num(32.0)),
                ("color", Color([1.0, 1.0, 1.0, 1.0])),
                ("centered", Bool(true)),
                ("solid", Bool(false)),
                ("solid_shape", Text("box".into())),
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
                ("opacity", Num(1.0)),
                ("blend", Text("normal".into())),
                ("filter", Text("project".into())),
                ("lit", Bool(true)),
                ("mouse_precise", Bool(false)),
                ("pivot_x", Num(0.5)),
                ("pivot_y", Num(0.5)),
                ("brightness", Num(0.0)),
                ("hue", Num(0.0)),
                ("saturation", Num(1.0)),
                ("invert", Num(0.0)),
                ("shader", Res(String::new())),
                ("value_1", Num(0.0)),
                ("value_2", Num(0.0)),
                ("value_3", Num(0.0)),
                ("value_4", Num(0.0)),
            ],
            ObjectKind::Light2D => vec![("color", Color([1.0, 0.9, 0.7, 1.0])), ("energy", Num(1.0)), ("radius", Num(200.0)), ("texture", Res(String::new()))],
            ObjectKind::TileMap2D => vec![
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
                ("opacity", Num(1.0)),
                ("blend", Text("normal".into())),
                ("filter", Text("project".into())),
                ("lit", Bool(true)),
            ],
            ObjectKind::Particles2D => vec![
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
                ("three_colors", Bool(false)),
                ("color_mid", Color([1.0, 0.55, 0.2, 0.5])),
                ("spin", Num(0.0)),
                ("start_rotation", Num(0.0)),
                ("start_rotation_random", Num(360.0)),
                ("align", Bool(false)),
                ("size_random", Num(0.0)),
                ("aspect", Num(1.0)),
                ("lifetime_random", Num(0.2)),
                ("acceleration", Num(0.0)),
                ("direction_wiggle", Num(0.0)),
                ("area_width", Num(0.0)),
                ("area_height", Num(0.0)),
                ("frame_count", Num(0.0)),
                ("gravity_direction", Num(90.0)),
                ("damping", Num(0.0)),
                ("hframes", Num(1.0)),
                ("vframes", Num(1.0)),
                ("anim_fps", Num(0.0)),
                ("frame_random", Bool(false)),
                ("area", Num(0.0)),
                ("layer", Num(1.0)),
                ("opacity", Num(1.0)),
                ("blend", Text("normal".into())),
                ("filter", Text("project".into())),
                ("lit", Bool(true)),
            ],
            ObjectKind::Panel => vec![
                ("width", Num(200.0)),
                ("height", Num(100.0)),
                ("color", Color([0.1, 0.12, 0.16, 0.92])),
                ("border_color", Color([0.4, 0.75, 0.96, 1.0])),
                ("border", Num(1.0)),
                ("radius", Num(4.0)),
                ("texture", Res(String::new())),
                ("slice", Num(0.0)),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::Button => vec![
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
                ("opacity", Num(1.0)),
            ],
            ObjectKind::Label => vec![
                ("text", Text("Label".into())),
                ("size", Num(18.0)),
                ("color", Color([1.0; 4])),
                ("align", Text("left".into())),
                ("font", Res(String::new())),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::TextInput => vec![
                ("text", Text(String::new())),
                ("placeholder", Text("type here".into())),
                ("width", Num(200.0)),
                ("height", Num(30.0)),
                ("size", Num(16.0)),
                ("font", Res(String::new())),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::ProgressBar => vec![
                ("value", Num(0.5)),
                ("max", Num(1.0)),
                ("width", Num(200.0)),
                ("height", Num(16.0)),
                ("color", Color([0.4, 0.75, 0.96, 1.0])),
                ("back_color", Color([0.1, 0.1, 0.12, 1.0])),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::Image => vec![
                ("texture", Res(String::new())),
                ("width", Num(64.0)),
                ("height", Num(64.0)),
                ("color", Color([1.0; 4])),
                ("slice", Num(0.0)),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::Row => vec![("spacing", Num(8.0)), ("padding", Num(0.0)), ("align", Text("top".into())), ("anchor", Text("top_left".into())), ("opacity", Num(1.0))],
            ObjectKind::Column => vec![("spacing", Num(8.0)), ("padding", Num(0.0)), ("align", Text("left".into())), ("anchor", Text("top_left".into())), ("opacity", Num(1.0))],
            ObjectKind::Grid => vec![("columns", Num(2.0)), ("spacing", Num(8.0)), ("padding", Num(0.0)), ("anchor", Text("top_left".into())), ("opacity", Num(1.0))],
            ObjectKind::ScrollBox => vec![
                ("width", Num(240.0)),
                ("height", Num(200.0)),
                ("spacing", Num(8.0)),
                ("padding", Num(0.0)),
                ("align", Text("left".into())),
                ("color", Color([0.0, 0.0, 0.0, 0.0])),
                ("bar_color", Color([0.4, 0.75, 0.96, 0.8])),
                ("scroll", Num(0.0)),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::Checkbox => vec![
                ("text", Text("Checkbox".into())),
                ("checked", Bool(false)),
                ("width", Num(160.0)),
                ("height", Num(28.0)),
                ("size", Num(16.0)),
                ("color", Color([0.14, 0.2, 0.28, 1.0])),
                ("check_color", Color([0.4, 0.75, 0.96, 1.0])),
                ("text_color", Color([0.78, 0.84, 0.88, 1.0])),
                ("disabled", Bool(false)),
                ("font", Res(String::new())),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::Slider => vec![
                ("value", Num(0.5)),
                ("min", Num(0.0)),
                ("max", Num(1.0)),
                ("step", Num(0.0)),
                ("width", Num(200.0)),
                ("height", Num(20.0)),
                ("color", Color([0.4, 0.75, 0.96, 1.0])),
                ("back_color", Color([0.1, 0.1, 0.12, 1.0])),
                ("disabled", Bool(false)),
                ("anchor", Text("top_left".into())),
                ("opacity", Num(1.0)),
            ],
            ObjectKind::Label2D => vec![
                ("text", Text("Label".into())),
                ("size", Num(16.0)),
                ("color", Color([1.0; 4])),
                ("layer", Num(10.0)),
                ("font", Res(String::new())),
                ("opacity", Num(1.0)),
                ("align", Text("left".into())),
                ("valign", Text("top".into())),
                ("wrap_width", Num(0.0)),
                ("outline", Num(0.0)),
                ("outline_color", Color([0.0, 0.0, 0.0, 1.0])),
            ],
            ObjectKind::Camera2D => vec![
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
            ObjectKind::Collider2D => vec![("width", Num(32.0)), ("height", Num(32.0)), ("layer", Text(String::new())), ("mask", Text(String::new()))],
            ObjectKind::Collider3D => vec![
                ("shape", Text("box".into())),
                ("width", Num(1.0)),
                ("height", Num(1.0)),
                ("depth", Num(1.0)),
                ("radius", Num(0.5)),
                ("layer", Text(String::new())),
                ("mask", Text(String::new())),
            ],
            ObjectKind::Mesh3D => vec![
                ("mesh", Text("cube".into())),
                ("color", Color([0.8, 0.8, 0.8, 1.0])),
                ("texture", Res(String::new())),
                ("uv_scale_x", Num(1.0)),
                ("uv_scale_y", Num(1.0)),
                ("face_front", Res(String::new())),
                ("face_back", Res(String::new())),
                ("face_left", Res(String::new())),
                ("face_right", Res(String::new())),
                ("face_top", Res(String::new())),
                ("face_bottom", Res(String::new())),
                ("hide_front", Bool(false)),
                ("hide_back", Bool(false)),
                ("hide_left", Bool(false)),
                ("hide_right", Bool(false)),
                ("hide_top", Bool(false)),
                ("hide_bottom", Bool(false)),
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
                ("sides", Text("auto".into())),
                ("emissive_color", Color([1.0; 4])),
                ("rim", Num(0.0)),
                ("rim_color", Color([1.0; 4])),
                ("opacity", Num(1.0)),
                ("blend", Text("normal".into())),
                ("filter", Text("project".into())),
                ("receive_shadows", Bool(true)),
                ("fog", Bool(true)),
                ("on_top", Bool(false)),
                ("alpha_cut", Num(0.5)),
                ("uv_offset_x", Num(0.0)),
                ("uv_offset_y", Num(0.0)),
                ("uv_scroll_x", Num(0.0)),
                ("uv_scroll_y", Num(0.0)),
                ("draw_distance", Num(0.0)),
            ],
            ObjectKind::Sprite3D => vec![
                ("texture", Res(String::new())),
                ("width", Num(1.0)),
                ("height", Num(1.0)),
                ("color", Color([1.0; 4])),
                ("billboard", Bool(true)),
                ("unlit", Bool(true)),
                ("cast_shadows", Bool(false)),
                ("emissive", Num(0.0)),
                ("opacity", Num(1.0)),
                ("blend", Text("normal".into())),
                ("filter", Text("project".into())),
                ("receive_shadows", Bool(true)),
                ("fog", Bool(true)),
                ("on_top", Bool(false)),
                ("alpha_cut", Num(0.5)),
                ("uv_offset_x", Num(0.0)),
                ("uv_offset_y", Num(0.0)),
                ("uv_scroll_x", Num(0.0)),
                ("uv_scroll_y", Num(0.0)),
                ("draw_distance", Num(0.0)),
            ],
            ObjectKind::Label3D => vec![("text", Text("Label".into())), ("size", Num(24.0)), ("color", Color([1.0; 4])), ("font", Res(String::new())), ("opacity", Num(1.0))],
            ObjectKind::Particles3D => vec![
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
                ("opacity", Num(1.0)),
                ("filter", Text("project".into())),
                ("fog", Bool(true)),
            ],
            ObjectKind::Camera3D => vec![("fov", Num(70.0)), ("current", Bool(true)), ("near", Num(0.05)), ("far", Num(500.0)), ("ortho", Bool(false)), ("ortho_size", Num(10.0))],
            ObjectKind::Light3D => vec![
                ("type", Text("directional".into())),
                ("color", Color([1.0, 1.0, 1.0, 1.0])),
                ("energy", Num(1.0)),
                ("range", Num(10.0)),
                ("angle", Num(45.0)),
                ("shadows", Bool(true)),
                ("softness", Num(0.1)),
                ("falloff", Num(2.0)),
                ("specular", Num(1.0)),
                ("shadow_strength", Num(1.0)),
                ("shadow_bias", Num(-1.0)),
            ],
            ObjectKind::Environment => vec![
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
                ("sky_energy", Num(1.0)),
                ("fog_mode", Text("linear".into())),
                ("fog_density", Num(0.02)),
                ("shadow_split", Num(0.25)),
                ("exposure", Num(1.0)),
                ("contrast", Num(1.0)),
                ("saturation", Num(1.0)),
                ("tint", Color([1.0; 4])),
                ("vignette", Num(0.0)),
            ],
            ObjectKind::Timer => vec![("wait_time", Num(1.0)), ("running", Bool(true)), ("repeat", Bool(true))],
            ObjectKind::Animator => vec![("clips", Res(String::new())), ("autoplay", Text(String::new())), ("speed", Num(1.0))],
            ObjectKind::Sound => vec![
                ("stream", Res(String::new())),
                ("volume", Num(1.0)),
                ("pitch", Num(1.0)),
                ("loop", Bool(false)),
                ("autoplay", Bool(false)),
                ("playing", Bool(false)),
                ("spatial", Bool(false)),
                ("range", Num(0.0)),
                ("pan", Num(0.0)),
                ("echo", Num(0.0)),
                ("echo_delay", Num(0.15)),
            ],
            ObjectKind::Body2D => vec![
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
                ("mass", Num(0.0)),
                ("linear_damping", Num(0.0)),
                ("angular_damping", Num(0.0)),
                ("ccd", Text("project".into())),
                ("can_sleep", Bool(true)),
                ("enabled", Bool(true)),
                ("lock_x", Bool(false)),
                ("lock_y", Bool(false)),
                ("spin", Num(0.0)),
                ("modulate", Color([1.0; 4])),
                ("layer", Text(String::new())),
                ("mask", Text(String::new())),
            ],
            ObjectKind::Body3D => vec![
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
                ("mass", Num(0.0)),
                ("linear_damping", Num(0.0)),
                ("angular_damping", Num(0.0)),
                ("ccd", Text("project".into())),
                ("can_sleep", Bool(true)),
                ("enabled", Bool(true)),
                ("lock_x", Bool(false)),
                ("lock_y", Bool(false)),
                ("lock_z", Bool(false)),
                ("spin_x", Num(0.0)),
                ("spin_y", Num(0.0)),
                ("spin_z", Num(0.0)),
                ("modulate", Color([1.0; 4])),
                ("layer", Text(String::new())),
                ("mask", Text(String::new())),
            ],
            ObjectKind::Object2D | ObjectKind::Object3D => vec![("modulate", Color([1.0; 4]))],
            _ => vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub position: Vec3,
    /// Euler angles in degrees (applied Y, then X, then Z). 2D objects use only `z`.
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
    /// An object's handle in its tree. Stays valid until the object is removed.
    pub struct ObjectId;
}

#[derive(Clone, Debug)]
pub struct Object {
    pub name: String,
    pub kind: ObjectKind,
    pub transform: Transform,
    pub visible: bool,
    /// Built-in properties of the kind, plus any custom ones you add.
    pub props: BTreeMap<String, Variant>,
    /// `res://` paths of the .jlb scripts attached to this object, run in this order.
    pub scripts: Vec<String>,
    /// Groups let scripts find sets of objects ("enemies") and colliders filter hits.
    pub groups: Vec<String>,
    pub parent: Option<ObjectId>,
    pub children: Vec<ObjectId>,
    /// Set on the root of an instanced blueprint: the blueprint file it came from.
    pub instance_of: Option<String>,
    /// True for objects that came from inside an instanced blueprint (saved by reference, not
    /// one by one).
    pub from_instance: bool,
    /// A number naming this object for as long as it lives, the same in every copy of the
    /// blueprint (working on a project together matches objects by it). Not saved: loading
    /// derives it from the object's path, and new objects get a fresh one.
    pub uid: u64,
}

/// A fresh object uid (never 0).
pub fn new_uid() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    splitmix(t ^ ((std::process::id() as u64) << 32) ^ N.fetch_add(0x9E37_79B9, Ordering::Relaxed)) | 1
}

/// An object uid from its path (the same on every computer).
pub fn path_uid(path: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    splitmix(h) | 1
}

pub fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Object {
    pub fn new(name: impl Into<String>, kind: ObjectKind) -> Object {
        let props = kind.default_props().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        Object {
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
            uid: new_uid(),
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

/// The `$name` variables a script declares (a line of its own, outside any block): values set
/// per instance in the editor instead of in the script.
pub fn script_vars(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix('$') else { continue };
        let name: String = rest.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_').collect();
        let after = rest[name.len()..].trim();
        if !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && (after.is_empty() || after.starts_with(">>")) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}
