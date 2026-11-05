//! TWEEN: slide a node property (a number, a position, a color) to a new value over time.

use gex_core::{NodeId, SceneTree, Variant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Color([f32; 4]),
}

#[derive(Clone, Debug)]
pub struct Tween {
    pub node: NodeId,
    pub prop: String,
    pub from: Val,
    pub to: Val,
    pub time: f64,
    pub length: f64,
    pub ease: String,
}

pub const EASES: &[&str] = &["linear", "in", "out", "in_out", "back", "bounce", "elastic"];

/// 0..1 → 0..1 along an easing curve.
pub fn ease(kind: &str, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match kind {
        "linear" => t,
        "in" => t * t * t,
        "in_out" => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
        "back" => {
            let (c1, c3) = (1.70158, 2.70158);
            1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
        }
        "bounce" => {
            let (n, d) = (7.5625, 2.75);
            if t < 1.0 / d {
                n * t * t
            } else if t < 2.0 / d {
                let t = t - 1.5 / d;
                n * t * t + 0.75
            } else if t < 2.5 / d {
                let t = t - 2.25 / d;
                n * t * t + 0.9375
            } else {
                let t = t - 2.625 / d;
                n * t * t + 0.984375
            }
        }
        "elastic" => {
            if t == 0.0 || t == 1.0 {
                t
            } else {
                2f64.powf(-10.0 * t) * ((t * 10.0 - 0.75) * (2.0 * std::f64::consts::PI / 3.0)).sin() + 1.0
            }
        }
        // "out" and anything unknown.
        _ => 1.0 - (1.0 - t).powi(3),
    }
}

/// A property's value now, if it's something a tween can move.
pub fn read(tree: &SceneTree, id: NodeId, k: &str) -> Option<Val> {
    let n = tree.get(id)?;
    let t = &n.transform;
    let f = |x: f32| Some(Val::Num(x as f64));
    match k {
        "x" => f(t.position.x),
        "y" => f(t.position.y),
        "z" => f(t.position.z),
        "rotation" | "rotation_z" => f(t.rotation.z),
        "rotation_x" => f(t.rotation.x),
        "rotation_y" => f(t.rotation.y),
        "scale_x" => f(t.scale.x),
        "scale_y" => f(t.scale.y),
        "scale_z" => f(t.scale.z),
        _ => match n.props.get(k)? {
            Variant::Num(x) => Some(Val::Num(*x)),
            Variant::Color(c) => Some(Val::Color(*c)),
            _ => None,
        },
    }
}

fn write(tree: &mut SceneTree, id: NodeId, k: &str, v: Val) {
    let Some(n) = tree.get_mut(id) else { return };
    let t = &mut n.transform;
    match v {
        Val::Num(x) => {
            let f = x as f32;
            match k {
                "x" => t.position.x = f,
                "y" => t.position.y = f,
                "z" => t.position.z = f,
                "rotation" | "rotation_z" => t.rotation.z = f,
                "rotation_x" => t.rotation.x = f,
                "rotation_y" => t.rotation.y = f,
                "scale_x" => t.scale.x = f,
                "scale_y" => t.scale.y = f,
                "scale_z" => t.scale.z = f,
                _ => {
                    n.props.insert(k.to_string(), Variant::Num(x));
                }
            }
        }
        Val::Color(c) => {
            n.props.insert(k.to_string(), Variant::Color(c));
        }
    }
}

/// Moves every tween on; gives the ones that finished (node, property).
pub fn update(tree: &mut SceneTree, tweens: &mut Vec<Tween>, dt: f64, frozen: &[NodeId]) -> Vec<(NodeId, String)> {
    let mut done = Vec::new();
    tweens.retain_mut(|tw| {
        if !tree.contains(tw.node) {
            return false;
        }
        if frozen.contains(&tw.node) {
            return true;
        }
        tw.time += dt;
        let k = ease(&tw.ease, tw.time / tw.length.max(1e-6));
        let v = match (tw.from, tw.to) {
            (Val::Num(a), Val::Num(b)) => Val::Num(a + (b - a) * k),
            (Val::Color(a), Val::Color(b)) => Val::Color([0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * k as f32)),
            _ => tw.to,
        };
        write(tree, tw.node, &tw.prop, v);
        if tw.time >= tw.length {
            write(tree, tw.node, &tw.prop, tw.to);
            done.push((tw.node, tw.prop.clone()));
            false
        } else {
            true
        }
    });
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_start_at_0_and_end_at_1() {
        for e in EASES {
            assert!(ease(e, 0.0).abs() < 1e-9, "{}", e);
            assert!((ease(e, 1.0) - 1.0).abs() < 1e-9, "{}", e);
        }
        assert!(ease("back", 0.8) > 1.0, "back overshoots");
        assert!(ease("in", 0.5) < 0.5 && ease("out", 0.5) > 0.5);
    }
}
