#![allow(deprecated)] // glam 0.33 moved the camera helpers; the old names still work.
//! Cameras: the game's own (Camera2D / Camera3D objects) or the editor's.

use gex_core::glam::{Mat4, Vec2, Vec3, Vec4Swizzles};
use gex_core::render::RenderList;

#[derive(Clone, Copy, Debug)]
pub struct Cameras {
    pub view_proj_3d: Mat4,
    pub view_proj_2d: Mat4,
    pub draw_3d: bool,
    pub draw_2d: bool,
    pub size: [u32; 2],
    /// Where the 3D camera is, and a point it's looking at (shadows are fitted around it).
    pub eye: Vec3,
    pub focus: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct EditorCamera2D {
    pub center: Vec2,
    pub zoom: f32,
}

impl Default for EditorCamera2D {
    fn default() -> Self {
        EditorCamera2D { center: Vec2::new(320.0, 180.0), zoom: 1.0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EditorCamera3D {
    pub target: Vec3,
    /// Radians: yaw around Y, pitch up/down.
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_deg: f32,
}

impl Default for EditorCamera3D {
    fn default() -> Self {
        EditorCamera3D { target: Vec3::ZERO, yaw: 0.6, pitch: -0.45, distance: 8.0, fov_deg: 60.0 }
    }
}

impl EditorCamera3D {
    pub fn eye(&self) -> Vec3 {
        let dir = Vec3::new(self.yaw.sin() * self.pitch.cos(), -self.pitch.sin(), self.yaw.cos() * self.pitch.cos());
        self.target + dir * self.distance
    }
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * 0.008;
        self.pitch = (self.pitch - dy * 0.008).clamp(-1.55, 1.55);
    }
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let fwd = (self.target - self.eye()).normalize();
        let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(fwd);
        let s = self.distance * 0.0015;
        self.target += (-right * dx + up * dy) * s;
    }
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * (1.0 - steps * 0.1)).clamp(0.2, 2000.0);
    }
    /// Moves the camera like flying (W/S forward, A/D sideways, Q/E down/up).
    pub fn fly(&mut self, forward: f32, right: f32, up: f32) {
        let fwd = (self.target - self.eye()).normalize();
        let r = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
        let s = self.distance.max(1.0) * 0.02;
        self.target += (fwd * forward + r * right + Vec3::Y * up) * s;
    }
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let proj = Mat4::perspective_rh(self.fov_deg.to_radians(), aspect, 0.05, 5000.0);
        proj * Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
    }
}

fn aspect(size: [u32; 2]) -> f32 {
    size[0].max(1) as f32 / size[1].max(1) as f32
}

/// 2D: world pixels with y down, `center` in the middle of the view.
pub fn ortho_2d(center: Vec2, zoom: f32, rotation: f32, size: [u32; 2]) -> Mat4 {
    let hw = size[0] as f32 / 2.0 / zoom;
    let hh = size[1] as f32 / 2.0 / zoom;
    let proj = Mat4::orthographic_rh(-hw, hw, hh, -hh, -1000.0, 1000.0);
    proj * Mat4::from_rotation_z(-rotation) * Mat4::from_translation(-center.extend(0.0))
}

/// Where a game's picture goes in a window, and the size it's drawn at, for a display
/// scale mode (see gex_core::settings::Display).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameView {
    /// Pixels the game is drawn at.
    pub render: [u32; 2],
    /// Where it's shown in the window, in window pixels: x, y, width, height.
    pub rect: [f32; 4],
}

impl GameView {
    pub fn new(mode: &str, window: [u32; 2], design: [u32; 2]) -> GameView {
        let (w, h) = (window[0].max(1) as f32, window[1].max(1) as f32);
        let design = [design[0].max(1), design[1].max(1)];
        let (dw, dh) = (design[0] as f32, design[1] as f32);
        let full = [0.0, 0.0, w, h];
        let centered = |s: f32| {
            let (rw, rh) = (dw * s, dh * s);
            [((w - rw) / 2.0).round(), ((h - rh) / 2.0).round(), rw, rh]
        };
        match mode {
            "expand" => GameView { render: [w as u32, h as u32], rect: full },
            "stretch" => GameView { render: design, rect: full },
            "pixel" => {
                let s = (w / dw).min(h / dh);
                GameView { render: design, rect: centered(if s >= 1.0 { s.floor() } else { s }) }
            }
            _ => GameView { render: design, rect: centered((w / dw).min(h / dh)) },
        }
    }

    /// A point in window pixels as a point in the game's drawn pixels.
    pub fn to_render(&self, p: Vec2) -> Vec2 {
        Vec2::new((p.x - self.rect[0]) / self.rect[2].max(1.0) * self.render[0] as f32, (p.y - self.rect[1]) / self.rect[3].max(1.0) * self.render[1] as f32)
    }
}

impl Cameras {
    /// What the game itself sees: its current Camera2D/Camera3D. `design` is the game's
    /// window size from the project: the 2D view is scaled to fit it whatever the real
    /// size is (keeping its shape). Without a Camera2D, (0, 0) is the top-left of the
    /// design window, like a GameMaker room.
    pub fn for_game(list: &RenderList, size: [u32; 2], design: [u32; 2]) -> Cameras {
        let fit = (size[0] as f32 / design[0].max(1) as f32).min(size[1] as f32 / design[1].max(1) as f32).max(0.01);
        let (vp3, eye, focus) = match list.camera3d {
            Some(c) => {
                let (near, far) = (c.near.max(0.001), c.far.max(c.near + 0.01));
                let proj = match c.ortho {
                    // No perspective: `h` metres tall, as wide as the window's shape makes it.
                    Some(h) => {
                        let (hh, hw) = (h / 2.0, h / 2.0 * aspect(size));
                        Mat4::orthographic_rh(-hw, hw, -hh, hh, near, far)
                    }
                    None => Mat4::perspective_rh(c.fov_deg.to_radians(), aspect(size), near, far),
                };
                let eye = c.world.w_axis.truncate();
                (proj * c.world.inverse(), eye, eye + c.world.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z) * 12.0)
            }
            None => {
                let d = EditorCamera3D::default();
                (d.view_proj(aspect(size)), d.eye(), d.target)
            }
        };
        let vp2 = match list.camera2d {
            Some(c) => ortho_2d(c.center, c.zoom * fit, c.rotation, size),
            None => ortho_2d(Vec2::new(design[0] as f32 / 2.0, design[1] as f32 / 2.0), fit, 0.0, size),
        };
        Cameras { view_proj_3d: vp3, view_proj_2d: vp2, draw_3d: true, draw_2d: true, size, eye, focus }
    }

    pub fn for_editor(mode_3d: bool, c2: &EditorCamera2D, c3: &EditorCamera3D, size: [u32; 2]) -> Cameras {
        Cameras {
            view_proj_3d: c3.view_proj(aspect(size)),
            view_proj_2d: ortho_2d(c2.center, c2.zoom, 0.0, size),
            draw_3d: mode_3d,
            draw_2d: !mode_3d,
            size,
            eye: c3.eye(),
            focus: c3.target,
        }
    }

    fn ndc(&self, screen: Vec2) -> Vec2 {
        Vec2::new(screen.x / self.size[0].max(1) as f32 * 2.0 - 1.0, 1.0 - screen.y / self.size[1].max(1) as f32 * 2.0)
    }

    /// A point in the view (pixels from its top-left) as a 2D world position.
    pub fn screen_to_2d(&self, screen: Vec2) -> Vec2 {
        let n = self.ndc(screen);
        (self.view_proj_2d.inverse() * n.extend(0.0).extend(1.0)).xy()
    }

    /// A 2D world position as a point in the view.
    pub fn world_2d_to_screen(&self, p: Vec2) -> Vec2 {
        let c = self.view_proj_2d * p.extend(0.0).extend(1.0);
        Vec2::new((c.x + 1.0) / 2.0 * self.size[0] as f32, (1.0 - c.y) / 2.0 * self.size[1] as f32)
    }

    /// A 3D world position as a point in the view (None when behind the camera).
    pub fn world_3d_to_screen(&self, p: Vec3) -> Option<Vec2> {
        let c = self.view_proj_3d * p.extend(1.0);
        if c.w <= 0.0 {
            return None;
        }
        let n = c.xyz() / c.w;
        Some(Vec2::new((n.x + 1.0) / 2.0 * self.size[0] as f32, (1.0 - n.y) / 2.0 * self.size[1] as f32))
    }

    /// The ray through a point in the view: (origin, direction).
    pub fn ray(&self, screen: Vec2) -> (Vec3, Vec3) {
        let n = self.ndc(screen);
        let inv = self.view_proj_3d.inverse();
        let a = inv * n.extend(0.0).extend(1.0);
        let b = inv * n.extend(1.0).extend(1.0);
        let (a, b) = (a.xyz() / a.w, b.xyz() / b.w);
        (a, (b - a).normalize())
    }
}

/// Distance along a ray to an object's unit box (a cube primitive scaled by `model`).
pub fn ray_box(origin: Vec3, dir: Vec3, model: Mat4) -> Option<f32> {
    let inv = model.inverse();
    let o = inv.transform_point3(origin);
    let d = inv.transform_vector3(dir);
    let (mut t0, mut t1) = (f32::MIN, f32::MAX);
    for k in 0..3 {
        if d[k].abs() < 1e-8 {
            if o[k].abs() > 0.5 {
                return None;
            }
            continue;
        }
        let a = (-0.5 - o[k]) / d[k];
        let b = (0.5 - o[k]) / d[k];
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    if t1 >= t0.max(0.0) {
        // Back to world distance.
        let hit = model.transform_point3(o + d * t0.max(0.0));
        Some(hit.distance(origin))
    } else {
        None
    }
}

#[cfg(test)]
mod game_view_tests {
    use super::*;

    #[test]
    fn scale_modes() {
        // A 320×180 game on a 1920×1200 screen.
        let fit = GameView::new("fit", [1920, 1200], [320, 180]);
        assert_eq!(fit.render, [320, 180]);
        assert_eq!(fit.rect, [0.0, 60.0, 1920.0, 1080.0]);
        let px = GameView::new("pixel", [1920, 1200], [320, 180]);
        assert_eq!(px.rect, [0.0, 60.0, 1920.0, 1080.0], "6× fits exactly");
        let px = GameView::new("pixel", [1900, 1200], [320, 180]);
        assert_eq!(px.rect[2], 1600.0, "5×, whole numbers only");
        let st = GameView::new("stretch", [1920, 1200], [320, 180]);
        assert_eq!(st.rect, [0.0, 0.0, 1920.0, 1200.0]);
        let ex = GameView::new("expand", [1920, 1200], [320, 180]);
        assert_eq!(ex.render, [1920, 1200]);
        // The middle of the screen is the middle of the game.
        assert_eq!(fit.to_render(Vec2::new(960.0, 600.0)), Vec2::new(160.0, 90.0));
    }
}
