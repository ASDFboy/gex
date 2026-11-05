//! Turning a tree into a flat list of things to draw. The renderer only ever sees a
//! `RenderList`, so it doesn't care whether the tree is being edited or played (a
//! running game sends one of these per frame from its own thread).

use glam::{Mat4, Vec2, Vec3};

use crate::node::{NodeId, NodeKind};
use crate::tree::SceneTree;

/// Which part of a sprite's texture to show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Region {
    Whole,
    /// A sprite sheet of `h` × `v` frames, showing one.
    Frame { h: u32, v: u32, frame: u32 },
    /// A tile of a tileset (tile sizes in texture pixels; ids count left to right, top down).
    Tile { w: f32, h: f32, id: u32 },
}

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
    pub region: Region,
    pub flip: [bool; 2],
}

/// A UI node, drawn in game-window pixels (by the editor/player, on top of the game).
#[derive(Clone, Debug)]
pub struct UiItem {
    pub node: NodeId,
    pub kind: NodeKind,
    /// Position from the node's transform, before the anchor is applied.
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
    /// How many times the texture repeats across the mesh.
    pub uv_scale: [f32; 2],
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
    pub ui: Vec<UiItem>,
    /// Lines drawn by scripts this frame (2D world): (from, to, color).
    pub lines_2d: Vec<([f32; 2], [f32; 2], [f32; 4])>,
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
                region: {
                    let (h, v) = (n.num("hframes", 1.0).max(1.0) as u32, n.num("vframes", 1.0).max(1.0) as u32);
                    if h * v > 1 { Region::Frame { h, v, frame: n.num("frame", 0.0).max(0.0) as u32 } } else { Region::Whole }
                },
                flip: [n.prop("flip_h").is_some_and(|v| v.as_bool()), n.prop("flip_v").is_some_and(|v| v.as_bool())],
            }),
            NodeKind::TileMap2D => {
                let grid = crate::tiles::parse(n.text("tiles"));
                let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
                let tex = n.text("tileset").to_string();
                let color = n.color("color");
                let layer = n.num("layer", -5.0) as f32;
                for (r, row) in grid.iter().enumerate() {
                    for (c, tile) in row.iter().enumerate() {
                        if *tile < 0 {
                            continue;
                        }
                        let local = Mat4::from_translation(Vec3::new(c as f32 * tw, r as f32 * th, 0.0));
                        out.sprites.push(Sprite {
                            node: id,
                            model: model * local,
                            size: Vec2::new(tw, th),
                            centered: false,
                            color,
                            texture: tex.clone(),
                            layer,
                            region: Region::Tile { w: tw, h: th, id: *tile as u32 },
                            flip: [false; 2],
                        });
                    }
                }
            }
            k if k.is_ui() => {
                let p = model.w_axis.truncate().truncate();
                out.ui.push(UiItem {
                    node: id,
                    kind: k,
                    pos: p,
                    size: Vec2::new(n.num("width", 100.0) as f32, n.num("height", 30.0) as f32),
                    props: n.props.clone(),
                });
            }
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
            NodeKind::Body2D => {
                let circle = matches!(n.text("shape"), "circle" | "ball");
                let r = n.num("radius", 16.0) as f32 * 2.0;
                out.colliders.push(Rect2D {
                    node: id,
                    model,
                    size: if circle { Vec2::splat(r) } else { Vec2::new(n.num("width", 32.0) as f32, n.num("height", 32.0) as f32) },
                });
            }
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
                uv_scale: [n.num("uv_scale_x", 1.0) as f32, n.num("uv_scale_y", 1.0) as f32],
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
