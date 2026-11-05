//! 3D models: glTF (.glb/.gltf, with skins and animations), OBJ, and from a Source game,
//! models (.mdl + .vvd + .dx90.vtx) and maps (.bsp). Everything becomes `ModelData`, in
//! metres with y up. Models are cached by path (they're shared by every thread).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};

use crate::image_load::Rgba;
use crate::vfs::Vfs;

#[derive(Clone, Default)]
pub struct Part {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
    /// Skinned parts: four joints and weights per vertex.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub color: [f32; 4],
    pub texture: Option<Texture>,
}

#[derive(Clone)]
pub enum Texture {
    /// An asset path (image, .vtf or .vmt).
    Path(String),
    /// Pixels that came inside the model file; `key` names them for caching.
    Embedded { key: String, image: Arc<Rgba> },
}

#[derive(Clone)]
pub struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    pub inverse_bind: Mat4,
    pub rest: (Vec3, Quat, Vec3),
}

#[derive(Clone)]
pub enum Track {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}

#[derive(Clone)]
pub struct Channel {
    pub joint: usize,
    pub times: Vec<f32>,
    pub track: Track,
}

#[derive(Clone)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<Channel>,
}

#[derive(Clone, Default)]
pub struct ModelData {
    pub parts: Vec<Part>,
    pub joints: Vec<Joint>,
    /// Transform above the skeleton's roots (from the file's node hierarchy).
    pub skeleton_root: Mat4,
    pub animations: Vec<Animation>,
    /// Source models and maps use the other triangle winding: draw both sides.
    pub two_sided: bool,
    pub min: Vec3,
    pub max: Vec3,
}

impl ModelData {
    pub fn animation(&self, name: &str) -> Option<&Animation> {
        self.animations.iter().find(|a| a.name == name).or_else(|| name.parse::<usize>().ok().and_then(|i| self.animations.get(i)))
    }

    pub fn is_skinned(&self) -> bool {
        !self.joints.is_empty() && self.parts.iter().any(|p| !p.joints.is_empty())
    }

    /// Joint matrices (world-from-bind) for an animation at a time; rest pose without one.
    pub fn pose(&self, anim: Option<&Animation>, t: f32) -> Vec<Mat4> {
        let mut local: Vec<(Vec3, Quat, Vec3)> = self.joints.iter().map(|j| j.rest).collect();
        if let Some(a) = anim {
            let t = if a.duration > 0.0 { t.rem_euclid(a.duration.max(1e-6)).min(a.duration) } else { 0.0 };
            for c in &a.channels {
                let Some(l) = local.get_mut(c.joint) else { continue };
                let (i, f) = sample_index(&c.times, t);
                match &c.track {
                    Track::Translation(v) if !v.is_empty() => l.0 = v[i].lerp(v[(i + 1).min(v.len() - 1)], f),
                    Track::Rotation(v) if !v.is_empty() => l.1 = v[i].slerp(v[(i + 1).min(v.len() - 1)], f),
                    Track::Scale(v) if !v.is_empty() => l.2 = v[i].lerp(v[(i + 1).min(v.len() - 1)], f),
                    _ => {}
                }
            }
        }
        let mut world = vec![Mat4::IDENTITY; self.joints.len()];
        // Joints are stored parents first.
        for (i, j) in self.joints.iter().enumerate() {
            let (t, r, s) = local[i];
            let m = Mat4::from_scale_rotation_translation(s, r, t);
            world[i] = match j.parent {
                Some(p) => world[p] * m,
                None => self.skeleton_root * m,
            };
        }
        world.iter().zip(&self.joints).map(|(w, j)| *w * j.inverse_bind).collect()
    }

    /// A part's vertices posed with the given joint matrices.
    pub fn skin(&self, part: &Part, joints: &[Mat4]) -> (Vec<Vec3>, Vec<Vec3>) {
        if part.joints.is_empty() || joints.is_empty() {
            return (part.positions.clone(), part.normals.clone());
        }
        let mut p = Vec::with_capacity(part.positions.len());
        let mut n = Vec::with_capacity(part.positions.len());
        for (k, pos) in part.positions.iter().enumerate() {
            let (js, ws) = (part.joints[k], part.weights[k]);
            let mut m = Mat4::ZERO;
            for q in 0..4 {
                if ws[q] > 0.0 {
                    if let Some(j) = joints.get(js[q] as usize) {
                        m += *j * ws[q];
                    }
                }
            }
            if m == Mat4::ZERO {
                m = Mat4::IDENTITY;
            }
            p.push(m.transform_point3(*pos));
            n.push(m.transform_vector3(part.normals.get(k).copied().unwrap_or(Vec3::Y)).normalize_or(Vec3::Y));
        }
        (p, n)
    }

    fn finish(mut self) -> ModelData {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in &mut self.parts {
            if p.normals.len() != p.positions.len() {
                p.normals = flat_normals(&p.positions, &p.indices);
            }
            if p.uvs.len() != p.positions.len() {
                p.uvs = vec![Vec2::ZERO; p.positions.len()];
            }
            for v in &p.positions {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        if lo.x > hi.x {
            lo = Vec3::ZERO;
            hi = Vec3::ZERO;
        }
        self.min = lo;
        self.max = hi;
        self
    }

    /// Every triangle in model space (for collision meshes).
    pub fn triangles(&self) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        let mut v = Vec::new();
        let mut t = Vec::new();
        for p in &self.parts {
            let base = v.len() as u32;
            v.extend_from_slice(&p.positions);
            for tri in p.indices.chunks_exact(3) {
                t.push([base + tri[0], base + tri[1], base + tri[2]]);
            }
        }
        (v, t)
    }
}

fn sample_index(times: &[f32], t: f32) -> (usize, f32) {
    if times.len() < 2 || t <= times[0] {
        return (0, 0.0);
    }
    let i = times.partition_point(|x| *x <= t).saturating_sub(1).min(times.len() - 1);
    if i + 1 >= times.len() {
        return (times.len() - 1, 0.0);
    }
    let f = (t - times[i]) / (times[i + 1] - times[i]).max(1e-6);
    (i, f.clamp(0.0, 1.0))
}

fn flat_normals(pos: &[Vec3], idx: &[u32]) -> Vec<Vec3> {
    let mut n = vec![Vec3::ZERO; pos.len()];
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        let f = (b - a).cross(c - a);
        for k in t {
            n[*k as usize] += f;
        }
    }
    n.into_iter().map(|v| v.normalize_or(Vec3::Y)).collect()
}

static CACHE: OnceLock<Mutex<HashMap<String, Arc<ModelData>>>> = OnceLock::new();

/// Loads a model once and shares it.
pub fn cached(vfs: &Vfs, path: &str) -> Result<Arc<ModelData>, String> {
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(m) = cache.lock().unwrap().get(path) {
        return Ok(m.clone());
    }
    let m = Arc::new(load(vfs, path)?);
    cache.lock().unwrap().insert(path.to_string(), m.clone());
    Ok(m)
}

/// Forgets cached models (after files change).
pub fn clear_cache() {
    if let Some(c) = CACHE.get() {
        c.lock().unwrap().clear();
    }
}

pub fn is_model(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    [".glb", ".gltf", ".obj", ".mdl", ".bsp"].iter().any(|e| l.ends_with(e))
}

pub fn load(vfs: &Vfs, path: &str) -> Result<ModelData, String> {
    let l = path.to_ascii_lowercase();
    let m = if l.ends_with(".glb") || l.ends_with(".gltf") {
        load_gltf(vfs, path)
    } else if l.ends_with(".obj") {
        load_obj(vfs, path)
    } else if l.ends_with(".mdl") {
        load_mdl(vfs, path, &mut |p| vfs.read(p))
    } else if l.ends_with(".bsp") {
        load_bsp(vfs, path)
    } else {
        Err(format!("{} isn't a model format GEX reads (.glb, .gltf, .obj, .mdl, .bsp)", path))
    }?;
    Ok(m.finish())
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|p| p.0).unwrap_or("")
}

// ---------------------------------------------------------------- glTF

fn to_rgba(img: &gltf::image::Data) -> Rgba {
    use gltf::image::Format::*;
    let n = (img.width * img.height) as usize;
    let mut out = Vec::with_capacity(n * 4);
    let px = &img.pixels;
    for i in 0..n {
        let c = match img.format {
            R8 => [px[i], px[i], px[i], 255],
            R8G8 => [px[i * 2], px[i * 2 + 1], 0, 255],
            R8G8B8 => [px[i * 3], px[i * 3 + 1], px[i * 3 + 2], 255],
            R8G8B8A8 => [px[i * 4], px[i * 4 + 1], px[i * 4 + 2], px[i * 4 + 3]],
            R16G16B16 => [px[i * 6 + 1], px[i * 6 + 3], px[i * 6 + 5], 255],
            R16G16B16A16 => [px[i * 8 + 1], px[i * 8 + 3], px[i * 8 + 5], px[i * 8 + 7]],
            _ => [255, 0, 255, 255],
        };
        out.extend_from_slice(&c);
    }
    Rgba { width: img.width, height: img.height, pixels: out }
}

fn load_gltf(vfs: &Vfs, path: &str) -> Result<ModelData, String> {
    // Project files can be imported from disk (so a .gltf finds its .bin and images);
    // anything else must be a self-contained .glb.
    let (doc, buffers, images) = match path.strip_prefix("res://") {
        Some(rel) if path.to_ascii_lowercase().ends_with(".gltf") => gltf::import(vfs.project_dir.join(rel)),
        _ => gltf::import_slice(vfs.read(path)?),
    }
    .map_err(|e| format!("{}: {}", path, e))?;
    let images: Vec<Arc<Rgba>> = images.iter().map(|i| Arc::new(to_rgba(i))).collect();
    let mut out = ModelData::default();

    // Node parents and global transforms.
    let n = doc.nodes().len();
    let mut parent = vec![None; n];
    for node in doc.nodes() {
        for c in node.children() {
            parent[c.index()] = Some(node.index());
        }
    }
    let local: Vec<Mat4> = doc.nodes().map(|nd| Mat4::from_cols_array_2d(&nd.transform().matrix())).collect();
    let global = |mut i: usize| {
        let mut m = local[i];
        while let Some(p) = parent[i] {
            m = local[p] * m;
            i = p;
        }
        m
    };

    // One skin (the first) drives the skeleton.
    let mut joint_of_node: HashMap<usize, usize> = HashMap::new();
    if let Some(skin) = doc.skins().next() {
        let reader = skin.reader(|b| Some(&buffers[b.index()]));
        let ibm: Vec<Mat4> = reader.read_inverse_bind_matrices().map(|it| it.map(|m| Mat4::from_cols_array_2d(&m)).collect()).unwrap_or_default();
        let nodes: Vec<usize> = skin.joints().map(|j| j.index()).collect();
        // Parents before children.
        let mut order: Vec<usize> = Vec::new();
        let depth = |mut i: usize| {
            let mut d = 0;
            while let Some(p) = parent[i] {
                d += 1;
                i = p;
            }
            d
        };
        let mut idx: Vec<usize> = (0..nodes.len()).collect();
        idx.sort_by_key(|k| depth(nodes[*k]));
        for k in idx {
            order.push(k);
        }
        for (new_i, k) in order.iter().enumerate() {
            joint_of_node.insert(nodes[*k], new_i);
        }
        for k in &order {
            let node = doc.nodes().nth(nodes[*k]).unwrap();
            let (t, r, s) = node.transform().decomposed();
            let p = parent[nodes[*k]].and_then(|pn| joint_of_node.get(&pn).copied());
            out.joints.push(Joint {
                name: node.name().unwrap_or("").to_string(),
                parent: p,
                inverse_bind: ibm.get(*k).copied().unwrap_or(Mat4::IDENTITY),
                rest: (Vec3::from(t), Quat::from_array(r), Vec3::from(s)),
            });
        }
        // Everything above the root joints.
        if let Some(root) = order.first().map(|k| nodes[*k]) {
            out.skeleton_root = parent[root].map(global).unwrap_or(Mat4::IDENTITY);
        }
    }
    if out.skeleton_root == Mat4::ZERO {
        out.skeleton_root = Mat4::IDENTITY;
    }

    for node in doc.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        let skinned = node.skin().is_some() && !out.joints.is_empty();
        let m = if skinned { Mat4::IDENTITY } else { global(node.index()) };
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let r = prim.reader(|b| Some(&buffers[b.index()]));
            let Some(pos) = r.read_positions() else { continue };
            let positions: Vec<Vec3> = pos.map(|p| m.transform_point3(Vec3::from(p))).collect();
            let normals: Vec<Vec3> = r.read_normals().map(|it| it.map(|p| m.transform_vector3(Vec3::from(p)).normalize_or(Vec3::Y)).collect()).unwrap_or_default();
            let uvs: Vec<Vec2> = r.read_tex_coords(0).map(|t| t.into_f32().map(Vec2::from).collect()).unwrap_or_default();
            let indices: Vec<u32> = r.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..positions.len() as u32).collect());
            let mut joints = Vec::new();
            let mut weights = Vec::new();
            if skinned {
                // Map the skin's joint indices to ours (they're the same skin, reordered).
                let skin = node.skin().unwrap();
                let remap: Vec<u16> = skin.joints().map(|j| *joint_of_node.get(&j.index()).unwrap_or(&0) as u16).collect();
                if let (Some(j), Some(w)) = (r.read_joints(0), r.read_weights(0)) {
                    joints = j.into_u16().map(|a| a.map(|x| remap.get(x as usize).copied().unwrap_or(0))).collect();
                    weights = w.into_f32().collect();
                }
            }
            let mat = prim.material();
            let pbr = mat.pbr_metallic_roughness();
            let texture = pbr.base_color_texture().and_then(|t| {
                let i = t.texture().source().index();
                images.get(i).map(|img| Texture::Embedded { key: format!("{}#{}", path, i), image: img.clone() })
            });
            out.parts.push(Part { positions, normals, uvs, indices, joints, weights, color: pbr.base_color_factor(), texture });
        }
    }

    for (ai, anim) in doc.animations().enumerate() {
        let mut a = Animation { name: anim.name().map(|s| s.to_string()).unwrap_or_else(|| format!("{}", ai)), duration: 0.0, channels: Vec::new() };
        for ch in anim.channels() {
            let Some(&joint) = joint_of_node.get(&ch.target().node().index()) else { continue };
            let r = ch.reader(|b| Some(&buffers[b.index()]));
            let Some(times) = r.read_inputs().map(|t| t.collect::<Vec<f32>>()) else { continue };
            let cubic = ch.sampler().interpolation() == gltf::animation::Interpolation::CubicSpline;
            // Cubic splines store (in-tangent, value, out-tangent): keep the values.
            let pick = |v: Vec<Vec3>| if cubic { v.chunks(3).filter_map(|c| c.get(1).copied()).collect() } else { v };
            let track = match r.read_outputs() {
                Some(gltf::animation::util::ReadOutputs::Translations(t)) => Track::Translation(pick(t.map(Vec3::from).collect())),
                Some(gltf::animation::util::ReadOutputs::Scales(t)) => Track::Scale(pick(t.map(Vec3::from).collect())),
                Some(gltf::animation::util::ReadOutputs::Rotations(t)) => {
                    let q: Vec<Quat> = t.into_f32().map(Quat::from_array).collect();
                    Track::Rotation(if cubic { q.chunks(3).filter_map(|c| c.get(1).copied()).collect() } else { q })
                }
                _ => continue,
            };
            a.duration = a.duration.max(times.last().copied().unwrap_or(0.0));
            a.channels.push(Channel { joint, times, track });
        }
        out.animations.push(a);
    }
    Ok(out)
}

// ---------------------------------------------------------------- OBJ

fn load_obj(vfs: &Vfs, path: &str) -> Result<ModelData, String> {
    let bytes = vfs.read(path)?;
    let dir = dir_of(path).to_string();
    let opts = tobj::LoadOptions { triangulate: true, single_index: true, ..Default::default() };
    let (models, mats) = tobj::load_obj_buf(&mut std::io::Cursor::new(bytes), &opts, |p| {
        let mp = format!("{}/{}", dir, p.to_string_lossy().replace('\\', "/"));
        match vfs.read(&mp) {
            Ok(b) => tobj::load_mtl_buf(&mut std::io::Cursor::new(b)),
            Err(_) => Err(tobj::LoadError::OpenFileFailed),
        }
    })
    .map_err(|e| format!("{}: {}", path, e))?;
    let mats = mats.unwrap_or_default();
    let mut out = ModelData::default();
    for m in models {
        let mesh = m.mesh;
        let positions: Vec<Vec3> = mesh.positions.chunks_exact(3).map(|c| Vec3::new(c[0], c[1], c[2])).collect();
        let normals: Vec<Vec3> = mesh.normals.chunks_exact(3).map(|c| Vec3::new(c[0], c[1], c[2])).collect();
        // OBJ's v runs up; textures run down.
        let uvs: Vec<Vec2> = mesh.texcoords.chunks_exact(2).map(|c| Vec2::new(c[0], 1.0 - c[1])).collect();
        let mat = mesh.material_id.and_then(|i| mats.get(i));
        let color = mat.and_then(|m| m.diffuse).map(|d| [d[0], d[1], d[2], 1.0]).unwrap_or([1.0; 4]);
        let texture = mat.and_then(|m| m.diffuse_texture.clone()).map(|t| Texture::Path(format!("{}/{}", dir, t.replace('\\', "/"))));
        out.parts.push(Part { positions, normals, uvs, indices: mesh.indices, color, texture, ..Default::default() });
    }
    Ok(out)
}

// ---------------------------------------------------------------- Source engine

/// Source units (inches, z up) to metres with y up.
fn src(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y) * 0.0254
}

/// A material name from a Source file as a .vmt path under materials/.
fn material_path(name: &str) -> String {
    let n = name.replace('\\', "/").to_ascii_lowercase();
    let n = n.trim_start_matches('/').trim_start_matches("materials/").trim_end_matches(".vmt");
    format!("materials/{}.vmt", n)
}

/// Finds a material's base texture, following "patch" materials' includes.
fn material_texture(scheme: &str, vmt: &str, read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>) -> Option<String> {
    let mut path = vmt.to_string();
    for _ in 0..4 {
        let text = read(&path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())?;
        let m = crate::vmt::parse(&text);
        if let Some(t) = m.base_texture() {
            return Some(format!("{}://{}", scheme, t));
        }
        let inc = m.get("include")?.replace('\\', "/").to_ascii_lowercase();
        path = format!("{}://{}", scheme, inc);
    }
    None
}

fn load_mdl(vfs: &Vfs, path: &str, read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>) -> Result<ModelData, String> {
    let base = path.strip_suffix(".mdl").or_else(|| path.strip_suffix(".MDL")).unwrap_or(path);
    let mdl = vmdl::Mdl::read(&read(path)?).map_err(|e| format!("{}: {}", path, e))?;
    let vtx_data = read(&format!("{}.dx90.vtx", base)).or_else(|_| read(&format!("{}.vtx", base)))?;
    let vtx = vmdl::Vtx::read(&vtx_data).map_err(|e| format!("{}.vtx: {}", base, e))?;
    let vvd = vmdl::Vvd::read(&read(&format!("{}.vvd", base))?).map_err(|e| format!("{}.vvd: {}", base, e))?;
    let model = vmdl::Model::from_parts(mdl, vtx, vvd);
    let scheme = path.split_once("://").map(|s| s.0).unwrap_or("res");
    let verts = model.vertices();
    let positions: Vec<Vec3> = verts.iter().map(|v| src(Vec3::new(v.position.x, v.position.y, v.position.z))).collect();
    let normals: Vec<Vec3> = verts.iter().map(|v| src(Vec3::new(v.normal.x, v.normal.y, v.normal.z)).normalize_or(Vec3::Y)).collect();
    let uvs: Vec<Vec2> = verts.iter().map(|v| Vec2::from(v.texture_coordinates)).collect();
    let skin = model.skin_tables().next();
    let mut out = ModelData { two_sided: true, ..Default::default() };
    let mut by_mat: HashMap<i32, Vec<u32>> = HashMap::new();
    for mesh in model.meshes() {
        by_mat.entry(mesh.material_index()).or_default().extend(mesh.vertex_strip_indices().flatten().map(|i| i as u32));
    }
    let mut mats: Vec<_> = by_mat.into_iter().collect();
    mats.sort_by_key(|m| m.0);
    for (mat, indices) in mats {
        let texture = skin.as_ref().and_then(|s| s.texture_info(mat)).and_then(|info| {
            for dir in info.search_paths.iter().map(|d| d.as_str()).chain(std::iter::once("")) {
                let vmt = format!("{}://{}", scheme, material_path(&format!("{}{}", dir, info.name)));
                if let Some(t) = material_texture(scheme, &vmt, read) {
                    return Some(Texture::Path(t));
                }
            }
            None
        });
        // Every part shares the model's vertex list; keep only what this part uses.
        let mut remap: HashMap<u32, u32> = HashMap::new();
        let mut p = Part { color: [1.0; 4], texture, ..Default::default() };
        for i in indices {
            let k = *remap.entry(i).or_insert_with(|| {
                p.positions.push(positions.get(i as usize).copied().unwrap_or(Vec3::ZERO));
                p.normals.push(normals.get(i as usize).copied().unwrap_or(Vec3::Y));
                p.uvs.push(uvs.get(i as usize).copied().unwrap_or(Vec2::ZERO));
                (p.positions.len() - 1) as u32
            });
            p.indices.push(k);
        }
        out.parts.push(p);
    }
    let _ = vfs;
    Ok(out)
}

fn load_bsp(vfs: &Vfs, path: &str) -> Result<ModelData, String> {
    let data = vfs.read(path)?;
    let bsp = vbsp::Bsp::read(&data).map_err(|e| format!("{}: {}", path, e))?;
    let scheme = path.split_once("://").map(|s| s.0).unwrap_or("res").to_string();
    // Files inside the map's own pak come first (cubemap-patched materials live there).
    let mut read = |p: &str| -> Result<Vec<u8>, String> {
        let rel = p.split_once("://").map(|s| s.1).unwrap_or(p);
        if let Ok(Some(b)) = bsp.pack.get(rel) {
            return Ok(b);
        }
        vfs.read(p)
    };
    let mut groups: HashMap<String, Part> = HashMap::new();
    let mut tex_cache: HashMap<String, Option<String>> = HashMap::new();
    let world = bsp.models().next().ok_or("the map has no world model")?;
    for face in world.faces() {
        let tex = face.texture();
        let name = tex.name().to_string();
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("tools/") || lower.contains("skybox") || !face.is_visible() {
            continue;
        }
        let (tw, th) = {
            let d = tex.texture_data();
            (d.width.max(1) as f32, d.height.max(1) as f32)
        };
        let part = groups.entry(name.clone()).or_insert_with(|| Part { color: [1.0; 4], ..Default::default() });
        let tris: Vec<[vbsp::Vector; 3]> = match face.displacement() {
            Some(d) => {
                let v: Vec<vbsp::Vector> = d.triangulated_displaced_vertices().collect();
                v.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
            }
            None => face.triangulate().collect(),
        };
        for t in tris {
            let base = part.positions.len() as u32;
            for v in t {
                let uv = tex.uv(v);
                part.positions.push(src(Vec3::new(v.x, v.y, v.z)));
                part.uvs.push(Vec2::new(uv[0] / tw, uv[1] / th));
            }
            part.indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
        tex_cache.entry(name).or_insert(None);
    }
    for (name, t) in tex_cache.iter_mut() {
        let mut candidates = vec![material_path(name)];
        // "maps/<map>/brick/wall_-12_34_56" → "brick/wall"
        let l = name.to_ascii_lowercase().replace('\\', "/");
        if let Some(rest) = l.strip_prefix("maps/") {
            if let Some((_, inner)) = rest.split_once('/') {
                let mut s = inner.to_string();
                for _ in 0..3 {
                    if let Some((head, tail)) = s.rsplit_once('_') {
                        if tail.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) {
                            s = head.to_string();
                        }
                    }
                }
                candidates.push(material_path(&s));
            }
        }
        for c in candidates {
            if let Some(found) = material_texture(&scheme, &format!("{}://{}", scheme, c), &mut read) {
                *t = Some(found);
                break;
            }
        }
    }
    let mut out = ModelData { two_sided: true, ..Default::default() };
    let mut names: Vec<_> = groups.into_iter().collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, mut p) in names {
        p.texture = tex_cache.get(&name).cloned().flatten().map(Texture::Path);
        out.parts.push(p);
    }
    // Static props, placed where the map puts them.
    let mut props: HashMap<String, Option<Arc<ModelData>>> = HashMap::new();
    for sp in bsp.static_props() {
        let model = sp.model().replace('\\', "/").to_ascii_lowercase();
        let m = props
            .entry(model.clone())
            .or_insert_with(|| {
                let p = format!("{}://{}", scheme, model);
                load_mdl(vfs, &p, &mut read).ok().map(|m| Arc::new(m.finish()))
            })
            .clone();
        let Some(m) = m else { continue };
        let o = sp.origin;
        let a = sp.angles;
        // Source: yaw about z, pitch about y, roll about x, in that order.
        let r_src = Quat::from_euler(EulerRot::ZYX, a.yaw.to_radians(), a.pitch.to_radians(), a.roll.to_radians());
        // Into y-up space: the same rotation seen through the axis swap.
        let swap = Mat4::from_cols(glam::Vec4::X, glam::Vec4::new(0.0, 0.0, -1.0, 0.0), glam::Vec4::Y, glam::Vec4::W);
        let rot = swap * Mat4::from_quat(r_src) * swap.inverse();
        let xf = Mat4::from_translation(src(Vec3::new(o.x, o.y, o.z))) * rot;
        for part in &m.parts {
            let mut p = part.clone();
            p.positions = p.positions.iter().map(|v| xf.transform_point3(*v)).collect();
            p.normals = p.normals.iter().map(|v| xf.transform_vector3(*v).normalize_or(Vec3::Y)).collect();
            out.parts.push(p);
        }
    }
    Ok(out)
}

/// A one-triangle glTF with a two-joint skin and a one-second animation ("lift"), built
/// in memory. For tests.
#[doc(hidden)]
pub fn tiny_skinned_glb() -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let f = |b: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes()));
        f(&mut bin, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]); // positions 0..36
        for _ in 0..3 {
            bin.extend_from_slice(&[0u8, 1, 0, 0]); // joints 36..48 (u8)
        }
        f(&mut bin, &[0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]); // weights 48..96: all on joint 1
        let ident = [1.0f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
        f(&mut bin, &ident);
        f(&mut bin, &ident); // inverse binds 96..224
        f(&mut bin, &[0.0, 1.0]); // times 224..232
        f(&mut bin, &[0.0, 0.0, 0.0, 0.0, 5.0, 0.0]); // translations 232..256
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"byteLength":{}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":12}},{{"buffer":0,"byteOffset":48,"byteLength":48}},
              {{"buffer":0,"byteOffset":96,"byteLength":128}},{{"buffer":0,"byteOffset":224,"byteLength":8}},{{"buffer":0,"byteOffset":232,"byteLength":24}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
              {{"bufferView":1,"componentType":5121,"count":3,"type":"VEC4"}},{{"bufferView":2,"componentType":5126,"count":3,"type":"VEC4"}},
              {{"bufferView":3,"componentType":5126,"count":2,"type":"MAT4"}},{{"bufferView":4,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]}},
              {{"bufferView":5,"componentType":5126,"count":2,"type":"VEC3"}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2}}}}]}}],
            "nodes":[{{"mesh":0,"skin":0}},{{"name":"root","children":[2]}},{{"name":"arm"}}],
            "skins":[{{"joints":[1,2],"inverseBindMatrices":3}}],
            "animations":[{{"name":"lift","channels":[{{"sampler":0,"target":{{"node":2,"path":"translation"}}}}],"samplers":[{{"input":4,"output":5}}]}}],
            "scenes":[{{"nodes":[0,1]}}],"scene":0}}"#,
            bin.len()
        );
        let mut j = json.into_bytes();
        while j.len() % 4 != 0 {
            j.push(b' ');
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&((12 + 8 + j.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(j.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&j);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gltf_skin_and_animation() {
        let dir = std::env::temp_dir().join(format!("gex-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t.glb"), tiny_skinned_glb()).unwrap();
        std::fs::write(dir.join("q.obj"), "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n").unwrap();
        let vfs = Vfs::new(&dir, &Default::default());
        let m = load(&vfs, "res://t.glb").unwrap();
        assert_eq!(m.parts.len(), 1);
        assert_eq!(m.joints.len(), 2);
        assert!(m.is_skinned());
        let a = m.animation("lift").unwrap();
        assert!((a.duration - 1.0).abs() < 1e-6);
        let pose = m.pose(Some(a), 0.5);
        let (p, _) = m.skin(&m.parts[0], &pose);
        assert!((p[0].y - 2.5).abs() < 1e-4, "halfway lifted: {:?}", p[0]);
        let rest = m.pose(None, 0.0);
        assert!(m.skin(&m.parts[0], &rest).0[0].y.abs() < 1e-5);
        let o = load(&vfs, "res://q.obj").unwrap();
        assert_eq!(o.parts[0].indices.len(), 6, "the quad is split into two triangles");
        assert!((o.max - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-6);
    }
}
