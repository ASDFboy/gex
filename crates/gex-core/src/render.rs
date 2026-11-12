//! Turning a tree into a flat list of things to draw. The renderer only ever sees a
//! `RenderList`, so it doesn't care whether the tree is being edited or played (a
//! running game sends one of these per frame from its own thread).

use glam::{Mat4, Vec2, Vec3};

use crate::object::{ObjectId, ObjectKind};
use crate::tree::BlueprintTree;

/// Which part of a sprite's texture to show.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Region {
    #[default]
    Whole,
    /// A sprite sheet of `h` × `v` frames, showing one.
    Frame { h: u32, v: u32, frame: u32 },
    /// A tile of a tileset (tile sizes in texture pixels; ids count left to right, top down).
    Tile { w: f32, h: f32, id: u32 },
}

/// How a picture mixes with what's behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Blend {
    /// Covers what's behind, by its opacity.
    #[default]
    Normal,
    /// Adds light: glows, fire, lasers.
    Add,
    /// Darkens what's behind: shadows, stains, tinted glass.
    Multiply,
}

impl Blend {
    pub fn from_name(s: &str) -> Blend {
        match s {
            "add" | "additive" => Blend::Add,
            "multiply" => Blend::Multiply,
            _ => Blend::Normal,
        }
    }
}

/// An object's own texture filtering: None follows the project's setting.
pub fn filter_of(n: &crate::Object) -> Option<bool> {
    match n.text("filter") {
        "pixel" => Some(true),
        "smooth" => Some(false),
        _ => None,
    }
}

/// The color every object passes down to its children (`modulate`), multiplied up the tree.
pub fn inherited_modulate(tree: &BlueprintTree, id: ObjectId) -> [f32; 4] {
    let mut c = [1.0f32; 4];
    let mut cur = tree.objects[id].parent;
    while let Some(p) = cur {
        let n = &tree.objects[p];
        if let Some(v) = n.props.get("modulate") {
            let m = v.as_color();
            for k in 0..4 {
                c[k] *= m[k];
            }
        }
        cur = n.parent;
    }
    c
}

/// An object's own color: its color times its opacity, its own modulate and its parents'.
pub fn tinted(tree: &BlueprintTree, id: ObjectId, color: [f32; 4]) -> [f32; 4] {
    let n = &tree.objects[id];
    let mut m = inherited_modulate(tree, id);
    if let Some(v) = n.props.get("modulate") {
        let own = v.as_color();
        for k in 0..4 {
            m[k] *= own[k];
        }
    }
    let o = n.num("opacity", 1.0).clamp(0.0, 1.0) as f32;
    [color[0] * m[0], color[1] * m[1], color[2] * m[2], color[3] * m[3] * o]
}

#[derive(Clone, Debug)]
pub struct Sprite {
    pub object: ObjectId,
    /// World transform of the object (2D: x, y in pixels, rotation about z).
    pub model: Mat4,
    pub size: Vec2,
    pub centered: bool,
    pub color: [f32; 4],
    pub texture: String,
    pub layer: f32,
    pub region: Region,
    pub flip: [bool; 2],
    /// How much it moves with the camera: 1 normally, 0.5 for a far-off background, 0 stuck to the screen.
    pub parallax: Vec2,
    /// Repeats sideways / up and down to fill the view (for backgrounds).
    pub repeat: [bool; 2],
    pub blend: Blend,
    /// Pixel (true) or smooth (false) filtering; None = the project's sprite filter.
    pub pixel: Option<bool>,
    /// Darkened by 2D lighting (false: drawn after it, at full brightness).
    pub lit: bool,
}

impl Default for Sprite {
    fn default() -> Self {
        Sprite {
            object: ObjectId::default(),
            model: Mat4::IDENTITY,
            size: Vec2::ONE,
            centered: false,
            color: [1.0; 4],
            texture: String::new(),
            layer: 0.0,
            region: Region::Whole,
            flip: [false; 2],
            parallax: Vec2::ONE,
            repeat: [false; 2],
            blend: Blend::Normal,
            pixel: None,
            lit: true,
        }
    }
}

/// A Light2D: brightens the dark set by a Camera2D's `ambient`.
#[derive(Clone, Debug)]
pub struct Light2DDraw {
    pub object: ObjectId,
    pub pos: Vec2,
    pub radius: f32,
    pub color: [f32; 4],
    pub texture: String,
}

/// A UI object, drawn in game-window pixels (by the editor/player, on top of the game).
#[derive(Clone, Debug)]
pub struct UiItem {
    pub object: ObjectId,
    pub kind: ObjectKind,
    /// Position from the object's transform, before the anchor is applied.
    pub pos: Vec2,
    pub size: Vec2,
    pub props: std::collections::BTreeMap<String, crate::Variant>,
}

impl UiItem {
    pub fn num(&self, k: &str, d: f64) -> f64 {
        self.props.get(k).and_then(crate::Variant::as_f64).unwrap_or(d)
    }
    pub fn text(&self, k: &str) -> &str {
        self.props.get(k).and_then(crate::Variant::as_str).unwrap_or("")
    }
    pub fn color(&self, k: &str) -> [f32; 4] {
        self.props.get(k).map(crate::Variant::as_color).unwrap_or([1.0; 4])
    }
    /// Top-left corner in a window of `screen` size, after the anchor.
    pub fn top_left(&self, screen: Vec2) -> Vec2 {
        let a = self.text("anchor");
        let fx = if a.ends_with("right") { 1.0 } else if a == "top" || a == "bottom" || a == "center" { 0.5 } else { 0.0 };
        let fy = if a.starts_with("bottom") { 1.0 } else if a == "left" || a == "right" || a == "center" { 0.5 } else { 0.0 };
        self.pos + Vec2::new(fx * (screen.x - self.size.x), fy * (screen.y - self.size.y))
    }
}

#[derive(Clone, Debug)]
pub struct Label {
    pub object: ObjectId,
    pub pos: Vec2,
    pub text: String,
    pub size: f32,
    pub color: [f32; 4],
    pub layer: f32,
    /// A .ttf / .otf font file, or "" for the default.
    pub font: String,
}

#[derive(Clone, Debug)]
pub struct MeshDraw {
    pub object: ObjectId,
    pub model: Mat4,
    /// "cube", "sphere", "plane", or a model resource path.
    pub mesh: String,
    pub color: [f32; 4],
    pub texture: String,
    /// How many times the texture repeats across the mesh.
    pub uv_scale: [f32; 2],
    pub cast_shadows: bool,
    /// Model animations: which one, and how far into it (seconds).
    pub animation: String,
    pub anim_time: f32,
    /// The animation it's blending from (crossfade), its time, and how far the blend is (0..1, 1 = done).
    pub prev_animation: String,
    pub prev_time: f32,
    pub blend: f32,
    /// Material: a normal map, shininess (0 = matte), how much it glows, and no lighting at all.
    pub normal_map: String,
    pub specular: f32,
    pub emissive: f32,
    pub unlit: bool,
    /// Always faces the camera (Sprite3D, 3D particles).
    pub billboard: bool,
    /// A .wgsl file with a custom `fn material(...)` (see docs/SHADERS.md).
    pub shader: String,
    /// Joint matrices set from outside (a ragdoll), instead of an animation.
    pub pose: Option<std::sync::Arc<Vec<Mat4>>>,
    pub blend_mode: Blend,
    /// Pixel (true) or smooth (false) texture filtering; None = the project's.
    pub pixel: Option<bool>,
    pub receive_shadows: bool,
    /// Fog from the Environment applies to it.
    pub fog: bool,
    /// Which sides draw: None = automatic, Some(true) = both, Some(false) = front only.
    pub two_sided: Option<bool>,
    /// Drawn over everything else (ignores depth).
    pub on_top: bool,
    /// Texels less opaque than this are holes (solid surfaces only).
    pub alpha_cut: f32,
    pub emissive_color: [f32; 3],
    /// Light around the edges as seen from the camera, and its color.
    pub rim: f32,
    pub rim_color: [f32; 3],
    /// Texture scrolling, in repeats per second.
    pub uv_offset: [f32; 2],
    pub uv_scroll: [f32; 2],
    /// Not drawn further than this from the camera (0 = always drawn).
    pub draw_distance: f32,
}

impl MeshDraw {
    pub fn new(object: ObjectId, model: Mat4, mesh: &str) -> MeshDraw {
        MeshDraw {
            object,
            model,
            mesh: mesh.to_string(),
            color: [1.0; 4],
            texture: String::new(),
            uv_scale: [1.0, 1.0],
            cast_shadows: true,
            animation: String::new(),
            anim_time: 0.0,
            prev_animation: String::new(),
            prev_time: 0.0,
            blend: 1.0,
            normal_map: String::new(),
            specular: 0.15,
            emissive: 0.0,
            unlit: false,
            billboard: false,
            shader: String::new(),
            pose: None,
            blend_mode: Blend::Normal,
            pixel: None,
            receive_shadows: true,
            fog: true,
            two_sided: None,
            on_top: false,
            alpha_cut: 0.5,
            emissive_color: [1.0; 3],
            rim: 0.0,
            rim_color: [1.0; 3],
            uv_offset: [0.0; 2],
            uv_scroll: [0.0; 2],
            draw_distance: 0.0,
        }
    }
}

/// Text placed in the 3D world (drawn on top, facing the screen).
#[derive(Clone, Debug)]
pub struct Label3D {
    pub object: ObjectId,
    pub pos: Vec3,
    pub text: String,
    pub size: f32,
    pub color: [f32; 4],
    pub font: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

#[derive(Clone, Debug)]
pub struct LightDraw {
    pub kind: LightKind,
    pub pos: Vec3,
    /// Direction the light shines along (world space).
    pub dir: Vec3,
    pub color: [f32; 3],
    pub range: f32,
    /// Spot lights: half the cone, in degrees.
    pub angle: f32,
    pub shadows: bool,
    /// Spot lights: how soft the cone's edge is (0 hard, 1 very soft).
    pub softness: f32,
    /// How quickly point and spot lights fade with distance (2 = natural).
    pub falloff: f32,
    /// Strength of the shiny highlights it makes.
    pub specular: f32,
    /// How dark its shadows are (0 none, 1 full).
    pub shadow_strength: f32,
    /// Its own shadow bias, or below 0 for the project's.
    pub shadow_bias: f32,
}

#[derive(Clone, Debug)]
pub struct Environment3D {
    pub sky_top: [f32; 4],
    pub sky_horizon: [f32; 4],
    pub ground: [f32; 4],
    pub sky_texture: String,
    pub ambient: [f32; 3],
    pub fog_color: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    /// How far from the camera shadows reach (metres).
    pub shadow_distance: f32,
    /// Glow around bright things: strength (0 = off), and how bright counts.
    pub bloom: f32,
    pub bloom_threshold: f32,
    /// Sky brightness.
    pub sky_energy: f32,
    /// Fog: "linear" (start to end) or "exp" (thickening by density).
    pub fog_exp: bool,
    pub fog_density: f32,
    /// Part of the shadow distance the sharp near cascade covers.
    pub shadow_split: f32,
    /// Color settings on top of the project's (multiplied, tint multiplied, vignette added).
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub tint: [f32; 4],
    pub vignette: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera2D {
    pub center: Vec2,
    pub rotation: f32,
    pub zoom: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Camera3D {
    /// The camera's world matrix (it looks along its -Z).
    pub world: Mat4,
    pub fov_deg: f32,
    pub near: f32,
    pub far: f32,
    /// Orthographic (no perspective): the view's height in metres.
    pub ortho: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Rect2D {
    pub object: ObjectId,
    pub model: Mat4,
    pub size: Vec2,
}

#[derive(Clone, Debug, Default)]
pub struct RenderList {
    pub sprites: Vec<Sprite>,
    pub labels: Vec<Label>,
    pub meshes: Vec<MeshDraw>,
    pub lights: Vec<LightDraw>,
    pub colliders: Vec<Rect2D>,
    pub camera2d: Option<Camera2D>,
    pub camera3d: Option<Camera3D>,
    /// Background color.
    pub clear: [f32; 4],
    pub ui: Vec<UiItem>,
    pub environment: Option<Environment3D>,
    /// Lines drawn by scripts this frame (2D world): (from, to, color).
    pub lines_2d: Vec<([f32; 2], [f32; 2], [f32; 4])>,
    /// The game wants the mouse hidden and held in the window (MOUSE_LOCK).
    pub mouse_locked: bool,
    /// A color over the whole game (fading between blueprints); alpha 0 = none.
    pub fade: [f32; 4],
    pub labels_3d: Vec<Label3D>,
    /// 2D lighting: the dark (from the Camera2D's ambient; white = none) and the lights in it.
    pub ambient_2d: [f32; 4],
    pub lights_2d: Vec<Light2DDraw>,
    /// What the game wants the window to be (FULLSCREEN, WINDOW_TITLE); None = as it is.
    pub fullscreen: Option<bool>,
    pub window_title: Option<String>,
    /// The project's graphics settings (scripts can change them while playing).
    pub graphics: crate::settings::Graphics,
}

impl RenderList {
    pub fn has_3d(&self) -> bool {
        !self.meshes.is_empty() || self.camera3d.is_some()
    }
}

fn rgb(c: [f32; 4]) -> [f32; 3] {
    [c[0], c[1], c[2]]
}

/// The look settings every 3D drawable shares.
fn mesh_look(n: &crate::Object, base: MeshDraw) -> MeshDraw {
    let b = |k: &str, d: bool| n.prop(k).map_or(d, |v| v.as_bool());
    MeshDraw {
        blend_mode: Blend::from_name(n.text("blend")),
        pixel: filter_of(n),
        receive_shadows: b("receive_shadows", true),
        fog: b("fog", true),
        on_top: b("on_top", false),
        alpha_cut: n.num("alpha_cut", 0.5).clamp(0.0, 1.0) as f32,
        uv_offset: [n.num("uv_offset_x", 0.0) as f32, n.num("uv_offset_y", 0.0) as f32],
        uv_scroll: [n.num("uv_scroll_x", 0.0) as f32, n.num("uv_scroll_y", 0.0) as f32],
        draw_distance: n.num("draw_distance", 0.0).max(0.0) as f32,
        ..base
    }
}

pub fn extract(tree: &BlueprintTree) -> RenderList {
    let mut out = RenderList { clear: [0.08, 0.08, 0.1, 1.0], ambient_2d: [1.0; 4], ..Default::default() };
    for id in tree.walk() {
        if !tree.visible_in_tree(id) {
            continue;
        }
        let n = &tree.objects[id];
        let mut model = tree.global_matrix(id);
        // 2D things are flat: their z (with their parents') is the order they draw in,
        // higher on top.
        let z = if n.kind.is_2d() { std::mem::take(&mut model.w_axis.z) } else { 0.0 };
        let blend = Blend::from_name(n.text("blend"));
        let pixel = filter_of(n);
        let lit = n.prop("lit").is_none_or(|v| v.as_bool());
        match n.kind {
            ObjectKind::Sprite2D => out.sprites.push(Sprite {
                object: id,
                model,
                size: Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32),
                centered: n.prop("centered").is_none_or(|v| v.as_bool()),
                color: tinted(tree, id, n.color("color")),
                blend,
                pixel,
                lit,
                texture: n.text("texture").to_string(),
                layer: n.num("layer", 0.0) as f32 + z,
                region: {
                    let (h, v) = (n.num("hframes", 1.0).max(1.0) as u32, n.num("vframes", 1.0).max(1.0) as u32);
                    if h * v > 1 { Region::Frame { h, v, frame: n.num("frame", 0.0).max(0.0) as u32 } } else { Region::Whole }
                },
                flip: [n.prop("flip_h").is_some_and(|v| v.as_bool()), n.prop("flip_v").is_some_and(|v| v.as_bool())],
                parallax: Vec2::new(n.num("parallax_x", 1.0) as f32, n.num("parallax_y", 1.0) as f32),
                repeat: [n.prop("repeat_x").is_some_and(|v| v.as_bool()), n.prop("repeat_y").is_some_and(|v| v.as_bool())],
            }),
            ObjectKind::TileMap2D => {
                let grid = crate::tiles::parse(n.text("tiles"));
                let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
                let tex = n.text("tileset").to_string();
                let color = tinted(tree, id, n.color("color"));
                let layer = n.num("layer", -5.0) as f32 + z;
                let parallax = Vec2::new(n.num("parallax_x", 1.0) as f32, n.num("parallax_y", 1.0) as f32);
                for (r, row) in grid.iter().enumerate() {
                    for (c, tile) in row.iter().enumerate() {
                        if *tile < 0 {
                            continue;
                        }
                        let local = Mat4::from_translation(Vec3::new(c as f32 * tw, r as f32 * th, 0.0));
                        out.sprites.push(Sprite {
                            object: id,
                            model: model * local,
                            size: Vec2::new(tw, th),
                            centered: false,
                            color,
                            texture: tex.clone(),
                            layer,
                            region: Region::Tile { w: tw, h: th, id: *tile as u32 },
                            flip: [false; 2],
                            parallax,
                            repeat: [false; 2],
                            blend,
                            pixel,
                            lit,
                        });
                    }
                }
            }
            k if k.is_ui() => {
                let p = model.w_axis.truncate().truncate();
                let mut props = n.props.clone();
                // The UI layer reads one opacity: this object's, times its parents' modulate alpha.
                let t = tinted(tree, id, [1.0; 4]);
                props.insert("opacity".into(), crate::Variant::Num(t[3] as f64));
                out.ui.push(UiItem { object: id, kind: k, pos: p, size: Vec2::new(n.num("width", 100.0) as f32, n.num("height", 30.0) as f32), props });
            }
            ObjectKind::Label2D => out.labels.push(Label {
                object: id,
                pos: model.w_axis.truncate().truncate(),
                text: n.text("text").to_string(),
                size: n.num("size", 16.0) as f32,
                color: tinted(tree, id, n.color("color")),
                layer: n.num("layer", 10.0) as f32 + z,
                font: n.text("font").to_string(),
            }),
            ObjectKind::Collider2D => out.colliders.push(Rect2D {
                object: id,
                model,
                size: Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32),
            }),
            ObjectKind::Body2D => {
                let circle = matches!(n.text("shape"), "circle" | "ball");
                let r = n.num("radius", 16.0) as f32 * 2.0;
                out.colliders.push(Rect2D {
                    object: id,
                    model,
                    size: if circle { Vec2::splat(r) } else { Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32) },
                });
            }
            ObjectKind::Light2D => out.lights_2d.push(Light2DDraw {
                object: id,
                pos: model.w_axis.truncate().truncate(),
                radius: n.num("radius", 200.0) as f32 * model.x_axis.truncate().length(),
                color: {
                    let c = tinted(tree, id, n.color("color"));
                    let e = n.num("energy", 1.0) as f32;
                    [c[0] * e, c[1] * e, c[2] * e, c[3]]
                },
                texture: n.text("texture").to_string(),
            }),
            ObjectKind::Camera2D => {
                if n.prop("current").is_none_or(|v| v.as_bool()) && out.camera2d.is_none() {
                    out.ambient_2d = n.color("ambient");
                    let (_, rot, pos) = model.to_scale_rotation_translation();
                    out.camera2d = Some(Camera2D {
                        center: pos.truncate(),
                        rotation: rot.to_euler(glam::EulerRot::YXZ).2,
                        zoom: n.num("zoom", 1.0).max(0.01) as f32,
                    });
                }
            }
            ObjectKind::Mesh3D => {
                let whole = MeshDraw {
                color: tinted(tree, id, n.color("color")),
                texture: n.text("texture").to_string(),
                uv_scale: [n.num("uv_scale_x", 1.0) as f32, n.num("uv_scale_y", 1.0) as f32],
                rim: n.num("rim", 0.0) as f32,
                rim_color: rgb(n.color("rim_color")),
                emissive_color: rgb(n.color("emissive_color")),
                two_sided: match n.text("sides") {
                    "both" => Some(true),
                    "front" => Some(false),
                    _ => None,
                },
                cast_shadows: n.prop("cast_shadows").is_none_or(|v| v.as_bool()),
                animation: n.text("animation").to_string(),
                anim_time: n.num("anim_time", 0.0) as f32,
                normal_map: n.text("normal_map").to_string(),
                specular: n.num("specular", 0.15) as f32,
                emissive: n.num("emissive", 0.0) as f32,
                unlit: n.prop("unlit").is_some_and(|v| v.as_bool()),
                shader: n.text("shader").to_string(),
                ..mesh_look(n, MeshDraw::new(id, model, n.text("mesh")))
            };
                // A cube with faces of their own (a texture, or hidden) is drawn face by face.
                const FACES: [(&str, [f32; 3], [f32; 3], [f32; 3]); 6] = [
                    ("right", [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
                    ("left", [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
                    ("top", [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
                    ("bottom", [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                    ("front", [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                    ("back", [0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                ];
                let face_tex = |f: &str| n.text(&format!("face_{}", f)).to_string();
                let hidden = |f: &str| n.prop(&format!("hide_{}", f)).is_some_and(|v| v.as_bool());
                if whole.mesh == "cube" && FACES.iter().any(|f| !face_tex(f.0).is_empty() || hidden(f.0)) {
                    for (f, nrm, u, w) in FACES {
                        if hidden(f) {
                            continue;
                        }
                        let (nrm, u, w) = (Vec3::from(nrm), Vec3::from(u), Vec3::from(w));
                        let m = Mat4::from_cols(u.extend(0.0), w.extend(0.0), nrm.extend(0.0), (nrm * 0.5).extend(1.0));
                        let t = face_tex(f);
                        out.meshes.push(MeshDraw {
                            model: model * m,
                            mesh: "quad".into(),
                            texture: if t.is_empty() { whole.texture.clone() } else { t },
                            // With a face missing you can see inside, unless told otherwise.
                            two_sided: whole.two_sided.or(Some(FACES.iter().any(|f| hidden(f.0)))),
                            ..whole.clone()
                        });
                    }
                } else {
                    out.meshes.push(whole);
                }
            }
            ObjectKind::Sprite3D => {
                let (w, h) = (n.num("width", 1.0) as f32, n.num("height", 1.0) as f32);
                out.meshes.push(MeshDraw {
                    color: tinted(tree, id, n.color("color")),
                    texture: n.text("texture").to_string(),
                    cast_shadows: n.prop("cast_shadows").is_some_and(|v| v.as_bool()),
                    unlit: n.prop("unlit").is_none_or(|v| v.as_bool()),
                    billboard: n.prop("billboard").is_none_or(|v| v.as_bool()),
                    emissive: n.num("emissive", 0.0) as f32,
                    ..mesh_look(n, MeshDraw::new(id, model * Mat4::from_scale(Vec3::new(w, h, 1.0)), "quad"))
                });
            }
            ObjectKind::Label3D => out.labels_3d.push(Label3D {
                object: id,
                pos: model.w_axis.truncate(),
                text: n.text("text").to_string(),
                size: n.num("size", 24.0) as f32,
                color: tinted(tree, id, n.color("color")),
                font: n.text("font").to_string(),
            }),
            ObjectKind::Environment => {
                if out.environment.is_none() {
                    let a = n.color("ambient");
                    let e = n.num("ambient_energy", 0.35) as f32;
                    out.environment = Some(Environment3D {
                        sky_top: n.color("sky_top"),
                        sky_horizon: n.color("sky_horizon"),
                        ground: n.color("ground"),
                        sky_texture: n.text("sky_texture").to_string(),
                        ambient: [a[0] * e, a[1] * e, a[2] * e],
                        fog_color: n.color("fog_color"),
                        fog_start: n.num("fog_start", 0.0) as f32,
                        fog_end: n.num("fog_end", 0.0) as f32,
                        shadow_distance: n.num("shadow_distance", 40.0) as f32,
                        bloom: n.num("bloom", 0.0) as f32,
                        bloom_threshold: n.num("bloom_threshold", 0.9) as f32,
                        sky_energy: n.num("sky_energy", 1.0).max(0.0) as f32,
                        fog_exp: n.text("fog_mode") == "exp",
                        fog_density: n.num("fog_density", 0.02).max(0.0) as f32,
                        shadow_split: n.num("shadow_split", 0.25).clamp(0.02, 0.95) as f32,
                        exposure: n.num("exposure", 1.0).max(0.0) as f32,
                        contrast: n.num("contrast", 1.0).max(0.0) as f32,
                        saturation: n.num("saturation", 1.0).max(0.0) as f32,
                        tint: n.prop("tint").map(crate::Variant::as_color).unwrap_or([1.0; 4]),
                        vignette: n.num("vignette", 0.0).clamp(0.0, 1.0) as f32,
                    });
                }
            }
            ObjectKind::Camera3D => {
                if n.prop("current").is_none_or(|v| v.as_bool()) && out.camera3d.is_none() {
                    out.camera3d = Some(Camera3D {
                        world: model,
                        fov_deg: n.num("fov", 70.0) as f32,
                        near: n.num("near", 0.05) as f32,
                        far: n.num("far", 500.0) as f32,
                        ortho: if n.prop("ortho").is_some_and(|v| v.as_bool()) { Some(n.num("ortho_size", 10.0).max(0.01) as f32) } else { None },
                    });
                }
            }
            ObjectKind::Light3D => {
                let c = tinted(tree, id, n.color("color"));
                let e = n.num("energy", 1.0) as f32;
                out.lights.push(LightDraw {
                    kind: match n.text("type") {
                        "point" => LightKind::Point,
                        "spot" => LightKind::Spot,
                        _ => LightKind::Directional,
                    },
                    pos: model.w_axis.truncate(),
                    dir: model.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Y),
                    color: [c[0] * e, c[1] * e, c[2] * e],
                    range: n.num("range", 10.0).max(0.01) as f32,
                    angle: n.num("angle", 45.0) as f32,
                    shadows: n.prop("shadows").is_none_or(|v| v.as_bool()),
                    softness: n.num("softness", 0.1).clamp(0.0, 1.0) as f32,
                    falloff: n.num("falloff", 2.0).clamp(0.1, 16.0) as f32,
                    specular: n.num("specular", 1.0).max(0.0) as f32,
                    shadow_strength: n.num("shadow_strength", 1.0).clamp(0.0, 1.0) as f32,
                    shadow_bias: n.num("shadow_bias", -1.0) as f32,
                });
            }
            _ => {}
        }
    }
    out.sprites.sort_by(|a, b| a.layer.total_cmp(&b.layer));
    out.labels.sort_by(|a, b| a.layer.total_cmp(&b.layer));
    out
}
