//! The handles drawn on the selected object in the blueprint view: arrows (move), rings (rotate)
//! and boxes (scale) for one axis, small two-colored squares for two axes at once. Dragging
//! the object itself, away from any handle, works relative to the camera.

use eframe::egui::{self, Color32, Pos2, Stroke};
use gex_core::glam::{Quat, Vec2, Vec3};
use gex_render::camera::Cameras;

use crate::state::Tool;

/// What was grabbed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Handle {
    Axis(usize),
    /// The two axes other than this one.
    Plane(usize),
    /// The object itself: every axis, relative to the camera.
    Free,
}

#[derive(Clone, Copy)]
pub struct Gizmo {
    pub pivot: Vec3,
    pub axes: [Vec3; 3],
    /// World units that span the handle length on screen.
    pub k: f32,
    pub mode_3d: bool,
    pub tool: Tool,
}

/// Handle length, in screen points.
const LEN: f32 = 78.0;
const COLORS: [Color32; 3] = [Color32::from_rgb(0xe5, 0x48, 0x4d), Color32::from_rgb(0x7f, 0xc8, 0x3c), Color32::from_rgb(0x4c, 0x8d, 0xf0)];

impl Gizmo {
    /// `rot` is the selected object's world rotation: scaling works along the object's own axes.
    pub fn new(cams: &Cameras, mode_3d: bool, tool: Tool, pivot: Vec3, rot: Quat, ppp: f32) -> Option<Gizmo> {
        let axes = if tool == Tool::Scale { [rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z] } else { [Vec3::X, Vec3::Y, Vec3::Z] };
        let mut g = Gizmo { pivot, axes, k: 1.0, mode_3d, tool };
        let right = if mode_3d { (cams.focus - cams.eye).cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X) } else { Vec3::X };
        let per_unit = (g.px(cams, pivot + right)? - g.px(cams, pivot)?).length();
        if per_unit < 1e-4 {
            return None;
        }
        g.k = LEN * ppp / per_unit;
        Some(g)
    }

    /// A world point in view pixels.
    pub fn px(&self, cams: &Cameras, p: Vec3) -> Option<Vec2> {
        if self.mode_3d {
            cams.world_3d_to_screen(p)
        } else {
            Some(cams.world_2d_to_screen(p.truncate()))
        }
    }

    fn count(&self) -> usize {
        if self.mode_3d {
            3
        } else {
            2
        }
    }

    /// The direction toward the viewer.
    pub fn toward_viewer(&self, cams: &Cameras) -> Vec3 {
        if self.mode_3d {
            (cams.eye - self.pivot).try_normalize().unwrap_or(Vec3::Z)
        } else {
            Vec3::Z
        }
    }

    /// The rotation rings' axes: all three in 3D, only Z in 2D.
    fn ring_axes(&self) -> std::ops::Range<usize> {
        if self.mode_3d {
            0..3
        } else {
            2..3
        }
    }

    fn ring(&self, cams: &Cameras, i: usize) -> Vec<Vec2> {
        let (u, v) = (self.axes[(i + 1) % 3], self.axes[(i + 2) % 3]);
        (0..=48).filter_map(|s| {
            let a = s as f32 / 48.0 * std::f32::consts::TAU;
            self.px(cams, self.pivot + (u * a.cos() + v * a.sin()) * self.k * 0.9)
        }).collect()
    }

    /// The corners of the two-axis square that leaves out axis `i`.
    fn square(&self, cams: &Cameras, i: usize) -> Option<[Vec2; 4]> {
        let (u, v) = (self.axes[(i + 1) % 3] * self.k, self.axes[(i + 2) % 3] * self.k);
        let p = |a: f32, b: f32| self.px(cams, self.pivot + u * a + v * b);
        Some([p(0.26, 0.26)?, p(0.52, 0.26)?, p(0.52, 0.52)?, p(0.26, 0.52)?])
    }

    fn planes(&self) -> std::ops::Range<usize> {
        // In 2D the only pair is X and Y: the square that leaves out Z.
        if self.mode_3d {
            0..3
        } else {
            2..3
        }
    }

    /// The handle under a point (view pixels), if any.
    pub fn hit(&self, cams: &Cameras, at: Vec2, ppp: f32) -> Option<Handle> {
        let near = 7.0 * ppp;
        let c = self.px(cams, self.pivot)?;
        if self.tool == Tool::Rotate {
            let mut best: Option<(f32, usize)> = None;
            for i in self.ring_axes() {
                let pts = self.ring(cams, i);
                let d = pts.windows(2).map(|w| seg_dist(at, w[0], w[1])).fold(f32::MAX, f32::min);
                if d < near && best.is_none_or(|b| d < b.0) {
                    best = Some((d, i));
                }
            }
            return best.map(|b| Handle::Axis(b.1));
        }
        for i in self.planes() {
            if let Some(q) = self.square(cams, i) {
                let mid = (q[0] + q[2]) / 2.0;
                if at.distance(mid) < (q[0].distance(q[2]) / 2.0).max(6.0 * ppp) {
                    return Some(Handle::Plane(i));
                }
            }
        }
        let mut best: Option<(f32, usize)> = None;
        for i in 0..self.count() {
            let Some(end) = self.px(cams, self.pivot + self.axes[i] * self.k) else { continue };
            let d = seg_dist(at, c + (end - c) * 0.15, end);
            if d < near && best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|b| Handle::Axis(b.1))
    }

    /// Draws the handles; `lit` is the one hovered or being dragged.
    pub fn paint(&self, p: &egui::Painter, cams: &Cameras, origin: Pos2, ppp: f32, lit: Option<Handle>) {
        let pt = |v: Vec2| origin + egui::vec2(v.x / ppp, v.y / ppp);
        let Some(c) = self.px(cams, self.pivot) else { return };
        let color = |h: Handle, i: usize| if lit == Some(h) { Color32::from_rgb(0xff, 0xe0, 0x66) } else { COLORS[i] };
        if self.tool == Tool::Rotate {
            if self.mode_3d {
                // The outer ring: turning about the line of sight (what dragging the object does).
                p.circle_stroke(pt(c), LEN * 1.08, Stroke::new(1.0, Color32::from_gray(if lit == Some(Handle::Free) { 230 } else { 130 })));
            }
            for i in self.ring_axes() {
                let pts: Vec<Pos2> = self.ring(cams, i).into_iter().map(pt).collect();
                p.add(egui::Shape::line(pts, Stroke::new(if lit == Some(Handle::Axis(i)) { 3.0 } else { 2.0 }, color(Handle::Axis(i), i))));
            }
            return;
        }
        for i in self.planes() {
            let Some(q) = self.square(cams, i) else { continue };
            let q = q.map(pt);
            let (a, b) = ((i + 1) % 3, (i + 2) % 3);
            let (ca, cb) = if lit == Some(Handle::Plane(i)) { (color(Handle::Plane(i), a), color(Handle::Plane(i), b)) } else { (COLORS[a].gamma_multiply(0.85), COLORS[b].gamma_multiply(0.85)) };
            // Two triangles, one in each axis's color.
            p.add(egui::Shape::convex_polygon(vec![q[0], q[1], q[2]], ca, Stroke::NONE));
            p.add(egui::Shape::convex_polygon(vec![q[0], q[2], q[3]], cb, Stroke::NONE));
        }
        for i in 0..self.count() {
            let Some(end) = self.px(cams, self.pivot + self.axes[i] * self.k) else { continue };
            let col = color(Handle::Axis(i), i);
            let dir = (end - c).try_normalize().unwrap_or(Vec2::X);
            let side = Vec2::new(-dir.y, dir.x);
            let head = 11.0 * ppp;
            p.line_segment([pt(c), pt(end - dir * head * 0.8)], Stroke::new(2.0, col));
            if self.tool == Tool::Move {
                p.add(egui::Shape::convex_polygon(vec![pt(end), pt(end - dir * head + side * head * 0.38), pt(end - dir * head - side * head * 0.38)], col, Stroke::NONE));
            } else {
                p.rect_filled(egui::Rect::from_center_size(pt(end - dir * head * 0.4), egui::vec2(9.0, 9.0)), 0.0, col);
            }
        }
        p.rect_filled(egui::Rect::from_center_size(pt(c), egui::vec2(6.0, 6.0)), 0.0, Color32::from_gray(if lit == Some(Handle::Free) { 255 } else { 200 }));
    }

    /// Where the ray under a view pixel meets the plane through the pivot with this normal.
    pub fn on_plane(&self, cams: &Cameras, at: Vec2, normal: Vec3) -> Option<Vec3> {
        if !self.mode_3d {
            return Some(cams.screen_to_2d(at).extend(self.pivot.z));
        }
        let (o, d) = cams.ray(at);
        let den = d.dot(normal);
        if den.abs() < 1e-5 {
            return None;
        }
        let t = (self.pivot - o).dot(normal) / den;
        (t > 0.0).then(|| o + d * t)
    }

    /// How far along axis `i` (world units) a drag across the screen goes.
    pub fn along(&self, cams: &Cameras, i: usize, drag: Vec2) -> f32 {
        let (Some(c), Some(e)) = (self.px(cams, self.pivot), self.px(cams, self.pivot + self.axes[i] * self.k)) else { return 0.0 };
        let s = e - c;
        if s.length_squared() < 1.0 {
            return 0.0;
        }
        drag.dot(s) / s.length_squared() * self.k
    }
}

fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-6 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}
