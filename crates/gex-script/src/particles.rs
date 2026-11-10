//! Particles2D: lots of small sprites that fly out of a node, fall, fade and shrink.
//! Particles live in world space, so moving the emitter leaves a trail.

use std::collections::HashMap;

use gex_core::glam::{Mat4, Quat, Vec2, Vec3};
use gex_core::render::{Region, RenderList, Sprite};
use gex_core::{NodeId, NodeKind, SceneTree};

/// More than this per emitter are dropped (keeps a typo like rate = 100000 from eating memory).
const MAX_PER_EMITTER: usize = 4000;

#[derive(Clone, Debug)]
struct Part {
    pos: Vec2,
    vel: Vec2,
    age: f32,
    life: f32,
    rot: f32,
    spin: f32,
}

#[derive(Debug, Default)]
pub struct Emitter {
    parts: Vec<Part>,
    /// Fractional particles owed from the rate.
    owed: f64,
    pub(crate) seed: u64,
}

impl Emitter {
    pub(crate) fn rand(&mut self) -> f32 {
        // xorshift64*: cheap and good enough for sparks.
        if self.seed == 0 {
            self.seed = 0x9E37_79B9_7F4A_7C15;
        }
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        ((self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    pub fn count(&self) -> usize {
        self.parts.len()
    }

    /// Adds `n` particles from the emitter's current place and settings.
    pub fn spawn(&mut self, tree: &SceneTree, id: NodeId, n: usize) {
        let node = &tree.nodes[id];
        let m = tree.global_matrix(id);
        let (_, rot, pos) = m.to_scale_rotation_translation();
        let base = rot.to_euler(gex_core::glam::EulerRot::YXZ).2.to_degrees();
        let dir = node.num("direction", -90.0) as f32 + base;
        let spread = node.num("spread", 30.0) as f32;
        let speed = node.num("speed", 120.0) as f32;
        let speed_rand = node.num("speed_random", 0.3).clamp(0.0, 1.0) as f32;
        let life = node.num("lifetime", 1.0).max(0.01) as f32;
        let area = node.num("area", 0.0) as f32;
        let spin = node.num("spin", 0.0) as f32;
        let n = n.min(MAX_PER_EMITTER.saturating_sub(self.parts.len()));
        for _ in 0..n {
            let a = (dir + (self.rand() * 2.0 - 1.0) * spread).to_radians();
            let s = speed * (1.0 - speed_rand * self.rand());
            let off = if area > 0.0 {
                let t = self.rand() * std::f32::consts::TAU;
                Vec2::new(t.cos(), t.sin()) * area * self.rand().sqrt()
            } else {
                Vec2::ZERO
            };
            let p = Part {
                pos: Vec2::new(pos.x, pos.y) + off,
                vel: Vec2::new(a.cos(), a.sin()) * s,
                age: 0.0,
                life: life * (0.8 + 0.4 * self.rand()),
                rot: self.rand() * 360.0,
                spin: spin * (self.rand() * 2.0 - 1.0),
            };
            self.parts.push(p);
        }
    }
}

/// Moves every particle on, spawns new ones from emitting nodes, and forgets emitters whose
/// node is gone.
pub fn update(tree: &SceneTree, emitters: &mut HashMap<NodeId, Emitter>, dt: f32, frozen: &[NodeId]) {
    emitters.retain(|id, e| tree.contains(*id) && (!e.parts.is_empty() || tree.nodes[*id].kind == NodeKind::Particles2D));
    for id in tree.walk() {
        let n = &tree.nodes[id];
        if n.kind != NodeKind::Particles2D || frozen.contains(&id) {
            continue;
        }
        let e = emitters.entry(id).or_insert_with(|| Emitter { seed: (id_bits(id) | 1).wrapping_mul(0x9E37_79B9), ..Default::default() });
        let gravity = n.num("gravity", 0.0) as f32;
        for p in &mut e.parts {
            p.age += dt;
            p.vel.y += gravity * dt;
            p.pos += p.vel * dt;
            p.rot += p.spin * dt;
        }
        e.parts.retain(|p| p.age < p.life);
        let on = n.prop("emitting").is_none_or(|v| v.as_bool()) && tree.visible_in_tree(id);
        if on {
            e.owed += n.num("rate", 30.0).max(0.0) * dt as f64;
            let k = e.owed.floor();
            e.owed -= k;
            e.spawn(tree, id, k as usize);
        } else {
            e.owed = 0.0;
        }
    }
}

fn id_bits(id: NodeId) -> u64 {
    use slotmap::Key;
    id.data().as_ffi()
}

/// Adds the particles to what's drawn.
pub fn draw(tree: &SceneTree, emitters: &HashMap<NodeId, Emitter>, list: &mut RenderList) {
    for (id, e) in emitters {
        let Some(n) = tree.get(*id) else { continue };
        let tex = match n.text("texture") {
            "" => "@circle".to_string(),
            t => t.to_string(),
        };
        let (s0, s1) = (n.num("size", 8.0) as f32, n.num("size_end", 2.0) as f32);
        let (c0, c1) = (n.color("color"), n.color("color_end"));
        let layer = n.num("layer", 1.0) as f32;
        let tint = gex_core::render::tinted(tree, *id, [1.0; 4]);
        let blend = gex_core::render::Blend::from_name(n.text("blend"));
        let pixel = gex_core::render::filter_of(n);
        let lit = n.prop("lit").is_none_or(|v| v.as_bool());
        for p in &e.parts {
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let size = s0 + (s1 - s0) * t;
            let color = [0, 1, 2, 3].map(|i| (c0[i] + (c1[i] - c0[i]) * t) * tint[i]);
            list.sprites.push(Sprite {
                node: *id,
                model: Mat4::from_rotation_translation(Quat::from_rotation_z(p.rot.to_radians()), Vec3::new(p.pos.x, p.pos.y, 0.0)),
                size: Vec2::splat(size),
                centered: true,
                color,
                texture: tex.clone(),
                layer,
                region: Region::Whole,
                flip: [false, false],
                parallax: Vec2::ONE,
                repeat: [false; 2],
                blend,
                pixel,
                lit,
            });
        }
    }
}

// ---------------------------------------------------------------- 3D

#[derive(Clone, Debug)]
struct Part3 {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    life: f32,
}

#[derive(Debug, Default)]
pub struct Emitter3 {
    parts: Vec<Part3>,
    owed: f64,
    rng: Emitter,
}

impl Emitter3 {
    pub fn count(&self) -> usize {
        self.parts.len()
    }

    pub fn spawn(&mut self, tree: &SceneTree, id: NodeId, n: usize) {
        let node = &tree.nodes[id];
        let m = tree.global_matrix(id);
        let pos = m.w_axis.truncate();
        let dir = m
            .transform_vector3(Vec3::new(node.num("direction_x", 0.0) as f32, node.num("direction_y", 1.0) as f32, node.num("direction_z", 0.0) as f32))
            .normalize_or(Vec3::Y);
        let spread = (node.num("spread", 25.0) as f32).to_radians();
        let speed = node.num("speed", 2.0) as f32;
        let speed_rand = node.num("speed_random", 0.3).clamp(0.0, 1.0) as f32;
        let life = node.num("lifetime", 1.5).max(0.01) as f32;
        let area = node.num("area", 0.0) as f32;
        // Two directions across `dir`, to tilt by the spread.
        let side = if dir.y.abs() < 0.9 { dir.cross(Vec3::Y).normalize() } else { dir.cross(Vec3::X).normalize() };
        let up = dir.cross(side);
        let n = n.min(MAX_PER_EMITTER.saturating_sub(self.parts.len()));
        for _ in 0..n {
            let r = &mut self.rng;
            let tilt = spread * r.rand().sqrt();
            let around = r.rand() * std::f32::consts::TAU;
            let d = (dir * tilt.cos() + (side * around.cos() + up * around.sin()) * tilt.sin()).normalize();
            let s = speed * (1.0 - speed_rand * r.rand());
            let off = if area > 0.0 { (Vec3::new(r.rand(), r.rand(), r.rand()) * 2.0 - Vec3::ONE) * area } else { Vec3::ZERO };
            let l = life * (0.8 + 0.4 * r.rand());
            self.parts.push(Part3 { pos: pos + off, vel: d * s, age: 0.0, life: l });
        }
    }
}

pub fn update_3d(tree: &SceneTree, emitters: &mut HashMap<NodeId, Emitter3>, dt: f32, frozen: &[NodeId]) {
    emitters.retain(|id, e| tree.contains(*id) && (!e.parts.is_empty() || tree.nodes[*id].kind == NodeKind::Particles3D));
    for id in tree.walk() {
        let n = &tree.nodes[id];
        if n.kind != NodeKind::Particles3D || frozen.contains(&id) {
            continue;
        }
        let e = emitters.entry(id).or_insert_with(|| Emitter3 { rng: Emitter { seed: (id_bits(id) | 1).wrapping_mul(0x9E37_79B9), ..Default::default() }, ..Default::default() });
        let g = n.num("gravity", -2.0) as f32;
        for p in &mut e.parts {
            p.age += dt;
            p.vel.y += g * dt;
            p.pos += p.vel * dt;
        }
        e.parts.retain(|p| p.age < p.life);
        if n.prop("emitting").is_none_or(|v| v.as_bool()) && tree.visible_in_tree(id) {
            e.owed += n.num("rate", 30.0).max(0.0) * dt as f64;
            let k = e.owed.floor();
            e.owed -= k;
            e.spawn(tree, id, k as usize);
        } else {
            e.owed = 0.0;
        }
    }
}

/// 3D particles as camera-facing quads.
pub fn draw_3d(tree: &SceneTree, emitters: &HashMap<NodeId, Emitter3>, list: &mut RenderList) {
    for (id, e) in emitters {
        let Some(n) = tree.get(*id) else { continue };
        let tex = match n.text("texture") {
            "" => "@glow".to_string(),
            t => t.to_string(),
        };
        let (s0, s1) = (n.num("size", 0.3) as f32, n.num("size_end", 0.05) as f32);
        let (c0, c1) = (n.color("color"), n.color("color_end"));
        let glow = n.prop("glow").is_none_or(|v| v.as_bool());
        let tint = gex_core::render::tinted(tree, *id, [1.0; 4]);
        let pixel = gex_core::render::filter_of(n);
        let fog = n.prop("fog").is_none_or(|v| v.as_bool());
        for p in &e.parts {
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let size = s0 + (s1 - s0) * t;
            let mut color = [0, 1, 2, 3].map(|i| (c0[i] + (c1[i] - c0[i]) * t) * tint[i]);
            // Glowing particles never quite reach full alpha, so they draw blended.
            color[3] = color[3].min(0.998);
            list.meshes.push(gex_core::render::MeshDraw {
                color,
                texture: tex.clone(),
                cast_shadows: false,
                unlit: true,
                billboard: true,
                emissive: if glow { 1.0 } else { 0.0 },
                pixel,
                fog,
                ..gex_core::render::MeshDraw::new(*id, Mat4::from_scale_rotation_translation(Vec3::splat(size), Quat::IDENTITY, p.pos), "quad")
            });
        }
    }
}
