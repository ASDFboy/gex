//! Moving values between node properties (Variant) and .jlb values.

use gex_core::{NodeId, Variant};
use jlb::embed::{as_f64, list_table, named_table, num, table_get};
use jlb::Value;
use slotmap::{Key, KeyData};

/// Node handles are plain numbers in .jlb.
pub fn id_value(id: NodeId) -> Value {
    num(id.data().as_ffi() as f64)
}

pub fn value_id(v: &Value) -> Option<NodeId> {
    let n = as_f64(v)?;
    if n < 0.0 || n.fract() != 0.0 {
        return None;
    }
    Some(NodeId::from(KeyData::from_ffi(n as u64)))
}

pub fn to_value(v: &Variant) -> Value {
    match v {
        Variant::Null => Value::Null,
        Variant::Bool(b) => Value::Bool(*b),
        Variant::Num(n) => num(*n),
        Variant::Text(s) | Variant::Res(s) => Value::text(s),
        Variant::Color(c) => named_table(vec![("r", num(c[0] as f64)), ("g", num(c[1] as f64)), ("b", num(c[2] as f64)), ("a", num(c[3] as f64))]),
        Variant::Vec2(p) => named_table(vec![("x", num(p[0])), ("y", num(p[1]))]),
        Variant::Vec3(p) => named_table(vec![("x", num(p[0])), ("y", num(p[1])), ("z", num(p[2]))]),
        Variant::List(l) => list_table(l.iter().map(to_value).collect()),
    }
}

fn field(v: &Value, k: &str, default: f64) -> f64 {
    table_get(v, k).as_ref().and_then(as_f64).unwrap_or(default)
}

/// A .jlb value as a property, shaped like `like` (the property's current value) when
/// there is one, so a color stays a color and a resource path stays a resource.
/// None for values properties can't hold (functions, tasks, ...).
pub fn to_variant(v: &Value, like: Option<&Variant>) -> Option<Variant> {
    Some(match (v, like) {
        (Value::Null, _) => Variant::Null,
        (Value::Bool(b), _) => Variant::Bool(*b),
        (Value::Num(_), _) => Variant::Num(as_f64(v)?),
        (Value::Text(s), Some(Variant::Res(_))) => Variant::Res(s.to_string()),
        // "#ff8800" or "red" into a color property.
        (Value::Text(_), Some(Variant::Color(_))) => Variant::Color(crate::commands::color_arg(Some(v))),
        // [1, 0.5, 0, 1] (no names) into a color property.
        (Value::Table(rc, _), Some(Variant::Color(_))) if rc.named.len() == 0 => Variant::Color(crate::commands::color_arg(Some(v))),
        (Value::Text(s), _) => Variant::Text(s.to_string()),
        (Value::Table(..), Some(Variant::Color(c))) => Variant::Color([
            field(v, "r", c[0] as f64) as f32,
            field(v, "g", c[1] as f64) as f32,
            field(v, "b", c[2] as f64) as f32,
            field(v, "a", c[3] as f64) as f32,
        ]),
        (Value::Table(..), Some(Variant::Vec2(p))) => Variant::Vec2([field(v, "x", p[0]), field(v, "y", p[1])]),
        (Value::Table(..), Some(Variant::Vec3(p))) => Variant::Vec3([field(v, "x", p[0]), field(v, "y", p[1]), field(v, "z", p[2])]),
        (Value::Table(rc, _), _) => {
            if rc.named.len() == 0 {
                Variant::List(rc.items.iter().filter_map(|s| to_variant(&s.borrow(), None)).collect())
            } else {
                return None;
            }
        }
        _ => return None,
    })
}
