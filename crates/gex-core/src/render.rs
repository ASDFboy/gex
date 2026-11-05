//! Turning a tree into a flat list of things to draw. The renderer only ever sees a
//! `RenderList`, so it doesn't care whether the tree is being edited or played (a
//! running game sends one of these per frame from its own thread).

use glam::{Mat4, Vec2, Vec3};

use crate::node::{NodeId, NodeKind};
use crate::tree::SceneTree;

#[derive(Clone, Debug)]
pub struct Sprite {
    pub node: NodeId,
    /// World transform of the node (2D: x, y in pixels, rotation about z).
    pub model: Mat4,
    pub size: Vec2,
    pub centered: bool,
    pub color: [f32; 4],
    pub texture: String,
    pub layer: f32,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub node: NodeId,
    pub pos: Vec2,
    pub text: String,
    pub size: f32,
    pub color: [f32; 4],
    pub layer: f32,
}

#[derive(Clone, Debug)]
pub struct MeshDraw {
    pub node: NodeId,
    pub model: Mat4,
    /// "cube", "sphere", "plane", or a model resource path.
    pub mesh: String,
    pub color: [f32; 4],
    pub texture: String,
}

#[derive(Clone, Debug)]
pub struct LightDraw {
    /// Direction the light shines along (world space).
    pub dir: Vec3,
    pub color: [f32; 3],
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
}

#[derive(Clone, Debug)]
pub struct Rect2D {
    pub node: NodeId,
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
}

impl RenderList {
    pub fn has_3d(&self) -> bool {
        !self.meshes.is_empty() || self.camera3d.is_some()
    }
}

pub fn extract(tree: &SceneTree) -> RenderList {
    let mut out = RenderList { clear: [0.08, 0.08, 0.1, 1.0], ..Default::default() };
    for id in tree.walk() {
        if !tree.visible_in_tree(id) {
            continue;
        }
        let n = &tree.nodes[id];
        let model = tree.global_matrix(id);
        match n.kind {
            NodeKind::Sprite2D => out.sprites.push(Sprite {
                node: id,
                model,
                size: Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32),
                centered: n.prop("centered").is_none_or(|v| v.as_bool()),
                color: n.color("color"),
                texture: n.text("texture").to_string(),
                layer: n.num("layer", 0.0) as f32,
            }),
            NodeKind::Label2D => out.labels.push(Label {
                node: id,
                pos: model.w_axis.truncate().truncate(),
                text: n.text("text").to_string(),
                size: n.num("size", 16.0) as f32,
                color: n.color("color"),
                layer: n.num("layer", 10.0) as f32,
            }),
            NodeKind::Collider2D => out.colliders.push(Rect2D {
                node: id,
                model,
                size: Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32),
            }),
            NodeKind::Camera2D => {
                if n.prop("current").is_none_or(|v| v.as_bool()) && out.camera2d.is_none() {
                    let (_, rot, pos) = model.to_scale_rotation_translation();
                    out.camera2d = Some(Camera2D {
                        center: pos.truncate(),
                        rotation: rot.to_euler(glam::EulerRot::YXZ).2,
                        zoom: n.num("zoom", 1.0).max(0.01) as f32,
                    });
                }
            }
            NodeKind::Mesh3D => out.meshes.push(MeshDraw {
                node: id,
                model,
                mesh: n.text("mesh").to_string(),
                color: n.color("color"),
                texture: n.text("texture").to_string(),
            }),
            NodeKind::Camera3D => {
                if n.prop("current").is_none_or(|v| v.as_bool()) && out.camera3d.is_none() {
                    out.camera3d = Some(Camera3D {
                        world: model,
                        fov_deg: n.num("fov", 70.0) as f32,
                        near: n.num("near", 0.05) as f32,
                        far: n.num("far", 500.0) as f32,
                    });
                }
            }
            NodeKind::Light3D => {
                let c = n.color("color");
                let e = n.num("energy", 1.0) as f32;
                out.lights.push(LightDraw {
                    dir: model.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Y),
                    color: [c[0] * e, c[1] * e, c[2] * e],
                });
            }
            _ => {}
        }
    }
    out.sprites.sort_by(|a, b| a.layer.total_cmp(&b.layer));
    out.labels.sort_by(|a, b| a.layer.total_cmp(&b.layer));
    out
}
