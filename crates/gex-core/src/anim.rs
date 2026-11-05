//! Animation clips (made in the editor's Timeline): property values keyed over time, for nodes
//! around an Animator node. Saved as `.anim` files (RON).

use serde::{Deserialize, Serialize};

use crate::tree::SceneTree;
use crate::{NodeId, Variant};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AnimFile {
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    /// Seconds.
    pub length: f32,
    #[serde(default)]
    pub looped: bool,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    /// The node, as a path from the Animator's parent ("" = the parent itself, "Arm/Hand").
    pub target: String,
    /// A property, or a transform part: x, y, z, rotation (rotation_x...), scale_x..., visible.
    pub prop: String,
    pub keys: Vec<Key>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub t: f32,
    pub value: Variant,
    /// How it moves on to the next key: linear, in, out, in_out, step...
    #[serde(default = "linear")]
    pub ease: String,
}

fn linear() -> String {
    "linear".into()
}

impl AnimFile {
    pub fn parse(text: &str) -> Result<AnimFile, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }
    pub fn to_text(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::new().struct_names(false)).unwrap_or_default()
    }
    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.clips.iter().find(|c| c.name == name)
    }
}

fn ease(kind: &str, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match kind {
        "step" => 0.0,
        "in" => t * t * t,
        "out" => 1.0 - (1.0 - t).powi(3),
        "in_out" => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
        _ => t,
    }
}

impl Track {
    /// The value at a time: numbers and colors blend between keys, everything else steps.
    pub fn sample(&self, t: f32) -> Option<Variant> {
        let first = self.keys.first()?;
        if t <= first.t {
            return Some(first.value.clone());
        }
        for w in self.keys.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t >= a.t && t < b.t {
                let k = ease(&a.ease, (t - a.t) / (b.t - a.t).max(1e-6));
                return Some(match (&a.value, &b.value) {
                    (Variant::Num(x), Variant::Num(y)) => Variant::Num(x + (y - x) * k as f64),
                    (Variant::Color(x), Variant::Color(y)) => Variant::Color([0, 1, 2, 3].map(|i| x[i] + (y[i] - x[i]) * k)),
                    _ => a.value.clone(),
                });
            }
        }
        Some(self.keys.last()?.value.clone())
    }

    pub fn sort(&mut self) {
        self.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
}

/// A node's value for a track property (transform parts included).
pub fn read(tree: &SceneTree, id: NodeId, prop: &str) -> Option<Variant> {
    let n = tree.get(id)?;
    let t = &n.transform;
    let f = |x: f32| Some(Variant::Num(x as f64));
    match prop {
        "x" => f(t.position.x),
        "y" => f(t.position.y),
        "z" => f(t.position.z),
        "rotation" | "rotation_z" => f(t.rotation.z),
        "rotation_x" => f(t.rotation.x),
        "rotation_y" => f(t.rotation.y),
        "scale_x" => f(t.scale.x),
        "scale_y" => f(t.scale.y),
        "scale_z" => f(t.scale.z),
        "visible" => Some(Variant::Bool(n.visible)),
        _ => n.props.get(prop).cloned(),
    }
}

pub fn write(tree: &mut SceneTree, id: NodeId, prop: &str, v: &Variant) {
    let Some(n) = tree.get_mut(id) else { return };
    let x = v.as_f64().unwrap_or(0.0) as f32;
    let t = &mut n.transform;
    match prop {
        "x" => t.position.x = x,
        "y" => t.position.y = x,
        "z" => t.position.z = x,
        "rotation" | "rotation_z" => t.rotation.z = x,
        "rotation_x" => t.rotation.x = x,
        "rotation_y" => t.rotation.y = x,
        "scale_x" => t.scale.x = x,
        "scale_y" => t.scale.y = x,
        "scale_z" => t.scale.z = x,
        "visible" => n.visible = v.as_bool(),
        _ => {
            n.props.insert(prop.to_string(), v.clone());
        }
    }
}

/// The node a track's path names, from an Animator.
pub fn target(tree: &SceneTree, animator: NodeId, path: &str) -> Option<NodeId> {
    let base = tree.get(animator)?.parent.unwrap_or(animator);
    if path.is_empty() || path == "." {
        Some(base)
    } else {
        tree.find_path(Some(base), path)
    }
}

/// Puts every track of a clip at time `t`.
pub fn apply(tree: &mut SceneTree, animator: NodeId, clip: &Clip, t: f32) {
    for tr in &clip.tracks {
        if let (Some(id), Some(v)) = (target(tree, animator, &tr.target), tr.sample(t)) {
            write(tree, id, &tr.prop, &v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_blend_and_step() {
        let tr = Track {
            target: String::new(),
            prop: "x".into(),
            keys: vec![
                Key { t: 0.0, value: Variant::Num(0.0), ease: "linear".into() },
                Key { t: 1.0, value: Variant::Num(10.0), ease: "step".into() },
                Key { t: 2.0, value: Variant::Num(20.0), ease: "linear".into() },
            ],
        };
        assert_eq!(tr.sample(0.5), Some(Variant::Num(5.0)));
        assert_eq!(tr.sample(1.5), Some(Variant::Num(10.0)), "step holds");
        assert_eq!(tr.sample(9.0), Some(Variant::Num(20.0)));
        let f = AnimFile { clips: vec![Clip { name: "walk".into(), length: 2.0, looped: true, tracks: vec![tr] }] };
        let back = AnimFile::parse(&f.to_text()).unwrap();
        assert_eq!(back.clips[0].tracks[0].keys.len(), 3);
    }
}
