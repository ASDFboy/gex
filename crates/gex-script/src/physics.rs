//! Physics for Body2D / Body3D nodes, with rapier. The node tree stays the source of
//! truth: before each step, bodies are made, removed or teleported to match their nodes;
//! after it, dynamic bodies write their position and velocity back.
//!
//! 2D works in pixels (y down) in the tree and in metres inside rapier (50 px = 1 m).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use gex_core::{NodeId, NodeKind, SceneTree, Variant};

pub const PIXELS_PER_METRE: f32 = 50.0;

/// Collision layers by name. A node's `layer` names what it is ("player", "enemies"; empty
/// means "default"), and its `mask` what it bumps into (empty means everything). Names get a
/// bit each the first time they're seen (up to 32).
#[derive(Debug)]
pub struct Layers {
    names: std::cell::RefCell<Vec<String>>,
}

impl Default for Layers {
    fn default() -> Self {
        Layers { names: std::cell::RefCell::new(vec!["default".to_string()]) }
    }
}

impl Layers {
    fn bit(&self, name: &str) -> u32 {
        let mut names = self.names.borrow_mut();
        let i = match names.iter().position(|n| n == name) {
            Some(i) => i,
            None if names.len() < 32 => {
                names.push(name.to_string());
                names.len() - 1
            }
            None => 31,
        };
        1 << i
    }

    /// The bits for a list like "player, enemies". Empty: `empty`. "all" / "none" work too.
    pub fn bits(&self, spec: &str, empty: u32) -> u32 {
        let mut out = 0;
        let mut any = false;
        for part in spec.split([',', ' ', ';']).map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
            any = true;
            match part.as_str() {
                "all" | "everything" => out |= u32::MAX,
                "none" | "nothing" => {}
                p => out |= self.bit(p),
            }
        }
        if any {
            out
        } else {
            empty
        }
    }

    /// A node's (what it is, what it collides with).
    pub fn of(&self, tree: &SceneTree, id: NodeId) -> (u32, u32) {
        let n = &tree.nodes[id];
        (self.bits(n.text("layer"), 1), self.bits(n.text("mask"), u32::MAX))
    }

    /// Whether two (layer, mask) pairs touch: each has to be in the other's mask.
    pub fn meet(a: (u32, u32), b: (u32, u32)) -> bool {
        a.0 & b.1 != 0 && b.0 & a.1 != 0
    }
}

fn groups2(g: (u32, u32)) -> rapier2d::prelude::InteractionGroups {
    use rapier2d::prelude::*;
    InteractionGroups::new(Group::from_bits_retain(g.0), Group::from_bits_retain(g.1), InteractionTestMode::And)
}
fn groups3(g: (u32, u32)) -> rapier3d::prelude::InteractionGroups {
    use rapier3d::prelude::*;
    InteractionGroups::new(Group::from_bits_retain(g.0), Group::from_bits_retain(g.1), InteractionTestMode::And)
}

/// A static shape that isn't a node of its own (tilemap tiles): world-space 2D box.
#[derive(Clone, Copy, Debug)]
pub struct StaticBox2 {
    pub owner: NodeId,
    pub center: Vec2,
    pub half: Vec2,
}

/// A static triangle mesh in world space (Mesh3D nodes with collision on): owner, vertices, triangles.
pub type StaticMesh = (NodeId, Arc<Vec<Vec3>>, Arc<Vec<[u32; 3]>>);

pub struct RayHit {
    pub node: NodeId,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

fn num(tree: &SceneTree, id: NodeId, k: &str, d: f64) -> f32 {
    tree.nodes[id].num(k, d) as f32
}

/// Everything about a body that, when changed, means building it again.
fn shape_key(tree: &SceneTree, id: NodeId, scale: Vec3) -> String {
    let n = &tree.nodes[id];
    let mut s = format!("{:?}", scale);
    for k in ["body", "shape", "width", "height", "depth", "radius", "gravity_scale", "friction", "bounce", "density", "fixed_rotation", "layer", "mask"] {
        s.push_str(&format!("|{:?}", n.props.get(k)));
    }
    s
}

fn parent_matrix(tree: &SceneTree, id: NodeId) -> Mat4 {
    tree.nodes[id].parent.map(|p| tree.global_matrix(p)).unwrap_or(Mat4::IDENTITY)
}

// ---------------------------------------------------------------- 2D

mod d2 {
    pub use rapier2d::control::{CharacterLength, KinematicCharacterController};
    pub use rapier2d::prelude::*;
}

struct Entry2 {
    body: d2::RigidBodyHandle,
    collider: d2::ColliderHandle,
    key: String,
    pos: Vec2,
    angle: f32,
    vel: Vec2,
}

pub struct World2 {
    world: d2::PhysicsWorld,
    bodies: HashMap<NodeId, Entry2>,
    owners: HashMap<d2::ColliderHandle, NodeId>,
    statics: Vec<d2::ColliderHandle>,
    statics_key: String,
    controller: d2::KinematicCharacterController,
}

fn pose2(tree: &SceneTree, id: NodeId) -> (Vec2, f32, Vec3) {
    let m = tree.global_matrix(id);
    let (scale, rot, pos) = m.to_scale_rotation_translation();
    (pos.truncate(), rot.to_euler(EulerRot::YXZ).2, scale.abs())
}

impl World2 {
    fn new(gravity: [f32; 2]) -> World2 {
        let mut world = d2::PhysicsWorld::new();
        world.gravity = d2::Vector::new(gravity[0], gravity[1]) / PIXELS_PER_METRE;
        world.integration_parameters.dt = 1.0 / 60.0;
        let mut controller = d2::KinematicCharacterController::default();
        controller.up = d2::Vector::new(0.0, -1.0);
        controller.offset = d2::CharacterLength::Absolute(0.01);
        controller.snap_to_ground = Some(d2::CharacterLength::Absolute(0.1));
        World2 { world, bodies: HashMap::new(), owners: HashMap::new(), statics: Vec::new(), statics_key: String::new(), controller }
    }

    fn build(&mut self, tree: &SceneTree, id: NodeId, key: String, layers: &Layers) {
        let n = &tree.nodes[id];
        let (pos, angle, scale) = pose2(tree, id);
        let p = pos / PIXELS_PER_METRE;
        let builder = match n.text("body") {
            "static" => d2::RigidBodyBuilder::fixed(),
            "kinematic" => d2::RigidBodyBuilder::kinematic_position_based(),
            _ => d2::RigidBodyBuilder::dynamic(),
        };
        let vel = Vec2::new(num(tree, id, "vx", 0.0), num(tree, id, "vy", 0.0));
        let mut builder = builder
            .translation(d2::Vector::new(p.x, p.y))
            .rotation(angle)
            .linvel(d2::Vector::new(vel.x, vel.y) / PIXELS_PER_METRE)
            .gravity_scale(num(tree, id, "gravity_scale", 1.0))
            .ccd_enabled(true);
        if n.prop("fixed_rotation").is_some_and(|v| v.as_bool()) {
            builder = builder.lock_rotations();
        }
        let (w, h) = (num(tree, id, "width", 32.0) * scale.x / PIXELS_PER_METRE, num(tree, id, "height", 32.0) * scale.y / PIXELS_PER_METRE);
        let r = num(tree, id, "radius", 16.0) * scale.x.max(scale.y) / PIXELS_PER_METRE;
        let col = match n.text("shape") {
            "circle" | "ball" => d2::ColliderBuilder::ball(r),
            "capsule" => d2::ColliderBuilder::capsule_y((h / 2.0 - r).max(0.001), r),
            _ => d2::ColliderBuilder::cuboid(w / 2.0, h / 2.0),
        }
        .collision_groups(groups2(layers.of(tree, id)))
        .friction(num(tree, id, "friction", 0.5))
        .restitution(num(tree, id, "bounce", 0.0))
        .density(num(tree, id, "density", 1.0).max(0.001));
        let (body, collider) = self.world.insert(builder, col);
        self.owners.insert(collider, id);
        self.bodies.insert(id, Entry2 { body, collider, key, pos, angle, vel });
    }

    fn remove(&mut self, id: NodeId) {
        if let Some(e) = self.bodies.remove(&id) {
            self.owners.remove(&e.collider);
            self.world.remove_body(e.body);
        }
    }

    fn set_statics(&mut self, boxes: &[StaticBox2]) {
        let key = format!("{:?}", boxes);
        if key == self.statics_key {
            return;
        }
        self.statics_key = key;
        for c in self.statics.drain(..) {
            self.owners.remove(&c);
            self.world.remove_collider(c);
        }
        for b in boxes {
            let c = d2::ColliderBuilder::cuboid(b.half.x / PIXELS_PER_METRE, b.half.y / PIXELS_PER_METRE)
                .translation(d2::Vector::new(b.center.x, b.center.y) / PIXELS_PER_METRE)
                .collision_groups(groups2((1, u32::MAX)))
                .friction(0.5);
            let h = self.world.insert_collider(c, None);
            self.owners.insert(h, b.owner);
            self.statics.push(h);
        }
    }

    /// Makes the physics match the tree.
    fn sync(&mut self, tree: &SceneTree, ids: &[NodeId], layers: &Layers) {
        let alive: HashSet<NodeId> = ids.iter().copied().collect();
        let gone: Vec<NodeId> = self.bodies.keys().filter(|k| !alive.contains(k)).copied().collect();
        for g in gone {
            self.remove(g);
        }
        for &id in ids {
            let (pos, angle, scale) = pose2(tree, id);
            let key = shape_key(tree, id, scale);
            if self.bodies.get(&id).is_some_and(|e| e.key != key) {
                self.remove(id);
            }
            let Some(e) = self.bodies.get_mut(&id) else {
                self.build(tree, id, key, layers);
                continue;
            };
            let b = &mut self.world.bodies[e.body];
            if pos.distance(e.pos) > 0.01 || (angle - e.angle).abs() > 1e-4 {
                // Moved by a script or the editor: teleport.
                let p = d2::Pose::new(d2::Vector::new(pos.x, pos.y) / PIXELS_PER_METRE, angle);
                if b.is_kinematic() {
                    b.set_next_kinematic_position(p);
                } else {
                    b.set_position(p, true);
                }
                e.pos = pos;
                e.angle = angle;
            }
            let vel = Vec2::new(num(tree, id, "vx", 0.0), num(tree, id, "vy", 0.0));
            if vel.distance(e.vel) > 1e-3 && b.is_dynamic() {
                b.set_linvel(d2::Vector::new(vel.x, vel.y) / PIXELS_PER_METRE, true);
                e.vel = vel;
            }
        }
    }

    fn step(&mut self, tree: &mut SceneTree) {
        self.world.step();
        let floor: HashSet<NodeId> = self.floor_contacts();
        for (id, e) in self.bodies.iter_mut() {
            let b = &self.world.bodies[e.body];
            if !b.is_dynamic() {
                continue;
            }
            let t = b.translation() * PIXELS_PER_METRE;
            let pos = Vec2::new(t.x, t.y);
            let angle = b.rotation().angle();
            let v = b.linvel() * PIXELS_PER_METRE;
            let pm = parent_matrix(tree, *id);
            let local = pm.inverse().transform_point3(pos.extend(0.0));
            let parent_angle = pm.to_scale_rotation_translation().1.to_euler(EulerRot::YXZ).2;
            let n = &mut tree.nodes[*id];
            n.transform.position.x = local.x;
            n.transform.position.y = local.y;
            n.transform.rotation.z = (angle - parent_angle).to_degrees();
            n.props.insert("vx".into(), Variant::Num(v.x as f64));
            n.props.insert("vy".into(), Variant::Num(v.y as f64));
            n.props.insert("on_floor".into(), Variant::Bool(floor.contains(id)));
            e.pos = pos;
            e.angle = angle;
            e.vel = Vec2::new(v.x, v.y);
        }
    }

    /// Bodies resting on something (a contact whose normal points against gravity).
    fn floor_contacts(&self) -> HashSet<NodeId> {
        let g = Vec2::new(self.world.gravity.x, self.world.gravity.y).normalize_or(Vec2::Y);
        let mut out = HashSet::new();
        for pair in self.world.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            for m in pair.manifolds() {
                if m.points.is_empty() {
                    continue;
                }
                let n = Vec2::new(m.data.normal.x, m.data.normal.y);
                // The normal points from collider1 to collider2.
                if n.dot(g) > 0.6 {
                    if let Some(id) = self.owners.get(&pair.collider1) {
                        out.insert(*id);
                    }
                } else if n.dot(g) < -0.6 {
                    if let Some(id) = self.owners.get(&pair.collider2) {
                        out.insert(*id);
                    }
                }
            }
        }
        out
    }

    fn touching(&self, out: &mut Vec<(NodeId, NodeId)>) {
        for pair in self.world.contact_pairs() {
            if pair.has_any_active_contact() {
                if let (Some(a), Some(b)) = (self.owners.get(&pair.collider1), self.owners.get(&pair.collider2)) {
                    if a != b {
                        out.push((*a, *b));
                    }
                }
            }
        }
    }

    /// Moves a kinematic body, sliding along what it hits. Gives the new world position
    /// and whether it ended up on the floor.
    fn move_body(&mut self, id: NodeId, delta: Vec2) -> Option<(Vec2, bool)> {
        let e = self.bodies.get_mut(&id)?;
        let col = self.world.colliders.get(e.collider)?;
        let filter = d2::QueryFilter::default().exclude_rigid_body(e.body).exclude_sensors().groups(col.collision_groups());
        let qp = self.world.query_pipeline_with_filter(filter);
        let pose = *self.world.bodies[e.body].position();
        let d = d2::Vector::new(delta.x, delta.y) / PIXELS_PER_METRE;
        let m = self.controller.move_shape(1.0 / 60.0, &qp, col.shape(), &pose, d, |_| {});
        let new_t = pose.translation + m.translation;
        let new_pose = d2::Pose::new(new_t, pose.rotation.angle());
        let b = &mut self.world.bodies[e.body];
        if b.is_kinematic() {
            b.set_next_kinematic_position(new_pose);
        } else {
            b.set_position(new_pose, true);
        }
        let p = Vec2::new(new_t.x, new_t.y) * PIXELS_PER_METRE;
        e.pos = p;
        Some((p, m.grounded))
    }

    fn impulse(&mut self, id: NodeId, v: Vec2) {
        if let Some(e) = self.bodies.get(&id) {
            let b = &mut self.world.bodies[e.body];
            let m = b.mass();
            b.apply_impulse(d2::Vector::new(v.x, v.y) / PIXELS_PER_METRE * m, true);
        }
    }

    fn raycast(&self, from: Vec2, dir: Vec2, max: f32, ignore: Option<NodeId>, mask: u32) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec2::ZERO {
            return None;
        }
        let ray = d2::Ray::new(d2::Vector::new(from.x, from.y) / PIXELS_PER_METRE, d2::Vector::new(dir.x, dir.y));
        let mut filter = d2::QueryFilter::default().exclude_sensors().groups(groups2((u32::MAX, mask)));
        if let Some(e) = ignore.and_then(|i| self.bodies.get(&i)) {
            filter = filter.exclude_rigid_body(e.body);
        }
        let (h, hit) = self.world.cast_ray_and_get_normal(&ray, max / PIXELS_PER_METRE, true, filter)?;
        let p = ray.point_at(hit.time_of_impact) * PIXELS_PER_METRE;
        Some(RayHit {
            node: *self.owners.get(&h)?,
            point: Vec3::new(p.x, p.y, 0.0),
            normal: Vec3::new(hit.normal.x, hit.normal.y, 0.0),
            distance: hit.time_of_impact * PIXELS_PER_METRE,
        })
    }
}

// ---------------------------------------------------------------- 3D

mod d3 {
    pub use rapier3d::control::{CharacterLength, KinematicCharacterController};
    pub use rapier3d::prelude::*;
}

struct Entry3 {
    body: d3::RigidBodyHandle,
    collider: d3::ColliderHandle,
    key: String,
    pos: Vec3,
    rot: Quat,
    vel: Vec3,
}

pub struct World3 {
    world: d3::PhysicsWorld,
    bodies: HashMap<NodeId, Entry3>,
    owners: HashMap<d3::ColliderHandle, NodeId>,
    statics: Vec<d3::ColliderHandle>,
    statics_key: String,
    controller: d3::KinematicCharacterController,
}

fn pose3(tree: &SceneTree, id: NodeId) -> (Vec3, Quat, Vec3) {
    let (s, r, t) = tree.global_matrix(id).to_scale_rotation_translation();
    (t, r, s.abs())
}

/// A static mesh-less box for 3D (map brushes and the like).
#[derive(Clone, Copy, Debug)]
pub struct StaticBox3 {
    pub owner: NodeId,
    pub center: Vec3,
    pub half: Vec3,
}

impl World3 {
    fn new(gravity: [f32; 3]) -> World3 {
        let mut world = d3::PhysicsWorld::new();
        world.gravity = d3::Vector::new(gravity[0], gravity[1], gravity[2]);
        world.integration_parameters.dt = 1.0 / 60.0;
        let mut controller = d3::KinematicCharacterController::default();
        controller.offset = d3::CharacterLength::Absolute(0.01);
        controller.snap_to_ground = Some(d3::CharacterLength::Absolute(0.1));
        World3 { world, bodies: HashMap::new(), owners: HashMap::new(), statics: Vec::new(), statics_key: String::new(), controller }
    }

    fn build(&mut self, tree: &SceneTree, id: NodeId, key: String, layers: &Layers) {
        let n = &tree.nodes[id];
        let (pos, rot, scale) = pose3(tree, id);
        let builder = match n.text("body") {
            "static" => d3::RigidBodyBuilder::fixed(),
            "kinematic" => d3::RigidBodyBuilder::kinematic_position_based(),
            _ => d3::RigidBodyBuilder::dynamic(),
        };
        let vel = Vec3::new(num(tree, id, "vx", 0.0), num(tree, id, "vy", 0.0), num(tree, id, "vz", 0.0));
        let mut builder = builder
            .pose(d3::Pose::from_parts(pos, rot))
            .linvel(vel)
            .gravity_scale(num(tree, id, "gravity_scale", 1.0))
            .ccd_enabled(true);
        if n.prop("fixed_rotation").is_some_and(|v| v.as_bool()) {
            builder = builder.lock_rotations();
        }
        let half = Vec3::new(num(tree, id, "width", 1.0), num(tree, id, "height", 1.0), num(tree, id, "depth", 1.0)) * scale / 2.0;
        let r = num(tree, id, "radius", 0.5) * scale.max_element();
        let col = match n.text("shape") {
            "sphere" | "ball" => d3::ColliderBuilder::ball(r),
            "capsule" => d3::ColliderBuilder::capsule_y((half.y - r).max(0.001), r),
            _ => d3::ColliderBuilder::cuboid(half.x, half.y, half.z),
        }
        .collision_groups(groups3(layers.of(tree, id)))
        .friction(num(tree, id, "friction", 0.5))
        .restitution(num(tree, id, "bounce", 0.0))
        .density(num(tree, id, "density", 1.0).max(0.001));
        let (body, collider) = self.world.insert(builder, col);
        self.owners.insert(collider, id);
        self.bodies.insert(id, Entry3 { body, collider, key, pos, rot, vel });
    }

    fn remove(&mut self, id: NodeId) {
        if let Some(e) = self.bodies.remove(&id) {
            self.owners.remove(&e.collider);
            self.world.remove_body(e.body);
        }
    }

    fn set_statics(&mut self, boxes: &[StaticBox3], meshes: &[StaticMesh]) {
        let key = format!("{:?}{:?}", boxes, meshes.iter().map(|m| (m.0, Arc::as_ptr(&m.1))).collect::<Vec<_>>());
        if key == self.statics_key {
            return;
        }
        self.statics_key = key;
        for c in self.statics.drain(..) {
            self.owners.remove(&c);
            self.world.remove_collider(c);
        }
        for b in boxes {
            let c = d3::ColliderBuilder::cuboid(b.half.x, b.half.y, b.half.z).translation(b.center).collision_groups(groups3((1, u32::MAX)));
            let h = self.world.insert_collider(c, None);
            self.owners.insert(h, b.owner);
            self.statics.push(h);
        }
        for (owner, v, i) in meshes {
            if let Ok(c) = d3::ColliderBuilder::trimesh(v.to_vec(), i.to_vec()) {
                let h = self.world.insert_collider(c.collision_groups(groups3((1, u32::MAX))), None);
                self.owners.insert(h, *owner);
                self.statics.push(h);
            }
        }
    }

    fn sync(&mut self, tree: &SceneTree, ids: &[NodeId], layers: &Layers) {
        let alive: HashSet<NodeId> = ids.iter().copied().collect();
        let gone: Vec<NodeId> = self.bodies.keys().filter(|k| !alive.contains(k)).copied().collect();
        for g in gone {
            self.remove(g);
        }
        for &id in ids {
            let (pos, rot, scale) = pose3(tree, id);
            let key = shape_key(tree, id, scale);
            if self.bodies.get(&id).is_some_and(|e| e.key != key) {
                self.remove(id);
            }
            let Some(e) = self.bodies.get_mut(&id) else {
                self.build(tree, id, key, layers);
                continue;
            };
            let b = &mut self.world.bodies[e.body];
            if pos.distance(e.pos) > 1e-4 || rot.angle_between(e.rot) > 1e-4 {
                let p = d3::Pose::from_parts(pos, rot);
                if b.is_kinematic() {
                    b.set_next_kinematic_position(p);
                } else {
                    b.set_position(p, true);
                }
                e.pos = pos;
                e.rot = rot;
            }
            let vel = Vec3::new(num(tree, id, "vx", 0.0), num(tree, id, "vy", 0.0), num(tree, id, "vz", 0.0));
            if vel.distance(e.vel) > 1e-4 && b.is_dynamic() {
                b.set_linvel(vel, true);
                e.vel = vel;
            }
        }
    }

    fn step(&mut self, tree: &mut SceneTree) {
        self.world.step();
        let floor = self.floor_contacts();
        for (id, e) in self.bodies.iter_mut() {
            let b = &self.world.bodies[e.body];
            if !b.is_dynamic() {
                continue;
            }
            let pos = b.translation();
            let rot = *b.rotation();
            let v = b.linvel();
            let pm = parent_matrix(tree, *id);
            let scale = tree.nodes[*id].transform.scale;
            let local = pm.inverse() * Mat4::from_rotation_translation(rot, pos);
            let (_, lr, lt) = local.to_scale_rotation_translation();
            let (y, x, z) = lr.to_euler(EulerRot::YXZ);
            let n = &mut tree.nodes[*id];
            n.transform.position = lt;
            n.transform.rotation = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
            n.transform.scale = scale;
            n.props.insert("vx".into(), Variant::Num(v.x as f64));
            n.props.insert("vy".into(), Variant::Num(v.y as f64));
            n.props.insert("vz".into(), Variant::Num(v.z as f64));
            n.props.insert("on_floor".into(), Variant::Bool(floor.contains(id)));
            e.pos = pos;
            e.rot = rot;
            e.vel = v;
        }
    }

    fn floor_contacts(&self) -> HashSet<NodeId> {
        let g = self.world.gravity.normalize_or(Vec3::NEG_Y);
        let mut out = HashSet::new();
        for pair in self.world.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            for m in pair.manifolds() {
                if m.points.is_empty() {
                    continue;
                }
                let n = m.data.normal;
                if n.dot(g) > 0.6 {
                    if let Some(id) = self.owners.get(&pair.collider1) {
                        out.insert(*id);
                    }
                } else if n.dot(g) < -0.6 {
                    if let Some(id) = self.owners.get(&pair.collider2) {
                        out.insert(*id);
                    }
                }
            }
        }
        out
    }

    fn touching(&self, out: &mut Vec<(NodeId, NodeId)>) {
        for pair in self.world.contact_pairs() {
            if pair.has_any_active_contact() {
                if let (Some(a), Some(b)) = (self.owners.get(&pair.collider1), self.owners.get(&pair.collider2)) {
                    if a != b {
                        out.push((*a, *b));
                    }
                }
            }
        }
    }

    fn move_body(&mut self, id: NodeId, delta: Vec3) -> Option<(Vec3, bool)> {
        let e = self.bodies.get_mut(&id)?;
        let col = self.world.colliders.get(e.collider)?;
        let filter = d3::QueryFilter::default().exclude_rigid_body(e.body).exclude_sensors().groups(col.collision_groups());
        let qp = self.world.query_pipeline_with_filter(filter);
        let pose = *self.world.bodies[e.body].position();
        let m = self.controller.move_shape(1.0 / 60.0, &qp, col.shape(), &pose, delta, |_| {});
        let new_t = pose.translation + m.translation;
        let new_pose = d3::Pose::from_parts(new_t, pose.rotation);
        let b = &mut self.world.bodies[e.body];
        if b.is_kinematic() {
            b.set_next_kinematic_position(new_pose);
        } else {
            b.set_position(new_pose, true);
        }
        e.pos = new_t;
        Some((new_t, m.grounded))
    }

    fn impulse(&mut self, id: NodeId, v: Vec3) {
        if let Some(e) = self.bodies.get(&id) {
            let b = &mut self.world.bodies[e.body];
            let m = b.mass();
            b.apply_impulse(v * m, true);
        }
    }

    fn raycast(&self, from: Vec3, dir: Vec3, max: f32, ignore: Option<NodeId>, mask: u32) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        let ray = d3::Ray::new(from, dir);
        let mut filter = d3::QueryFilter::default().exclude_sensors().groups(groups3((u32::MAX, mask)));
        if let Some(e) = ignore.and_then(|i| self.bodies.get(&i)) {
            filter = filter.exclude_rigid_body(e.body);
        }
        let (h, hit) = self.world.cast_ray_and_get_normal(&ray, max, true, filter)?;
        Some(RayHit { node: *self.owners.get(&h)?, point: ray.point_at(hit.time_of_impact), normal: hit.normal, distance: hit.time_of_impact })
    }
}

// ---------------------------------------------------------------- both

#[derive(Default)]
pub struct Physics {
    w2: Option<World2>,
    w3: Option<World3>,
    pub gravity_2d: [f32; 2],
    pub gravity_3d: [f32; 3],
    /// Extra static shapes from tilemaps and maps (set before each step).
    pub statics_2d: Vec<StaticBox2>,
    pub statics_3d: Vec<StaticBox3>,
    pub meshes_3d: Vec<StaticMesh>,
    pub layers: Layers,
}

impl Physics {
    pub fn new(gravity_2d: [f32; 2], gravity_3d: [f32; 3]) -> Physics {
        Physics { gravity_2d, gravity_3d, ..Default::default() }
    }

    /// Matches bodies to the tree (making them the first time they're seen).
    pub fn sync(&mut self, tree: &SceneTree) {
        let mut b2 = Vec::new();
        let mut b3 = Vec::new();
        for id in tree.walk() {
            match tree.nodes[id].kind {
                NodeKind::Body2D => b2.push(id),
                NodeKind::Body3D => b3.push(id),
                _ => {}
            }
        }
        if !b2.is_empty() || !self.statics_2d.is_empty() || self.w2.is_some() {
            let g = self.gravity_2d;
            let w = self.w2.get_or_insert_with(|| World2::new(g));
            w.set_statics(&self.statics_2d);
            w.sync(tree, &b2, &self.layers);
        }
        if !b3.is_empty() || !self.statics_3d.is_empty() || !self.meshes_3d.is_empty() || self.w3.is_some() {
            let g = self.gravity_3d;
            let w = self.w3.get_or_insert_with(|| World3::new(g));
            w.set_statics(&self.statics_3d, &self.meshes_3d);
            w.sync(tree, &b3, &self.layers);
        }
    }

    /// One fixed step (1/60 s).
    pub fn step(&mut self, tree: &mut SceneTree) {
        self.sync(tree);
        if let Some(w) = &mut self.w2 {
            w.step(tree);
        }
        if let Some(w) = &mut self.w3 {
            w.step(tree);
        }
    }

    /// Pairs of nodes whose bodies touch now (both orders are not included).
    pub fn touching(&self) -> Vec<(NodeId, NodeId)> {
        let mut out = Vec::new();
        if let Some(w) = &self.w2 {
            w.touching(&mut out);
        }
        if let Some(w) = &self.w3 {
            w.touching(&mut out);
        }
        out
    }

    /// Moves a body by `delta` (2D: pixels; 3D: metres), stopping and sliding at walls.
    /// Updates the node, and gives whether it's on the floor afterwards.
    pub fn move_node(&mut self, tree: &mut SceneTree, id: NodeId, delta: Vec3) -> Result<bool, String> {
        let kind = tree.nodes[id].kind;
        if !matches!(kind, NodeKind::Body2D | NodeKind::Body3D) {
            // Not a body: just move it.
            tree.nodes[id].transform.position += delta;
            return Ok(false);
        }
        self.sync(tree);
        let pm = parent_matrix(tree, id);
        let (world_pos, grounded) = if kind == NodeKind::Body2D {
            let w = self.w2.as_mut().ok_or("no 2D physics")?;
            let (p, g) = w.move_body(id, delta.truncate()).ok_or("that body isn't in the physics world")?;
            (p.extend(tree.global_matrix(id).w_axis.z), g)
        } else {
            let w = self.w3.as_mut().ok_or("no 3D physics")?;
            w.move_body(id, delta).ok_or("that body isn't in the physics world")?
        };
        let local = pm.inverse().transform_point3(world_pos);
        let n = &mut tree.nodes[id];
        if kind == NodeKind::Body2D {
            n.transform.position.x = local.x;
            n.transform.position.y = local.y;
        } else {
            n.transform.position = local;
        }
        n.props.insert("on_floor".into(), Variant::Bool(grounded));
        Ok(grounded)
    }

    pub fn impulse(&mut self, tree: &SceneTree, id: NodeId, v: Vec3) {
        self.sync(tree);
        match tree.nodes[id].kind {
            NodeKind::Body2D => {
                if let Some(w) = &mut self.w2 {
                    w.impulse(id, v.truncate());
                }
            }
            NodeKind::Body3D => {
                if let Some(w) = &mut self.w3 {
                    w.impulse(id, v);
                }
            }
            _ => {}
        }
    }

    pub fn raycast_2d(&self, from: Vec2, dir: Vec2, max: f32, ignore: Option<NodeId>, mask: u32) -> Option<RayHit> {
        self.w2.as_ref()?.raycast(from, dir, max, ignore, mask)
    }

    pub fn raycast_3d(&self, from: Vec3, dir: Vec3, max: f32, ignore: Option<NodeId>, mask: u32) -> Option<RayHit> {
        self.w3.as_ref()?.raycast(from, dir, max, ignore, mask)
    }
}
