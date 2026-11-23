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
/// Text measured so far too (TEXT_SIZE, clicking labels).
#[derive(Default)]
pub struct Images {
    sizes: std::cell::RefCell<HashMap<String, Option<[u32; 2]>>>,
    /// Pictures' own sprite settings (`<picture>.sprite`), when they have them.
    metas: std::cell::RefCell<HashMap<String, Option<Rc<gex_core::render::PictureSettings>>>>,
    /// Whether the project has any (looked at once).
    any_meta: std::cell::Cell<Option<bool>>,
    masks: HashMap<String, Option<Rc<AlphaMask>>>,
    fonts: gex_assets::text::Fonts,
    texts: HashMap<(String, u32, String, u32), [f32; 2]>,
}

impl Images {
    pub fn size(&self, vfs: &gex_assets::Vfs, path: &str) -> Option<[u32; 2]> {
        if path.is_empty() {
            return None;
        }
        *self.sizes.borrow_mut().entry(path.to_string()).or_insert_with(|| gex_assets::image_size(vfs, path).ok())
    }

    /// Whether any picture of the project has its own sprite settings (looked at once).
    pub fn any_meta(&self, vfs: &gex_assets::Vfs) -> bool {
        self.any_meta.get().unwrap_or_else(|| {
            let a = vfs.has_picture_settings();
            self.any_meta.set(Some(a));
            a
        })
    }

    /// A picture's own sprite settings and its size, when it has settings.
    pub fn meta(&self, vfs: &gex_assets::Vfs, path: &str) -> Option<(Rc<gex_core::render::PictureSettings>, [u32; 2])> {
        if path.is_empty() || path.starts_with('@') {
            return None;
        }
        if !self.any_meta(vfs) {
            return None;
        }
        let m = self
            .metas
            .borrow_mut()
            .entry(path.to_string())
            .or_insert_with(|| vfs.read_text(&gex_core::render::PictureSettings::file_for(path)).ok().and_then(|t| gex_core::render::PictureSettings::parse(&t)).map(Rc::new))
            .clone()?;
        Some((m, self.size(vfs, path)?))
    }

    /// Measured again next time (the file changed).
    pub fn forget(&mut self, path: &str) {
        self.sizes.get_mut().remove(path);
        self.metas.get_mut().remove(path);
        self.metas.get_mut().remove(path.strip_suffix(".sprite").unwrap_or(path));
        if path.ends_with(".sprite") {
            self.any_meta.set(None);
        }
        self.masks.remove(path);
        if self.texts.keys().any(|k| k.2 == path) {
            self.texts.clear();
            self.fonts = Default::default();
        }
    }

    /// How big text is in game pixels, laid out exactly as a Label2D draws it (size, font,
    /// wrap_width; style tags like [size=..] included).
    pub fn text_size(&mut self, vfs: &gex_assets::Vfs, text: &str, size: f32, font: &str, wrap: f32) -> [f32; 2] {
        let key = (text.to_string(), size.to_bits(), font.to_string(), wrap.to_bits());
        if let Some(s) = self.texts.get(&key) {
            return *s;
        }
        if self.texts.len() > 4096 {
            self.texts.clear();
        }
        let style = gex_assets::text::Style { font, size, scale: 1.0, color: [255; 4], align: 0.0, wrap, outline: 0.0, outline_color: [0; 4] };
        let s = gex_assets::text::measure(&mut self.fonts, vfs, text, &style, &mut Vec::new());
        self.texts.insert(key, s);
        s
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
            // A picture's own settings (origin, mask) come first.
            let meta = st.images.meta(&st.vfs, n.text("texture"));
            let pivot = meta.as_ref().map(|(p, size)| p.pivot(*size)).unwrap_or_else(|| gex_core::render::pivot_of(n));
            let uv = (local + pivot * Vec2::new(w, h)) / Vec2::new(w.max(1e-6), h.max(1e-6));
            if uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 {
                return false;
            }
            let mask = meta.as_ref().map(|(p, _)| p.mask.as_str()).unwrap_or("");
            if mask == "ellipse" {
                let d = uv * 2.0 - Vec2::ONE;
                return d.length_squared() <= 1.0;
            }
            if !(precise || mask == "precise") {
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
            let mut region = if hf * vf > 1 { Region::Frame { h: hf, v: vf, frame: n.num("frame", 0.0).max(0.0) as u32 } } else { Region::Whole };
            let tex = n.text("texture").to_string();
            if tex.is_empty() {
                return true;
            }
            let vfs = &st.vfs;
            if let Some((p, _)) = st.images.meta(vfs, &tex) {
                let (h, v) = (p.hframes.max(1), p.vframes.max(1));
                region = if h * v > 1 { Region::Frame { h, v, frame: n.num("frame", 0.0).max(0.0) as u32 % p.count().max(1) } } else { Region::Whole };
            }
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
            let (text, size, font, wrap) = (n.text("text").to_string(), n.num("size", 16.0) as f32, n.text("font").to_string(), n.num("wrap_width", 0.0) as f32);
            let a = gex_core::render::label_anchor(n);
            let vfs = &st.vfs;
            let [bw, bh] = st.images.text_size(vfs, &text, size, &font, wrap);
            let x0 = -a.x * bw;
            let y0 = -a.y * bh;
            local.x >= x0 && local.y >= y0 && local.x <= x0 + bw && local.y <= y0 + bh
        }
        _ => false,
    }
}

/// An object's box in its own space (before its transform): what OBJECTS_IN looks at.
fn local_box(st: &mut WorldState, id: ObjectId) -> Option<(Vec2, Vec2)> {
    let n = st.tree.get(id)?;
    let w = n.num("width", 32.0) as f32;
    let h = n.num("height", 32.0) as f32;
    match n.kind {
        ObjectKind::Sprite2D => {
            let pivot = st.images.meta(&st.vfs, n.text("texture")).map(|(p, size)| p.pivot(size)).unwrap_or_else(|| gex_core::render::pivot_of(n));
            let p = pivot * Vec2::new(w, h);
            Some((-p, Vec2::new(w, h) - p))
        }
        ObjectKind::Collider2D => Some((Vec2::new(-w / 2.0, -h / 2.0), Vec2::new(w / 2.0, h / 2.0))),
        ObjectKind::Body2D => {
            if matches!(n.text("shape"), "circle" | "ball") {
                let r = n.num("radius", 16.0) as f32;
                Some((Vec2::splat(-r), Vec2::splat(r)))
            } else {
                Some((Vec2::new(-w / 2.0, -h / 2.0), Vec2::new(w / 2.0, h / 2.0)))
            }
        }
        ObjectKind::Label2D => {
            let (text, size, font, wrap) = (n.text("text").to_string(), n.num("size", 16.0) as f32, n.text("font").to_string(), n.num("wrap_width", 0.0) as f32);
            let a = gex_core::render::label_anchor(n);
            let vfs = &st.vfs;
            let b = Vec2::from(st.images.text_size(vfs, &text, size, &font, wrap));
            Some((-a * b, (Vec2::ONE - a) * b))
        }
        _ => None,
    }
}

/// The 2D objects that can be found by place (sprites, labels, colliders, bodies), the one
/// drawn on top first (higher layer, then later in the tree).
fn by_place(st: &WorldState) -> Vec<ObjectId> {
    let ids = st.tree.of_kinds(&[ObjectKind::Sprite2D, ObjectKind::Label2D, ObjectKind::Collider2D, ObjectKind::Body2D]);
    let mut v: Vec<(f64, usize, ObjectId)> = ids.iter().enumerate().map(|(i, id)| (st.tree.objects[*id].num("layer", 0.0), i, *id)).collect();
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    v.into_iter().map(|x| x.2).collect()
}

/// The objects at a 2D world point, top first (OBJECTS_AT).
pub fn objects_at(st: &mut WorldState, p: Vec2, precise: bool) -> Vec<ObjectId> {
    by_place(st).into_iter().filter(|id| point_on(st, *id, p, precise)).collect()
}

/// The objects whose box overlaps a 2D world rectangle, top first (OBJECTS_IN).
pub fn objects_in(st: &mut WorldState, lo: Vec2, hi: Vec2) -> Vec<ObjectId> {
    let mut out = Vec::new();
    for id in by_place(st) {
        if !st.tree.visible_in_tree(id) {
            continue;
        }
        let Some((a, b)) = local_box(st, id) else { continue };
        let m = st.tree.global_matrix(id);
        let corners = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)].map(|c| m.transform_point3(c.extend(0.0)).truncate());
        let (mn, mx) = corners.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(mn, mx), c| (mn.min(*c), mx.max(*c)));
        if mn.x <= hi.x && mx.x >= lo.x && mn.y <= hi.y && mx.y >= lo.y {
            out.push(id);
        }
    }
    out
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
