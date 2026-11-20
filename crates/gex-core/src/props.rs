//! What each built-in property is: which section of the Inspector it sits in, the
//! choices or range it takes, and a line of help (shown on hover and in the docs).

use crate::object::ObjectKind;

#[derive(Clone, Copy, Debug, Default)]
pub struct PropInfo {
    pub group: &'static str,
    pub choices: &'static [&'static str],
    /// Slider range (min, max) for numbers; None = any value.
    pub range: Option<(f64, f64)>,
    pub doc: &'static str,
}

const BLEND: &[&str] = &["normal", "add", "multiply"];
const FILTER: &[&str] = &["project", "pixel", "smooth"];
const ANCHOR: &[&str] = &["top_left", "top", "top_right", "left", "center", "right", "bottom_left", "bottom", "bottom_right"];

/// The Inspector section a property belongs in.
fn group(kind: ObjectKind, k: &str) -> &'static str {
    use ObjectKind::*;
    match kind {
        Light3D => match k {
            "shadows" | "shadow_strength" | "shadow_bias" => "Shadows",
            _ => "Main",
        },
        Environment => match k {
            "sky_top" | "sky_horizon" | "ground" | "sky_texture" | "sky_energy" => "Sky",
            "ambient" | "ambient_energy" => "Ambient light",
            k if k.starts_with("fog") => "Fog",
            "shadow_distance" | "shadow_split" => "Shadows",
            _ => "Effects",
        },
        Camera3D => match k {
            "ortho" | "ortho_size" | "near" | "far" => "Projection",
            _ => "Main",
        },
        Body2D | Body3D => match k {
            "body" | "shape" | "width" | "height" | "depth" | "radius" => "Main",
            "modulate" => "Look",
            "vx" | "vy" | "vz" | "spin" | "spin_x" | "spin_y" | "spin_z" | "on_floor" => "Motion (live)",
            "layer" | "mask" => "Collision layers",
            _ => "Physics",
        },
        Collider2D | Collider3D => match k {
            "layer" | "mask" => "Collision layers",
            _ => "Main",
        },
        _ => match k {
            k if kind == Mesh3D && (k.starts_with("face_") || k.starts_with("hide_")) => "Advanced texture settings",
            "opacity" | "blend" | "filter" | "lit" | "modulate" | "sides" | "emissive_color" | "rim" | "rim_color" | "alpha_cut" | "on_top" | "fog" | "receive_shadows"
            | "cast_shadows" | "draw_distance" | "unlit" | "specular" | "emissive" | "normal_map" | "shader" | "flip_h" | "flip_v" | "glow" => "Look",
            k if k.starts_with("uv_") || k.starts_with("parallax") || k.starts_with("repeat") => "Look",
            "animations" | "animation" | "hframes" | "vframes" | "frame" | "anim_time" | "anim_blend" => "Animation",
            k if k.starts_with("anim_") => "Animation",
            _ => "Main",
        },
    }
}

/// Everything known about a built-in property (custom properties get the defaults).
pub fn info(kind: ObjectKind, k: &str) -> PropInfo {
    use ObjectKind::*;
    let g = group(kind, k);
    let p = |choices: &'static [&'static str], range: Option<(f64, f64)>, doc: &'static str| PropInfo { group: g, choices, range, doc };
    match (kind, k) {
        // ---- look, shared ----
        (Sprite2D, "solid_shape") => p(&["box", "circle"], None, "The shape of a solid sprite: its box (turning with it), or a circle as wide as the sprite."),
        (Sprite2D, "solid") => p(&[], None, "A wall the size of the sprite (turned and scaled with it; solid_shape: box or circle): bodies and MOVE stop at it, and on_collide events reach it. No Collider2D needed. It stays put (it isn't pushed or knocked about)."),
        (_, "opacity") => p(&[], Some((0.0, 1.0)), "How see-through it is: 1 solid, 0 invisible (it still runs and collides)."),
        (_, "blend") => p(BLEND, None, "How it mixes with what's behind: normal covers, add glows (fire, lasers), multiply darkens (shadows, stains)."),
        (_, "filter") => p(FILTER, None, "Texture filtering: pixel keeps texels blocky, smooth blends them, project uses the project's setting."),
        (_, "lit") => p(&[], None, "Darkened by 2D lighting. Off: always full brightness (drawn above lit sprites), for glowing things and HUD-like sprites."),
        (_, "modulate") => p(&[], None, "A color multiplied into this object and everything under it (alpha fades the whole group)."),
        (_, "sides") => p(&["auto", "front", "both"], None, "Which faces draw: front only, both (thin things like leaves and paper), or auto (planes both, solids front)."),
        (_, "emissive") => p(&[], Some((0.0, 10.0)), "How much it glows on its own, without light (works with bloom)."),
        (_, "emissive_color") => p(&[], None, "The color of its glow."),
        (_, "rim") => p(&[], Some((0.0, 4.0)), "Light around its edges as seen from the camera: a soft outline or backlight."),
        (_, "rim_color") => p(&[], None, "The color of the rim light."),
        (_, "alpha_cut") => p(&[], Some((0.0, 1.0)), "Texels less opaque than this become holes (fences, leaves). 0 keeps everything."),
        (_, "on_top") => p(&[], None, "Drawn over everything else, ignoring what's in front (markers, held weapons, x-ray)."),
        (_, "fog") => p(&[], None, "Fog from the Environment covers it (off for things that should stay clear, like a sun or UI in 3D)."),
        (_, "receive_shadows") => p(&[], None, "Other things' shadows fall on it."),
        (_, "cast_shadows") => p(&[], None, "It casts a shadow from the sun."),
        (_, "draw_distance") => p(&[], Some((0.0, 1000.0)), "Not drawn when further than this from the camera, in metres (0 = always drawn). Saves time on small far things."),
        (_, "uv_offset_x") | (_, "uv_offset_y") => p(&[], None, "Shifts the texture across the surface, in repeats."),
        (_, "uv_scroll_x") | (_, "uv_scroll_y") => p(&[], Some((-10.0, 10.0)), "Slides the texture across the surface, in repeats per second (water, conveyor belts)."),
        (Mesh3D, "mesh") => p(&["cube", "sphere", "plane", "quad"], None, "A built-in shape, or a model file: .glb, .gltf, .fbx, .dae, .obj, .3ds, .stl, .ply, .vox, or a Source game's .mdl or .bsp map (drop one from the assets, yours or a mounted game's)."),
        (Mesh3D, k) if k.starts_with("face_") => p(&[], None, "A texture for just this face of a cube (empty: the mesh's texture)."),
        (Mesh3D, k) if k.starts_with("hide_") => p(&[], None, "Don't draw this face of a cube."),
        (_, "uv_scale_x") | (_, "uv_scale_y") => p(&[], None, "How many times the texture repeats across the surface."),
        (_, "unlit") => p(&[], None, "Ignores lights: drawn at its plain color."),
        (_, "specular") if kind == Light3D => p(&[], Some((0.0, 4.0)), "Strength of the shiny highlights this light makes."),
        (_, "specular") => p(&[], Some((0.0, 1.0)), "How shiny it is: 0 matte, 1 glossy."),
        (_, "normal_map") => p(&[], None, "A normal-map texture that gives the surface bumps and grooves."),
        (_, "shader") => p(&[], None, "A .wgsl file with a custom material function (see docs/SHADERS.md)."),
        (_, "color") => p(&[], None, "Its color (multiplies the texture, if there is one)."),
        (_, "texture") => p(&[], None, "An image file. Drag one in from the Assets panel."),
        (_, "layer") if kind.is_2d() && !matches!(kind, Body2D | Collider2D) => p(&[], None, "Drawing order: higher layers draw on top. Added to the object's z (Position), which does the same and carries to its children."),
        (_, "anchor") => p(ANCHOR, None, "Its origin: which point of it sits at its position. top_left puts its top-left corner there, top the middle of its top edge, center its middle, bottom_right its bottom-right corner."),
        (_, "align") => p(&["left", "center", "right"], None, "How the text lines up."),
        // ---- lights ----
        (Light3D, "type") => p(&["directional", "point", "spot"], None, "directional: sunlight from one direction everywhere; point: a bulb shining all ways; spot: a cone (a flashlight)."),
        (Light3D, "energy") => p(&[], Some((0.0, 16.0)), "Brightness."),
        (Light3D, "range") => p(&[], Some((0.0, 200.0)), "Point and spot lights: how far the light reaches, in metres."),
        (Light3D, "angle") => p(&[], Some((1.0, 89.0)), "Spot lights: half the cone's width, in degrees."),
        (Light3D, "softness") => p(&[], Some((0.0, 1.0)), "Spot lights: how soft the cone's edge is."),
        (Light3D, "falloff") => p(&[], Some((0.1, 8.0)), "How quickly light fades toward the range: 1 even, 2 natural, higher stays bright then drops."),
        (Light3D, "shadows") => p(&[], None, "Casts shadows (the first directional light with this on is the shadow sun)."),
        (Light3D, "shadow_strength") => p(&[], Some((0.0, 1.0)), "How dark its shadows are: 1 full, 0.5 half."),
        (Light3D, "shadow_bias") => p(&[], Some((-1.0, 0.05)), "Its own shadow bias (below 0 uses the project's). Raise it if surfaces get speckled shadows."),
        (Light2D, "energy") => p(&[], Some((0.0, 8.0)), "Brightness."),
        (Light2D, "radius") => p(&[], None, "How far the light reaches, in pixels."),
        // ---- environment ----
        (Environment, "sky_energy") => p(&[], Some((0.0, 4.0)), "Sky brightness."),
        (Environment, "ambient_energy") => p(&[], Some((0.0, 4.0)), "How much ambient light fills the shadows."),
        (Environment, "fog_mode") => p(&["linear", "exp"], None, "linear: clear until fog_start, solid at fog_end. exp: thickens with distance by fog_density."),
        (Environment, "fog_density") => p(&[], Some((0.0, 0.5)), "exp fog: how thick it is."),
        (Environment, "fog_start") | (Environment, "fog_end") => p(&[], None, "linear fog: where it starts and where it's solid, in metres (end above start turns it on)."),
        (Environment, "shadow_distance") => p(&[], Some((2.0, 500.0)), "How far from the camera shadows reach, in metres. Shorter is sharper."),
        (Environment, "shadow_split") => p(&[], Some((0.02, 0.95)), "How much of the shadow distance gets the sharp near shadows."),
        (Environment, "bloom") => p(&[], Some((0.0, 4.0)), "Glow around bright things (0 = off)."),
        (Environment, "bloom_threshold") => p(&[], Some((0.0, 1.0)), "How bright something must be to glow."),
        (Environment, "exposure") => p(&[], Some((0.0, 8.0)), "Brightness of the light (multiplies the project's exposure)."),
        (Environment, "contrast") | (Environment, "saturation") => p(&[], Some((0.0, 3.0)), "Multiplies the project's setting (1 = unchanged)."),
        (Environment, "tint") => p(&[], None, "Multiplies the whole picture by a color while this Environment is used."),
        (Environment, "vignette") => p(&[], Some((0.0, 1.0)), "Darkens the screen's corners (added to the project's)."),
        // ---- cameras ----
        (Camera3D, "fov") => p(&[], Some((10.0, 150.0)), "Field of view, in degrees (wider sees more, looks faster)."),
        (Camera3D, "ortho") => p(&[], None, "No perspective: far things are as big as near ones (strategy and puzzle views)."),
        (Camera3D, "ortho_size") => p(&[], Some((0.1, 200.0)), "With ortho: how many metres tall the view is."),
        (Camera2D, "ambient") => p(&[], None, "The dark that 2D lights shine through (white = no 2D lighting)."),
        // ---- bodies ----
        (Body2D | Body3D, "body") => p(&["dynamic", "static", "kinematic"], None, "dynamic falls and bounces; static never moves (walls); kinematic is moved by scripts with MOVE (characters)."),
        (Body2D, "shape") => p(&["box", "circle", "capsule"], None, "Its collision shape."),
        (Body3D, "shape") => p(&["box", "sphere", "capsule", "hull"], None, "Its collision shape (hull: wraps the model of a Mesh3D under it)."),
        (Body2D | Body3D, "mass") => p(&[], Some((0.0, 1000.0)), "Its mass (0 = worked out from its size and density)."),
        (Body2D | Body3D, "density") => p(&[], None, "Mass per size, used when mass is 0."),
        (Body2D | Body3D, "friction") => p(&[], Some((0.0, 2.0)), "How much it grips what it slides on."),
        (Body2D | Body3D, "bounce") => p(&[], Some((0.0, 1.0)), "How bouncy it is: 0 thud, 1 bounces back as high."),
        (Body2D | Body3D, "gravity_scale") => p(&[], Some((-2.0, 4.0)), "Multiplies gravity for this body (0 floats, negative rises)."),
        (Body2D | Body3D, "linear_damping") => p(&[], Some((0.0, 20.0)), "Slows its movement over time, like air or water."),
        (Body2D | Body3D, "angular_damping") => p(&[], Some((0.0, 20.0)), "Slows its spinning over time."),
        (Body2D | Body3D, "ccd") => p(&["project", "on", "off"], None, "Continuous collision, so it can't pass through thin walls when fast (project: the project's setting)."),
        (Body2D | Body3D, "can_sleep") => p(&[], None, "May go to sleep when still (cheaper). Off keeps it always awake."),
        (Body2D | Body3D, "enabled") => p(&[], None, "Off takes it out of physics entirely (no falling, no collisions) until turned back on."),
        (Body2D | Body3D, "fixed_rotation") => p(&[], None, "Never rotates from collisions."),
        (Body2D | Body3D, "lock_x") | (Body2D | Body3D, "lock_y") | (Body3D, "lock_z") => p(&[], None, "Never moves along this axis (a body sliding on a rail, a 2.5D game)."),
        (Body2D, "spin") => p(&[], None, "How fast it spins, degrees per second (live: set it to spin the body)."),
        (Body3D, "spin_x") | (Body3D, "spin_y") | (Body3D, "spin_z") => p(&[], None, "How fast it spins around this axis, degrees per second (live)."),
        (Body2D | Body3D, "vx") | (Body2D | Body3D, "vy") | (Body3D, "vz") => p(&[], None, "Its velocity (live: set it to push the body)."),
        (Body2D | Body3D, "on_floor") => p(&[], None, "TRUE while it rests on something (read it from scripts)."),
        (Collider3D, "shape") => p(&["box", "sphere"], None, "The area's shape."),
        (_, "layer") | (_, "mask") => p(&[], None, "Collision layers by name, comma separated: layer is what it is, mask is what it hits (empty = default / everything)."),
        _ => p(&[], None, ""),
    }
}

/// The Inspector sections of a kind, in order (Main first).
pub fn groups(kind: ObjectKind) -> Vec<&'static str> {
    let mut out = vec!["Main"];
    for (k, _) in kind.default_props() {
        let g = group(kind, k);
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ALL_KINDS;

    #[test]
    fn choices_include_the_default() {
        for &k in ALL_KINDS {
            for (name, v) in k.default_props() {
                let i = info(k, name);
                if !i.choices.is_empty() {
                    let d = v.as_str().unwrap_or("");
                    assert!(i.choices.contains(&d), "{:?}.{} default {:?} not in {:?}", k, name, d, i.choices);
                }
            }
        }
        assert_eq!(info(ObjectKind::Mesh3D, "rim").group, "Look");
        assert_eq!(groups(ObjectKind::Body3D)[0], "Main");
    }
}
