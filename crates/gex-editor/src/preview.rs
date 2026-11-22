//! Asset previews in the Assets panel: models (a small shaded view you can turn by dragging)
//! and sounds (play / stop). Models load on a background thread, since maps take a moment.

use crate::docs::Tip as _;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};
use gex_core::glam::{Mat3, Vec3};

use crate::theme;

/// The most triangles a preview draws (the biggest ones, so shapes still read).
const MAX_TRIS: usize = 6000;

pub struct ModelPreview {
    /// Triangles, centred on the model, and each one's color.
    tris: Vec<([Vec3; 3], [f32; 4])>,
    radius: f32,
    pub info: Vec<String>,
}

type Slot = Arc<Mutex<Option<Result<ModelPreview, String>>>>;

pub struct Previews {
    models: HashMap<String, Slot>,
    yaw: f32,
    pitch: f32,
    audio: gex_script::audio::Audio,
    sound: Option<(String, u64)>,
}

impl Default for Previews {
    fn default() -> Self {
        Previews { models: HashMap::new(), yaw: 0.6, pitch: -0.35, audio: Default::default(), sound: None }
    }
}

fn build(m: &gex_assets::model::ModelData) -> ModelPreview {
    let mut all: Vec<([Vec3; 3], [f32; 4], f32)> = Vec::new();
    let mut verts = 0;
    for p in &m.parts {
        verts += p.positions.len();
        for t in p.indices.chunks_exact(3) {
            let (Some(a), Some(b), Some(c)) = (p.positions.get(t[0] as usize), p.positions.get(t[1] as usize), p.positions.get(t[2] as usize)) else { continue };
            let area = (*b - *a).cross(*c - *a).length();
            all.push(([*a, *b, *c], p.color, area));
        }
    }
    let total = all.len();
    if all.len() > MAX_TRIS {
        all.select_nth_unstable_by(MAX_TRIS, |x, y| y.2.total_cmp(&x.2));
        all.truncate(MAX_TRIS);
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (t, _, _) in &all {
        for v in t {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
    }
    let center = if all.is_empty() { Vec3::ZERO } else { (lo + hi) * 0.5 };
    let radius = if all.is_empty() { 1.0 } else { ((hi - lo).length() * 0.5).max(1e-4) };
    let size = hi - lo;
    let mut info = vec![
        format!("{} part(s) · {} vertices · {} triangles", m.parts.len(), verts, total),
        format!("size {:.2} × {:.2} × {:.2}", size.x.max(0.0), size.y.max(0.0), size.z.max(0.0)),
    ];
    if !m.joints.is_empty() {
        info.push(format!("{} bones", m.joints.len()));
    }
    if !m.animations.is_empty() {
        let names: Vec<&str> = m.animations.iter().take(12).map(|a| a.name.as_str()).collect();
        info.push(format!("animations: {}{}", names.join(", "), if m.animations.len() > 12 { ", …" } else { "" }));
    }
    if !m.entities.is_empty() {
        info.push(format!("{} map entities", m.entities.len()));
    }
    ModelPreview { tris: all.into_iter().map(|(t, c, _)| (t.map(|v| v - center), c)).collect(), radius, info }
}

impl Previews {
    /// Forget cached previews (a file changed).
    pub fn forget(&mut self, path: &str) {
        self.models.remove(path);
    }

    pub fn model(&mut self, ui: &mut egui::Ui, project: &gex_core::project::Project, path: &str, width: f32) {
        let slot = self
            .models
            .entry(path.to_string())
            .or_insert_with(|| {
                let slot: Slot = Arc::new(Mutex::new(None));
                let (s2, dir, config, p, ctx) = (slot.clone(), project.dir.clone(), project.config.clone(), path.to_string(), ui.ctx().clone());
                std::thread::spawn(move || {
                    let vfs = gex_assets::Vfs::new(&dir, &config);
                    let r = gex_assets::model::load(&vfs, &p).map(|m| build(&m));
                    *s2.lock().unwrap() = Some(r);
                    ctx.request_repaint();
                });
                slot
            })
            .clone();
        let guard = slot.lock().unwrap();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, width), egui::Sense::drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(0x0c, 0x0c, 0x0c));
        match &*guard {
            None => {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "loading…", egui::FontId::proportional(12.0), theme::TEXT_DIM);
            }
            Some(Err(e)) => {
                drop(painter);
                ui.label(RichText::new(e).small().color(theme::BAD));
            }
            Some(Ok(m)) => {
                if resp.dragged() {
                    let d = resp.drag_delta();
                    self.yaw += d.x * 0.012;
                    self.pitch = (self.pitch + d.y * 0.012).clamp(-1.5, 1.5);
                }
                let rot = Mat3::from_rotation_x(self.pitch) * Mat3::from_rotation_y(self.yaw);
                let scale = width * 0.45 / m.radius;
                let light = Vec3::new(0.4, 0.7, 0.6).normalize();
                let mut tris: Vec<([Vec3; 3], f32, [f32; 4])> = m
                    .tris
                    .iter()
                    .map(|(t, c)| {
                        let v = t.map(|p| rot * p);
                        let n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
                        let shade = 0.3 + 0.7 * n.dot(light).abs();
                        (v, shade, *c)
                    })
                    .collect();
                // Far to near (painter's order): +z is toward the viewer.
                tris.sort_by(|a, b| (a.0[0].z + a.0[1].z + a.0[2].z).total_cmp(&(b.0[0].z + b.0[1].z + b.0[2].z)));
                let mut mesh = egui::Mesh::default();
                let c = rect.center();
                for (v, shade, col) in tris {
                    let color = Color32::from_rgb(
                        (col[0].clamp(0.0, 1.0) * shade * 255.0) as u8,
                        (col[1].clamp(0.0, 1.0) * shade * 255.0) as u8,
                        (col[2].clamp(0.0, 1.0) * shade * 255.0) as u8,
                    );
                    let i = mesh.vertices.len() as u32;
                    for p in v {
                        mesh.colored_vertex(egui::pos2(c.x + p.x * scale, c.y - p.y * scale), color);
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                }
                painter.add(egui::Shape::mesh(mesh));
                drop(painter);
                for l in &m.info {
                    ui.label(RichText::new(l).small().color(theme::TEXT_DIM));
                }
                resp.tip("drag to turn it");
            }
        }
    }

    pub fn sound(&mut self, ui: &mut egui::Ui, vfs: &gex_assets::Vfs, path: &str) {
        self.audio.tidy();
        let playing = self.sound.as_ref().is_some_and(|(p, id)| p == path && self.audio.playing(*id));
        ui.horizontal(|ui| {
            if playing {
                if ui.button("⏹ Stop").clicked() {
                    self.stop_sound();
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            } else if ui.button("▶ Play").clicked() {
                self.stop_sound();
                match self.audio.play(vfs, path, 1.0, 1.0, false) {
                    Ok(0) => {
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("sound preview error", path)), "no sound device".to_string()));
                    }
                    Ok(id) => self.sound = Some((path.to_string(), id)),
                    Err(e) => {
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(("sound preview error", path)), e));
                    }
                }
            }
        });
        if let Some(e) = ui.ctx().data(|d| d.get_temp::<String>(egui::Id::new(("sound preview error", path)))) {
            ui.label(RichText::new(e).small().color(theme::BAD));
        }
    }

    /// Whether this sound file is being played as a preview.
    pub fn sound_playing(&mut self, path: &str) -> bool {
        self.audio.tidy();
        self.sound.as_ref().is_some_and(|(p, id)| p == path && self.audio.playing(*id))
    }

    /// Plays a sound file as a preview, or stops it when it's the one playing.
    pub fn toggle_sound(&mut self, vfs: &gex_assets::Vfs, path: &str) {
        if self.sound_playing(path) {
            self.stop_sound();
            return;
        }
        self.stop_sound();
        if let Ok(id) = self.audio.play(vfs, path, 1.0, 1.0, false) {
            if id != 0 {
                self.sound = Some((path.to_string(), id));
            }
        }
    }

    pub fn stop_sound(&mut self) {
        if let Some((_, id)) = self.sound.take() {
            self.audio.stop(id);
        }
    }
}
