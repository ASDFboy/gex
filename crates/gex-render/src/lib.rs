#![allow(deprecated)] // glam 0.33 moved the camera helpers; the old names still work.
//! gex-render: draws a `RenderList` into an offscreen texture with wgpu. The editor
//! shows that texture in its viewport; the game player shows it full-window. Labels
//! (text) are left to the caller, who draws them with egui on top.
//!
//! 3D: sky → shadow map (the first directional light) → opaque meshes → sorted
//! transparent meshes → 3D lines. 2D: sprites in layer order, then 2D lines.

pub mod camera;
pub mod mesh;

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use gex_assets::model::{ModelData, Texture as ModelTexture};
use gex_assets::Vfs;
use gex_core::glam::{Mat4, Vec3};
use gex_core::render::{LightKind, RenderList};
use gex_core::NodeId;
use wgpu::util::DeviceExt;

pub use camera::{Cameras, EditorCamera2D, EditorCamera3D};

/// The viewport target holds sRGB-encoded values (the shaders encode), which is what
/// egui expects of the textures it shows.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Image assets: sampled as linear color.
const TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_SIZE: u32 = 2048;
const SHADOW_EXTENT: f32 = 25.0;
const MAX_LIGHTS: usize = 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightU {
    pos: [f32; 4],
    dir: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    vp3: [[f32; 4]; 4],
    vp2: [[f32; 4]; 4],
    inv_vp3: [[f32; 4]; 4],
    light_vp: [[f32; 4]; 4],
    eye: [f32; 4],
    ambient: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    ground: [f32; 4],
    counts: [f32; 4],
    lights: [LightU; MAX_LIGHTS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    /// Texture rectangle: u0, v0, u1, v1 (meshes: offset and repeat).
    uv: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
    pub space: f32,
}

/// Extra lines to draw on top: grids, selection boxes, collider outlines.
#[derive(Default)]
pub struct Overlay {
    pub lines_3d: Vec<LineVertex>,
    pub lines_2d: Vec<LineVertex>,
}

impl Overlay {
    pub fn line3(&mut self, a: Vec3, b: Vec3, c: [f32; 4]) {
        self.lines_3d.push(LineVertex { pos: a.into(), color: c, space: 0.0 });
        self.lines_3d.push(LineVertex { pos: b.into(), color: c, space: 0.0 });
    }
    pub fn line2(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 4]) {
        self.lines_2d.push(LineVertex { pos: [a[0], a[1], 0.0], color: c, space: 1.0 });
        self.lines_2d.push(LineVertex { pos: [b[0], b[1], 0.0], color: c, space: 1.0 });
    }
    /// The outline of a rectangle given by its four world corners.
    pub fn quad2(&mut self, p: [[f32; 2]; 4], c: [f32; 4]) {
        for k in 0..4 {
            self.line2(p[k], p[(k + 1) % 4], c);
        }
    }
}

struct GpuMesh {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    count: u32,
}

struct GpuPart {
    mesh: GpuMesh,
    texture: String,
    color: [f32; 4],
}

struct GpuModel {
    parts: Vec<GpuPart>,
    data: Arc<ModelData>,
}

struct Tex {
    bind: wgpu::BindGroup,
    size: [u32; 2],
}

struct Target {
    color: wgpu::Texture,
    view: wgpu::TextureView,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    size: [u32; 2],
}

/// Which geometry a 3D draw uses.
#[derive(Clone)]
enum Geo {
    Prim(&'static str),
    Part(String, usize),
    Skinned(NodeId, String, usize),
}

struct Draw3 {
    geo: Geo,
    texture: String,
    inst: u32,
    two_sided: bool,
    transparent: bool,
    shadow: bool,
    depth: f32,
}

pub struct Renderer {
    mesh_pipe: wgpu::RenderPipeline,
    mesh_pipe_2s: wgpu::RenderPipeline,
    alpha_pipe: wgpu::RenderPipeline,
    shadow_pipe: wgpu::RenderPipeline,
    sky_pipe: wgpu::RenderPipeline,
    sprite_pipe: wgpu::RenderPipeline,
    line3_pipe: wgpu::RenderPipeline,
    line2_pipe: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    shadow_globals: wgpu::Buffer,
    shadow_bg: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    tex_layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    meshes: HashMap<&'static str, GpuMesh>,
    quad: GpuMesh,
    models: HashMap<String, Result<GpuModel, String>>,
    skinned: HashMap<(NodeId, String), Vec<wgpu::Buffer>>,
    /// (path, pixel filtering) → texture, or the reason it couldn't load.
    textures: HashMap<(String, bool), Result<Tex, String>>,
    white_linear: Tex,
    white_nearest: Tex,
    missing: Tex,
    circle: Tex,
    instances: Option<wgpu::Buffer>,
    lines: Option<wgpu::Buffer>,
    target: Option<Target>,
    /// Asset problems not reported yet (the caller logs them once).
    pub problems: Vec<String>,
}

fn srgb_to_linear(c: [f32; 4]) -> [f32; 4] {
    let f = |x: f32| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

fn lin3(c: [f32; 4]) -> [f32; 4] {
    let l = srgb_to_linear(c);
    [l[0], l[1], l[2], 1.0]
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gex shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gex shadow"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()),
        });
        let uniform = |size: u64, label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
        };
        let globals = uniform(std::mem::size_of::<Globals>() as u64, "globals");
        let shadow_globals = uniform(64, "shadow globals");
        let buffer_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                buffer_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow globals"), entries: &[buffer_entry(0)] });
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow map"),
            size: wgpu::Extent3d { width: SHADOW_SIZE, height: SHADOW_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&Default::default());
        let shadow_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow_samp) },
            ],
        });
        let shadow_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow globals"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: shadow_globals.as_entire_binding() }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gex"),
            bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gex lines"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gex shadow"),
            bind_group_layouts: &[Some(&shadow_layout)],
            immediate_size: 0,
        });

        let instance_attrs = wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4];
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attrs,
        };
        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<mesh::Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &mesh_attrs,
        };
        let sprite_attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];
        let sprite_layout = wgpu::VertexBufferLayout { array_stride: 16, step_mode: wgpu::VertexStepMode::Vertex, attributes: &sprite_attrs };
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32];
        let line_vlayout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };

        let blend = Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL });
        let depth = |write: bool, cmp: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(write), depth_compare: Some(cmp), stencil: Default::default(), bias: Default::default() })
        };
        #[allow(clippy::too_many_arguments)]
        let pipe = |label: &str,
                    module: &wgpu::ShaderModule,
                    layout: &wgpu::PipelineLayout,
                    vs: &str,
                    fs: Option<&str>,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    cull: Option<wgpu::Face>,
                    ds: Option<wgpu::DepthStencilState>| {
            let targets = [blend.clone()];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { topology, cull_mode: cull, ..Default::default() },
                depth_stencil: ds,
                multisample: Default::default(),
                fragment: fs.map(|fs| wgpu::FragmentState { module, entry_point: Some(fs), compilation_options: Default::default(), targets: &targets }),
                multiview_mask: None,
                cache: None,
            })
        };
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let lines = wgpu::PrimitiveTopology::LineList;
        let mesh_bufs = [Some(mesh_layout.clone()), Some(instance_layout.clone())];
        let less = wgpu::CompareFunction::Less;
        let mesh_pipe = pipe("meshes", &shader, &layout, "vs_mesh", Some("fs_mesh"), &mesh_bufs, tri, Some(wgpu::Face::Back), depth(true, less));
        let mesh_pipe_2s = pipe("meshes, two-sided", &shader, &layout, "vs_mesh", Some("fs_mesh"), &mesh_bufs, tri, None, depth(true, less));
        let alpha_pipe = pipe("transparent meshes", &shader, &layout, "vs_mesh", Some("fs_mesh_alpha"), &mesh_bufs, tri, None, depth(false, less));
        let shadow_pipe = pipe("shadows", &shadow_shader, &shadow_pl, "vs_shadow", None, &mesh_bufs, tri, None, depth(true, wgpu::CompareFunction::LessEqual));
        let sky_pipe = pipe("sky", &shader, &layout, "vs_sky", Some("fs_sky"), &[], tri, None, depth(false, wgpu::CompareFunction::Always));
        let sprite_pipe = pipe("sprites", &shader, &layout, "vs_sprite", Some("fs_sprite"), &[Some(sprite_layout), Some(instance_layout)], tri, None, depth(false, wgpu::CompareFunction::Always));
        let line3_pipe = pipe("lines 3d", &shader, &line_layout, "vs_line", Some("fs_line"), &[Some(line_vlayout.clone())], lines, None, depth(false, wgpu::CompareFunction::LessEqual));
        let line2_pipe = pipe("lines 2d", &shader, &line_layout, "vs_line", Some("fs_line"), &[Some(line_vlayout)], lines, None, depth(false, wgpu::CompareFunction::Always));

        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let mut meshes = HashMap::new();
        meshes.insert("cube", upload_mesh(device, &mesh::cube()));
        meshes.insert("plane", upload_mesh(device, &mesh::plane()));
        meshes.insert("sphere", upload_mesh(device, &mesh::sphere(16, 24)));
        let quad_v: [f32; 16] = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let quad = GpuMesh {
            vb: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("quad"), contents: bytemuck::cast_slice(&quad_v), usage: wgpu::BufferUsages::VERTEX }),
            ib: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("quad"), contents: bytemuck::cast_slice(&[0u32, 1, 2, 0, 2, 3]), usage: wgpu::BufferUsages::INDEX }),
            count: 6,
        };
        let mut check = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                check.extend_from_slice(if (x + y) % 2 == 0 { &[255, 0, 255, 255] } else { &[20, 20, 20, 255] });
            }
        }
        let mut disc = Vec::with_capacity(64 * 64 * 4);
        for y in 0..64 {
            for x in 0..64 {
                let d = ((x as f32 - 31.5).powi(2) + (y as f32 - 31.5).powi(2)).sqrt();
                let a = ((32.0 - d) * 1.5).clamp(0.0, 1.0);
                disc.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
            }
        }
        let white_linear = make_tex(device, queue, &tex_layout, &linear, 1, 1, &[255; 4], false);
        let white_nearest = make_tex(device, queue, &tex_layout, &nearest, 1, 1, &[255; 4], false);
        let missing = make_tex(device, queue, &tex_layout, &nearest, 8, 8, &check, false);
        let circle = make_tex(device, queue, &tex_layout, &linear, 64, 64, &disc, false);
        Renderer {
            mesh_pipe,
            mesh_pipe_2s,
            alpha_pipe,
            shadow_pipe,
            sky_pipe,
            sprite_pipe,
            line3_pipe,
            line2_pipe,
            globals,
            globals_bg,
            shadow_globals,
            shadow_bg,
            shadow_view,
            tex_layout,
            linear,
            nearest,
            meshes,
            quad,
            models: HashMap::new(),
            skinned: HashMap::new(),
            textures: HashMap::new(),
            white_linear,
            white_nearest,
            missing,
            circle,
            instances: None,
            lines: None,
            target: None,
            problems: Vec::new(),
        }
    }

    /// Forgets loaded textures and models (after files change on disk).
    pub fn reload_textures(&mut self) {
        self.textures.clear();
        self.models.clear();
        self.skinned.clear();
        gex_assets::model::clear_cache();
    }

    /// Loads (once) the texture at an asset path. Pixel textures use nearest filtering.
    fn ensure_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vfs: &Vfs, path: &str, pixel: bool) {
        let key = (path.to_string(), pixel);
        if path.is_empty() || path.starts_with('@') || self.textures.contains_key(&key) {
            return;
        }
        let sampler = if pixel { &self.nearest } else { &self.linear };
        let t = gex_assets::load_image(vfs, path).map(|img| make_tex(device, queue, &self.tex_layout, sampler, img.width, img.height, &img.pixels, !pixel));
        if let Err(e) = &t {
            self.problems.push(format!("texture {}: {}", path, e));
        }
        self.textures.insert(key, t);
    }

    fn tex(&self, path: &str, pixel: bool) -> &Tex {
        if path.is_empty() {
            return if pixel { &self.white_nearest } else { &self.white_linear };
        }
        if path == "@circle" {
            return &self.circle;
        }
        match self.textures.get(&(path.to_string(), pixel)) {
            Some(Ok(t)) => t,
            _ => &self.missing,
        }
    }

    /// The pixel size of a loaded texture (for sizing sprites to their image).
    pub fn texture_size(&self, path: &str) -> Option<[u32; 2]> {
        match self.textures.get(&(path.to_string(), true)) {
            Some(Ok(t)) => Some(t.size),
            _ => None,
        }
    }

    fn ensure_model(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vfs: &Vfs, path: &str) {
        if self.models.contains_key(path) {
            return;
        }
        let r = gex_assets::model::cached(vfs, path).map(|data| {
            let mut parts = Vec::new();
            for (i, p) in data.parts.iter().enumerate() {
                let verts: Vec<mesh::Vertex> = (0..p.positions.len())
                    .map(|k| mesh::Vertex { pos: p.positions[k].into(), normal: p.normals[k].into(), uv: p.uvs[k].into() })
                    .collect();
                let texture = match &p.texture {
                    None => String::new(),
                    Some(ModelTexture::Path(t)) => t.clone(),
                    Some(ModelTexture::Embedded { key, image }) => {
                        let k = (key.clone(), false);
                        if !self.textures.contains_key(&k) {
                            let t = make_tex(device, queue, &self.tex_layout, &self.linear, image.width, image.height, &image.pixels, true);
                            self.textures.insert(k, Ok(t));
                        }
                        key.clone()
                    }
                };
                let _ = i;
                parts.push(GpuPart { mesh: upload_mesh(device, &mesh::MeshData { vertices: verts, indices: p.indices.clone() }), texture, color: p.color });
            }
            GpuModel { parts, data }
        });
        if let Err(e) = &r {
            self.problems.push(format!("model {}: {}", path, e));
        }
        if let Ok(m) = &r {
            let paths: Vec<String> = m.parts.iter().map(|p| p.texture.clone()).filter(|t| !self.textures.contains_key(&(t.clone(), false))).collect();
            for t in paths {
                self.ensure_texture(device, queue, vfs, &t, false);
            }
        }
        self.models.insert(path.to_string(), r);
    }

    fn ensure_target(&mut self, device: &wgpu::Device, size: [u32; 2]) -> bool {
        let size = [size[0].clamp(1, 8192), size[1].clamp(1, 8192)];
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return false;
        }
        let mk = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = mk(COLOR_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let depth = mk(DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        self.target = Some(Target { color, view, _depth: depth, depth_view, size });
        true
    }

    /// The texture rendered into (for reading it back).
    pub fn target_texture(&self) -> Option<&wgpu::Texture> {
        self.target.as_ref().map(|t| &t.color)
    }

    /// The color texture of the last render (for showing it, e.g. in egui).
    pub fn target_view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|t| &t.view)
    }

    fn geo(&self, g: &Geo) -> Option<(&wgpu::Buffer, &wgpu::Buffer, u32)> {
        match g {
            Geo::Prim(p) => self.meshes.get(p).map(|m| (&m.vb, &m.ib, m.count)),
            Geo::Part(path, i) => match self.models.get(path) {
                Some(Ok(m)) => m.parts.get(*i).map(|p| (&p.mesh.vb, &p.mesh.ib, p.mesh.count)),
                _ => None,
            },
            Geo::Skinned(node, path, i) => {
                let vb = self.skinned.get(&(*node, path.clone()))?.get(*i)?;
                match self.models.get(path) {
                    Some(Ok(m)) => m.parts.get(*i).map(|p| (vb, &p.mesh.ib, p.mesh.count)),
                    _ => None,
                }
            }
        }
    }

    /// Draws a frame. Returns true when the target texture was (re)created, so a texture
    /// registered with egui must be updated to the new view.
    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vfs: &Vfs, size: [u32; 2], list: &RenderList, cams: &Cameras, overlay: &Overlay) -> bool {
        let new_target = self.ensure_target(device, size);
        // Load what this frame needs.
        for s in &list.sprites {
            self.ensure_texture(device, queue, vfs, &s.texture, true);
        }
        if cams.draw_3d {
            for m in &list.meshes {
                self.ensure_texture(device, queue, vfs, &m.texture, false);
                if gex_assets::model::is_model(&m.mesh) {
                    self.ensure_model(device, queue, vfs, &m.mesh);
                }
            }
        }
        let env = list.environment.clone();
        let sky_tex = env.as_ref().map(|e| e.sky_texture.clone()).unwrap_or_default();
        if cams.draw_3d && !sky_tex.is_empty() {
            self.ensure_texture(device, queue, vfs, &sky_tex, false);
        }

        // ---- 3D draws ----
        let mut inst: Vec<Instance> = Vec::new();
        let mut draws: Vec<Draw3> = Vec::new();
        if cams.draw_3d {
            for m in &list.meshes {
                let color = srgb_to_linear(m.color);
                let uv = [0.0, 0.0, m.uv_scale[0], m.uv_scale[1]];
                let depth = m.model.w_axis.truncate().distance(cams.eye);
                if gex_assets::model::is_model(&m.mesh) {
                    let Some(Ok(model)) = self.models.get(&m.mesh) else { continue };
                    let data = model.data.clone();
                    let skinned = data.is_skinned();
                    if skinned {
                        // Pose on the CPU and upload this node's own vertices.
                        let anim = data.animation(&m.animation);
                        let joints = data.pose(anim, m.anim_time);
                        let key = (m.node, m.mesh.clone());
                        let bufs = self.skinned.entry(key).or_insert_with(|| {
                            data.parts
                                .iter()
                                .map(|p| {
                                    device.create_buffer(&wgpu::BufferDescriptor {
                                        label: Some("skinned"),
                                        size: (p.positions.len().max(1) * std::mem::size_of::<mesh::Vertex>()) as u64,
                                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                                        mapped_at_creation: false,
                                    })
                                })
                                .collect()
                        });
                        for (i, p) in data.parts.iter().enumerate() {
                            let (pos, nor) = data.skin(p, &joints);
                            let verts: Vec<mesh::Vertex> = (0..pos.len()).map(|k| mesh::Vertex { pos: pos[k].into(), normal: nor[k].into(), uv: p.uvs[k].into() }).collect();
                            if !verts.is_empty() {
                                queue.write_buffer(&bufs[i], 0, bytemuck::cast_slice(&verts));
                            }
                        }
                    }
                    let model = match self.models.get(&m.mesh) {
                        Some(Ok(model)) => model,
                        _ => continue,
                    };
                    for (i, part) in model.parts.iter().enumerate() {
                        let pc = srgb_to_linear(part.color);
                        let c = [color[0] * pc[0], color[1] * pc[1], color[2] * pc[2], color[3] * pc[3]];
                        let texture = if m.texture.is_empty() { part.texture.clone() } else { m.texture.clone() };
                        draws.push(Draw3 {
                            geo: if skinned { Geo::Skinned(m.node, m.mesh.clone(), i) } else { Geo::Part(m.mesh.clone(), i) },
                            texture,
                            inst: inst.len() as u32,
                            two_sided: data.two_sided,
                            transparent: c[3] < 0.999,
                            shadow: m.cast_shadows,
                            depth,
                        });
                        inst.push(Instance { model: m.model.to_cols_array_2d(), color: c, uv });
                    }
                } else {
                    draws.push(Draw3 {
                        geo: Geo::Prim(mesh::primitive(&m.mesh)),
                        texture: m.texture.clone(),
                        inst: inst.len() as u32,
                        two_sided: mesh::primitive(&m.mesh) == "plane",
                        transparent: color[3] < 0.999,
                        shadow: m.cast_shadows,
                        depth,
                    });
                    inst.push(Instance { model: m.model.to_cols_array_2d(), color, uv });
                }
            }
            // Opaque front to back (less overdraw), transparent back to front.
            draws.sort_by(|a, b| a.transparent.cmp(&b.transparent).then(if a.transparent { b.depth.total_cmp(&a.depth) } else { a.depth.total_cmp(&b.depth) }));
        }

        // ---- lights, shadows, globals ----
        let mut lights = [LightU { pos: [0.0; 4], dir: [0.0; 4], color: [0.0; 4] }; MAX_LIGHTS];
        let mut ls: Vec<&gex_core::render::LightDraw> = list.lights.iter().collect();
        // The shadow-casting sun goes first (the shader shadows light 0).
        ls.sort_by_key(|l| !(l.kind == LightKind::Directional && l.shadows));
        let mut n = 0;
        for l in ls.iter().take(MAX_LIGHTS) {
            let kind = match l.kind {
                LightKind::Directional => 0.0,
                LightKind::Point => 1.0,
                LightKind::Spot => 2.0,
            };
            lights[n] = LightU {
                pos: [l.pos.x, l.pos.y, l.pos.z, kind],
                dir: [l.dir.x, l.dir.y, l.dir.z, l.angle.clamp(1.0, 89.0).to_radians().cos()],
                color: [l.color[0], l.color[1], l.color[2], l.range],
            };
            n += 1;
        }
        let default_sun = n == 0;
        if default_sun {
            // No lights in the scene: a soft default so things are visible.
            lights[0] = LightU { pos: [0.0; 4], dir: Vec3::new(-0.4, -1.0, -0.6).normalize().extend(0.0).into(), color: [0.85, 0.85, 0.85, 10.0] };
            n = 1;
        }
        let sun = ls.first().filter(|l| l.kind == LightKind::Directional && l.shadows).map(|l| l.dir).or(if default_sun { Some(Vec3::new(-0.4, -1.0, -0.6).normalize()) } else { None });
        let shadows = cams.draw_3d && sun.is_some() && draws.iter().any(|d| d.shadow);
        let light_vp = match sun {
            Some(dir) => {
                let dir = dir.normalize_or(Vec3::NEG_Y);
                let up = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
                // Snap the center to shadow texels so edges don't shimmer as the camera moves.
                let texel = SHADOW_EXTENT * 2.0 / SHADOW_SIZE as f32;
                let view = Mat4::look_at_rh(cams.focus - dir * 60.0, cams.focus, up);
                let c = view.transform_point3(cams.focus);
                let snap = Vec3::new((c.x / texel).round() * texel - c.x, (c.y / texel).round() * texel - c.y, 0.0);
                let view = Mat4::from_translation(snap) * view;
                Mat4::orthographic_rh(-SHADOW_EXTENT, SHADOW_EXTENT, -SHADOW_EXTENT, SHADOW_EXTENT, 0.1, 160.0) * view
            }
            None => Mat4::IDENTITY,
        };
        let (ambient, fog_color, fog, sky_top, sky_horizon, ground) = match &env {
            Some(e) => (
                [e.ambient[0], e.ambient[1], e.ambient[2], 1.0],
                lin3(e.fog_color),
                [e.fog_start, e.fog_end, if e.fog_end > e.fog_start { 1.0 } else { 0.0 }, shadows as i32 as f32],
                lin3(e.sky_top),
                lin3(e.sky_horizon),
                lin3(e.ground),
            ),
            None => ([0.18, 0.18, 0.22, 1.0], [0.0; 4], [0.0, 0.0, 0.0, shadows as i32 as f32], [0.0; 4], [0.0; 4], [0.0; 4]),
        };
        let g = Globals {
            vp3: cams.view_proj_3d.to_cols_array_2d(),
            vp2: cams.view_proj_2d.to_cols_array_2d(),
            inv_vp3: cams.view_proj_3d.inverse().to_cols_array_2d(),
            light_vp: light_vp.to_cols_array_2d(),
            eye: cams.eye.extend(1.0).into(),
            ambient,
            fog_color,
            fog,
            sky_top,
            sky_horizon,
            ground,
            counts: [n as f32, (!sky_tex.is_empty()) as i32 as f32, 1.0 / SHADOW_SIZE as f32, 0.0],
            lights,
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        queue.write_buffer(&self.shadow_globals, 0, bytemuck::bytes_of(&light_vp.to_cols_array_2d()));

        // ---- sprites ----
        let mut sprite_batches: Vec<(String, u32, u32)> = Vec::new();
        if cams.draw_2d {
            for s in &list.sprites {
                // Unit quad → sprite rectangle (optionally centered), then the node's transform.
                let off = if s.centered { Vec3::new(-0.5 * s.size.x, -0.5 * s.size.y, 0.0) } else { Vec3::ZERO };
                let local = Mat4::from_translation(off) * Mat4::from_scale(Vec3::new(s.size.x, s.size.y, 1.0));
                let start = inst.len() as u32;
                let tex_size = self.tex(&s.texture, true).size;
                let mut uv = region_uv(s.region, tex_size);
                if s.flip[0] {
                    uv.swap(0, 2);
                }
                if s.flip[1] {
                    uv.swap(1, 3);
                }
                inst.push(Instance { model: (s.model * local).to_cols_array_2d(), color: srgb_to_linear(s.color), uv });
                match sprite_batches.last_mut() {
                    Some(b) if b.0 == s.texture => b.2 += 1,
                    _ => sprite_batches.push((s.texture.clone(), start, 1)),
                }
            }
        }
        if !inst.is_empty() {
            upload(device, queue, &mut self.instances, bytemuck::cast_slice(&inst), wgpu::BufferUsages::VERTEX);
        }

        // ---- lines ----
        // 3D lines are clipped to the view here: some drivers (llvmpipe, at least) draw
        // lines that run far outside the view as huge filled shapes.
        let m = cams.view_proj_3d;
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2];
        let mut lines: Vec<LineVertex> = Vec::with_capacity(overlay.lines_3d.len());
        'next: for pair in overlay.lines_3d.chunks_exact(2) {
            let (a, b) = (Vec3::from(pair[0].pos).extend(1.0), Vec3::from(pair[1].pos).extend(1.0));
            let (mut t0, mut t1) = (0.0f32, 1.0f32);
            for p in &planes {
                let (da, db) = (p.dot(a), p.dot(b));
                if da < 0.0 && db < 0.0 {
                    continue 'next;
                }
                if da < 0.0 {
                    t0 = t0.max(da / (da - db));
                } else if db < 0.0 {
                    t1 = t1.min(da / (da - db));
                }
            }
            if t0 >= t1 {
                continue;
            }
            let (mut va, mut vb) = (pair[0], pair[1]);
            va.pos = a.lerp(b, t0).truncate().into();
            vb.pos = a.lerp(b, t1).truncate().into();
            lines.push(va);
            lines.push(vb);
        }
        let n3 = lines.len() as u32;
        for (a, b, c) in &list.lines_2d {
            lines.push(LineVertex { pos: [a[0], a[1], 0.0], color: *c, space: 1.0 });
            lines.push(LineVertex { pos: [b[0], b[1], 0.0], color: *c, space: 1.0 });
        }
        lines.extend_from_slice(&overlay.lines_2d);
        let n2 = lines.len() as u32 - n3;
        if !lines.is_empty() {
            upload(device, queue, &mut self.lines, bytemuck::cast_slice(&lines), wgpu::BufferUsages::VERTEX);
        }

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gex frame") });
        // ---- shadow pass ----
        if shadows {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gex shadows"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipe);
            pass.set_bind_group(0, &self.shadow_bg, &[]);
            pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
            for d in draws.iter().filter(|d| d.shadow && !d.transparent) {
                if let Some((vb, ib, count)) = self.geo(&d.geo) {
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..count, 0, d.inst..d.inst + 1);
                }
            }
        }
        // ---- main pass ----
        {
            let t = self.target.as_ref().unwrap();
            let lc = list.clear;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gex"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: lc[0] as f64, g: lc[1] as f64, b: lc[2] as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            if cams.draw_3d && env.is_some() {
                pass.set_pipeline(&self.sky_pipe);
                pass.set_bind_group(1, &self.tex(&sky_tex, false).bind, &[]);
                pass.draw(0..3, 0..1);
            }
            if cams.draw_3d && !draws.is_empty() {
                pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
                let mut current: Option<u8> = None;
                for d in &draws {
                    let which = if d.transparent { 2 } else if d.two_sided { 1 } else { 0 };
                    if current != Some(which) {
                        pass.set_pipeline(match which {
                            0 => &self.mesh_pipe,
                            1 => &self.mesh_pipe_2s,
                            _ => &self.alpha_pipe,
                        });
                        current = Some(which);
                    }
                    let Some((vb, ib, count)) = self.geo(&d.geo) else { continue };
                    pass.set_bind_group(1, &self.tex(&d.texture, false).bind, &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..count, 0, d.inst..d.inst + 1);
                }
            }
            if n3 > 0 && cams.draw_3d {
                pass.set_pipeline(&self.line3_pipe);
                pass.set_vertex_buffer(0, self.lines.as_ref().unwrap().slice(..));
                pass.draw(0..n3, 0..1);
            }
            if cams.draw_2d && !sprite_batches.is_empty() {
                pass.set_pipeline(&self.sprite_pipe);
                pass.set_vertex_buffer(0, self.quad.vb.slice(..));
                pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
                pass.set_index_buffer(self.quad.ib.slice(..), wgpu::IndexFormat::Uint32);
                for (tex, start, count) in &sprite_batches {
                    pass.set_bind_group(1, &self.tex(tex, true).bind, &[]);
                    pass.draw_indexed(0..self.quad.count, 0, *start..start + count);
                }
            }
            if n2 > 0 {
                pass.set_pipeline(&self.line2_pipe);
                pass.set_vertex_buffer(0, self.lines.as_ref().unwrap().slice(..));
                pass.draw(n3..n3 + n2, 0..1);
            }
        }
        queue.submit([enc.finish()]);
        // Forget skinned buffers of nodes that weren't drawn.
        let live: std::collections::HashSet<(NodeId, String)> = list.meshes.iter().map(|m| (m.node, m.mesh.clone())).collect();
        self.skinned.retain(|k, _| live.contains(k));
        new_target
    }
}

fn upload_mesh(device: &wgpu::Device, m: &mesh::MeshData) -> GpuMesh {
    let verts: &[u8] = if m.vertices.is_empty() { &[0u8; 32] } else { bytemuck::cast_slice(&m.vertices) };
    let idx: &[u8] = if m.indices.is_empty() { &[0u8; 4] } else { bytemuck::cast_slice(&m.indices) };
    GpuMesh {
        vb: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: verts, usage: wgpu::BufferUsages::VERTEX }),
        ib: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: idx, usage: wgpu::BufferUsages::INDEX }),
        count: m.indices.len() as u32,
    }
}

/// Writes `data` into a growable buffer.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, buf: &mut Option<wgpu::Buffer>, data: &[u8], usage: wgpu::BufferUsages) {
    let need = data.len() as u64;
    if buf.as_ref().is_none_or(|b| b.size() < need) {
        let size = need.next_power_of_two().max(4096);
        *buf = Some(device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
    }
    queue.write_buffer(buf.as_ref().unwrap(), 0, data);
}

/// The texture rectangle for a sprite region.
fn region_uv(r: gex_core::render::Region, tex: [u32; 2]) -> [f32; 4] {
    use gex_core::render::Region;
    match r {
        Region::Whole => [0.0, 0.0, 1.0, 1.0],
        Region::Frame { h, v, frame } => {
            let f = frame % (h * v).max(1);
            let (c, r) = ((f % h) as f32, (f / h) as f32);
            [c / h as f32, r / v as f32, (c + 1.0) / h as f32, (r + 1.0) / v as f32]
        }
        Region::Tile { w, h, id } => {
            let (tw, th) = (tex[0].max(1) as f32, tex[1].max(1) as f32);
            let cols = (tw / w.max(1.0)).floor().max(1.0) as u32;
            let (c, r) = ((id % cols) as f32, (id / cols) as f32);
            // A hair inside the tile, so neighbours don't bleed in.
            let e = 0.01;
            [(c * w + e) / tw, (r * h + e) / th, ((c + 1.0) * w - e) / tw, ((r + 1.0) * h - e) / th]
        }
    }
}

/// Halves an RGBA image (box filter).
fn half(w: u32, h: u32, px: &[u8]) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let mut s = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    s += px[((sy * w + sx) * 4 + c) as usize] as u32;
                }
                out[((y * nw + x) * 4 + c) as usize] = (s / 4) as u8;
            }
        }
    }
    (nw, nh, out)
}

#[allow(clippy::too_many_arguments)]
fn make_tex(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, w: u32, h: u32, rgba: &[u8], mips: bool) -> Tex {
    let levels = if mips { 32 - w.max(h).max(1).leading_zeros() } else { 1 };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TEX_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (mut cw, mut ch, mut data) = (w, h, rgba.to_vec());
    for level in 0..levels {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(cw * 4), rows_per_image: Some(ch) },
            wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        );
        if level + 1 < levels {
            let (nw, nh, nd) = half(cw, ch, &data);
            cw = nw;
            ch = nh;
            data = nd;
        }
    }
    let view = tex.create_view(&Default::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    Tex { bind, size: [w, h] }
}
