//! Which files GEX reads, by extension, and the 3D model formats beyond glTF, OBJ and
//! Source: FBX (binary), COLLADA (.dae), 3D Studio (.3ds), STL, PLY and MagicaVoxel (.vox).
//! Every loader gives `ModelData` in metres with y up; models come without animation
//! (glTF has skins and animations).

use std::collections::HashMap;
use std::sync::Arc;

use glam::{EulerRot, Mat3, Mat4, Quat, Vec2, Vec3};

use crate::image_load::Rgba;
use crate::model::{ModelData, Part, Texture};
use crate::vfs::Vfs;

/// Pictures: anything here can be a sprite, a texture or an icon.
pub const IMAGES: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "gif", "bmp", "tga", "webp", "tif", "tiff", "ico", "qoi", "dds", "hdr", "exr", "pbm", "pgm", "ppm", "pnm", "pam", "ff", "svg", "psd", "vtf", "vmt",
];
/// 3D models (Mesh3D's mesh).
pub const MODELS: &[&str] = &["glb", "gltf", "obj", "fbx", "dae", "3ds", "stl", "ply", "vox", "mdl", "bsp"];
/// Sounds and music (Sound objects, PLAY_SOUND, MUSIC).
pub const AUDIO: &[&str] = &[
    "wav", "wave", "mp3", "mp2", "mp1", "ogg", "oga", "flac", "aif", "aiff", "aifc", "caf", "m4a", "m4b", "mp4", "aac", "alac", "mka", "mkv", "webm", "opus", "mod", "xm", "s3m", "it",
];
/// Tracker music: played by the module player, not the audio decoder.
pub const TRACKER: &[&str] = &["mod", "xm", "s3m", "it"];

/// The file's extension, lower case (without a #fragment or ?query).
pub fn extension(path: &str) -> String {
    let base = crate::model::map_base(path);
    let name = base.rsplit('/').next().unwrap_or(base);
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

fn has(list: &[&str], path: &str) -> bool {
    let e = extension(path);
    !e.is_empty() && list.contains(&e.as_str())
}

pub fn is_image(path: &str) -> bool {
    has(IMAGES, path)
}

pub fn is_model(path: &str) -> bool {
    has(MODELS, path)
}

pub fn is_audio(path: &str) -> bool {
    has(AUDIO, path)
}

pub fn is_tracker(path: &str) -> bool {
    has(TRACKER, path)
}

/// Loads the formats this module handles (None: not one of them).
pub fn load(vfs: &Vfs, path: &str) -> Option<Result<ModelData, String>> {
    let r = match extension(path).as_str() {
        "fbx" => vfs.read(path).and_then(|b| fbx(&b, &dir_of(path))),
        "dae" => vfs.read(path).and_then(|b| dae(&text_of(&b), &dir_of(path))),
        "3ds" => vfs.read(path).and_then(|b| three_ds(&b, &dir_of(path))),
        "stl" => vfs.read(path).and_then(|b| stl(&b)),
        "ply" => vfs.read(path).and_then(|b| ply(&b)),
        "vox" => vfs.read(path).and_then(|b| vox(&b, path)),
        _ => return None,
    };
    Some(r.map_err(|e| format!("{}: {}", path, e)))
}

/// A text file in UTF-8 or (with its byte-order mark) UTF-16.
fn text_of(b: &[u8]) -> String {
    let utf16 = |be: bool| {
        let units: Vec<u16> = b[2..].chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    match b {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(b).into_owned(),
    }
}

fn dir_of(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// A texture file named inside a model: relative to the model's folder (only its file
/// name, if the full path came from another computer).
fn texture_path(dir: &str, file: &str) -> Texture {
    let f = file.replace('\\', "/");
    let f = f.trim_start_matches("file://").trim_start_matches("./");
    let rel = if f.contains(':') || f.starts_with('/') { f.rsplit('/').next().unwrap_or(f) } else { f };
    Texture::Path(if dir.is_empty() { rel.to_string() } else { format!("{}/{}", dir, rel) })
}

/// Z-up files (3D printing, 3ds Max, MagicaVoxel) to GEX's y up.
fn z_up(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y)
}

/// Collects triangles into one part per material.
#[derive(Default)]
struct Builder {
    parts: Vec<Part>,
    by_material: HashMap<String, usize>,
    missing_normals: Vec<bool>,
}

impl Builder {
    fn part(&mut self, material: &str, color: [f32; 4], texture: Option<Texture>) -> usize {
        if let Some(&i) = self.by_material.get(material) {
            return i;
        }
        self.parts.push(Part { color, texture, ..Default::default() });
        self.missing_normals.push(false);
        self.by_material.insert(material.to_string(), self.parts.len() - 1);
        self.parts.len() - 1
    }

    fn vertex(&mut self, part: usize, p: Vec3, n: Option<Vec3>, uv: Option<Vec2>) {
        let pt = &mut self.parts[part];
        pt.indices.push(pt.positions.len() as u32);
        pt.positions.push(p);
        match n {
            Some(n) => pt.normals.push(n.normalize_or_zero()),
            None => self.missing_normals[part] = true,
        }
        pt.uvs.push(uv.unwrap_or(Vec2::ZERO));
    }

    fn finish(mut self) -> ModelData {
        for (p, missing) in self.parts.iter_mut().zip(&self.missing_normals) {
            if *missing {
                // Worked out from the triangles when the model loads.
                p.normals.clear();
            }
        }
        self.parts.retain(|p| !p.indices.is_empty());
        ModelData { parts: self.parts, ..Default::default() }
    }
}

// ---------------------------------------------------------------- STL

fn stl(bytes: &[u8]) -> Result<ModelData, String> {
    let mut b = Builder::default();
    let part = b.part("", [1.0; 4], None);
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    if bytes.starts_with(b"solid") && head.contains("facet") {
        // Text: "facet normal x y z / outer loop / vertex x y z ×3 / endloop / endfacet".
        let text = String::from_utf8_lossy(bytes);
        let mut normal = None;
        let mut corners = Vec::new();
        for line in text.lines() {
            let w: Vec<&str> = line.split_whitespace().collect();
            let v = |i: usize| -> Option<Vec3> { Some(Vec3::new(w.get(i)?.parse().ok()?, w.get(i + 1)?.parse().ok()?, w.get(i + 2)?.parse().ok()?)) };
            match w.first().copied() {
                Some("facet") => {
                    normal = v(2).map(z_up).filter(|n| n.length_squared() > 0.0);
                    corners.clear();
                }
                Some("vertex") => corners.extend(v(1).map(z_up)),
                Some("endfacet") => {
                    for i in 1..corners.len().saturating_sub(1) {
                        for c in [corners[0], corners[i], corners[i + 1]] {
                            b.vertex(part, c, normal, None);
                        }
                    }
                }
                _ => {}
            }
        }
    } else {
        // Binary: an 80-byte header, a count, then 50 bytes a triangle (only whole ones are read).
        if bytes.len() < 84 {
            return Err("cut short".into());
        }
        let n = (u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize).min((bytes.len() - 84) / 50);
        let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        for t in 0..n {
            let o = 84 + t * 50;
            let v = |k: usize| Vec3::new(f(o + k * 12), f(o + k * 12 + 4), f(o + k * 12 + 8));
            let normal = Some(z_up(v(0))).filter(|n| n.length_squared() > 0.0 && n.is_finite());
            for k in [1, 2, 3] {
                b.vertex(part, z_up(v(k)), normal, None);
            }
        }
    }
    // z_up mirrors one axis: put the winding back.
    for p in &mut b.parts {
        for t in p.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
    }
    Ok(b.finish())
}

// ---------------------------------------------------------------- PLY

#[derive(Clone, Copy)]
enum PlyType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl PlyType {
    fn parse(s: &str) -> Option<PlyType> {
        Some(match s {
            "char" | "int8" => PlyType::I8,
            "uchar" | "uint8" => PlyType::U8,
            "short" | "int16" => PlyType::I16,
            "ushort" | "uint16" => PlyType::U16,
            "int" | "int32" => PlyType::I32,
            "uint" | "uint32" => PlyType::U32,
            "float" | "float32" => PlyType::F32,
            "double" | "float64" => PlyType::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            PlyType::I8 | PlyType::U8 => 1,
            PlyType::I16 | PlyType::U16 => 2,
            PlyType::I32 | PlyType::U32 | PlyType::F32 => 4,
            PlyType::F64 => 8,
        }
    }
    fn read(self, d: &[u8], big: bool) -> f64 {
        macro_rules! num {
            ($t:ty) => {{
                let a = d[..std::mem::size_of::<$t>()].try_into().unwrap();
                (if big { <$t>::from_be_bytes(a) } else { <$t>::from_le_bytes(a) }) as f64
            }};
        }
        match self {
            PlyType::I8 => d[0] as i8 as f64,
            PlyType::U8 => d[0] as f64,
            PlyType::I16 => num!(i16),
            PlyType::U16 => num!(u16),
            PlyType::I32 => num!(i32),
            PlyType::U32 => num!(u32),
            PlyType::F32 => num!(f32),
            PlyType::F64 => num!(f64),
        }
    }
}

struct PlyProp {
    name: String,
    /// Lists: the type of the count.
    count: Option<PlyType>,
    kind: PlyType,
}

/// A row: each property's numbers (one, or a list's).
type PlyRow = Vec<Vec<f64>>;

fn ply(bytes: &[u8]) -> Result<ModelData, String> {
    let end = bytes.windows(10).position(|w| w == b"end_header").ok_or("no end_header")?;
    let mut body = end + 10;
    while body < bytes.len() && (bytes[body] == b'\r' || bytes[body] == b'\n') {
        body += 1;
        // Binary data starts right after the line break.
        if bytes[body - 1] == b'\n' {
            break;
        }
    }
    let header = String::from_utf8_lossy(&bytes[..end]);
    let mut format = "ascii".to_string();
    let mut elements: Vec<(String, usize, Vec<PlyProp>)> = Vec::new();
    for line in header.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["format", f, ..] => format = f.to_string(),
            ["element", name, n, ..] => elements.push((name.to_string(), n.parse().unwrap_or(0), Vec::new())),
            ["property", "list", c, t, name, ..] => {
                if let (Some(e), Some(c), Some(t)) = (elements.last_mut(), PlyType::parse(c), PlyType::parse(t)) {
                    e.2.push(PlyProp { name: name.to_string(), count: Some(c), kind: t });
                }
            }
            ["property", t, name, ..] => {
                if let (Some(e), Some(t)) = (elements.last_mut(), PlyType::parse(t)) {
                    e.2.push(PlyProp { name: name.to_string(), count: None, kind: t });
                }
            }
            // Comments, obj_info and anything else a writer added.
            _ => {}
        }
    }
    let mut data: HashMap<String, (Vec<String>, Vec<PlyRow>)> = HashMap::new();
    if format == "ascii" {
        let text = String::from_utf8_lossy(&bytes[body..]);
        let mut lines = text.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("comment"));
        for (name, n, props) in &elements {
            let mut rows = Vec::with_capacity((*n).min(1 << 22));
            for _ in 0..*n {
                let Some(line) = lines.next() else { break };
                let mut t = line.split_whitespace().filter_map(|x| x.parse::<f64>().ok());
                let row: PlyRow = props
                    .iter()
                    .map(|p| match p.count {
                        // A list the line doesn't have is empty (some writers declare one anyway).
                        Some(_) => {
                            let k = t.next().unwrap_or(0.0) as usize;
                            (0..k).filter_map(|_| t.next()).collect()
                        }
                        None => vec![t.next().unwrap_or(0.0)],
                    })
                    .collect();
                rows.push(row);
            }
            data.insert(name.clone(), (props.iter().map(|p| p.name.clone()).collect(), rows));
        }
        // Faces without a face element (a hand-edited header): "n i j k" lines after the vertices.
        if !data.contains_key("face") {
            let faces: Vec<PlyRow> = lines
                .map(|l| l.split_whitespace().filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<_>>())
                .filter(|v| v.first().is_some_and(|&n| n >= 3.0 && v.len() == n as usize + 1))
                .map(|v| vec![v[1..].to_vec()])
                .collect();
            data.insert("face".into(), (vec!["vertex_indices".into()], faces));
        }
    } else {
        let big = format.starts_with("binary_big");
        let mut at = body;
        let take = |at: &mut usize, t: PlyType| -> Result<f64, String> {
            let d = bytes.get(*at..*at + t.size()).ok_or("cut short")?;
            *at += t.size();
            Ok(t.read(d, big))
        };
        for (name, n, props) in &elements {
            let mut rows = Vec::with_capacity((*n).min(1 << 22));
            for _ in 0..*n {
                let mut row = Vec::with_capacity(props.len());
                for p in props {
                    row.push(match p.count {
                        Some(c) => {
                            let k = take(&mut at, c)? as usize;
                            (0..k).map(|_| take(&mut at, p.kind)).collect::<Result<_, _>>()?
                        }
                        None => vec![take(&mut at, p.kind)?],
                    });
                }
                rows.push(row);
            }
            data.insert(name.clone(), (props.iter().map(|p| p.name.clone()).collect(), rows));
        }
    }
    let (vnames, verts) = data.get("vertex").ok_or("no vertices")?;
    let col = |names: &[&str]| names.iter().find_map(|n| vnames.iter().position(|v| v == n));
    let get = |row: &PlyRow, i: Option<usize>| i.and_then(|i| row.get(i)).and_then(|v| v.first()).map(|&x| x as f32);
    let (x, y, z) = (col(&["x"]), col(&["y"]), col(&["z"]));
    let (nx, ny, nz) = (col(&["nx"]), col(&["ny"]), col(&["nz"]));
    let (u, v) = (col(&["u", "s", "texture_u"]), col(&["v", "t", "texture_v"]));
    let (r, g, bl) = (col(&["red", "r"]), col(&["green", "g"]), col(&["blue", "b"]));
    let mut part = Part { color: [1.0; 4], ..Default::default() };
    let mut sum = [0.0f32; 3];
    let mut colored = 0;
    for row in verts {
        part.positions.push(Vec3::new(get(row, x).unwrap_or(0.0), get(row, y).unwrap_or(0.0), get(row, z).unwrap_or(0.0)));
        if let (Some(a), Some(b2), Some(c)) = (get(row, nx), get(row, ny), get(row, nz)) {
            part.normals.push(Vec3::new(a, b2, c));
        }
        if let (Some(a), Some(b2)) = (get(row, u), get(row, v)) {
            part.uvs.push(Vec2::new(a, 1.0 - b2));
        }
        if let (Some(a), Some(b2), Some(c)) = (get(row, r), get(row, g), get(row, bl)) {
            // 0-255, or 0-1 when stored as floats.
            let k = if a > 1.0 || b2 > 1.0 || c > 1.0 { 255.0 } else { 1.0 };
            sum = [sum[0] + a / k, sum[1] + b2 / k, sum[2] + c / k];
            colored += 1;
        }
    }
    if colored > 0 {
        // Per-vertex colours (scans and the like) average into the part's colour.
        part.color = [sum[0] / colored as f32, sum[1] / colored as f32, sum[2] / colored as f32, 1.0];
    }
    let n = part.positions.len() as u32;
    if let Some((fnames, faces)) = data.get("face") {
        let li = fnames.iter().position(|f| f == "vertex_indices" || f == "vertex_index").unwrap_or(0);
        for f in faces {
            let l: Vec<u32> = f.get(li).map(|l| l.iter().map(|&i| i as u32).collect()).unwrap_or_default();
            if l.iter().any(|&i| i >= n) {
                continue;
            }
            for i in 1..l.len().saturating_sub(1) {
                part.indices.extend_from_slice(&[l[0], l[i], l[i + 1]]);
            }
        }
    }
    if part.normals.len() != part.positions.len() {
        part.normals.clear();
    }
    if part.uvs.len() != part.positions.len() {
        part.uvs.clear();
    }
    Ok(ModelData { parts: vec![part], ..Default::default() })
}

// ---------------------------------------------------------------- MagicaVoxel

/// Voxels are this many metres across.
const VOXEL: f32 = 0.1;

fn vox(bytes: &[u8], path: &str) -> Result<ModelData, String> {
    let data = dot_vox::load_bytes(bytes).map_err(|e| e.to_string())?;
    if data.models.is_empty() {
        return Err("no models in it".into());
    }
    // Where each model goes: the file's scene (translations), or just the first model.
    let mut placed: Vec<(usize, Vec3)> = Vec::new();
    fn walk(data: &dot_vox::DotVoxData, node: u32, at: Vec3, out: &mut Vec<(usize, Vec3)>, depth: u32) {
        if depth > 64 {
            return;
        }
        match data.scenes.get(node as usize) {
            Some(dot_vox::SceneNode::Transform { frames, child, .. }) => {
                let t = frames.first().and_then(|f| f.attributes.get("_t")).map(|t| {
                    let v: Vec<f32> = t.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    Vec3::new(*v.first().unwrap_or(&0.0), *v.get(1).unwrap_or(&0.0), *v.get(2).unwrap_or(&0.0))
                });
                walk(data, *child, at + t.unwrap_or(Vec3::ZERO), out, depth + 1);
            }
            Some(dot_vox::SceneNode::Group { children, .. }) => {
                for c in children {
                    walk(data, *c, at, out, depth + 1);
                }
            }
            Some(dot_vox::SceneNode::Shape { models, .. }) => {
                for m in models {
                    out.push((m.model_id as usize, at));
                }
            }
            None => {}
        }
    }
    if !data.scenes.is_empty() {
        walk(&data, 0, Vec3::ZERO, &mut placed, 0);
    }
    if placed.is_empty() {
        let s = data.models[0].size;
        placed.push((0, Vec3::new(s.x as f32, s.y as f32, s.z as f32) * 0.5));
    }
    // The palette as a 256×1 texture; each face samples its colour's texel.
    let mut pal = Vec::with_capacity(256 * 4);
    for i in 0..256 {
        let c = data.palette.get(i).copied().unwrap_or(dot_vox::Color { r: 255, g: 0, b: 255, a: 255 });
        pal.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }
    let image = Arc::new(Rgba { width: 256, height: 1, pixels: pal });
    let mut part = Part { color: [1.0; 4], texture: Some(Texture::Embedded { key: format!("{}#palette", path), image }), ..Default::default() };
    const FACES: [([i32; 3], [[f32; 3]; 4]); 6] = [
        ([1, 0, 0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
        ([-1, 0, 0], [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]]),
        ([0, 1, 0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([0, -1, 0], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0, 0, 1], [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
        ([0, 0, -1], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
    ];
    for (mi, at) in placed {
        let Some(m) = data.models.get(mi) else { continue };
        let filled: std::collections::HashSet<(i32, i32, i32)> = m.voxels.iter().map(|v| (v.x as i32, v.y as i32, v.z as i32)).collect();
        // MagicaVoxel places a model by its middle.
        let centre = Vec3::new((m.size.x / 2) as f32, (m.size.y / 2) as f32, (m.size.z / 2) as f32);
        for v in &m.voxels {
            let (x, y, z) = (v.x as i32, v.y as i32, v.z as i32);
            let uv = Vec2::new((v.i as f32 + 0.5) / 256.0, 0.5);
            for (n, corners) in FACES {
                if filled.contains(&(x + n[0], y + n[1], z + n[2])) {
                    continue;
                }
                let base = part.positions.len() as u32;
                for c in corners {
                    let p = Vec3::new(x as f32 + c[0], y as f32 + c[1], z as f32 + c[2]) - centre + at;
                    part.positions.push(z_up(p) * VOXEL);
                    part.normals.push(z_up(Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32)));
                    part.uvs.push(uv);
                }
                part.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
    // z_up flips one axis: put the winding back.
    for t in part.indices.chunks_exact_mut(3) {
        t.swap(1, 2);
    }
    Ok(ModelData { parts: vec![part], ..Default::default() })
}

// ---------------------------------------------------------------- 3D Studio (.3ds)

fn three_ds(data: &[u8], dir: &str) -> Result<ModelData, String> {
    struct Mat {
        color: [f32; 4],
        texture: Option<String>,
    }
    struct Obj {
        verts: Vec<Vec3>,
        uvs: Vec<Vec2>,
        faces: Vec<[u16; 3]>,
        face_mats: Vec<(String, Vec<u16>)>,
    }
    fn u16le(d: &[u8], at: usize) -> Option<u16> {
        d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32le(d: &[u8], at: usize) -> Option<u32> {
        d.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32le(d: &[u8], at: usize) -> Option<f32> {
        u32le(d, at).map(f32::from_bits)
    }
    fn cstr(d: &[u8], at: usize) -> (String, usize) {
        let end = d[at.min(d.len())..].iter().position(|&c| c == 0).map(|p| at + p).unwrap_or(d.len());
        (String::from_utf8_lossy(&d[at.min(end)..end]).into_owned(), end + 1)
    }
    /// The chunks in d[from..to]: (id, start of its data, end).
    fn chunks(d: &[u8], from: usize, to: usize) -> Vec<(u16, usize, usize)> {
        let mut out = Vec::new();
        let mut at = from;
        while at + 6 <= to {
            let (Some(id), Some(len)) = (u16le(d, at), u32le(d, at + 2)) else { break };
            let end = at + len as usize;
            if len < 6 || end > to {
                break;
            }
            out.push((id, at + 6, end));
            at = end;
        }
        out
    }
    fn color(d: &[u8], from: usize, to: usize) -> Option<[f32; 4]> {
        for (id, s, _) in chunks(d, from, to) {
            match id {
                0x0011 | 0x0012 => return d.get(s..s + 3).map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]),
                0x0010 | 0x0013 => return Some([f32le(d, s)?, f32le(d, s + 4)?, f32le(d, s + 8)?, 1.0]),
                _ => {}
            }
        }
        None
    }
    if u16le(data, 0) != Some(0x4D4D) {
        return Err("not a 3D Studio file".into());
    }
    let mut mats: HashMap<String, Mat> = HashMap::new();
    let mut objs: Vec<Obj> = Vec::new();
    let main_end = (u32le(data, 2).unwrap_or(0) as usize).min(data.len());
    for (id, s, e) in chunks(data, 6, main_end) {
        if id != 0x3D3D {
            continue;
        }
        for (id, s, e) in chunks(data, s, e) {
            match id {
                0xAFFF => {
                    let mut name = String::new();
                    let mut m = Mat { color: [1.0; 4], texture: None };
                    for (id, s, e) in chunks(data, s, e) {
                        match id {
                            0xA000 => name = cstr(data, s).0,
                            0xA020 => m.color = color(data, s, e).unwrap_or(m.color),
                            0xA200 => {
                                for (id, s, _) in chunks(data, s, e) {
                                    if id == 0xA300 {
                                        m.texture = Some(cstr(data, s).0);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    mats.insert(name, m);
                }
                0x4000 => {
                    let (_, after) = cstr(data, s);
                    for (id, s, e) in chunks(data, after, e) {
                        if id != 0x4100 {
                            continue;
                        }
                        let mut o = Obj { verts: Vec::new(), uvs: Vec::new(), faces: Vec::new(), face_mats: Vec::new() };
                        for (id, s, e) in chunks(data, s, e) {
                            match id {
                                0x4110 => {
                                    let n = u16le(data, s).unwrap_or(0) as usize;
                                    for i in 0..n {
                                        let a = s + 2 + i * 12;
                                        if let (Some(x), Some(y), Some(z)) = (f32le(data, a), f32le(data, a + 4), f32le(data, a + 8)) {
                                            o.verts.push(Vec3::new(x, y, z));
                                        }
                                    }
                                }
                                0x4140 => {
                                    let n = u16le(data, s).unwrap_or(0) as usize;
                                    for i in 0..n {
                                        let a = s + 2 + i * 8;
                                        if let (Some(u), Some(v)) = (f32le(data, a), f32le(data, a + 4)) {
                                            o.uvs.push(Vec2::new(u, 1.0 - v));
                                        }
                                    }
                                }
                                0x4120 => {
                                    let n = u16le(data, s).unwrap_or(0) as usize;
                                    for i in 0..n {
                                        let a = s + 2 + i * 8;
                                        if let (Some(x), Some(y), Some(z)) = (u16le(data, a), u16le(data, a + 2), u16le(data, a + 4)) {
                                            o.faces.push([x, y, z]);
                                        }
                                    }
                                    for (id, s, _) in chunks(data, s + 2 + n * 8, e) {
                                        if id == 0x4130 {
                                            let (name, at) = cstr(data, s);
                                            let k = u16le(data, at).unwrap_or(0) as usize;
                                            let list = (0..k).filter_map(|i| u16le(data, at + 2 + i * 2)).collect();
                                            o.face_mats.push((name, list));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        objs.push(o);
                    }
                }
                _ => {}
            }
        }
    }
    // 3ds files are z up, and in whatever unit the artist used: kept as they are.
    let mut b = Builder::default();
    for o in &objs {
        let mut mat_of = vec![String::new(); o.faces.len()];
        for (name, list) in &o.face_mats {
            for &f in list {
                if let Some(m) = mat_of.get_mut(f as usize) {
                    *m = name.clone();
                }
            }
        }
        for (fi, f) in o.faces.iter().enumerate() {
            let name = &mat_of[fi];
            let part = match mats.get(name) {
                Some(m) => b.part(name, m.color, m.texture.as_deref().map(|t| texture_path(dir, t))),
                None => b.part(name, [1.0; 4], None),
            };
            // z_up mirrors: reverse the winding to keep faces facing out.
            for &i in &[f[0], f[2], f[1]] {
                let i = i as usize;
                let Some(&v) = o.verts.get(i) else { continue };
                b.vertex(part, z_up(v), None, o.uvs.get(i).copied());
            }
        }
    }
    Ok(b.finish())
}

// ---------------------------------------------------------------- COLLADA (.dae)

fn dae(text: &str, dir: &str) -> Result<ModelData, String> {
    use roxmltree::{Document, Node};
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let child = xml_child;
    let id_of = |url: &str| url.trim_start_matches('#').to_string();
    let by_id: HashMap<&str, Node> = doc.descendants().filter_map(|n| n.attribute("id").map(|i| (i, n))).collect();
    let floats = dae_floats;
    let ints = |s: &str| -> Vec<usize> { s.split_whitespace().filter_map(|x| x.parse().ok()).collect() };

    let asset = child(root, "asset");
    let up_z = asset.and_then(|a| child(a, "up_axis")).and_then(|u| u.text()).is_some_and(|t| t.trim() == "Z_UP");
    let unit = asset.and_then(|a| child(a, "unit")).and_then(|u| u.attribute("meter")).and_then(|m| m.parse::<f32>().ok()).unwrap_or(1.0);
    let fix = Mat4::from_scale(Vec3::splat(unit)) * if up_z { Mat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]) } else { Mat4::IDENTITY };

    // A material id: its colour and texture, through its effect.
    let material = |id: &str| -> ([f32; 4], Option<Texture>) {
        let Some(m) = by_id.get(id) else { return ([1.0; 4], None) };
        let Some(effect) = child(*m, "instance_effect").and_then(|i| i.attribute("url")).and_then(|u| by_id.get(id_of(u).as_str())) else { return ([1.0; 4], None) };
        let Some(diffuse) = effect.descendants().find(|n| n.has_tag_name("diffuse")) else { return ([1.0; 4], None) };
        let color = child(diffuse, "color").and_then(|c| c.text()).map(floats).filter(|v| v.len() >= 3).map(|v| [v[0], v[1], v[2], *v.get(3).unwrap_or(&1.0)]);
        let texture = child(diffuse, "texture").and_then(|t| t.attribute("texture")).and_then(|sampler| {
            // sampler → surface → image, or straight to an image (some exporters).
            let param = |sid: &str| effect.descendants().find(|n| n.has_tag_name("newparam") && n.attribute("sid") == Some(sid));
            let image_id = match param(sampler) {
                Some(p) => {
                    let surface = p.descendants().find(|n| n.has_tag_name("source")).and_then(|s| s.text()).map(str::trim);
                    let from_surface = surface.and_then(param).and_then(|s| s.descendants().find(|n| n.has_tag_name("init_from"))).and_then(|i| i.text()).map(|t| t.trim().to_string());
                    // COLLADA 1.5: the sampler names the image itself.
                    from_surface.or_else(|| p.descendants().find(|n| n.has_tag_name("instance_image")).and_then(|i| i.attribute("url")).map(id_of))
                }
                None => Some(sampler.to_string()),
            }?;
            let img = by_id.get(image_id.as_str())?;
            let file = img.descendants().find(|n| n.has_tag_name("init_from")).and_then(|i| i.descendants().find(|n| n.is_text()).and_then(|t| t.text()).or(i.text()))?;
            let file = file.trim();
            let file = percent_decode(file);
            Some(texture_path(dir, &file))
        });
        (if texture.is_some() { color.unwrap_or([1.0; 4]) } else { color.unwrap_or([0.8, 0.8, 0.8, 1.0]) }, texture)
    };

    // A source's values, grouped by its accessor's stride.
    let source = |id: &str| -> Option<(Vec<f32>, usize)> {
        let s = by_id.get(id)?;
        let vals = floats(s.descendants().find(|n| n.has_tag_name("float_array"))?.text().unwrap_or(""));
        let stride = s.descendants().find(|n| n.has_tag_name("accessor")).and_then(|a| a.attribute("stride")).and_then(|x| x.parse().ok()).unwrap_or(3);
        Some((vals, stride))
    };

    let mut b = Builder::default();
    let mut add_geometry = |geom: &str, world: Mat4, bind: &HashMap<String, String>| -> Result<(), String> {
        let Some(g) = by_id.get(geom) else { return Ok(()) };
        let Some(mesh) = child(*g, "mesh") else { return Ok(()) };
        let normal_m = Mat3::from_mat4(world).inverse().transpose();
        // <vertices> may carry positions and normals.
        let mut vert_inputs: Vec<(String, String)> = Vec::new();
        if let Some(v) = child(mesh, "vertices") {
            for i in v.children().filter(|c| c.has_tag_name("input")) {
                vert_inputs.push((i.attribute("semantic").unwrap_or("").to_string(), id_of(i.attribute("source").unwrap_or(""))));
            }
        }
        for prim in mesh.children().filter(|c| matches!(c.tag_name().name(), "triangles" | "polylist" | "polygons" | "tristrips" | "trifans")) {
            let symbol = prim.attribute("material").unwrap_or("");
            let mat_id = bind.get(symbol).cloned().unwrap_or_else(|| symbol.to_string());
            let (color, texture) = material(&mat_id);
            let part = b.part(&mat_id, color, texture);
            let inputs: Vec<(String, String, usize, u32)> = prim
                .children()
                .filter(|c| c.has_tag_name("input"))
                .map(|i| (i.attribute("semantic").unwrap_or("").to_string(), id_of(i.attribute("source").unwrap_or("")), i.attribute("offset").and_then(|o| o.parse().ok()).unwrap_or(0), i.attribute("set").and_then(|o| o.parse().ok()).unwrap_or(0)))
                .collect();
            let stride = inputs.iter().map(|i| i.2).max().unwrap_or(0) + 1;
            let find = |sem: &str| inputs.iter().filter(|i| i.0 == sem).min_by_key(|i| i.3);
            let vertex_in = find("VERTEX").ok_or("a primitive without vertices")?;
            let pos_src = vert_inputs.iter().find(|v| v.0 == "POSITION").map(|v| v.1.clone()).ok_or("no positions")?;
            let positions = source(&pos_src).ok_or("missing position source")?;
            let normals_in = find("NORMAL").map(|n| (n.2, source(&n.1)));
            let normals_v = vert_inputs.iter().find(|v| v.0 == "NORMAL").and_then(|v| source(&v.1));
            let uv_in = find("TEXCOORD").map(|t| (t.2, source(&t.1)));
            // Polygons: the vertex counts (triangles: all 3).
            let mut idx: Vec<usize> = Vec::new();
            let mut counts: Vec<usize> = Vec::new();
            match prim.tag_name().name() {
                // Strips and fans become separate triangles (corners repeated).
                kind @ ("tristrips" | "trifans") => {
                    for p in prim.children().filter(|c| c.has_tag_name("p")) {
                        let v = ints(p.text().unwrap_or(""));
                        let n = v.len() / stride;
                        let corner = |i: usize| &v[i * stride..i * stride + stride];
                        for i in 2..n {
                            let tri = if kind == "trifans" { [0, i - 1, i] } else if i % 2 == 0 { [i - 2, i - 1, i] } else { [i - 1, i - 2, i] };
                            for c in tri {
                                idx.extend_from_slice(corner(c));
                            }
                            counts.push(3);
                        }
                    }
                }
                "polygons" => {
                    for p in prim.children().filter(|c| c.has_tag_name("p")) {
                        let v = ints(p.text().unwrap_or(""));
                        counts.push(v.len() / stride);
                        idx.extend(v);
                    }
                }
                name => {
                    idx = ints(child(prim, "p").and_then(|p| p.text()).unwrap_or(""));
                    counts = if name == "polylist" { ints(child(prim, "vcount").and_then(|p| p.text()).unwrap_or("")) } else { vec![3; idx.len() / (stride * 3)] };
                }
            }
            let get = |src: &(Vec<f32>, usize), i: usize, n: usize| -> Option<Vec<f32>> { src.0.get(i * src.1..i * src.1 + n).map(|s| s.to_vec()) };
            let mut at = 0;
            for c in counts {
                let corner = |k: usize| -> Option<(Vec3, Option<Vec3>, Option<Vec2>)> {
                    let base = (at + k) * stride;
                    let vi = *idx.get(base + vertex_in.2)?;
                    let p = get(&positions, vi, 3)?;
                    let n = match &normals_in {
                        Some((off, Some(src))) => idx.get(base + off).and_then(|&ni| get(src, ni, 3)),
                        _ => normals_v.as_ref().and_then(|src| get(src, vi, 3)),
                    };
                    let uv = match &uv_in {
                        Some((off, Some(src))) => idx.get(base + off).and_then(|&ti| get(src, ti, 2)),
                        _ => None,
                    };
                    Some((
                        world.transform_point3(Vec3::new(p[0], p[1], p[2])),
                        n.map(|n| normal_m * Vec3::new(n[0], n[1], n[2])),
                        uv.map(|t| Vec2::new(t[0], 1.0 - t[1])),
                    ))
                };
                for k in 1..c.saturating_sub(1) {
                    for j in [0, k, k + 1] {
                        if let Some((p, n, uv)) = corner(j) {
                            b.vertex(part, p, n, uv);
                        }
                    }
                }
                at += c;
            }
        }
        Ok(())
    };

    // The scene: nodes with their transforms; without one, every geometry as it is.
    fn node_matrix(n: Node<'_, '_>) -> Mat4 {
        let floats = dae_floats;
        let mut m = Mat4::IDENTITY;
        for c in n.children().filter(|c| c.is_element()) {
            let v = floats(c.text().unwrap_or(""));
            m *= match (c.tag_name().name(), v.len()) {
                ("matrix", 16) => Mat4::from_cols_array(&v.try_into().unwrap()).transpose(),
                ("translate", 3) => Mat4::from_translation(Vec3::new(v[0], v[1], v[2])),
                ("rotate", 4) => Mat4::from_axis_angle(Vec3::new(v[0], v[1], v[2]).normalize_or(Vec3::Y), v[3].to_radians()),
                ("scale", 3) => Mat4::from_scale(Vec3::new(v[0], v[1], v[2])),
                _ => Mat4::IDENTITY,
            };
        }
        m
    }
    let mut stack: Vec<(Node, Mat4, u32)> = Vec::new();
    let scene = child(root, "scene").and_then(|s| child(s, "instance_visual_scene")).and_then(|i| i.attribute("url")).and_then(|u| by_id.get(id_of(u).as_str()).copied()).or_else(|| doc.descendants().find(|n| n.has_tag_name("visual_scene")));
    match scene {
        Some(s) => {
            for n in s.children().filter(|c| c.has_tag_name("node")) {
                stack.push((n, fix, 0));
            }
            while let Some((n, parent, depth)) = stack.pop() {
                if depth > 64 {
                    continue;
                }
                let m = parent * node_matrix(n);
                for ig in n.children().filter(|c| c.has_tag_name("instance_geometry") || c.has_tag_name("instance_controller")) {
                    let bind: HashMap<String, String> = ig.descendants().filter(|c| c.has_tag_name("instance_material")).map(|im| (im.attribute("symbol").unwrap_or("").to_string(), id_of(im.attribute("target").unwrap_or("")))).collect();
                    let mut geom = id_of(ig.attribute("url").unwrap_or(""));
                    // A skinned mesh: drawn in its rest pose (no animation from COLLADA).
                    if ig.has_tag_name("instance_controller") {
                        let skin = by_id.get(geom.as_str()).and_then(|c| c.descendants().find(|d| d.has_tag_name("skin") || d.has_tag_name("morph")));
                        match skin.and_then(|s| s.attribute("source")) {
                            Some(src) => geom = id_of(src),
                            None => continue,
                        }
                    }
                    add_geometry(&geom, m, &bind)?;
                }
                for c in n.children().filter(|c| c.has_tag_name("node")) {
                    stack.push((c, m, depth + 1));
                }
                for inst in n.children().filter(|c| c.has_tag_name("instance_node")) {
                    if let Some(t) = inst.attribute("url").and_then(|u| by_id.get(id_of(u).as_str())) {
                        stack.push((*t, m, depth + 1));
                    }
                }
            }
        }
        None => {
            for g in doc.descendants().filter(|n| n.has_tag_name("geometry")) {
                if let Some(id) = g.attribute("id") {
                    add_geometry(id, fix, &HashMap::new())?;
                }
            }
        }
    }
    Ok(b.finish())
}

/// COLLADA numbers (some exporters write a decimal comma).
fn dae_floats(s: &str) -> Vec<f32> {
    s.split_whitespace().filter_map(|x| x.replace(',', ".").parse().ok()).collect()
}

fn xml_child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.has_tag_name(name))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------- FBX (binary)

#[derive(Debug, Clone)]
enum Prop {
    Int(i64),
    Float(f64),
    Str(String),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
    Bytes(Vec<u8>),
}

impl Prop {
    fn int(&self) -> Option<i64> {
        match self {
            Prop::Int(v) => Some(*v),
            Prop::Float(v) => Some(*v as i64),
            _ => None,
        }
    }
    fn float(&self) -> Option<f64> {
        match self {
            Prop::Float(v) => Some(*v),
            Prop::Int(v) => Some(*v as f64),
            _ => None,
        }
    }
    fn str(&self) -> &str {
        match self {
            Prop::Str(s) => s,
            _ => "",
        }
    }
}

#[derive(Debug, Default)]
struct Node {
    name: String,
    props: Vec<Prop>,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// A Properties70 entry's values (after its name and three type words).
    fn p70(&self, key: &str) -> Option<&[Prop]> {
        // Properties70 (FBX 7): name, three type words, values; Properties60 (FBX 6): name, two.
        for (list, entry, skip) in [("Properties70", "P", 4), ("Properties60", "Property", 3)] {
            if let Some(p) = self.child(list).and_then(|l| l.children.iter().find(|p| p.name == entry && p.props.first().map(|x| x.str()) == Some(key))) {
                return Some(p.props.get(skip..).unwrap_or(&[]));
            }
        }
        None
    }
    fn p70_vec(&self, key: &str) -> Option<Vec3> {
        let v = self.p70(key)?;
        Some(Vec3::new(v.first()?.float()? as f32, v.get(1)?.float()? as f32, v.get(2)?.float()? as f32))
    }
    fn floats(&self, child: &str) -> Vec<f64> {
        match self.child(child).and_then(|c| c.props.first()) {
            Some(Prop::Floats(v)) => v.clone(),
            Some(Prop::Ints(v)) => v.iter().map(|&x| x as f64).collect(),
            _ => Vec::new(),
        }
    }
    fn ints(&self, child: &str) -> Vec<i64> {
        match self.child(child).and_then(|c| c.props.first()) {
            Some(Prop::Ints(v)) => v.clone(),
            Some(Prop::Floats(v)) => v.iter().map(|&x| x as i64).collect(),
            _ => Vec::new(),
        }
    }
    fn text(&self, child: &str) -> String {
        self.child(child).and_then(|c| c.props.first()).map(|p| p.str().to_string()).unwrap_or_default()
    }
}

struct FbxReader<'a> {
    d: &'a [u8],
    wide: bool,
}

impl FbxReader<'_> {
    fn num(&self, at: usize, wide: bool) -> Result<(u64, usize), String> {
        if wide {
            let b = self.d.get(at..at + 8).ok_or("cut short")?;
            Ok((u64::from_le_bytes(b.try_into().unwrap()), at + 8))
        } else {
            let b = self.d.get(at..at + 4).ok_or("cut short")?;
            Ok((u32::from_le_bytes(b.try_into().unwrap()) as u64, at + 4))
        }
    }

    /// The node at `at`: it and where the next one starts (None for the empty end marker).
    fn node(&self, at: usize, depth: u32) -> Result<(Option<Node>, usize), String> {
        if depth > 64 {
            return Err("nested too deep".into());
        }
        let (end, p) = self.num(at, self.wide)?;
        let (count, p) = self.num(p, self.wide)?;
        let (_list_len, p) = self.num(p, self.wide)?;
        let name_len = *self.d.get(p).ok_or("cut short")? as usize;
        let p = p + 1;
        if end == 0 {
            return Ok((None, p));
        }
        let end = end as usize;
        if end > self.d.len() || end <= at {
            return Err("a broken node".into());
        }
        let name = String::from_utf8_lossy(self.d.get(p..p + name_len).ok_or("cut short")?).into_owned();
        let mut p = p + name_len;
        let mut node = Node { name, ..Default::default() };
        for _ in 0..count {
            let (prop, next) = self.prop(p)?;
            node.props.push(prop);
            p = next;
        }
        while p < end {
            let (child, next) = self.node(p, depth + 1)?;
            match child {
                Some(c) => node.children.push(c),
                None => break,
            }
            p = next;
        }
        Ok((Some(node), end))
    }

    fn prop(&self, at: usize) -> Result<(Prop, usize), String> {
        let d = self.d;
        let t = *d.get(at).ok_or("cut short")?;
        let p = at + 1;
        let take = |n: usize| d.get(p..p + n).ok_or_else(|| "cut short".to_string());
        Ok(match t {
            b'Y' => (Prop::Int(i16::from_le_bytes(take(2)?.try_into().unwrap()) as i64), p + 2),
            b'C' => (Prop::Int(take(1)?[0] as i64), p + 1),
            b'I' => (Prop::Int(i32::from_le_bytes(take(4)?.try_into().unwrap()) as i64), p + 4),
            b'F' => (Prop::Float(f32::from_le_bytes(take(4)?.try_into().unwrap()) as f64), p + 4),
            b'D' => (Prop::Float(f64::from_le_bytes(take(8)?.try_into().unwrap())), p + 8),
            b'L' => (Prop::Int(i64::from_le_bytes(take(8)?.try_into().unwrap())), p + 8),
            b'S' | b'R' => {
                let n = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
                let s = d.get(p + 4..p + 4 + n).ok_or("cut short")?;
                let prop = if t == b'S' { Prop::Str(String::from_utf8_lossy(s).into_owned()) } else { Prop::Bytes(s.to_vec()) };
                (prop, p + 4 + n)
            }
            b'f' | b'd' | b'l' | b'i' | b'b' | b'c' => {
                let h = take(12)?;
                let count = u32::from_le_bytes(h[0..4].try_into().unwrap()) as usize;
                let enc = u32::from_le_bytes(h[4..8].try_into().unwrap());
                let len = u32::from_le_bytes(h[8..12].try_into().unwrap()) as usize;
                let raw = d.get(p + 12..p + 12 + len).ok_or("cut short")?;
                let bytes = if enc == 1 { miniz_oxide::inflate::decompress_to_vec_zlib(raw).map_err(|_| "a compressed array that won't unpack")? } else { raw.to_vec() };
                let size = match t {
                    b'f' | b'i' => 4,
                    b'd' | b'l' => 8,
                    _ => 1,
                };
                if bytes.len() < count * size {
                    return Err("an array shorter than it says".into());
                }
                let prop = match t {
                    b'f' => Prop::Floats(bytes.chunks_exact(4).take(count).map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64).collect()),
                    b'd' => Prop::Floats(bytes.chunks_exact(8).take(count).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()),
                    b'i' => Prop::Ints(bytes.chunks_exact(4).take(count).map(|c| i32::from_le_bytes(c.try_into().unwrap()) as i64).collect()),
                    b'l' => Prop::Ints(bytes.chunks_exact(8).take(count).map(|c| i64::from_le_bytes(c.try_into().unwrap())).collect()),
                    _ => Prop::Ints(bytes.iter().take(count).map(|&c| c as i64).collect()),
                };
                (prop, p + 12 + len)
            }
            other => return Err(format!("an unknown value type {:?}", other as char)),
        })
    }
}

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for v in s.bytes().filter_map(val) {
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// The text form of FBX: `Name: value, value { children }`, arrays as `*count { a: ... }`.
struct FbxText<'a> {
    s: &'a [u8],
    at: usize,
}

impl FbxText<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }
    /// Skips spaces, and line breaks and ;comments too when `lines`.
    fn skip(&mut self, lines: bool) {
        while let Some(c) = self.peek() {
            match c {
                b' ' | b'\t' | b'\r' => self.at += 1,
                b'\n' if lines => self.at += 1,
                b';' if lines => {
                    while self.peek().is_some_and(|c| c != b'\n') {
                        self.at += 1;
                    }
                }
                _ => break,
            }
        }
    }
    fn nodes(&mut self, depth: u32) -> Result<Vec<Node>, String> {
        if depth > 64 {
            return Err("nested too deep".into());
        }
        let mut out = Vec::new();
        loop {
            self.skip(true);
            match self.peek() {
                None => return Ok(out),
                Some(b'}') => {
                    self.at += 1;
                    return Ok(out);
                }
                _ => {}
            }
            let start = self.at;
            while self.peek().is_some_and(|c| c != b':' && c != b'\n' && c != b'{' && c != b'}') {
                self.at += 1;
            }
            if self.peek() != Some(b':') {
                // Not a node: skip the line.
                self.at += 1;
                continue;
            }
            let name = String::from_utf8_lossy(&self.s[start..self.at]).trim().to_string();
            self.at += 1;
            let mut node = Node { name, ..Default::default() };
            loop {
                self.skip(false);
                match self.peek() {
                    Some(b'{') | Some(b'\n') | Some(b'}') | Some(b';') | None => break,
                    _ => {}
                }
                node.props.push(self.value()?);
                self.skip(false);
                if self.peek() == Some(b',') {
                    self.at += 1;
                    self.skip(true);
                } else {
                    break;
                }
            }
            self.skip(false);
            if self.peek() == Some(b'{') {
                self.at += 1;
                let kids = self.nodes(depth + 1)?;
                // An array: "*count { a: values }".
                if matches!(node.props.first(), Some(Prop::Str(s)) if s.starts_with('*')) {
                    let vals = kids.into_iter().find(|k| k.name == "a").map(|a| a.props).unwrap_or_default();
                    let all_int = vals.iter().all(|v| matches!(v, Prop::Int(_)));
                    node.props = vec![if all_int { Prop::Ints(vals.iter().filter_map(|v| v.int()).collect()) } else { Prop::Floats(vals.iter().filter_map(|v| v.float()).collect()) }];
                } else {
                    node.children = kids;
                }
            }
            out.push(node);
        }
    }
    fn value(&mut self) -> Result<Prop, String> {
        if self.peek() == Some(b'"') {
            self.at += 1;
            let start = self.at;
            while self.peek().is_some_and(|c| c != b'"') {
                self.at += 1;
            }
            let v = String::from_utf8_lossy(&self.s[start..self.at]).into_owned();
            self.at += 1;
            return Ok(Prop::Str(v));
        }
        let start = self.at;
        while self.peek().is_some_and(|c| !matches!(c, b',' | b'\n' | b'\r' | b'{' | b'}' | b' ' | b'\t')) {
            self.at += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.at]).unwrap_or("");
        Ok(if let Ok(i) = t.parse::<i64>() {
            Prop::Int(i)
        } else if let Ok(f) = t.parse::<f64>() {
            Prop::Float(f)
        } else {
            Prop::Str(t.to_string())
        })
    }
}

/// A layer element's value for polygon-vertex `pv` (of control point `cp`, polygon `poly`).
fn layer<const N: usize>(el: &Node, values: &str, index: &str, pv: usize, cp: usize, poly: usize) -> Option<[f32; N]> {
    let vals = el.floats(values);
    let mapping = el.text("MappingInformationType");
    let reference = el.text("ReferenceInformationType");
    let i = match mapping.as_str() {
        "ByPolygonVertex" => pv,
        "ByVertex" | "ByVertice" => cp,
        "ByPolygon" => poly,
        "AllSame" => 0,
        _ => pv,
    };
    let i = if reference == "IndexToDirect" || reference == "Index" { *el.ints(index).get(i)? as usize } else { i };
    let s = vals.get(i * N..i * N + N)?;
    let mut out = [0.0; N];
    for (o, v) in out.iter_mut().zip(s) {
        *o = *v as f32;
    }
    Some(out)
}

fn fbx(data: &[u8], dir: &str) -> Result<ModelData, String> {
    let mut top = Node::default();
    if data.starts_with(b"Kaydara FBX Binary") {
        let version = u32::from_le_bytes(data.get(23..27).ok_or("cut short")?.try_into().unwrap());
        let r = FbxReader { d: data, wide: version >= 7500 };
        let mut p = 27;
        while p < data.len() {
            match r.node(p, 0)? {
                (Some(n), next) => {
                    top.children.push(n);
                    p = next;
                }
                (None, _) => break,
            }
        }
    } else {
        // The text form.
        let text = String::from_utf8_lossy(data);
        if !text.contains("Objects") {
            return Err("not an FBX file".into());
        }
        let mut a = FbxText { s: text.as_bytes(), at: 0 };
        top.children = a.nodes(0)?;
        if top.child("Objects").and_then(|o| o.children.first()).and_then(|n| n.props.first()).is_some_and(|p| p.int().is_none()) {
            return Err("this is an old FBX (version 6): export it again as FBX 2011 or newer".into());
        }
    }
    // Axes and units.
    let settings = top.child("GlobalSettings");
    let gi = |k: &str, d: i64| settings.and_then(|s| s.p70(k)).and_then(|v| v.first()).and_then(|x| x.int()).unwrap_or(d);
    let unit = settings.and_then(|s| s.p70("UnitScaleFactor")).and_then(|v| v.first()).and_then(|x| x.float()).unwrap_or(1.0) as f32 / 100.0;
    let axis = |i: i64, sign: i64| {
        let mut v = Vec3::ZERO;
        v[(i.clamp(0, 2)) as usize] = sign.signum() as f32;
        v
    };
    let (up, front, coord) = (axis(gi("UpAxis", 1), gi("UpAxisSign", 1)), axis(gi("FrontAxis", 2), gi("FrontAxisSign", 1)), axis(gi("CoordAxis", 0), gi("CoordAxisSign", 1)));
    // File axes → GEX (x right, y up, z towards the viewer).
    let basis = Mat3::from_cols(coord, up, front).transpose();
    let fix = Mat4::from_mat3(basis) * Mat4::from_scale(Vec3::splat(unit));
    let objects = top.child("Objects").ok_or("no objects")?;
    let by_id: HashMap<i64, &Node> = objects.children.iter().filter_map(|n| Some((n.props.first()?.int()?, n))).collect();
    // child → parents, and parent → children (object-object links; texture → material links).
    let mut parents: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut children: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    if let Some(conns) = top.child("Connections") {
        for c in conns.all("C") {
            let (Some(a), Some(b)) = (c.props.get(1).and_then(|x| x.int()), c.props.get(2).and_then(|x| x.int())) else { continue };
            parents.entry(a).or_default().push(b);
            children.entry(b).or_default().push((a, c.props.get(3).map(|x| x.str().to_string()).unwrap_or_default()));
        }
    }
    let is = |id: i64, kind: &str| by_id.get(&id).is_some_and(|n| n.name == kind);

    fn local(m: &Node) -> Mat4 {
        let t = m.p70_vec("Lcl Translation").unwrap_or(Vec3::ZERO);
        let r = m.p70_vec("Lcl Rotation").unwrap_or(Vec3::ZERO);
        let s = m.p70_vec("Lcl Scaling").unwrap_or(Vec3::ONE);
        let pre = m.p70_vec("PreRotation").unwrap_or(Vec3::ZERO);
        let post = m.p70_vec("PostRotation").unwrap_or(Vec3::ZERO);
        let rot = |v: Vec3| Mat4::from_quat(Quat::from_euler(EulerRot::ZYX, v.z.to_radians(), v.y.to_radians(), v.x.to_radians()));
        let pivot = m.p70_vec("RotationPivot").unwrap_or(Vec3::ZERO);
        let spivot = m.p70_vec("ScalingPivot").unwrap_or(Vec3::ZERO);
        Mat4::from_translation(t + pivot) * rot(pre) * rot(r) * rot(post).inverse() * Mat4::from_translation(-pivot + spivot) * Mat4::from_scale(s) * Mat4::from_translation(-spivot)
    }
    let world = |model: i64| -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut id = model;
        for _ in 0..64 {
            let Some(n) = by_id.get(&id) else { break };
            m = local(n) * m;
            match parents.get(&id).and_then(|ps| ps.iter().find(|p| is(**p, "Model"))) {
                Some(p) => id = *p,
                None => break,
            }
        }
        m
    };
    let material = |id: i64| -> ([f32; 4], Option<Texture>) {
        let Some(m) = by_id.get(&id) else { return ([1.0; 4], None) };
        let c = m.p70_vec("DiffuseColor").or_else(|| m.p70_vec("Diffuse")).unwrap_or(Vec3::ONE);
        let tex = children.get(&id).into_iter().flatten().filter(|(c, _)| is(*c, "Texture")).find(|(_, prop)| prop.is_empty() || prop.contains("Diffuse") || prop.contains("Base")).and_then(|(c, _)| {
            let t = by_id.get(c)?;
            // A picture packed inside the file (its Video's Content), or a file beside it.
            let packed = children.get(c).into_iter().flatten().filter(|(v, _)| is(*v, "Video")).find_map(|(v, _)| {
                let content = by_id.get(v)?.child("Content")?;
                let bytes: Vec<u8> = match content.props.as_slice() {
                    [Prop::Bytes(b)] => b.clone(),
                    // The text form: base64, maybe in several strings.
                    parts => base64(&parts.iter().map(|p| p.str()).collect::<String>()),
                };
                let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                Some(Texture::Embedded { key: format!("{}#fbx{}", dir, v), image: Arc::new(Rgba { width: img.width(), height: img.height(), pixels: img.into_raw() }) })
            });
            packed.or_else(|| {
                let f = Some(t.text("RelativeFilename")).filter(|s| !s.is_empty()).unwrap_or_else(|| t.text("FileName"));
                (!f.is_empty()).then(|| texture_path(dir, &f))
            })
        });
        let c = if tex.is_some() { [1.0; 4] } else { [c.x, c.y, c.z, 1.0] };
        (c, tex)
    };

    let mut b = Builder::default();
    for (&gid, g) in by_id.iter().filter(|(_, n)| n.name == "Geometry") {
        let verts = g.floats("Vertices");
        let poly = g.ints("PolygonVertexIndex");
        if verts.is_empty() || poly.is_empty() {
            continue;
        }
        // Every model that uses this geometry draws it, where that model is.
        let models: Vec<i64> = parents.get(&gid).map(|p| p.iter().copied().filter(|p| is(*p, "Model")).collect()).unwrap_or_default();
        for model in if models.is_empty() { vec![0] } else { models } {
            let m = fix * if model == 0 { Mat4::IDENTITY } else { world(model) } * by_id.get(&model).map_or(Mat4::IDENTITY, |n| {
                let gt = n.p70_vec("GeometricTranslation").unwrap_or(Vec3::ZERO);
                let gr = n.p70_vec("GeometricRotation").unwrap_or(Vec3::ZERO);
                let gs = n.p70_vec("GeometricScaling").unwrap_or(Vec3::ONE);
                Mat4::from_translation(gt) * Mat4::from_quat(Quat::from_euler(EulerRot::ZYX, gr.z.to_radians(), gr.y.to_radians(), gr.x.to_radians())) * Mat4::from_scale(gs)
            });
            let nm = Mat3::from_mat4(m).inverse().transpose();
            let mirrored = Mat3::from_mat4(m).determinant() < 0.0;
            let mats: Vec<i64> = children.get(&model).map(|c| c.iter().filter(|(c, _)| is(*c, "Material")).map(|(c, _)| *c).collect()).unwrap_or_default();
            let normals = g.child("LayerElementNormal");
            let uvs = g.child("LayerElementUV");
            let mat_layer = g.child("LayerElementMaterial");
            let mat_index = |poly_i: usize| -> usize {
                let Some(l) = mat_layer else { return 0 };
                let idx = l.ints("Materials");
                if l.text("MappingInformationType") == "AllSame" { idx.first().copied().unwrap_or(0) as usize } else { idx.get(poly_i).copied().unwrap_or(0) as usize }
            };
            let mut start = 0;
            let mut poly_i = 0;
            for (k, &raw) in poly.iter().enumerate() {
                if raw >= 0 {
                    continue;
                }
                // A polygon: corners start..=k (the last one is stored as -index-1).
                let corner = |j: usize| -> Option<(Vec3, Option<Vec3>, Option<Vec2>)> {
                    let r = poly[j];
                    let cp = if r < 0 { (-r - 1) as usize } else { r as usize };
                    let p = verts.get(cp * 3..cp * 3 + 3)?;
                    let n = normals.and_then(|l| layer::<3>(l, "Normals", "NormalsIndex", j, cp, poly_i));
                    let uv = uvs.and_then(|l| layer::<2>(l, "UV", "UVIndex", j, cp, poly_i));
                    Some((m.transform_point3(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)), n.map(|n| nm * Vec3::from_array(n)), uv.map(|t| Vec2::new(t[0], 1.0 - t[1]))))
                };
                let mi = mats.get(mat_index(poly_i)).copied();
                let key = format!("{:?}", mi);
                let (color, tex) = mi.map(material).unwrap_or(([1.0; 4], None));
                let part = b.part(&key, color, tex);
                for t in 1..(k - start).max(1) {
                    let tri = if mirrored { [start, start + t + 1, start + t] } else { [start, start + t, start + t + 1] };
                    if tri.iter().any(|&j| j > k) {
                        continue;
                    }
                    for j in tri {
                        if let Some((p, n, uv)) = corner(j) {
                            b.vertex(part, p, n, uv);
                        }
                    }
                }
                start = k + 1;
                poly_i += 1;
            }
        }
    }
    Ok(b.finish())
}
