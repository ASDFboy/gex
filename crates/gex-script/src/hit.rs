//! Is a point on an object? For the mouse events of 2D objects (on_mouse_enter,
//! on_clicked...) and POINT_ON. A Sprite2D counts by its picture (turned, scaled, its
//! pivot and frame), and with `mouse_precise` only where the picture isn't see-through.

use std::collections::HashMap;
use std::rc::Rc;

use gex_core::glam::Vec2;
use gex_core::render::Region;
use gex_core::{ObjectId, ObjectKind};

use crate::world::WorldState;

/// The see-through-ness of a picture, for precise hits.
pub struct AlphaMask {
    pub w: u32,
    pub h: u32,
    pub a: Vec<u8>,
}

/// Pictures looked at so far (by path): their size, and their alpha once a precise hit needs it.
#[derive(Default)]
pub struct Images {
    sizes: HashMap<String, Option<[u32; 2]>>,
    masks: HashMap<String, Option<Rc<AlphaMask>>>,
}

impl Images {
    pub fn size(&mut self, vfs: &gex_assets::Vfs, path: &str) -> Option<[u32; 2]> {
        if path.is_empty() {
            return None;
        }
        *self.sizes.entry(path.to_string()).or_insert_with(|| gex_assets::image_size(vfs, path).ok())
    }

    pub fn mask(&mut self, vfs: &gex_assets::Vfs, path: &str) -> Option<Rc<AlphaMask>> {
        if path.is_empty() {
            return None;
        }
        self.masks
            .entry(path.to_string())
            .or_insert_with(|| {
                gex_assets::load_image(vfs, path).ok().map(|i| Rc::new(AlphaMask { w: i.width, h: i.height, a: i.pixels.chunks_exact(4).map(|p| p[3]).collect() }))
            })
            .clone()
    }
}

/// The texture pixel rectangle a region shows: x, y, w, h.
fn region_px(r: Region, tex: [u32; 2]) -> [f32; 4] {
    let (tw, th) = (tex[0].max(1) as f32, tex[1].max(1) as f32);
    match r {
        Region::Whole => [0.0, 0.0, tw, th],
        Region::Frame { h, v, frame } => {
            let f = frame % (h * v).max(1);
            let (fw, fh) = (tw / h as f32, th / v as f32);
            [(f % h) as f32 * fw, (f / h) as f32 * fh, fw, fh]
        }
        Region::Tile { w, h, id } => {
            let cols = (tw / w.max(1.0)).floor().max(1.0) as u32;
            [(id % cols) as f32 * w, (id / cols) as f32 * h, w, h]
        }
        Region::Rect { x, y, w, h } => [x, y, w, h],
    }
}

/// Whether a picture is solid at a spot (u, v from 0 to 1 across what's shown).
fn solid_at(mask: &AlphaMask, region: Region, u: f32, v: f32) -> bool {
    let r = region_px(region, [mask.w, mask.h]);
    let x = (r[0] + u * r[2]).floor() as i64;
    let y = (r[1] + v * r[3]).floor() as i64;
    if x < 0 || y < 0 || x >= mask.w as i64 || y >= mask.h as i64 {
        return false;
    }
    mask.a[(y as u32 * mask.w + x as u32) as usize] > 0
}

/// Whether a 2D world point is on an object. `precise`: only a Sprite2D's visible pixels.
pub fn point_on(st: &mut WorldState, id: ObjectId, p: Vec2, precise: bool) -> bool {
    let Some(n) = st.tree.get(id) else { return false };
    if !st.tree.visible_in_tree(id) {
        return false;
    }
    let m = st.tree.global_matrix(id);
    let mut m2 = m;
    m2.w_axis.z = 0.0;
    let local = m2.inverse().transform_point3(p.extend(0.0)).truncate();
    let w = n.num("width", 32.0) as f32;
    let h = n.num("height", 32.0) as f32;
    match n.kind {
        ObjectKind::Sprite2D => {
            let pivot = gex_core::render::pivot_of(n);
            let uv = (local + pivot * Vec2::new(w, h)) / Vec2::new(w.max(1e-6), h.max(1e-6));
            if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 {
                return false;
            }
            if !precise {
                return true;
            }
            let mut u = uv.x;
            let mut v = uv.y;
            if n.prop("flip_h").is_some_and(|f| f.as_bool()) {
                u = 1.0 - u;
            }
            if n.prop("flip_v").is_some_and(|f| f.as_bool()) {
                v = 1.0 - v;
            }
            let (hf, vf) = (n.num("hframes", 1.0).max(1.0) as u32, n.num("vframes", 1.0).max(1.0) as u32);
            let region = if hf * vf > 1 { Region::Frame { h: hf, v: vf, frame: n.num("frame", 0.0).max(0.0) as u32 } } else { Region::Whole };
            let tex = n.text("texture").to_string();
            if tex.is_empty() {
                return true;
            }
            let vfs = &st.vfs;
            match st.images.mask(vfs, &tex) {
                Some(mask) => solid_at(&mask, region, u.min(0.9999), v.min(0.9999)),
                None => true,
            }
        }
        ObjectKind::Collider2D => local.x.abs() <= w / 2.0 && local.y.abs() <= h / 2.0,
        ObjectKind::Body2D => {
            if matches!(n.text("shape"), "circle" | "ball") {
                local.length() <= n.num("radius", 16.0) as f32
            } else {
                local.x.abs() <= w / 2.0 && local.y.abs() <= h / 2.0
            }
        }
        ObjectKind::Label2D => {
            let size = n.num("size", 16.0) as f32;
            let lines = n.text("text").lines().count().max(1) as f32;
            let longest = n.text("text").lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
            let (bw, bh) = (longest * size * 0.55, lines * size * 1.2);
            let a = gex_core::render::label_anchor(n);
            let x0 = -a.x * bw;
            let y0 = -a.y * bh;
            local.x >= x0 && local.y >= y0 && local.x <= x0 + bw && local.y <= y0 + bh
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions() {
        assert_eq!(region_px(Region::Frame { h: 2, v: 1, frame: 1 }, [64, 32]), [32.0, 0.0, 32.0, 32.0]);
        let m = AlphaMask { w: 2, h: 1, a: vec![0, 255] };
        assert!(!solid_at(&m, Region::Whole, 0.2, 0.5));
        assert!(solid_at(&m, Region::Whole, 0.7, 0.5));
    }
}
