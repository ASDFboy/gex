//! Property values. Every node property (built-in or custom) is a `Variant`, so the
//! inspector, scene files and JLB scripts all speak the same small set of types.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    Null,
    Bool(bool),
    Num(f64),
    Text(String),
    /// RGBA, 0..1.
    Color([f32; 4]),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    /// A path to an asset: `res://...` (the project) or a mount like `hl2://...`.
    Res(String),
    List(Vec<Variant>),
}

impl Variant {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Variant::Num(n) => Some(*n),
            Variant::Bool(b) => Some(*b as i32 as f64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> bool {
        match self {
            Variant::Bool(b) => *b,
            Variant::Num(n) => *n != 0.0,
            Variant::Null => false,
            _ => true,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Variant::Text(s) | Variant::Res(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_color(&self) -> [f32; 4] {
        match self {
            Variant::Color(c) => *c,
            _ => [1.0; 4],
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Variant::Null => "null",
            Variant::Bool(_) => "bool",
            Variant::Num(_) => "number",
            Variant::Text(_) => "text",
            Variant::Color(_) => "color",
            Variant::Vec2(_) => "vec2",
            Variant::Vec3(_) => "vec3",
            Variant::Res(_) => "resource",
            Variant::List(_) => "list",
        }
    }
}

impl From<f64> for Variant {
    fn from(v: f64) -> Self {
        Variant::Num(v)
    }
}
impl From<bool> for Variant {
    fn from(v: bool) -> Self {
        Variant::Bool(v)
    }
}
impl From<&str> for Variant {
    fn from(v: &str) -> Self {
        Variant::Text(v.to_string())
    }
}
