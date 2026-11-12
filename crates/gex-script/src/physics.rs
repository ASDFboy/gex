//! Physics for Body2D / Body3D nodes, with rapier. The node tree stays the source of
//! truth: before each step, bodies are made, removed or teleported to match their nodes;
//! after it, dynamic bodies write their position and velocity back.
//!
//! 2D works in pixels (y down) in the tree and in metres inside rapier (50 px = 1 m).

use gex_core::settings::PhysicsSettings;
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
fn shape_key(tree: &SceneTree, id: NodeId, scale: Vec3, settings: &PhysicsSettings) -> String {
    let n = &tree.nodes[id];
    // Rounded: a turned node's scale comes back out of its matrix with noise in the last
    // digits, and that must not count as a new shape (the body would be rebuilt every step).
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    let mut s = format!("{:?}|{}", [r(scale.x), r(scale.y), r(scale.z)], settings.sleeping);
    for k in ["body", "shape", "width", "height", "depth", "radius", "density", "mass", "can_sleep", "layer", "mask"] {
        s.push_str(&format!("|{:?}", n.props.get(k)));
    }
    s
}

/// The body settings that change in place (no rebuild), as one comparable text.
fn params_key(tree: &SceneTree, id: NodeId, settings: &PhysicsSettings) -> String {
    let n = &tree.nodes[id];
    let mut s = format!("{}|{}|{}", settings.linear_damping, settings.angular_damping, settings.ccd);
    for k in ["gravity_scale", "friction", "bounce", "linear_damping", "angular_damping", "ccd", "enabled", "fixed_rotation", "lock_x", "lock_y", "lock_z"] {
        s.push_str(&format!("|{:?}", n.props.get(k)));
    }
    s
}

/// A body's own settings, worked out with the project's.
struct BodyParams {
    gravity_scale: f32,
    friction: f32,
    bounce: f32,
    linear_damping: f32,
    angular_damping: f32,
    ccd: bool,
    enabled: bool,
    fixed_rotation: bool,
    lock: [bool; 3],
}

impl BodyParams {
    fn of(tree: &SceneTree, id: NodeId, s: &PhysicsSettings) -> BodyParams {
        let n = &tree.nodes[id];
        let b = |k: &str| n.prop(k).is_some_and(|v| v.as_bool());
        BodyParams {
            gravity_scale: num(tree, id, "gravity_scale", 1.0),
            friction: num(tree, id, "friction", 0.5).max(0.0),
            bounce: num(tree, id, "bounce", 0.0).max(0.0),
            linear_damping: num(tree, id, "linear_damping", 0.0).max(0.0) + s.linear_damping,
            angular_damping: num(tree, id, "angular_damping", 0.0).max(0.0) + s.angular_damping,
            ccd: match n.text("ccd") {
                "on" => true,
                "off" => false,
                _ => s.ccd,
            },
            enabled: n.prop("enabled").is_none_or(|v| v.as_bool()),
            fixed_rotation: b("fixed_rotation"),
            lock: [b("lock_x"), b("lock_y"), b("lock_z")],
        }
    }
}

fn can_sleep(tree: &SceneTree, id: NodeId, s: &PhysicsSettings) -> bool {
    s.sleeping && tree.nodes[id].prop("can_sleep").is_none_or(|v| v.as_bool())
}

/// A 3D body's spin settings (degrees per second) as an angular velocity (radians).
fn spin3(tree: &SceneTree, id: NodeId) -> Vec3 {
    Vec3::new(num(tree, id, "spin_x", 0.0), num(tree, id, "spin_y", 0.0), num(tree, id, "spin_z", 0.0)) * (std::f32::consts::PI / 180.0)
}

fn parent_matrix(tree: &SceneTree, id: NodeId) -> Mat4 {
    tree.nodes[id].parent.map(|p| tree.global_matrix(p)).unwrap_or(Mat4::IDENTITY)
}

// ---------------------------------------------------------------- 2D

mod d2 {
    pub use rapier2d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
    pub use rapier2d::prelude::*;
}

struct Entry2 {
    body: d2::RigidBodyHandle,
    collider: d2::ColliderHandle,
    key: String,
    params: String,
    spin: f32,
    /// Where the node was last seen or put (what a script would have to change to move it).
    pos: Vec2,
    angle: f32,
    vel: Vec2,
    /// Where the body was before the last step: what's shown glides from here to where it is.
    from: (Vec2, f32),
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
    fn new(settings: &PhysicsSettings) -> World2 {
        let world = d2::PhysicsWorld::new();
        let mut controller = d2::KinematicCharacterController::default();
        controller.up = d2::Vector::new(0.0, -1.0);
        controller.offset = d2::CharacterLength::Absolute(0.01);
        let mut w = World2 { world, bodies: HashMap::new(), owners: HashMap::new(), statics: Vec::new(), statics_key: String::new(), controller };
        w.configure(settings);
        w
    }

    /// Applies the project's physics settings (cheap; done every step so scripts can change them).
    fn configure(&mut self, s: &PhysicsSettings) {
        self.world.gravity = d2::Vector::new(s.gravity_2d[0], s.gravity_2d[1]) / PIXELS_PER_METRE;
        let ip = &mut self.world.integration_parameters;
        ip.dt = 1.0 / s.tick_rate.clamp(10.0, 480.0);
        ip.num_solver_iterations = s.solver_iterations.max(1) as usize;
        ip.max_ccd_substeps = s.ccd_substeps.max(1) as usize;
        ip.contact_softness.natural_frequency = s.contact_stiffness.max(1.0);
        let c = &mut self.controller;
        c.max_slope_climb_angle = s.max_slope.to_radians();
        c.min_slope_slide_angle = s.max_slope.to_radians();
        c.snap_to_ground = if s.snap_to_ground > 0.0 { Some(d2::CharacterLength::Absolute(s.snap_to_ground)) } else { None };
        c.autostep = if s.step_height > 0.0 {
            Some(d2::CharacterAutostep { max_height: d2::CharacterLength::Absolute(s.step_height), min_width: d2::CharacterLength::Absolute(0.05), include_dynamic_bodies: false })
        } else {
            None
        };
    }

    fn apply_params(&mut self, id: NodeId, p: &BodyParams) {
        let Some(e) = self.bodies.get(&id) else { return };
        let b = &mut self.world.bodies[e.body];
        b.set_gravity_scale(p.gravity_scale, true);
        b.set_linear_damping(p.linear_damping);
        b.set_angular_damping(p.angular_damping);
        b.enable_ccd(p.ccd);
        b.set_enabled(p.enabled);
        b.lock_rotations(p.fixed_rotation, true);
        b.set_enabled_translations(!p.lock[0], !p.lock[1], true);
        if let Some(c) = self.world.colliders.get_mut(e.collider) {
            c.set_friction(p.friction);
            c.set_restitution(p.bounce);
        }
    }

    fn build(&mut self, tree: &SceneTree, id: NodeId, key: String, layers: &Layers, settings: &PhysicsSettings) {
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
            .angvel(num(tree, id, "spin", 0.0).to_radians())
            .can_sleep(can_sleep(tree, id, settings));
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
        .collision_groups(groups2(layers.of(tree, id)));
        let mass = num(tree, id, "mass", 0.0);
        let col = if mass > 0.0 { col.mass(mass) } else { col.density(num(tree, id, "density", 1.0).max(0.001)) };
        let (body, collider) = self.world.insert(builder, col);
        self.owners.insert(collider, id);
        let spin = num(tree, id, "spin", 0.0);
        self.bodies.insert(id, Entry2 { body, collider, key, params: params_key(tree, id, settings), pos, angle, vel, spin, from: (pos, angle) });
        self.apply_params(id, &BodyParams::of(tree, id, settings));
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
    fn sync(&mut self, tree: &SceneTree, ids: &[NodeId], layers: &Layers, settings: &PhysicsSettings) {
        self.configure(settings);
        let alive: HashSet<NodeId> = ids.iter().copied().collect();
        let gone: Vec<NodeId> = self.bodies.keys().filter(|k| !alive.contains(k)).copied().collect();
        for g in gone {
            self.remove(g);
        }
        for &id in ids {
            let (pos, angle, scale) = pose2(tree, id);
            let key = shape_key(tree, id, scale, settings);
            if self.bodies.get(&id).is_some_and(|e| e.key != key) {
                self.remove(id);
            }
            if !self.bodies.contains_key(&id) {
                self.build(tree, id, key, layers, settings);
                continue;
            }
            let params = params_key(tree, id, settings);
            if self.bodies[&id].params != params {
                self.apply_params(id, &BodyParams::of(tree, id, settings));
                self.bodies.get_mut(&id).unwrap().params = params;
            }
            let Some(e) = self.bodies.get(&id) else { continue };
            if pos.distance(e.pos) > 0.01 + pos.length() * 1e-5 || (angle - e.angle).abs() > 1e-4 {
                self.moved_by_script(id, pos, angle);
            }
            let Some(e) = self.bodies.get_mut(&id) else { continue };
            let b = &mut self.world.bodies[e.body];
            let vel = Vec2::new(num(tree, id, "vx", 0.0), num(tree, id, "vy", 0.0));
            if vel.distance(e.vel) > 1e-3 && b.is_dynamic() {
                b.set_linvel(d2::Vector::new(vel.x, vel.y) / PIXELS_PER_METRE, true);
                e.vel = vel;
            }
            let spin = num(tree, id, "spin", 0.0);
            if (spin - e.spin).abs() > 1e-3 + e.spin.abs() * 1e-4 && b.is_dynamic() {
                b.set_angvel(spin.to_radians(), true);
                e.spin = spin;
            }
        }
    }

    /// A script (or the editor) changed a body's node. A dynamic body nudged a short way is
    /// pushed there, stopping and sliding at what's in the way, so walking it into a wall
    /// doesn't put it through the wall; a jump further than its own size is a teleport.
    fn moved_by_script(&mut self, id: NodeId, pos: Vec2, angle: f32) {
        let Some(e) = self.bodies.get(&id) else { return };
        let (body, collider, seen, seen_angle) = (e.body, e.collider, e.pos, e.angle);
        let b = &self.world.bodies[body];
        let cur = *b.position();
        let (kinematic, dynamic) = (b.is_kinematic(), b.is_dynamic());
        let delta = (pos - seen) / PIXELS_PER_METRE;
        let size = self.world.colliders.get(collider).map(|c| c.compute_aabb().extents().length()).unwrap_or(1.0);
        let turned = cur.rotation.angle() + (angle - seen_angle);
        let mut new_pose = d2::Pose::new(d2::Vector::new(pos.x, pos.y) / PIXELS_PER_METRE, angle);
        let mut nudged = false;
        if dynamic && delta.length() <= size {
            if let Some(col) = self.world.colliders.get(collider) {
                let filter = d2::QueryFilter::default().exclude_rigid_body(body).exclude_sensors().groups(col.collision_groups());
                let qp = self.world.query_pipeline_with_filter(filter);
                let mut plain = d2::KinematicCharacterController::default();
                plain.offset = d2::CharacterLength::Absolute(0.01);
                plain.snap_to_ground = None;
                plain.autostep = None;
                plain.max_slope_climb_angle = std::f32::consts::PI;
                plain.min_slope_slide_angle = std::f32::consts::PI;
                let m = plain.move_shape(self.world.integration_parameters.dt, &qp, col.shape(), &cur, d2::Vector::new(delta.x, delta.y), |_| {});
                new_pose = d2::Pose::new(cur.translation + m.translation, turned);
                nudged = true;
            }
        }
        let b = &mut self.world.bodies[body];
        if kinematic {
            b.set_next_kinematic_position(new_pose);
        } else {
            b.set_position(new_pose, true);
        }
        let e = self.bodies.get_mut(&id).unwrap();
        let moved = Vec2::new(new_pose.translation.x - cur.translation.x, new_pose.translation.y - cur.translation.y) * PIXELS_PER_METRE;
        e.from = if nudged { (e.from.0 + moved, e.from.1 + (angle - seen_angle)) } else { (pos, angle) };
        e.pos = pos;
        e.angle = angle;
    }

    /// Puts a body's node where the body is (world pixels, radians).
    fn place(tree: &mut SceneTree, id: NodeId, pos: Vec2, angle: f32) {
        let pm = parent_matrix(tree, id);
        let local = pm.inverse().transform_point3(pos.extend(0.0));
        let parent_angle = pm.to_scale_rotation_translation().1.to_euler(EulerRot::YXZ).2;
        let n = &mut tree.nodes[id];
        n.transform.position.x = local.x;
        n.transform.position.y = local.y;
        n.transform.rotation.z = (angle - parent_angle).to_degrees();
    }

    fn body_pose(b: &d2::RigidBody) -> (Vec2, f32) {
        let t = b.translation() * PIXELS_PER_METRE;
        (Vec2::new(t.x, t.y), b.rotation().angle())
    }

    fn step(&mut self, tree: &mut SceneTree) {
        for e in self.bodies.values_mut() {
            e.from = Self::body_pose(&self.world.bodies[e.body]);
        }
        self.world.step();
        let floor: HashSet<NodeId> = self.floor_contacts();
        for (id, e) in self.bodies.iter_mut() {
            let b = &self.world.bodies[e.body];
            if !b.is_dynamic() {
                continue;
            }
            let (pos, angle) = Self::body_pose(b);
            let v = b.linvel() * PIXELS_PER_METRE;
            Self::place(tree, *id, pos, angle);
            let n = &mut tree.nodes[*id];
            n.props.insert("vx".into(), Variant::Num(v.x as f64));
            n.props.insert("vy".into(), Variant::Num(v.y as f64));
            let spin = b.angvel().to_degrees();
            n.props.insert("spin".into(), Variant::Num(spin as f64));
            n.props.insert("on_floor".into(), Variant::Bool(floor.contains(id)));
            e.pos = pos;
            e.angle = angle;
            e.vel = Vec2::new(v.x, v.y);
            e.spin = spin;
        }
    }

    /// Between physics steps: shows each moving body part of the way (`alpha`, 0..1) from
    /// where it was before the last step to where it is, so motion is smooth at any frame rate.
    fn present(&mut self, tree: &mut SceneTree, alpha: f32) {
        for (id, e) in self.bodies.iter_mut() {
            let b = &self.world.bodies[e.body];
            if !b.is_dynamic() || b.is_sleeping() || !tree.contains(*id) {
                continue;
            }
            let (pos, angle) = Self::body_pose(b);
            let mut turn = (angle - e.from.1) % std::f32::consts::TAU;
            if turn.abs() > std::f32::consts::PI {
                turn -= std::f32::consts::TAU * turn.signum();
            }
            let (p, a) = (e.from.0.lerp(pos, alpha), e.from.1 + turn * alpha);
            Self::place(tree, *id, p, a);
            e.pos = p;
            e.angle = a;
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
        let mut hits = Vec::new();
        let m = self.controller.move_shape(self.world.integration_parameters.dt, &qp, col.shape(), &pose, d, |c| hits.push(c));
        // What it walked into gets pushed (a crate slides instead of acting like a wall).
        if !hits.is_empty() {
            let (shape, mass, dt) = (col.shared_shape().clone(), col.mass().max(1.0), self.world.integration_parameters.dt);
            let filter = d2::QueryFilter::default().exclude_rigid_body(e.body).exclude_sensors();
            let w = &mut self.world;
            let mut qm = w.broad_phase.as_query_pipeline_mut(w.narrow_phase.query_dispatcher(), &mut w.bodies, &mut w.colliders, filter);
            self.controller.solve_character_collision_impulses(dt, &mut qm, &*shape, mass, &hits);
        }
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
    pub use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
    pub use rapier3d::prelude::*;
}

struct Entry3 {
    body: d3::RigidBodyHandle,
    collider: d3::ColliderHandle,
    key: String,
    params: String,
    spin: Vec3,
    /// Where the node was last seen or put (what a script would have to change to move it).
    pos: Vec3,
    rot: Quat,
    vel: Vec3,
    /// Where the body was before the last step: what's shown glides from here to where it is.
    from: (Vec3, Quat),
}

/// A skinned model gone limp: a body per bone, held together at the joints.
struct Ragdoll {
    /// For each joint: the body it follows, and where the joint sits on that body.
    follow: Vec<(d3::RigidBodyHandle, Mat4)>,
    bodies: Vec<d3::RigidBodyHandle>,
    inverse_bind: Vec<Mat4>,
}

pub struct World3 {
    ragdolls: HashMap<NodeId, Ragdoll>,
    /// Static triangle meshes by node: which vertex list they were made from, and the collider.
    static_meshes: HashMap<NodeId, (usize, d3::ColliderHandle)>,
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
    fn new(settings: &PhysicsSettings) -> World3 {
        let world = d3::PhysicsWorld::new();
        let mut controller = d3::KinematicCharacterController::default();
        controller.offset = d3::CharacterLength::Absolute(0.01);
        let mut w = World3 { ragdolls: HashMap::new(), static_meshes: HashMap::new(), world, bodies: HashMap::new(), owners: HashMap::new(), statics: Vec::new(), statics_key: String::new(), controller };
        w.configure(settings);
        w
    }

    fn configure(&mut self, s: &PhysicsSettings) {
        self.world.gravity = d3::Vector::new(s.gravity_3d[0], s.gravity_3d[1], s.gravity_3d[2]);
        let ip = &mut self.world.integration_parameters;
        ip.dt = 1.0 / s.tick_rate.clamp(10.0, 480.0);
        ip.num_solver_iterations = s.solver_iterations.max(1) as usize;
        ip.max_ccd_substeps = s.ccd_substeps.max(1) as usize;
        let c = &mut self.controller;
        c.max_slope_climb_angle = s.max_slope.to_radians();
        c.min_slope_slide_angle = s.max_slope.to_radians();
        c.snap_to_ground = if s.snap_to_ground > 0.0 { Some(d3::CharacterLength::Absolute(s.snap_to_ground)) } else { None };
        c.autostep = if s.step_height > 0.0 {
            Some(d3::CharacterAutostep { max_height: d3::CharacterLength::Absolute(s.step_height), min_width: d3::CharacterLength::Absolute(0.05), include_dynamic_bodies: false })
        } else {
            None
        };
    }

    fn apply_params(&mut self, id: NodeId, p: &BodyParams) {
        let Some(e) = self.bodies.get(&id) else { return };
        let b = &mut self.world.bodies[e.body];
        b.set_gravity_scale(p.gravity_scale, true);
        b.set_linear_damping(p.linear_damping);
        b.set_angular_damping(p.angular_damping);
        b.enable_ccd(p.ccd);
        b.set_enabled(p.enabled);
        b.lock_rotations(p.fixed_rotation, true);
        b.set_enabled_translations(!p.lock[0], !p.lock[1], !p.lock[2], true);
        if let Some(c) = self.world.colliders.get_mut(e.collider) {
            c.set_friction(p.friction);
            c.set_restitution(p.bounce);
        }
    }

    fn build(&mut self, tree: &SceneTree, id: NodeId, key: String, layers: &Layers, hulls: &HashMap<NodeId, (String, Arc<Vec<Vec3>>)>, settings: &PhysicsSettings) {
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
            .angvel(spin3(tree, id))
            .can_sleep(can_sleep(tree, id, settings));
        if n.prop("fixed_rotation").is_some_and(|v| v.as_bool()) {
            builder = builder.lock_rotations();
        }
        let half = Vec3::new(num(tree, id, "width", 1.0), num(tree, id, "height", 1.0), num(tree, id, "depth", 1.0)) * scale / 2.0;
        let r = num(tree, id, "radius", 0.5) * scale.max_element();
        let col = match n.text("shape") {
            "sphere" | "ball" => d3::ColliderBuilder::ball(r),
            "capsule" => d3::ColliderBuilder::capsule_y((half.y - r).max(0.001), r),
            "hull" | "model" => {
                let pts: Vec<Vec3> = hulls.get(&id).map(|h| h.1.iter().map(|p| *p * scale).collect()).unwrap_or_default();
                d3::ColliderBuilder::convex_hull(&pts).unwrap_or_else(|| d3::ColliderBuilder::cuboid(half.x, half.y, half.z))
            }
            _ => d3::ColliderBuilder::cuboid(half.x, half.y, half.z),
        }
        .collision_groups(groups3(layers.of(tree, id)));
        let mass = num(tree, id, "mass", 0.0);
        let col = if mass > 0.0 { col.mass(mass) } else { col.density(num(tree, id, "density", 1.0).max(0.001)) };
        let (body, collider) = self.world.insert(builder, col);
        self.owners.insert(collider, id);
        self.bodies.insert(id, Entry3 { body, collider, key, params: params_key(tree, id, settings), pos, rot, vel, spin: spin3(tree, id), from: (pos, rot) });
        self.apply_params(id, &BodyParams::of(tree, id, settings));
    }

    fn remove(&mut self, id: NodeId) {
        if let Some(e) = self.bodies.remove(&id) {
            self.owners.remove(&e.collider);
            self.world.remove_body(e.body);
        }
    }

    fn set_statics(&mut self, boxes: &[StaticBox3], meshes: &[StaticMesh]) {
        let key = format!("{:?}", boxes);
        if key != self.statics_key {
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
        }
        // Meshes one by one, so a moving door doesn't rebuild the whole map.
        let live: HashSet<NodeId> = meshes.iter().map(|m| m.0).collect();
        let gone: Vec<NodeId> = self.static_meshes.keys().filter(|k| !live.contains(k)).copied().collect();
        for g in gone {
            if let Some((_, h)) = self.static_meshes.remove(&g) {
                self.owners.remove(&h);
                self.world.remove_collider(h);
            }
        }
        for (owner, v, i) in meshes {
            let ptr = Arc::as_ptr(v) as usize;
            if self.static_meshes.get(owner).is_some_and(|(p, _)| *p == ptr) {
                continue;
            }
            if let Some((_, h)) = self.static_meshes.remove(owner) {
                self.owners.remove(&h);
                self.world.remove_collider(h);
            }
            if let Ok(c) = d3::ColliderBuilder::trimesh(v.to_vec(), i.to_vec()) {
                let h = self.world.insert_collider(c.collision_groups(groups3((1, u32::MAX))), None);
                self.owners.insert(h, *owner);
                self.static_meshes.insert(*owner, (ptr, h));
            }
        }
    }

    fn sync(&mut self, tree: &SceneTree, ids: &[NodeId], layers: &Layers, hulls: &HashMap<NodeId, (String, Arc<Vec<Vec3>>)>, settings: &PhysicsSettings) {
        self.configure(settings);
        let alive: HashSet<NodeId> = ids.iter().copied().collect();
        let gone: Vec<NodeId> = self.bodies.keys().filter(|k| !alive.contains(k)).copied().collect();
        for g in gone {
            self.remove(g);
        }
        for &id in ids {
            let (pos, rot, scale) = pose3(tree, id);
            let mut key = shape_key(tree, id, scale, settings);
            if let Some(h) = hulls.get(&id) {
                key.push_str(&h.0);
            }
            if self.bodies.get(&id).is_some_and(|e| e.key != key) {
                self.remove(id);
            }
            if !self.bodies.contains_key(&id) {
                self.build(tree, id, key, layers, hulls, settings);
                continue;
            }
            let params = params_key(tree, id, settings);
            if self.bodies[&id].params != params {
                self.apply_params(id, &BodyParams::of(tree, id, settings));
                self.bodies.get_mut(&id).unwrap().params = params;
            }
            let Some(e) = self.bodies.get(&id) else { continue };
            if pos.distance(e.pos) > 2e-4 + pos.length() * 1e-5 || rot.angle_between(e.rot) > 2e-3 {
                self.moved_by_script(id, pos, rot);
            }
            let Some(e) = self.bodies.get_mut(&id) else { continue };
            let b = &mut self.world.bodies[e.body];
            let vel = Vec3::new(num(tree, id, "vx", 0.0), num(tree, id, "vy", 0.0), num(tree, id, "vz", 0.0));
            if vel.distance(e.vel) > 1e-4 + e.vel.length() * 1e-5 && b.is_dynamic() {
                b.set_linvel(vel, true);
                e.vel = vel;
            }
            let spin = spin3(tree, id);
            if spin.distance(e.spin) > 1e-3 + e.spin.length() * 1e-4 && b.is_dynamic() {
                b.set_angvel(spin, true);
                e.spin = spin;
            }
        }
    }

    /// A script (or the editor) changed a body's node: see the 2D one.
    fn moved_by_script(&mut self, id: NodeId, pos: Vec3, rot: Quat) {
        let Some(e) = self.bodies.get(&id) else { return };
        let (body, collider, seen, seen_rot) = (e.body, e.collider, e.pos, e.rot);
        let b = &self.world.bodies[body];
        let cur = *b.position();
        let (kinematic, dynamic) = (b.is_kinematic(), b.is_dynamic());
        let delta = pos - seen;
        let size = self.world.colliders.get(collider).map(|c| c.compute_aabb().extents().length()).unwrap_or(1.0);
        let turned = (rot * seen_rot.inverse() * cur.rotation).normalize();
        let mut new_pose = d3::Pose::from_parts(pos, rot);
        let mut nudged = false;
        if dynamic && delta.length() <= size {
            if let Some(col) = self.world.colliders.get(collider) {
                let filter = d3::QueryFilter::default().exclude_rigid_body(body).exclude_sensors().groups(col.collision_groups());
                let qp = self.world.query_pipeline_with_filter(filter);
                let mut plain = d3::KinematicCharacterController::default();
                plain.offset = d3::CharacterLength::Absolute(0.01);
                plain.snap_to_ground = None;
                plain.autostep = None;
                plain.max_slope_climb_angle = std::f32::consts::PI;
                plain.min_slope_slide_angle = std::f32::consts::PI;
                let m = plain.move_shape(self.world.integration_parameters.dt, &qp, col.shape(), &cur, delta, |_| {});
                new_pose = d3::Pose::from_parts(cur.translation + m.translation, turned);
                nudged = true;
            }
        }
        let b = &mut self.world.bodies[body];
        if kinematic {
            b.set_next_kinematic_position(new_pose);
        } else {
            b.set_position(new_pose, true);
        }
        let e = self.bodies.get_mut(&id).unwrap();
        let moved = new_pose.translation - cur.translation;
        e.from = if nudged { (e.from.0 + moved, (rot * seen_rot.inverse() * e.from.1).normalize()) } else { (pos, rot) };
        e.pos = pos;
        e.rot = rot;
    }

    /// Puts a body's node where the body is (world space).
    fn place(tree: &mut SceneTree, id: NodeId, pos: Vec3, rot: Quat) {
        let pm = parent_matrix(tree, id);
        let local = pm.inverse() * Mat4::from_rotation_translation(rot, pos);
        let (_, lr, lt) = local.to_scale_rotation_translation();
        let (y, x, z) = lr.to_euler(EulerRot::YXZ);
        let n = &mut tree.nodes[id];
        n.transform.position = lt;
        n.transform.rotation = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    }

    fn step(&mut self, tree: &mut SceneTree) {
        for e in self.bodies.values_mut() {
            let b = &self.world.bodies[e.body];
            e.from = (b.translation(), *b.rotation());
        }
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
            Self::place(tree, *id, pos, rot);
            let n = &mut tree.nodes[*id];
            n.props.insert("vx".into(), Variant::Num(v.x as f64));
            n.props.insert("vy".into(), Variant::Num(v.y as f64));
            n.props.insert("vz".into(), Variant::Num(v.z as f64));
            let w = b.angvel();
            for (k, a) in [("spin_x", w.x), ("spin_y", w.y), ("spin_z", w.z)] {
                n.props.insert(k.into(), Variant::Num(a.to_degrees() as f64));
            }
            n.props.insert("on_floor".into(), Variant::Bool(floor.contains(id)));
            // What the node now says (after the trip through degrees), so only a real
            // change by a script counts as one.
            let (p, r, _) = pose3(tree, *id);
            e.pos = p;
            e.rot = r;
            e.vel = v;
            e.spin = w;
        }
    }

    /// Between physics steps: see the 2D one.
    fn present(&mut self, tree: &mut SceneTree, alpha: f32) {
        for (id, e) in self.bodies.iter_mut() {
            let b = &self.world.bodies[e.body];
            if !b.is_dynamic() || b.is_sleeping() || !tree.contains(*id) {
                continue;
            }
            Self::place(tree, *id, e.from.0.lerp(b.translation(), alpha), e.from.1.slerp(*b.rotation(), alpha));
            let (p, r, _) = pose3(tree, *id);
            e.pos = p;
            e.rot = r;
        }
    }

    /// Turns a posed skeleton into physics bodies. `joints` are each joint's world transform
    /// now, `parents` the skeleton's parent links, `inverse_bind` its bind matrices.
    fn make_ragdoll(&mut self, id: NodeId, joints: &[Mat4], parents: &[Option<usize>], inverse_bind: &[Mat4], groups: (u32, u32), velocity: Vec3) {
        self.remove_ragdoll(id);
        let n = joints.len();
        let pos: Vec<Vec3> = joints.iter().map(|m| m.w_axis.truncate()).collect();
        let mut children = vec![Vec::new(); n];
        for (i, p) in parents.iter().enumerate() {
            if let Some(p) = p.filter(|p| *p < n) {
                children[p].push(i);
            }
        }
        // A bone gets a body when it reaches far enough to its child; tiny ones ride along.
        let mut body_of: Vec<Option<d3::RigidBodyHandle>> = vec![None; n];
        let mut bodies = Vec::new();
        let mut body_pose: HashMap<d3::RigidBodyHandle, Mat4> = HashMap::new();
        for i in 0..n {
            // The farthest child sets the bone's length and direction.
            let tip = children[i].iter().map(|c| pos[*c]).max_by(|a, b| a.distance(pos[i]).total_cmp(&b.distance(pos[i])));
            let Some(tip) = tip else { continue };
            let len = tip.distance(pos[i]);
            if len < 0.05 {
                continue;
            }
            let dir = (tip - pos[i]) / len;
            let rot = Quat::from_rotation_arc(Vec3::Y, dir);
            let centre = (pos[i] + tip) * 0.5;
            let r = (len * 0.22).clamp(0.025, 0.12);
            let half = (len * 0.5 - r).max(0.01);
            let body = d3::RigidBodyBuilder::dynamic().pose(d3::Pose::from_parts(centre, rot)).linvel(velocity).linear_damping(0.1).angular_damping(0.6);
            let col = d3::ColliderBuilder::capsule_y(half, r).density(900.0).friction(0.7).collision_groups(groups3(groups));
            let (h, c) = self.world.insert(body, col);
            self.owners.insert(c, id);
            body_of[i] = Some(h);
            bodies.push(h);
            body_pose.insert(h, Mat4::from_rotation_translation(rot, centre));
        }
        if bodies.is_empty() {
            return;
        }
        // Which body each joint follows: its own, else its nearest ancestor's (else the first).
        let mut follow_body = vec![bodies[0]; n];
        for i in 0..n {
            let mut k = Some(i);
            while let Some(j) = k {
                if let Some(b) = body_of[j] {
                    follow_body[i] = b;
                    break;
                }
                k = parents[j];
            }
            if k.is_none() {
                // A root without a body: the nearest child body.
                if let Some(b) = children[i].iter().find_map(|c| body_of[*c]) {
                    follow_body[i] = b;
                }
            }
        }
        // Ball joints where a bone's body meets its parent's.
        for i in 0..n {
            let Some(b) = body_of[i] else { continue };
            let mut k = parents[i];
            let parent_body = loop {
                match k {
                    Some(j) if body_of[j].is_some() => break body_of[j],
                    Some(j) => k = parents[j],
                    None => break None,
                }
            };
            let Some(pb) = parent_body else { continue };
            let a1 = body_pose[&pb].inverse().transform_point3(pos[i]);
            let a2 = body_pose[&b].inverse().transform_point3(pos[i]);
            let joint = d3::SphericalJointBuilder::new().local_anchor1(a1).local_anchor2(a2).contacts_enabled(false);
            self.world.insert_impulse_joint(pb, b, joint);
        }
        let follow = (0..n).map(|i| (follow_body[i], body_pose[&follow_body[i]].inverse() * joints[i])).collect();
        self.ragdolls.insert(id, Ragdoll { follow, bodies, inverse_bind: inverse_bind.to_vec() });
    }

    fn remove_ragdoll(&mut self, id: NodeId) {
        if let Some(r) = self.ragdolls.remove(&id) {
            for b in r.bodies {
                self.world.remove_body(b);
            }
            self.owners.retain(|_, o| *o != id);
        }
    }

    /// A ragdoll's joint matrices now, in the space of its node (`node_world`).
    fn ragdoll_pose(&self, id: NodeId, node_world: Mat4) -> Option<Vec<Mat4>> {
        let r = self.ragdolls.get(&id)?;
        let inv = node_world.inverse();
        Some(
            r.follow
                .iter()
                .zip(&r.inverse_bind)
                .map(|((b, off), ib)| {
                    let body = &self.world.bodies[*b];
                    inv * Mat4::from_rotation_translation(*body.rotation(), body.translation()) * *off * *ib
                })
                .collect(),
        )
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
        let mut hits = Vec::new();
        let m = self.controller.move_shape(self.world.integration_parameters.dt, &qp, col.shape(), &pose, delta, |c| hits.push(c));
        // What it walked into gets pushed (a crate slides instead of acting like a wall).
        if !hits.is_empty() {
            let (shape, mass, dt) = (col.shared_shape().clone(), col.mass().max(1.0), self.world.integration_parameters.dt);
            let filter = d3::QueryFilter::default().exclude_rigid_body(e.body).exclude_sensors();
            let w = &mut self.world;
            let mut qm = w.broad_phase.as_query_pipeline_mut(w.narrow_phase.query_dispatcher(), &mut w.bodies, &mut w.colliders, filter);
            self.controller.solve_character_collision_impulses(dt, &mut qm, &*shape, mass, &hits);
        }
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
        if let Some(r) = self.ragdolls.get(&id) {
            for h in &r.bodies {
                let b = &mut self.world.bodies[*h];
                let m = b.mass();
                b.apply_impulse(v * m, true);
            }
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
    /// The project's physics settings (scripts change them with SET_PHYSICS).
    pub settings: PhysicsSettings,
    /// Extra static shapes from tilemaps and maps (set before each step).
    pub statics_2d: Vec<StaticBox2>,
    pub statics_3d: Vec<StaticBox3>,
    pub meshes_3d: Vec<StaticMesh>,
    pub layers: Layers,
    /// Points for Body3D "hull" shapes (from their model), in the body's own space: key, points.
    pub hulls: HashMap<NodeId, (String, Arc<Vec<Vec3>>)>,
}

impl Physics {
    pub fn new(settings: PhysicsSettings) -> Physics {
        Physics { settings, ..Default::default() }
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
            let s = &self.settings;
            let w = self.w2.get_or_insert_with(|| World2::new(s));
            w.set_statics(&self.statics_2d);
            w.sync(tree, &b2, &self.layers, s);
        }
        if !b3.is_empty() || !self.statics_3d.is_empty() || !self.meshes_3d.is_empty() || self.w3.is_some() {
            let s = &self.settings;
            let w = self.w3.get_or_insert_with(|| World3::new(s));
            w.set_statics(&self.statics_3d, &self.meshes_3d);
            w.sync(tree, &b3, &self.layers, &self.hulls, s);
            let gone: Vec<NodeId> = w.ragdolls.keys().filter(|id| !tree.contains(**id)).copied().collect();
            for g in gone {
                w.remove_ragdoll(g);
            }
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

    /// After the frame's steps: shows moving bodies `alpha` (0..1) of the way into the step
    /// that's still to come, so motion is smooth whatever the frame rate.
    pub fn present(&mut self, tree: &mut SceneTree, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        if let Some(w) = &mut self.w2 {
            w.present(tree, alpha);
        }
        if let Some(w) = &mut self.w3 {
            w.present(tree, alpha);
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
            _ => {
                if let Some(w) = &mut self.w3 {
                    w.impulse(id, v);
                }
            }
        }
    }

    /// Makes a skinned model's node a ragdoll (see World3::make_ragdoll).
    #[allow(clippy::too_many_arguments)]
    pub fn ragdoll(&mut self, tree: &SceneTree, id: NodeId, joints: &[Mat4], parents: &[Option<usize>], inverse_bind: &[Mat4], velocity: Vec3) {
        let groups = self.layers.of(tree, id);
        let s = &self.settings;
        let w = self.w3.get_or_insert_with(|| World3::new(s));
        w.make_ragdoll(id, joints, parents, inverse_bind, groups, velocity);
    }

    pub fn end_ragdoll(&mut self, id: NodeId) {
        if let Some(w) = &mut self.w3 {
            w.remove_ragdoll(id);
        }
    }

    pub fn is_ragdoll(&self, id: NodeId) -> bool {
        self.w3.as_ref().is_some_and(|w| w.ragdolls.contains_key(&id))
    }

    /// Joint matrices for every ragdoll whose node still exists.
    pub fn ragdoll_poses(&self, tree: &SceneTree) -> Vec<(NodeId, Vec<Mat4>)> {
        let Some(w) = &self.w3 else { return Vec::new() };
        w.ragdolls.keys().filter(|id| tree.contains(**id)).filter_map(|id| w.ragdoll_pose(*id, tree.global_matrix(*id)).map(|p| (*id, p))).collect()
    }

    pub fn raycast_2d(&self, from: Vec2, dir: Vec2, max: f32, ignore: Option<NodeId>, mask: u32) -> Option<RayHit> {
        self.w2.as_ref()?.raycast(from, dir, max, ignore, mask)
    }

    pub fn raycast_3d(&self, from: Vec3, dir: Vec3, max: f32, ignore: Option<NodeId>, mask: u32) -> Option<RayHit> {
        self.w3.as_ref()?.raycast(from, dir, max, ignore, mask)
    }
}
