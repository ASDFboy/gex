//! Project-wide graphics and physics settings. They live in the project file, show up in
//! Project settings, and scripts change them while the game runs with SET_GRAPHICS /
//! SET_PHYSICS (and read them with GRAPHICS / PHYSICS).
//!
//! Each setting is declared once below with its default, range, choices and a line of
//! help; the editor's settings window and the script commands are built from that list.

use serde::{Deserialize, Serialize};

use crate::variant::Variant;

/// What one setting is, for the settings window, the docs and scripts.
#[derive(Clone, Copy, Debug)]
pub struct FieldInfo {
    pub name: &'static str,
    pub section: &'static str,
    /// "number", "whole", "bool", "choice", "color", "vec2", "vec3"
    pub kind: &'static str,
    pub min: f64,
    pub max: f64,
    pub choices: &'static [&'static str],
    pub doc: &'static str,
}

/// A type a setting can have.
pub trait SettingValue: Sized {
    const KIND: &'static str;
    fn to_variant(&self) -> Variant;
    fn from_variant(v: &Variant) -> Option<Self>;
    fn clamp_to(self, _min: f64, _max: f64) -> Self {
        self
    }
}

impl SettingValue for f32 {
    const KIND: &'static str = "number";
    fn to_variant(&self) -> Variant {
        Variant::Num(*self as f64)
    }
    fn from_variant(v: &Variant) -> Option<Self> {
        v.as_f64().map(|x| x as f32)
    }
    fn clamp_to(self, min: f64, max: f64) -> Self {
        self.clamp(min as f32, max as f32)
    }
}

impl SettingValue for u32 {
    const KIND: &'static str = "whole";
    fn to_variant(&self) -> Variant {
        Variant::Num(*self as f64)
    }
    fn from_variant(v: &Variant) -> Option<Self> {
        v.as_f64().map(|x| x.round().max(0.0) as u32)
    }
    fn clamp_to(self, min: f64, max: f64) -> Self {
        self.clamp(min as u32, max as u32)
    }
}

impl SettingValue for bool {
    const KIND: &'static str = "bool";
    fn to_variant(&self) -> Variant {
        Variant::Bool(*self)
    }
    fn from_variant(v: &Variant) -> Option<Self> {
        match v {
            Variant::Bool(b) => Some(*b),
            Variant::Num(n) => Some(*n != 0.0),
            _ => None,
        }
    }
}

impl SettingValue for String {
    const KIND: &'static str = "choice";
    fn to_variant(&self) -> Variant {
        Variant::Text(self.clone())
    }
    fn from_variant(v: &Variant) -> Option<Self> {
        v.as_str().map(|s| s.to_lowercase())
    }
}

impl SettingValue for [f32; 4] {
    const KIND: &'static str = "color";
    fn to_variant(&self) -> Variant {
        Variant::Color(*self)
    }
    fn from_variant(v: &Variant) -> Option<Self> {
        match v {
            Variant::Color(c) => Some(*c),
            _ => None,
        }
    }
}

impl SettingValue for [f32; 2] {
    const KIND: &'static str = "vec2";
    fn to_variant(&self) -> Variant {
        Variant::Vec2([self[0] as f64, self[1] as f64])
    }
    fn from_variant(v: &Variant) -> Option<Self> {
        match v {
            Variant::Vec2(p) => Some([p[0] as f32, p[1] as f32]),
            Variant::Vec3(p) => Some([p[0] as f32, p[1] as f32]),
            Variant::List(l) if l.len() >= 2 => Some([l[0].as_f64()? as f32, l[1].as_f64()? as f32]),
            _ => None,
        }
    }
}

impl SettingValue for [f32; 3] {
    const KIND: &'static str = "vec3";
    fn to_variant(&self) -> Variant {
        Variant::Vec3([self[0] as f64, self[1] as f64, self[2] as f64])
    }
    fn from_variant(v: &Variant) -> Option<Self> {
        match v {
            Variant::Vec3(p) => Some([p[0] as f32, p[1] as f32, p[2] as f32]),
            Variant::List(l) if l.len() >= 3 => Some([l[0].as_f64()? as f32, l[1].as_f64()? as f32, l[2].as_f64()? as f32]),
            _ => None,
        }
    }
}

macro_rules! settings {
    ($(#[$meta:meta])* $name:ident {
        $( ($section:literal, $field:ident, $ty:ty, $def:expr, $min:expr, $max:expr, [$($choice:literal),*], $doc:literal) ),* $(,)?
    }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct $name {
            $( #[doc = $doc] pub $field: $ty, )*
        }

        impl Default for $name {
            fn default() -> Self {
                $name { $( $field: $def, )* }
            }
        }

        impl $name {
            /// Every setting, in the order the settings window shows them.
            pub const FIELDS: &'static [FieldInfo] = &[
                $( FieldInfo {
                    name: stringify!($field),
                    section: $section,
                    kind: if { let c: &[&str] = &[$($choice),*]; !c.is_empty() } { "choice" } else { <$ty as SettingValue>::KIND },
                    min: $min as f64,
                    max: $max as f64,
                    choices: &[$($choice),*],
                    doc: $doc,
                }, )*
            ];

            pub fn info(name: &str) -> Option<&'static FieldInfo> {
                Self::FIELDS.iter().find(|f| f.name == name)
            }

            pub fn get(&self, name: &str) -> Option<Variant> {
                match name {
                    $( stringify!($field) => Some(self.$field.to_variant()), )*
                    _ => None,
                }
            }

            /// Changes a setting by name. Numbers are kept in range; choices must be one
            /// of the listed ones.
            pub fn set(&mut self, name: &str, v: &Variant) -> Result<(), String> {
                match name {
                    $( stringify!($field) => {
                        let info = Self::info(name).unwrap();
                        let new = <$ty as SettingValue>::from_variant(v)
                            .ok_or_else(|| format!("{} needs a {} (got a {})", name, info.kind, v.type_name()))?
                            .clamp_to(info.min, info.max);
                        if !info.choices.is_empty() {
                            let s = new.to_variant();
                            let s = s.as_str().unwrap_or("");
                            if !info.choices.contains(&s) {
                                return Err(format!("{} is one of: {}", name, info.choices.join(", ")));
                            }
                        }
                        self.$field = new;
                        Ok(())
                    } )*
                    _ => Err(format!("there's no setting called {} (see Project settings for the list)", name)),
                }
            }
        }
    };
}

settings! {
    /// How the game looks, project-wide. Environment nodes and each node's own look
    /// settings work on top of these.
    Graphics {
        ("Picture", render_scale, f32, 1.0, 0.25, 2.0, [], "How many pixels are drawn for each pixel on screen: below 1 is faster and softer, above 1 is sharper (and slower)."),
        ("Picture", antialiasing, String, "fxaa".into(), 0, 0, ["off", "fxaa"], "Smooths jagged edges: fxaa is a fast blur along edges; off keeps every pixel crisp."),
        ("Picture", fps_cap, u32, 0, 0, 1000, [], "The most frames drawn each second (0 = no limit beyond vsync). Lower saves power."),
        ("Textures", texture_filter, String, "smooth".into(), 0, 0, ["smooth", "pixel"], "3D textures: smooth blends between texels; pixel keeps them blocky (retro look)."),
        ("Textures", sprite_filter, String, "pixel".into(), 0, 0, ["pixel", "smooth"], "2D sprites and tiles: pixel keeps pixel art sharp; smooth blends (for painted art)."),
        ("Textures", mipmaps, bool, true, 0, 0, [], "Smaller copies of each texture for far-away surfaces: less shimmering at a distance."),
        ("Textures", anisotropy, u32, 8, 1, 16, [], "Sharpness of textures seen at a slant, like a floor stretching away (1 = off, 16 = sharpest)."),
        ("Lighting", lighting, bool, true, 0, 0, [], "3D lights and ambient light. Off draws every 3D surface at its plain color."),
        ("Lighting", max_lights, u32, 8, 0, 16, [], "The most 3D lights used at once (the shadow-casting sun counts first)."),
        ("Lighting", specular, bool, true, 0, 0, [], "Shiny highlights on 3D surfaces."),
        ("Lighting", normal_maps, bool, true, 0, 0, [], "Surface bumps from normal-map textures."),
        ("Lighting", lights_2d, bool, true, 0, 0, [], "2D lighting (a Camera2D's ambient dark and Light2D nodes)."),
        ("Shadows", shadows, bool, true, 0, 0, [], "Shadows from the sun (the first directional Light3D with shadows on)."),
        ("Shadows", shadow_resolution, u32, 2048, 256, 8192, [], "Pixels across each shadow map: higher is sharper and uses more memory."),
        ("Shadows", shadow_quality, String, "medium".into(), 0, 0, ["low", "medium", "high"], "How many samples soften each shadow edge: 4, 9 or 16."),
        ("Shadows", shadow_softness, f32, 1.0, 0.0, 6.0, [], "How blurry shadow edges are (0 = hard)."),
        ("Shadows", shadow_bias, f32, 0.002, 0.0, 0.05, [], "Pushes shadows back a little so surfaces don't shadow themselves (acne). Too much and shadows float."),
        ("Shadows", shadow_distance_scale, f32, 1.0, 0.1, 4.0, [], "Multiplies every Environment's shadow distance: lower is sharper and shorter."),
        ("Effects", sky, bool, true, 0, 0, [], "Draws the Environment's sky (off shows the background color)."),
        ("Effects", fog, bool, true, 0, 0, [], "Fog from Environment nodes."),
        ("Effects", bloom, bool, true, 0, 0, [], "Glow around bright things (each Environment sets how much)."),
        ("Effects", bloom_quality, u32, 1, 1, 4, [], "How many blur passes bloom uses: more is wider and smoother."),
        ("Effects", particles, f32, 1.0, 0.0, 1.0, [], "How many particles emitters make, 0 to 1 of their rate (lower is faster)."),
        ("Color", tonemap, String, "none".into(), 0, 0, ["none", "aces", "reinhard", "filmic"], "How very bright 3D light is squeezed into the screen's range: aces and filmic look like film, reinhard is gentle."),
        ("Color", exposure, f32, 1.0, 0.0, 8.0, [], "Brightness of 3D light before tonemapping (like a camera's exposure)."),
        ("Color", brightness, f32, 0.0, -1.0, 1.0, [], "Added to the whole picture."),
        ("Color", contrast, f32, 1.0, 0.0, 3.0, [], "Pushes darks and lights apart (1 = unchanged)."),
        ("Color", saturation, f32, 1.0, 0.0, 3.0, [], "Color strength: 0 is black and white, 1 unchanged."),
        ("Color", gamma, f32, 1.0, 0.2, 3.0, [], "Brightens (above 1) or darkens (below 1) the middle tones."),
        ("Color", tint, [f32; 4], [1.0; 4], 0, 0, [], "Multiplies the whole picture by a color."),
        ("Color", vignette, f32, 0.0, 0.0, 1.0, [], "Darkens the corners of the screen."),
        ("Color", grain, f32, 0.0, 0.0, 1.0, [], "Moving film grain over the picture."),
    }
}

settings! {
    /// How physics works, project-wide. Each body's own settings work on top of these.
    PhysicsSettings {
        ("World", gravity_2d, [f32; 2], [0.0, 980.0], 0, 0, [], "Gravity for 2D bodies, in pixels per second² (y grows downward)."),
        ("World", gravity_3d, [f32; 3], [0.0, -9.81, 0.0], 0, 0, [], "Gravity for 3D bodies, in metres per second²."),
        ("World", linear_damping, f32, 0.0, 0.0, 50.0, [], "Air resistance on every body's movement (added to each body's own)."),
        ("World", angular_damping, f32, 0.0, 0.0, 50.0, [], "Air resistance on every body's spinning (added to each body's own)."),
        ("World", sleeping, bool, true, 0, 0, [], "Bodies that stop moving go to sleep (cheaper) until something touches them."),
        ("Timing", tick_rate, f32, 60.0, 10.0, 480.0, [], "Physics steps each second: higher is more accurate (fast objects, stacks) and slower."),
        ("Timing", max_steps, u32, 5, 1, 30, [], "The most physics steps one frame may catch up on when the game falls behind."),
        ("Solver", solver_iterations, u32, 8, 1, 32, [], "How hard the solver works each step: more keeps stacks and joints steadier."),
        ("Solver", ccd, bool, true, 0, 0, [], "Continuous collision: fast bodies can't pass through thin walls (a body's own ccd setting can turn it off)."),
        ("Solver", ccd_substeps, u32, 1, 1, 8, [], "Extra checks per step for fast bodies."),
        ("Solver", contact_stiffness, f32, 30.0, 1.0, 120.0, [], "How hard touching bodies push apart: higher is stiffer, lower is squishier."),
        ("Characters", max_slope, f32, 45.0, 0.0, 89.0, [], "The steepest slope (degrees) a body moved with MOVE can walk up."),
        ("Characters", step_height, f32, 0.0, 0.0, 2.0, [], "Steps up ledges this high when moved with MOVE (3D: metres, 2D: x100 pixels; 0 = off)."),
        ("Characters", snap_to_ground, f32, 0.1, 0.0, 2.0, [], "Keeps a body moved with MOVE stuck to the ground going down slopes and steps (3D: metres, 2D: x100 pixels)."),
    }
}

impl Graphics {
    /// Shadow samples per pixel for the chosen quality.
    pub fn shadow_taps(&self) -> u32 {
        match self.shadow_quality.as_str() {
            "low" => 4,
            "high" => 16,
            _ => 9,
        }
    }
    pub fn tonemap_id(&self) -> u32 {
        match self.tonemap.as_str() {
            "aces" => 1,
            "reinhard" => 2,
            "filmic" => 3,
            _ => 0,
        }
    }
    /// True when the color settings change nothing (the color pass can be skipped).
    pub fn color_neutral(&self) -> bool {
        self.brightness == 0.0 && self.contrast == 1.0 && self.saturation == 1.0 && self.gamma == 1.0 && self.tint == [1.0; 4] && self.vignette == 0.0 && self.grain == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_and_set_by_name() {
        let mut g = Graphics::default();
        assert_eq!(g.get("render_scale"), Some(Variant::Num(1.0)));
        g.set("render_scale", &Variant::Num(9.0)).unwrap();
        assert_eq!(g.render_scale, 2.0, "kept in range");
        assert!(g.set("tonemap", &Variant::Text("nope".into())).is_err());
        g.set("tonemap", &Variant::Text("ACES".into())).unwrap();
        assert_eq!(g.tonemap_id(), 1);
        assert!(g.set("shadows", &Variant::Text("x".into())).is_err());
        assert!(g.set("nothing", &Variant::Null).is_err());
        assert_eq!(Graphics::info("antialiasing").unwrap().kind, "choice");
        let mut p = PhysicsSettings::default();
        p.set("gravity_2d", &Variant::List(vec![Variant::Num(0.0), Variant::Num(100.0)])).unwrap();
        assert_eq!(p.gravity_2d, [0.0, 100.0]);
        assert_eq!(PhysicsSettings::info("gravity_3d").unwrap().kind, "vec3");
        // docs/SETTINGS.md lists them all.
        let doc = include_str!("../../../docs/SETTINGS.md");
        for f in Graphics::FIELDS.iter().chain(PhysicsSettings::FIELDS) {
            assert!(doc.contains(&format!("| `{}` |", f.name)), "add {} to docs/SETTINGS.md", f.name);
        }
        // Every field has help and a section.
        for f in Graphics::FIELDS.iter().chain(PhysicsSettings::FIELDS) {
            assert!(!f.doc.is_empty() && !f.section.is_empty(), "{}", f.name);
        }
    }
}
