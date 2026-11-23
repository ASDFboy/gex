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
        Sprite2D if matches!(k, "brightness" | "hue" | "saturation" | "invert" | "shader") || k.starts_with("value_") => "Effects",
        Particles2D if matches!(k, "hframes" | "vframes" | "anim_fps" | "frame_random" | "frame_count") => "Animation",
        Particles2D if matches!(k, "start_rotation" | "start_rotation_random" | "align" | "spin") => "Turning",
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
        (Label2D, "align") => p(&["left", "center", "right"], None, "Which point of the text sits at the label's position, across: its left edge, middle or right edge (lines line up the same way)."),
        (Label2D, "valign") => p(&["top", "middle", "bottom"], None, "Which point of the text sits at the label's position, up and down."),
        (Label2D, "wrap_width") => p(&[], None, "Lines longer than this many pixels break at a space (0: no wrapping)."),
        (Label2D, "outline") => p(&[], Some((0.0, 8.0)), "An outline around the letters this many pixels wide (0: none), in outline_color."),
        (Sprite2D, "mouse_precise") => p(&[], None, "Mouse events (on_mouse_enter, on_clicked...) only count the picture's visible pixels, not its whole rectangle."),
        (Sprite2D, "pivot_x") | (Sprite2D, "pivot_y") => p(&[], Some((0.0, 1.0)), "Which point of the picture sits at the sprite's position (and it turns and scales around): 0 the left / top edge, 0.5 the middle, 1 the right / bottom edge. Used when centered is on."),
        (Sprite2D, "brightness") => p(&[], Some((-1.0, 1.0)), "Lighter (up to 1, white) or darker (down to -1, black): added to every color."),
        (Sprite2D, "hue") => p(&[], Some((0.0, 360.0)), "Turns every color around the color wheel by this many degrees (red becomes green at 120)."),
        (Sprite2D, "saturation") => p(&[], Some((0.0, 2.0)), "How colorful: 1 as drawn, 0 grey, above 1 stronger."),
        (Sprite2D, "invert") => p(&[], Some((0.0, 1.0)), "Inverts the colors (1), like a photo negative; in between fades to it."),
        (Sprite2D, "shader") => p(&[], None, "A .wgsl file with fn sprite(...) that decides each pixel's color (see Custom shaders in the docs). value_1 to value_4 reach it as values.x to values.w."),
        (Sprite2D, "value_1") | (Sprite2D, "value_2") | (Sprite2D, "value_3") | (Sprite2D, "value_4") => p(&[], None, "A number for the custom shader (values.x, .y, .z, .w); scripts can change it every frame."),
        (Particles2D, "start_rotation") => p(&[], None, "The angle each particle starts at, in degrees."),
        (Particles2D, "start_rotation_random") => p(&[], Some((0.0, 360.0)), "Up to this many degrees added at random to each particle's starting angle (360: any angle)."),
        (Particles2D, "align") => p(&[], None, "Each particle faces the way it's moving (its rotation is added on top)."),
        (Particles2D, "size_random") => p(&[], Some((0.0, 1.0)), "Particles are up to this much smaller at random (0.3: between 70% and 100% of size)."),
        (Particles2D, "gravity_direction") => p(&[], None, "Which way gravity pulls, in degrees: 90 down, -90 up, 0 right."),
        (Particles2D, "damping") => p(&[], Some((0.0, 10.0)), "How quickly particles slow down: the share of their speed lost each second (negative speeds them up)."),
        (Particles2D, "hframes") | (Particles2D, "vframes") => p(&[], None, "The texture is a sheet of frames this many across / down; particles play through them."),
        (Particles2D, "anim_fps") => p(&[], None, "Frames a second for animated particles; 0 plays the frames once over each particle's life (stretched to fit)."),
        (Particles2D, "frame_random") => p(&[], None, "Each particle starts on a random frame of the sheet (and plays on from there)."),
        (Particles2D, "frame_count") => p(&[], None, "How many frames the sheet has, when its last row isn't full (0: every cell)."),
        (Particles2D, "three_colors") => p(&[], None, "Particles go from color to color_mid (halfway through their life) to color_end, instead of straight from color to color_end."),
        (Particles2D, "color_mid") => p(&[], None, "The color halfway through a particle's life, when three_colors is on."),
        (Particles2D, "aspect") => p(&[], None, "Particle height as a share of its width (size): 1 square; use the texture's height / width to keep its shape."),
        (Particles2D, "lifetime_random") => p(&[], Some((0.0, 1.0)), "Each particle lives up to this much longer or shorter at random (0.2: 80% to 120% of lifetime)."),
        (Particles2D, "acceleration") => p(&[], None, "Speed gained each second along the way it's going, in pixels per second (negative slows it down to a stop)."),
        (Particles2D, "direction_wiggle") => p(&[], None, "Particles weave from side to side by up to this many degrees as they fly."),
        (Particles2D, "area_width") | (Particles2D, "area_height") => p(&[], None, "Particles start anywhere in a rectangle this wide and tall around the emitter (both 0: the area circle instead)."),
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
        (ScrollBox, "color") => p(&[], None, "Its background (clear by default: put it on a Panel, or give it a color)."),
        (_, "color") => p(&[], None, "Its color (multiplies the texture, if there is one)."),
        (_, "texture") => p(&[], None, "An image file. Drag one in from the Assets panel."),
        (_, "layer") if kind.is_2d() && !matches!(kind, Body2D | Collider2D) => p(&[], None, "Drawing order: higher layers draw on top. Added to the object's z (Position), which does the same and carries to its children."),
        (_, "anchor") => p(ANCHOR, None, "Its origin: which point of it sits at its position. top_left puts its top-left corner there, top the middle of its top edge, center its middle, bottom_right its bottom-right corner."),
        // ---- layout and the other UI ----
        (Row, "align") => p(&["top", "center", "bottom"], None, "How objects of different heights line up in the row."),
        (Column | ScrollBox, "align") => p(&["left", "center", "right"], None, "How objects of different widths line up in the column."),
        (_, "spacing") => p(&[], Some((0.0, 64.0)), "The gap between the objects it places, in pixels."),
        (_, "padding") => p(&[], Some((0.0, 64.0)), "Space kept free inside its edges, in pixels."),
        (Grid, "columns") => p(&[], Some((1.0, 12.0)), "How many objects go across before the next row starts."),
        (ScrollBox, "scroll") => p(&[], None, "How far down it's scrolled, in pixels (live: set it to scroll from a script)."),
        (ScrollBox, "bar_color") => p(&[], None, "The scroll bar's color (it shows when there's more than fits)."),
        (Checkbox, "checked") => p(&[], None, "Whether it's ticked (live: clicking it changes this and runs on_toggled)."),
        (Checkbox, "check_color") => p(&[], None, "The tick's color."),
        (Slider, "value") => p(&[], None, "Where the handle is, between min and max (live: dragging it changes this and runs on_value_changed)."),
        (Slider, "min") | (Slider, "max") => p(&[], None, "The lowest and highest value."),
        (Slider, "step") => p(&[], None, "The value moves in steps of this (1 = whole numbers; 0 = smoothly)."),
        (_, "disabled") => p(&[], None, "Greyed out and can't be used."),
        // ---- timers ----
        (Timer, "wait_time") => p(&[], Some((0.0, 60.0)), "Seconds it counts down from."),
        (Timer, "running") => p(&[], None, "Counting down (from the start of the game when on). Turning it off pauses it; PLAY(timer) starts it over, STOP(timer) stops it."),
        (Timer, "repeat") => p(&[], None, "Starts over each time it reaches zero; off, it stops after once."),
        (Timer, "time_left") => p(&[], None, "Seconds left (live)."),
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
        (Canvas2D, "width") => p(&[], None, "How many pixels across its picture is (what's drawn into it keeps these pixels)."),
        (Canvas2D, "height") => p(&[], None, "How many pixels tall its picture is."),
        (Canvas2D, "clear_color") => p(&[], None, "What it starts as, and what CLEAR_CANVAS fills it with when no color is given (see-through by default)."),
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
