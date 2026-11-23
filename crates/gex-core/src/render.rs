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
    /// Any rectangle of the texture, in texture pixels.
    Rect { x: f32, y: f32, w: f32, h: f32 },
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

/// A Sprite2D's pivot (see Sprite::pivot): pivot_x / pivot_y when it's centered (the
/// default; 0.5, 0.5 is the middle), its top-left corner when `centered` is off. Collider2D
/// boxes are always centered.
pub fn pivot_of(n: &crate::Object) -> Vec2 {
    if n.kind == ObjectKind::Sprite2D && !n.prop("centered").is_none_or(|v| v.as_bool()) {
        return Vec2::ZERO;
    }
    if n.kind != ObjectKind::Sprite2D {
        return Vec2::splat(0.5);
    }
    Vec2::new(n.num("pivot_x", 0.5) as f32, n.num("pivot_y", 0.5) as f32)
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

/// Puts sprites in layer order, keeping the order they came in within a layer. Sprites are
/// big, so the order is worked out on their positions and each is moved once.
pub fn sort_by_layer(sprites: &mut Vec<Sprite>) {
    if sprites.windows(2).all(|w| w[0].layer <= w[1].layer) {
        return;
    }
    let mut order: Vec<u32> = (0..sprites.len() as u32).collect();
    order.sort_by(|a, b| sprites[*a as usize].layer.total_cmp(&sprites[*b as usize].layer));
    let mut old: Vec<Option<Sprite>> = std::mem::take(sprites).into_iter().map(Some).collect();
    sprites.extend(order.into_iter().map(|i| old[i as usize].take().unwrap()));
}

/// A Sprite2D as drawn: its properties read in one pass over them (one look-up each, by
/// name, costs a lot when there are thousands of sprites).
#[allow(clippy::too_many_arguments)]
fn sprite_2d(id: ObjectId, n: &crate::Object, model: Mat4, z: f32, tint_down: [f32; 4], blend: Blend, pixel: Option<bool>, lit: bool) -> Sprite {
    use crate::Variant;
    let (mut w, mut h, mut color, mut centered, mut pivot) = (32.0f64, 32.0f64, [1.0f32; 4], true, [0.5f64, 0.5f64]);
    let (mut texture, mut layer, mut hf, mut vf, mut frame) = ("", 0.0f64, 1.0f64, 1.0f64, 0.0f64);
    let (mut flip, mut parallax, mut repeat) = ([false; 2], [1.0f64, 1.0f64], [false; 2]);
    let (mut fx, mut shader, mut values, mut opacity) = ([0.0f64, 0.0, 1.0, 0.0], "", [0.0f32; 4], 1.0f64);
    let num = |v: &Variant, d: f64| v.as_f64().unwrap_or(d);
    for (k, v) in &n.props {
        match k.as_str() {
            "width" => w = num(v, 32.0),
            "height" => h = num(v, 32.0),
            "color" => color = v.as_color(),
            "centered" => centered = v.as_bool(),
            "pivot_x" => pivot[0] = num(v, 0.5),
            "pivot_y" => pivot[1] = num(v, 0.5),
            "texture" => texture = v.as_str().unwrap_or(""),
            "layer" => layer = num(v, 0.0),
            "hframes" => hf = num(v, 1.0),
            "vframes" => vf = num(v, 1.0),
            "frame" => frame = num(v, 0.0),
            "flip_h" => flip[0] = v.as_bool(),
            "flip_v" => flip[1] = v.as_bool(),
            "parallax_x" => parallax[0] = num(v, 1.0),
            "parallax_y" => parallax[1] = num(v, 1.0),
            "repeat_x" => repeat[0] = v.as_bool(),
            "repeat_y" => repeat[1] = v.as_bool(),
            "brightness" => fx[0] = num(v, 0.0),
            "hue" => fx[1] = num(v, 0.0),
            "saturation" => fx[2] = num(v, 1.0),
            "invert" => fx[3] = match v { Variant::Bool(b) => *b as u8 as f64, v => num(v, 0.0) },
            "shader" => shader = v.as_str().unwrap_or(""),
            "value_1" => values[0] = num(v, 0.0) as f32,
            "value_2" => values[1] = num(v, 0.0) as f32,
            "value_3" => values[2] = num(v, 0.0) as f32,
            "value_4" => values[3] = num(v, 0.0) as f32,
            "opacity" => opacity = num(v, 1.0),
            _ => {}
        }
    }
    let (hf, vf) = (hf.max(1.0) as u32, vf.max(1.0) as u32);
    let o = opacity.clamp(0.0, 1.0) as f32;
    let m = tint_down;
    Sprite {
        object: id,
        model,
        size: Vec2::new(w as f32, h as f32),
        pivot: if centered { Vec2::new(pivot[0] as f32, pivot[1] as f32) } else { Vec2::ZERO },
        color: [color[0] * m[0], color[1] * m[1], color[2] * m[2], color[3] * m[3] * o],
        blend,
        pixel,
        lit,
        texture: texture.to_string(),
        layer: layer as f32 + z,
        region: if hf * vf > 1 { Region::Frame { h: hf, v: vf, frame: frame.max(0.0) as u32 } } else { Region::Whole },
        flip,
        parallax: Vec2::new(parallax[0] as f32, parallax[1] as f32),
        repeat,
        effects: [fx[0] as f32, (fx[1] / 360.0) as f32, (1.0 - fx[2]).clamp(-4.0, 1.0) as f32, fx[3].clamp(0.0, 1.0) as f32],
        shader: shader.to_string(),
        values,
    }
}

/// A TileMap2D's tiles, parsed once and kept while the text stays the same (a big map's
/// text is long; parsing it every frame was most of the work of drawing one).
fn parsed_tiles(text: &str) -> std::rc::Rc<Vec<Vec<i32>>> {
    use std::hash::{Hash, Hasher};
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<u64, (String, std::rc::Rc<Vec<Vec<i32>>>, u64)>> = Default::default();
        static TICK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    let key = h.finish();
    let tick = TICK.with(|t| {
        t.set(t.get() + 1);
        t.get()
    });
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((t, g, used)) = c.get_mut(&key) {
            if t == text {
                *used = tick;
                return g.clone();
            }
        }
        let g = std::rc::Rc::new(crate::tiles::parse(text));
        // Maps not drawn for a while are forgotten.
        if c.len() > 64 {
            c.retain(|_, v| tick - v.2 < 1000);
        }
        c.insert(key, (text.to_string(), g.clone(), tick));
        g
    })
}

/// `tinted` with the modulate already worked out (its own times its parents').
fn tint(n: &crate::Object, m: [f32; 4], color: [f32; 4]) -> [f32; 4] {
    let o = n.num("opacity", 1.0).clamp(0.0, 1.0) as f32;
    [color[0] * m[0], color[1] * m[1], color[2] * m[2], color[3] * m[3] * o]
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
    /// Which point of the picture sits at the object's position, as a fraction of its size:
    /// (0.5, 0.5) the middle, (0, 0) the top-left corner.
    pub pivot: Vec2,
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
    /// Color effects: brightness (added, -1 to 1), hue shift (turns), how far toward grey
    /// (0 to 1), how far inverted (0 to 1). All 0 = none.
    pub effects: [f32; 4],
    /// A custom sprite shader (.wgsl with `fn sprite(...)`), or "".
    pub shader: String,
    /// The custom shader's four values (`values` in the shader).
    pub values: [f32; 4],
}

impl Sprite {
    /// The color effects of an object's properties: brightness, hue, saturation, invert.
    pub fn effects_of(n: &crate::Object) -> [f32; 4] {
        let inv = match n.prop("invert") {
            Some(crate::Variant::Bool(b)) => *b as u8 as f32,
            Some(v) => v.as_f64().unwrap_or(0.0) as f32,
            None => 0.0,
        };
        [
            n.num("brightness", 0.0) as f32,
            (n.num("hue", 0.0) / 360.0) as f32,
            (1.0 - n.num("saturation", 1.0)).clamp(-4.0, 1.0) as f32,
            inv.clamp(0.0, 1.0),
        ]
    }

    /// A custom shader's values from an object's properties (value_1 .. value_4).
    pub fn values_of(n: &crate::Object) -> [f32; 4] {
        [n.num("value_1", 0.0) as f32, n.num("value_2", 0.0) as f32, n.num("value_3", 0.0) as f32, n.num("value_4", 0.0) as f32]
    }
}

impl Default for Sprite {
    fn default() -> Self {
        Sprite {
            object: ObjectId::default(),
            model: Mat4::IDENTITY,
            size: Vec2::ONE,
            pivot: Vec2::ZERO,
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
            effects: [0.0; 4],
            shader: String::new(),
            values: [0.0; 4],
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
    /// Its origin point (from its transform): where the anchor point of it sits.
    pub pos: Vec2,
    pub size: Vec2,
    pub props: std::collections::BTreeMap<String, crate::Variant>,
    /// Inside a ScrollBox: only this part of the window shows it (top-left, bottom-right).
    pub clip: Option<(Vec2, Vec2)>,
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
    /// Its top-left corner. The anchor is its origin: which point of it sits at its position
    /// (top_left: the top-left corner; top: the middle of the top edge; center: the middle...).
    pub fn top_left(&self, _screen: Vec2) -> Vec2 {
        self.pos - anchor_fraction(self.text("anchor")) * self.size
    }
}

#[derive(Clone, Debug)]
pub struct Label {
    pub object: ObjectId,
    /// Where its top-left corner is (world 2D).
    pub pos: Vec2,
    /// Its whole transform (turning and scaling turn and scale the text).
    pub model: Mat4,
    pub text: String,
    pub size: f32,
    pub color: [f32; 4],
    pub layer: f32,
    /// A .ttf / .otf font file, or "" for the default.
    pub font: String,
    /// Which point of the text block sits at `pos`, as a fraction of its size (0, 0 its
    /// top-left; 0.5, 0.5 its middle). Lines line up the same way (left, center, right).
    pub anchor: Vec2,
    /// Lines longer than this (game pixels) wrap at a space; 0 = no wrapping.
    pub wrap: f32,
    /// An outline around the letters: its width in game pixels (0 = none) and color.
    pub outline: f32,
    pub outline_color: [f32; 4],
}

/// About how big a label's text block is (for clicking on it): style tags like [color=..]
/// take no room, and lines longer than `wrap` (when it's above 0) wrap.
pub fn label_box(text: &str, size: f32, wrap: f32) -> Vec2 {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        plain.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let tag = after.find(']').map(|c| &after[..c]);
        let name = tag.map(|t| t.trim_start_matches('/').split('=').next().unwrap_or("").trim().to_ascii_lowercase());
        match (tag, name.as_deref()) {
            (Some(t), Some("color" | "bg" | "size" | "i" | "u" | "s")) => rest = &after[t.len() + 1..],
            _ => {
                plain.push('[');
                rest = after;
            }
        }
    }
    plain.push_str(rest);
    let char_w = size * 0.55;
    let (mut lines, mut widest) = (0.0f32, 0.0f32);
    for l in plain.split('\n') {
        let w = l.chars().count() as f32 * char_w;
        if wrap > 0.0 && w > wrap {
            lines += (w / wrap).ceil();
            widest = widest.max(wrap);
        } else {
            lines += 1.0;
            widest = widest.max(w);
        }
    }
    Vec2::new(widest, lines.max(1.0) * size * 1.2)
}

/// A Label2D's anchor (see Label::anchor), from its align and valign.
pub fn label_anchor(n: &crate::Object) -> Vec2 {
    let x = match n.text("align") {
        "center" | "middle" => 0.5,
        "right" => 1.0,
        _ => 0.0,
    };
    let y = match n.text("valign") {
        "center" | "middle" => 0.5,
        "bottom" => 1.0,
        _ => 0.0,
    };
    Vec2::new(x, y)
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
    /// Pictures the game will want soon (PRELOAD, and those placed in the blueprint):
    /// the renderer gets them ready in the background.
    pub preload: Vec<String>,
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
    /// Pictures drawn into this frame (Canvas2Ds, SURFACE_CREATE's), before the rest is drawn.
    pub canvases: Vec<CanvasDraw>,
    /// Surfaces the game let go of (SURFACE_FREE).
    pub canvases_freed: Vec<String>,
    /// The game draws its own mouse pointer (SET_CURSOR): the system's is hidden over it.
    pub hide_cursor: bool,
    /// The four values the full-screen shader (graphics.post_shader) gets (POST_SHADER).
    pub post_values: [f32; 4],
    /// The game's window title (WINDOW_TITLE); None = as it is.
    pub window_title: Option<String>,
    /// The project's display settings: resolution, full screen, scaling (scripts can change them).
    pub display: crate::settings::Display,
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

/// Where an anchor puts the origin, as a fraction of the size (0,0 top-left, 1,1 bottom-right).
pub fn anchor_fraction(a: &str) -> Vec2 {
    let fx = if a.ends_with("right") { 1.0 } else if a == "top" || a == "bottom" || a == "center" { 0.5 } else { 0.0 };
    let fy = if a.starts_with("bottom") { 1.0 } else if a == "left" || a == "right" || a == "center" { 0.5 } else { 0.0 };
    Vec2::new(fx, fy)
}

/// A picture that's drawn into and kept from frame to frame (a Canvas2D, or a surface a script
/// made): its name as a texture ("@canvas:..." / "@surface:..."), its size in pixels, and what's
/// drawn into it this frame, in its own pixels ((0, 0) its top-left).
#[derive(Clone, Debug, Default)]
pub struct CanvasDraw {
    pub key: String,
    pub size: [u32; 2],
    /// What it's filled with when made, or when cleared (CLEAR_CANVAS).
    pub clear_color: [f32; 4],
    /// Cleared before this frame's drawing.
    pub clear: bool,
    pub sprites: Vec<Sprite>,
    pub labels: Vec<Label>,
    pub lines: Vec<([f32; 2], [f32; 2], [f32; 4])>,
}

/// A picture's own sprite settings, kept beside it in `<picture>.sprite` (made in the Assets
/// panel): how it's cut into frames, where its origin is, how fast it animates and how the
/// mouse finds it. A Sprite2D showing the picture uses these instead of its own hframes,
/// vframes, pivot and mouse_precise.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PictureSettings {
    /// Frames across and down the sheet.
    pub hframes: u32,
    pub vframes: u32,
    /// How many frames there are (0: all of them; fewer when the sheet's last row isn't full).
    pub frames: u32,
    /// The origin, in pixels from a frame's top-left: where the sprite's position is.
    pub origin: [f32; 2],
    /// Frames a second it plays at by itself (0: it doesn't).
    pub fps: f32,
    /// How the mouse finds it: "box", "ellipse" or "precise" (its visible pixels).
    pub mask: String,
}

impl Default for PictureSettings {
    fn default() -> Self {
        PictureSettings { hframes: 1, vframes: 1, frames: 0, origin: [0.0, 0.0], fps: 0.0, mask: "box".into() }
    }
}

impl PictureSettings {
    /// Where a picture's settings are kept.
    pub fn file_for(picture: &str) -> String {
        format!("{}.sprite", picture)
    }
    pub fn parse(text: &str) -> Option<PictureSettings> {
        ron::from_str(text).ok()
    }
    pub fn to_text(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::new().struct_names(false)).unwrap_or_default()
    }
    pub fn count(&self) -> u32 {
        let all = self.hframes.max(1) * self.vframes.max(1);
        if self.frames == 0 { all } else { self.frames.min(all) }
    }
    /// One frame's size in pixels, for a picture this big.
    pub fn frame_size(&self, picture: [u32; 2]) -> Vec2 {
        Vec2::new(picture[0] as f32 / self.hframes.max(1) as f32, picture[1] as f32 / self.vframes.max(1) as f32)
    }
    /// The pivot (0 to 1 across a frame) its origin makes.
    pub fn pivot(&self, picture: [u32; 2]) -> Vec2 {
        Vec2::from(self.origin) / self.frame_size(picture).max(Vec2::splat(1e-3))
    }
}

/// Sprites showing a picture that has its own settings take its frames and origin (`lookup`
/// gives a picture's settings and size, when it has settings).
pub fn apply_picture_settings(sprites: &mut [Sprite], frames: &dyn Fn(ObjectId) -> f64, lookup: &mut dyn FnMut(&str) -> Option<(std::rc::Rc<PictureSettings>, [u32; 2])>) {
    for s in sprites {
        if s.texture.is_empty() || s.texture.starts_with('@') || matches!(s.region, Region::Tile { .. } | Region::Rect { .. }) {
            continue;
        }
        let Some((p, size)) = lookup(&s.texture) else { continue };
        let (h, v) = (p.hframes.max(1), p.vframes.max(1));
        let frame = frames(s.object).max(0.0) as u32 % p.count().max(1);
        s.region = if h * v > 1 { Region::Frame { h, v, frame } } else { Region::Whole };
        s.pivot = p.pivot(size);
    }
}

/// The texture name of a Canvas2D's picture.
pub fn canvas_key(id: ObjectId) -> String {
    format!("@canvas:{:?}", id)
}

pub fn extract(tree: &BlueprintTree) -> RenderList {
    let mut out = RenderList { clear: [0.08, 0.08, 0.1, 1.0], ambient_2d: [1.0; 4], ..Default::default() };
    // One pass down the tree works out each object's world transform, whether it shows and
    // the color its parents pass down, from its parent's (instead of climbing up the tree from
    // every object, three times over).
    let mut info: slotmap::SecondaryMap<ObjectId, (Mat4, bool, [f32; 4])> = slotmap::SecondaryMap::with_capacity(tree.objects.len());
    for id in tree.walk() {
        let n = &tree.objects[id];
        let (pm, pvis, pmod) = n.parent.and_then(|p| info.get(p).copied()).unwrap_or((Mat4::IDENTITY, true, [1.0; 4]));
        let world = pm * n.transform.matrix();
        let vis = pvis && n.visible;
        let tint_down = match n.props.get("modulate") {
            Some(v) => {
                let m = v.as_color();
                [pmod[0] * m[0], pmod[1] * m[1], pmod[2] * m[2], pmod[3] * m[3]]
            }
            None => pmod,
        };
        info.insert(id, (world, vis, tint_down));
        if !vis {
            continue;
        }
        let mut model = world;
        // 2D things are flat: their z (with their parents') is the order they draw in,
        // higher on top.
        let z = if n.kind.is_2d() { std::mem::take(&mut model.w_axis.z) } else { 0.0 };
        let blend = Blend::from_name(n.text("blend"));
        let pixel = filter_of(n);
        let lit = n.prop("lit").is_none_or(|v| v.as_bool());
        // A solid sprite's wall, outlined with the colliders.
        if n.kind == ObjectKind::Sprite2D && n.prop("solid").is_some_and(|v| v.as_bool()) {
            let (w, h) = (n.num("width", 32.0) as f32, n.num("height", 32.0) as f32);
            let mid = (Vec2::splat(0.5) - pivot_of(n)) * Vec2::new(w, h);
            let size = if n.text("solid_shape") == "circle" { Vec2::splat(w.max(h)) } else { Vec2::new(w, h) };
            out.colliders.push(Rect2D { object: id, model: model * Mat4::from_translation(mid.extend(0.0)), size });
        }
        match n.kind {
            ObjectKind::Sprite2D => out.sprites.push(sprite_2d(id, n, model, z, tint_down, blend, pixel, lit)),
            ObjectKind::TileMap2D => {
                let grid = parsed_tiles(n.text("tiles"));
                let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
                let tex = n.text("tileset").to_string();
                let color = tint(n, tint_down, n.color("color"));
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
                            pivot: Vec2::ZERO,
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
                            ..Default::default()
                        });
                    }
                }
            }
            k if k.is_ui() => {
                let p = model.w_axis.truncate().truncate();
                let mut props = n.props.clone();
                // The UI layer reads one opacity: this object's, times its parents' modulate alpha.
                let t = tint(n, tint_down, [1.0; 4]);
                props.insert("opacity".into(), crate::Variant::Num(t[3] as f64));
                out.ui.push(UiItem { object: id, kind: k, pos: p, size: Vec2::new(n.num("width", 100.0) as f32, n.num("height", 30.0) as f32), props, clip: None });
            }
            ObjectKind::Label2D => out.labels.push(Label {
                object: id,
                pos: model.w_axis.truncate().truncate(),
                model,
                text: n.text("text").to_string(),
                size: n.num("size", 16.0) as f32,
                color: tint(n, tint_down, n.color("color")),
                layer: n.num("layer", 10.0) as f32 + z,
                font: n.text("font").to_string(),
                anchor: label_anchor(n),
                wrap: n.num("wrap_width", 0.0) as f32,
                outline: n.num("outline", 0.0) as f32,
                outline_color: n.color("outline_color"),
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
            ObjectKind::Canvas2D => {
                let (w, h) = (n.num("width", 256.0).clamp(1.0, 8192.0) as u32, n.num("height", 256.0).clamp(1.0, 8192.0) as u32);
                let key = canvas_key(id);
                let mut c = tint(n, tint_down, n.color("color"));
                c[3] *= n.num("opacity", 1.0) as f32;
                out.sprites.push(Sprite {
                    object: id,
                    model,
                    size: Vec2::new(w as f32, h as f32),
                    pivot: Vec2::new(n.num("pivot_x", 0.0) as f32, n.num("pivot_y", 0.0) as f32),
                    color: c,
                    texture: key.clone(),
                    layer: n.num("layer", 0.0) as f32 + z,
                    blend,
                    pixel,
                    lit: n.prop("lit").is_some_and(|v| v.as_bool()),
                    ..Default::default()
                });
                out.canvases.push(CanvasDraw { key, size: [w, h], clear_color: n.color("clear_color"), ..Default::default() });
            }
            ObjectKind::Light2D => out.lights_2d.push(Light2DDraw {
                object: id,
                pos: model.w_axis.truncate().truncate(),
                radius: n.num("radius", 200.0) as f32 * model.x_axis.truncate().length(),
                color: {
                    let c = tint(n, tint_down, n.color("color"));
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
                color: tint(n, tint_down, n.color("color")),
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
                    color: tint(n, tint_down, n.color("color")),
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
                color: tint(n, tint_down, n.color("color")),
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
                let c = tint(n, tint_down, n.color("color"));
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
    sort_by_layer(&mut out.sprites);
    out.labels.sort_by(|a, b| a.layer.total_cmp(&b.layer));
    layout_ui(tree, &mut out.ui);
    out
}

/// How much room a UI object takes in a Row, Column, Grid or ScrollBox. A Label without a
/// width is as wide as its longest line (roughly: the fonts are only measured when drawn).
fn room(u: &UiItem) -> Vec2 {
    if u.kind == ObjectKind::Label && !u.props.contains_key("width") {
        let size = u.num("size", 18.0) as f32;
        let text = u.text("text");
        let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
        return Vec2::new(longest as f32 * size * 0.55, text.lines().count().max(1) as f32 * size * 1.25);
    }
    u.size
}

/// Places the UI objects under Rows, Columns, Grids and ScrollBoxes (each with everything under
/// it), sizes the Rows, Columns and Grids to what's in them, and scrolls and clips ScrollBoxes.
pub fn layout_ui(tree: &BlueprintTree, items: &mut [UiItem]) {
    if !items.iter().any(|u| u.kind.is_layout()) {
        return;
    }
    let index: std::collections::HashMap<ObjectId, usize> = items.iter().enumerate().map(|(i, u)| (u.object, i)).collect();
    let kids = |items: &[UiItem], i: usize| -> Vec<usize> { tree.objects[items[i].object].children.iter().filter_map(|c| index.get(c).copied()).collect() };
    // A Grid's column widths and row heights.
    let grid = |items: &[UiItem], i: usize, ks: &[usize]| -> (Vec<f32>, Vec<f32>) {
        let cols = (items[i].num("columns", 2.0).max(1.0) as usize).min(ks.len().max(1));
        let (mut w, mut h) = (vec![0.0f32; cols], vec![0.0f32; ks.len().div_ceil(cols)]);
        for (n, &j) in ks.iter().enumerate() {
            let r = room(&items[j]);
            w[n % cols] = w[n % cols].max(r.x);
            h[n / cols] = h[n / cols].max(r.y);
        }
        (w, h)
    };
    // Sizes, from the innermost out (children come after their parents).
    for i in (0..items.len()).rev() {
        let k = items[i].kind;
        if !k.is_layout() {
            continue;
        }
        let ks = kids(items, i);
        let (gap, pad) = (items[i].num("spacing", 8.0).max(0.0) as f32, items[i].num("padding", 0.0).max(0.0) as f32);
        let gaps = gap * ks.len().saturating_sub(1) as f32;
        let sizes: Vec<Vec2> = ks.iter().map(|&j| room(&items[j])).collect();
        let content = match k {
            ObjectKind::Row => Vec2::new(sizes.iter().map(|s| s.x).sum::<f32>() + gaps, sizes.iter().fold(0.0f32, |m, s| m.max(s.y))),
            ObjectKind::Grid => {
                let (w, h) = grid(items, i, &ks);
                Vec2::new(w.iter().sum::<f32>() + gap * w.len().saturating_sub(1) as f32, h.iter().sum::<f32>() + gap * h.len().saturating_sub(1) as f32)
            }
            _ => Vec2::new(sizes.iter().fold(0.0f32, |m, s| m.max(s.x)), sizes.iter().map(|s| s.y).sum::<f32>() + gaps),
        } + Vec2::splat(pad * 2.0);
        if k == ObjectKind::ScrollBox {
            items[i].props.insert("content_height".into(), crate::Variant::Num(content.y as f64));
        } else {
            items[i].size = content;
        }
    }
    // Places, from the outermost in: each object with everything under it moves to its spot.
    for i in 0..items.len() {
        let k = items[i].kind;
        if !k.is_layout() {
            continue;
        }
        let ks = kids(items, i);
        let (gap, pad) = (items[i].num("spacing", 8.0).max(0.0) as f32, items[i].num("padding", 0.0).max(0.0) as f32);
        let tl = items[i].top_left(Vec2::ZERO);
        let inner = items[i].size - Vec2::splat(pad * 2.0);
        let frac = match items[i].text("align") {
            "center" => 0.5,
            "bottom" | "right" => 1.0,
            _ => 0.0,
        };
        let mut start = tl + Vec2::splat(pad);
        let mut clip = None;
        if k == ObjectKind::ScrollBox {
            let most = (items[i].num("content_height", 0.0) as f32 - items[i].size.y).max(0.0);
            start.y -= (items[i].num("scroll", 0.0) as f32).clamp(0.0, most);
            let mine = (tl, tl + items[i].size);
            clip = Some(match items[i].clip {
                Some((a, b)) => (a.max(mine.0), b.min(mine.1)),
                None => mine,
            });
        }
        let cells = if k == ObjectKind::Grid { Some(grid(items, i, &ks)) } else { None };
        let mut at = start;
        let mut moves = Vec::new();
        for (n, &j) in ks.iter().enumerate() {
            let r = room(&items[j]);
            let spot = match (k, &cells) {
                (ObjectKind::Row, _) => {
                    let p = Vec2::new(at.x, at.y + (inner.y - r.y) * frac);
                    at.x += r.x + gap;
                    p
                }
                (ObjectKind::Grid, Some((w, h))) => {
                    let (c, row) = (n % w.len(), n / w.len());
                    Vec2::new(start.x + w[..c].iter().sum::<f32>() + gap * c as f32, start.y + h[..row].iter().sum::<f32>() + gap * row as f32)
                }
                _ => {
                    let p = Vec2::new(at.x + (inner.x - r.x) * frac, at.y);
                    at.y += r.y + gap;
                    p
                }
            };
            moves.push((j, spot, r));
        }
        for (j, spot, r) in moves {
            // A Label without a width takes the room worked out for it (so centering works).
            if items[j].kind == ObjectKind::Label && !items[j].props.contains_key("width") {
                items[j].size = r;
            }
            let delta = spot - items[j].top_left(Vec2::ZERO);
            let mut under = Vec::new();
            tree.walk_from(items[j].object, &mut under);
            for o in under {
                if let Some(&x) = index.get(&o) {
                    items[x].pos += delta;
                    if clip.is_some() {
                        items[x].clip = clip;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    #[test]
    fn anchor_is_the_origin() {
        let mut props = std::collections::BTreeMap::new();
        props.insert("anchor".to_string(), crate::Variant::Text("top".into()));
        let mut u = UiItem { object: Default::default(), kind: crate::object::ObjectKind::Button, pos: Vec2::ZERO, size: Vec2::new(140.0, 36.0), props, clip: None };
        assert_eq!(u.top_left(Vec2::new(1280.0, 720.0)), Vec2::new(-70.0, 0.0), "top: the middle of its top edge at the position");
        u.props.insert("anchor".into(), crate::Variant::Text("bottom_right".into()));
        assert_eq!(u.top_left(Vec2::new(1280.0, 720.0)), Vec2::new(-140.0, -36.0));
        u.props.insert("anchor".into(), crate::Variant::Text("top_left".into()));
        assert_eq!(u.top_left(Vec2::new(1280.0, 720.0)), Vec2::ZERO);
    }
}
