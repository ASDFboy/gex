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
use gex_core::render::{Blend, LightKind, RenderList};
use gex_core::settings::Graphics;
use gex_core::ObjectId;
use wgpu::util::DeviceExt;

pub use camera::{Cameras, EditorCamera2D, EditorCamera3D};

/// The viewport target holds sRGB-encoded values (the shaders encode), which is what
/// egui expects of the textures it shows.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Image assets: sampled as linear color.
const TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const MAX_LIGHTS: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightU {
    pos: [f32; 4],
    dir: [f32; 4],
    color: [f32; 4],
    /// Spot softness, falloff, specular strength, own shadow bias (below 0 = project's).
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    vp3: [[f32; 4]; 4],
    vp2: [[f32; 4]; 4],
    inv_vp3: [[f32; 4]; 4],
    light_vp: [[f32; 4]; 4],
    light_vp1: [[f32; 4]; 4],
    shadow: [f32; 4],
    post: [f32; 4],
    eye: [f32; 4],
    ambient: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    sky_top: [f32; 4],
    sky_horizon: [f32; 4],
    ground: [f32; 4],
    counts: [f32; 4],
    /// Exposure, specular on, normal maps on, lighting on.
    light_opts: [f32; 4],
    /// Shadow samples across (1-4), softness, bias, the sun's shadow strength.
    shadow_opts: [f32; 4],
    /// Exponential fog, fog density, sky brightness, tonemap (0 none, 1 aces, 2 reinhard, 3 filmic).
    fog_opts: [f32; 4],
    /// Brightness, contrast, saturation, gamma.
    grade0: [f32; 4],
    /// Tint.
    grade1: [f32; 4],
    /// Vignette, grain, fxaa on, color grading on.
    grade2: [f32; 4],
    /// Width, height, 1/width, 1/height of the picture.
    screen: [f32; 4],
    lights: [LightU; MAX_LIGHTS],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    /// Texture rectangle: u0, v0, u1, v1 (meshes: offset and repeat).
    uv: [f32; 4],
    /// Meshes: specular, emissive, unlit, has a normal map.
    mat: [f32; 4],
    /// Meshes: glow color, rim strength.
    extra0: [f32; 4],
    /// Meshes: rim color, alpha cut.
    extra1: [f32; 4],
    /// Meshes: receives shadows, fogged, texture scroll x, y.
    extra2: [f32; 4],
}

impl Instance {
    fn flat(model: Mat4, color: [f32; 4], uv: [f32; 4]) -> Instance {
        Instance { model: model.to_cols_array_2d(), color, uv, ..Zeroable::zeroed() }
    }
}

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 10] =
    wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4, 11 => Float32x4, 12 => Float32x4, 13 => Float32x4];

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
    /// Maps: the baked light atlas.
    lightmap: Option<Tex>,
}

struct Tex {
    bind: wgpu::BindGroup,
    view: wgpu::TextureView,
    size: [u32; 2],
}

struct Target {
    color: wgpu::Texture,
    view: wgpu::TextureView,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    size: [u32; 2],
    /// The picture, for full-screen passes to read.
    color_bind: wgpu::BindGroup,
    /// A copy of the picture for the last pass (edge smoothing, color) to read.
    post_tex: wgpu::Texture,
    post: (wgpu::TextureView, wgpu::BindGroup),
    /// 2D light map, and bloom's two half-size buffers (each with a bind group to read it).
    light: (wgpu::TextureView, wgpu::BindGroup),
    bloom_a: (wgpu::TextureView, wgpu::BindGroup),
    bloom_b: (wgpu::TextureView, wgpu::BindGroup),
}

/// Which geometry a 3D draw uses.
#[derive(Clone)]
enum Geo {
    Prim(&'static str),
    Part(String, usize),
    Skinned(ObjectId, String, usize),
}

/// Sprites drawn together: texture, pixel filtering, blend, first instance, count.
type SpriteBatch = (String, bool, Blend, u32, u32);

struct Draw3 {
    /// In the view (off-screen draws still cast shadows).
    visible: bool,
    /// The model whose lightmap this uses ("" for none).
    lightmap: String,
    geo: Geo,
    texture: String,
    normal: String,
    shader: String,
    /// 3D pixel filtering (blocky texels).
    pixel: bool,
    blend: Blend,
    on_top: bool,
    inst: u32,
    two_sided: bool,
    transparent: bool,
    shadow: bool,
    depth: f32,
}

pub struct Renderer {
    light2d_pipe: wgpu::RenderPipeline,
    multiply_pipe: wgpu::RenderPipeline,
    bright_pipe: wgpu::RenderPipeline,
    blur_h_pipe: wgpu::RenderPipeline,
    blur_v_pipe: wgpu::RenderPipeline,
    bloom_add_pipe: wgpu::RenderPipeline,
    final_pipe: wgpu::RenderPipeline,
    sprite_add_pipe: wgpu::RenderPipeline,
    sprite_mul_pipe: wgpu::RenderPipeline,
    add_pipe: wgpu::RenderPipeline,
    mul_pipe: wgpu::RenderPipeline,
    top_pipe: wgpu::RenderPipeline,
    globals_layout: wgpu::BindGroupLayout,
    shadow_samp: wgpu::Sampler,
    shadow_size: u32,
    nearest_repeat: wgpu::Sampler,
    /// Settings the GPU objects were made with: anisotropy, mipmaps, shadow map size.
    applied: (u32, bool, u32),
    /// Custom material shaders by file: the pipeline, or why it didn't build.
    custom: HashMap<String, Result<wgpu::RenderPipeline, String>>,
    mesh_pl: wgpu::PipelineLayout,
    mat_layout: wgpu::BindGroupLayout,
    /// (texture, normal map, lightmap's model, pixel filtering) → the bind group meshes draw with.
    materials: HashMap<(String, String, String, bool), wgpu::BindGroup>,
    normals: HashMap<String, Result<Tex, String>>,
    flat_normal: Tex,
    glow: Tex,
    clamp: wgpu::Sampler,
    shadow_bg1: wgpu::BindGroup,
    shadow_globals1: wgpu::Buffer,
    clock: std::time::Instant,
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
    skinned: HashMap<(ObjectId, String), Vec<wgpu::Buffer>>,
    /// (path, pixel filtering) → texture, or the reason it couldn't load.
    textures: HashMap<(String, bool), Result<Tex, String>>,
    white_linear: Tex,
    white_nearest: Tex,
    missing: Tex,
    circle: Tex,
    instances: Option<wgpu::Buffer>,
    lines: Option<wgpu::Buffer>,
    target: Option<Target>,
    /// Smooth textures get mipmaps (a setting).
    mipmaps: bool,
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
        let shadow_globals1 = uniform(64, "shadow globals, far");
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
        let shadow_samp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_size = Graphics::default().shadow_resolution;
        let (shadow_view, globals_bg, shadow_size) = make_shadow_map(device, shadow_size, &globals_layout, &globals, &shadow_samp);
        let shadow_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow globals"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: shadow_globals.as_entire_binding() }],
        });
        let shadow_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow globals, far"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: shadow_globals1.as_entire_binding() }],
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
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let mat_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                tex_entry(0),
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                tex_entry(2),
                tex_entry(3),
            ],
        });
        let mesh_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gex meshes"),
            bind_group_layouts: &[Some(&globals_layout), Some(&mat_layout)],
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

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE_ATTRS,
        };
        let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 10 => Float32x2];
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
        let mesh_pipe = pipe("meshes", &shader, &mesh_pl, "vs_mesh", Some("fs_mesh"), &mesh_bufs, tri, Some(wgpu::Face::Back), depth(true, less));
        let mesh_pipe_2s = pipe("meshes, two-sided", &shader, &mesh_pl, "vs_mesh", Some("fs_mesh"), &mesh_bufs, tri, None, depth(true, less));
        let alpha_pipe = pipe("transparent meshes", &shader, &mesh_pl, "vs_mesh", Some("fs_mesh_alpha"), &mesh_bufs, tri, None, depth(false, less));
        let shadow_pipe = pipe("shadows", &shadow_shader, &shadow_pl, "vs_shadow", None, &mesh_bufs, tri, None, depth(true, wgpu::CompareFunction::LessEqual));
        let sky_pipe = pipe("sky", &shader, &layout, "vs_sky", Some("fs_sky"), &[], tri, None, depth(false, wgpu::CompareFunction::Always));
        let sprite_pipe = pipe("sprites", &shader, &layout, "vs_sprite", Some("fs_sprite"), &[Some(sprite_layout.clone()), Some(instance_layout.clone())], tri, None, depth(false, wgpu::CompareFunction::Always));
        let line3_pipe = pipe("lines 3d", &shader, &line_layout, "vs_line", Some("fs_line"), &[Some(line_vlayout.clone())], lines, None, depth(false, wgpu::CompareFunction::LessEqual));
        let line2_pipe = pipe("lines 2d", &shader, &line_layout, "vs_line", Some("fs_line"), &[Some(line_vlayout)], lines, None, depth(false, wgpu::CompareFunction::Always));
        // Full-screen passes and blends of their own.
        let pipe_b = |label: &str, vs: &str, fs: &str, buffers: &[Option<wgpu::VertexBufferLayout>], blend: Option<wgpu::BlendState>, ds: Option<wgpu::DepthStencilState>| {
            let targets = [Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { topology: tri, ..Default::default() },
                depth_stencil: ds,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(fs), compilation_options: Default::default(), targets: &targets }),
                multiview_mask: None,
                cache: None,
            })
        };
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Dst, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let add_alpha = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let mesh_b = |label: &str, fs: &str, blend: wgpu::BlendState, cmp: wgpu::CompareFunction, cull: Option<wgpu::Face>| {
            let targets = [Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&mesh_pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &mesh_bufs },
                primitive: wgpu::PrimitiveState { topology: tri, cull_mode: cull, ..Default::default() },
                depth_stencil: depth(false, cmp),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some(fs), compilation_options: Default::default(), targets: &targets }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mul_pre = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Dst, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        // Added and multiplied surfaces show their front only (both sides would count twice).
        let add_pipe = mesh_b("meshes, add", "fs_mesh_alpha", add_alpha, less, Some(wgpu::Face::Back));
        let mul_pipe = mesh_b("meshes, multiply", "fs_mesh_mul", mul_pre, less, Some(wgpu::Face::Back));
        let top_pipe = mesh_b("meshes, on top", "fs_mesh_alpha", wgpu::BlendState::ALPHA_BLENDING, wgpu::CompareFunction::Always, Some(wgpu::Face::Back));
        let sprite_bufs = [Some(sprite_layout.clone()), Some(instance_layout.clone())];
        let sprite_add_pipe = pipe_b("sprites, add", "vs_sprite", "fs_sprite", &sprite_bufs, Some(add_alpha), depth(false, wgpu::CompareFunction::Always));
        let sprite_mul_pipe = pipe_b("sprites, multiply", "vs_sprite", "fs_sprite_mul", &sprite_bufs, Some(mul_pre), depth(false, wgpu::CompareFunction::Always));
        let final_pipe = pipe_b("final", "vs_sky", "fs_final", &[], None, None);
        let light2d_pipe = pipe_b("2d lights", "vs_sprite", "fs_sprite", &[Some(sprite_layout.clone()), Some(instance_layout.clone())], Some(add_alpha), None);
        let multiply_pipe = pipe_b("2d light map", "vs_sky", "fs_copy", &[], Some(multiply), depth(false, wgpu::CompareFunction::Always));
        let bright_pipe = pipe_b("bloom bright", "vs_sky", "fs_bright", &[], None, None);
        let blur_h_pipe = pipe_b("bloom blur h", "vs_sky", "fs_blur_h", &[], None, None);
        let blur_v_pipe = pipe_b("bloom blur v", "vs_sky", "fs_blur_v", &[], None, None);
        let bloom_add_pipe = pipe_b("bloom add", "vs_sky", "fs_bloom_add", &[], Some(add), None);

        let linear = make_linear(device, Graphics::default().anisotropy);
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let nearest_repeat = make_nearest_repeat(device);

        let mut meshes = HashMap::new();
        meshes.insert("cube", upload_mesh(device, &mesh::cube()));
        meshes.insert("plane", upload_mesh(device, &mesh::plane()));
        meshes.insert("quad", upload_mesh(device, &mesh::quad()));
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
        let white_linear = make_tex(device, queue, &tex_layout, &linear, 1, 1, &[255; 4], false, TEX_FORMAT);
        let white_nearest = make_tex(device, queue, &tex_layout, &nearest, 1, 1, &[255; 4], false, TEX_FORMAT);
        let missing = make_tex(device, queue, &tex_layout, &nearest, 8, 8, &check, false, TEX_FORMAT);
        let circle = make_tex(device, queue, &tex_layout, &linear, 64, 64, &disc, false, TEX_FORMAT);
        // A soft round glow (2D lights, 3D particles).
        let mut soft = Vec::with_capacity(64 * 64 * 4);
        for y in 0..64 {
            for x in 0..64 {
                let d = (((x as f32 - 31.5).powi(2) + (y as f32 - 31.5).powi(2)).sqrt() / 32.0).min(1.0);
                let a = (1.0 - d) * (1.0 - d);
                soft.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
            }
        }
        let glow = make_tex(device, queue, &tex_layout, &linear, 64, 64, &soft, false, TEX_FORMAT);
        let flat_normal = make_tex(device, queue, &tex_layout, &linear, 1, 1, &[128, 128, 255, 255], false, wgpu::TextureFormat::Rgba8Unorm);
        let clamp = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Renderer {
            light2d_pipe,
            multiply_pipe,
            bright_pipe,
            blur_h_pipe,
            blur_v_pipe,
            bloom_add_pipe,
            final_pipe,
            sprite_add_pipe,
            sprite_mul_pipe,
            add_pipe,
            mul_pipe,
            top_pipe,
            globals_layout,
            shadow_samp,
            shadow_size,
            nearest_repeat,
            applied: (Graphics::default().anisotropy, Graphics::default().mipmaps, shadow_size),
            custom: HashMap::new(),
            mesh_pl,
            mat_layout,
            materials: HashMap::new(),
            normals: HashMap::new(),
            flat_normal,
            glow,
            clamp,
            shadow_bg1,
            shadow_globals1,
            clock: std::time::Instant::now(),
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
            mipmaps: true,
            problems: Vec::new(),
        }
    }

    /// Forgets loaded textures and models (after files change on disk).
    pub fn reload_textures(&mut self) {
        self.textures.clear();
        self.materials.clear();
        self.normals.clear();
        self.custom.clear();
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
        let mips = !pixel && self.mipmaps;
        let t = gex_assets::load_image(vfs, path).map(|img| make_tex(device, queue, &self.tex_layout, sampler, img.width, img.height, &img.pixels, mips, TEX_FORMAT));
        if let Err(e) = &t {
            self.problems.push(format!("texture {}: {}", path, e));
        }
        self.textures.insert(key, t);
    }

    /// A normal map (stored as plain values, not as colors).
    fn ensure_normal(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vfs: &Vfs, path: &str) {
        if path.is_empty() || self.normals.contains_key(path) {
            return;
        }
        let t = gex_assets::load_image(vfs, path).map(|img| make_tex(device, queue, &self.tex_layout, &self.linear, img.width, img.height, &img.pixels, true, wgpu::TextureFormat::Rgba8Unorm));
        if let Err(e) = &t {
            self.problems.push(format!("normal map {}: {}", path, e));
        }
        self.normals.insert(path.to_string(), t);
    }

    /// The bind group a mesh draws with: its texture and normal map.
    fn material(&mut self, device: &wgpu::Device, texture: &str, normal: &str, lightmap: &str, pixel: bool) {
        let key = (texture.to_string(), normal.to_string(), lightmap.to_string(), pixel);
        if self.materials.contains_key(&key) {
            return;
        }
        let base = &self.tex(texture, false).view;
        let nor = match self.normals.get(normal) {
            Some(Ok(t)) => &t.view,
            _ => &self.flat_normal.view,
        };
        let lm = match self.models.get(lightmap) {
            Some(Ok(GpuModel { lightmap: Some(t), .. })) => &t.view,
            _ => &self.white_linear.view,
        };
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout: &self.mat_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(base) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(if pixel { &self.nearest_repeat } else { &self.linear }) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(nor) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(lm) },
            ],
        });
        self.materials.insert(key, bg);
    }

    /// A custom material shader's pipeline (built once; errors are reported, not fatal).
    fn ensure_custom(&mut self, device: &wgpu::Device, vfs: &Vfs, path: &str) {
        if path.is_empty() || self.custom.contains_key(path) {
            return;
        }
        let r = (|| -> Result<wgpu::RenderPipeline, String> {
            let user = vfs.read_text(path)?;
            let src = format!(
                "{}\n// ---- {} ----\n{}\n\n@fragment\nfn fs_custom(o: MeshOut) -> @location(0) vec4<f32> {{\n    var base = textureSample(tex, samp, o.uv) * o.color;\n    let nm = textureSample(normal_tex, samp, o.uv);\n    let lm = textureSample(lightmap_tex, samp, o.uv2);\n    base = material(base, o.uv, o.world, normalize(o.normal), g.post.z);\n    if (base.a < 0.01) {{\n        discard;\n    }}\n    return encode(finish3(shade(o, base, nm, lm)));\n}}\n",
                include_str!("shaders.wgsl"),
                path,
                user
            );
            // Check it first: a bad shader handed to the GPU would stop the program.
            let module = wgpu::naga::front::wgsl::parse_str(&src).map_err(|e| e.emit_to_string(&src))?;
            wgpu::naga::valid::Validator::new(wgpu::naga::valid::ValidationFlags::all(), wgpu::naga::valid::Capabilities::all())
                .validate(&module)
                .map_err(|e| e.emit_to_string(&src))?;
            let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(path), source: wgpu::ShaderSource::Wgsl(src.into()) });
            let instance_attrs = INSTANCE_ATTRS;
            let mesh_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 10 => Float32x2];
            let bufs = [
                Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<mesh::Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &mesh_attrs }),
                Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Instance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &instance_attrs }),
            ];
            let targets = [Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })];
            Ok(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(path),
                layout: Some(&self.mesh_pl),
                vertex: wgpu::VertexState { module: &sm, entry_point: Some("vs_mesh"), compilation_options: Default::default(), buffers: &bufs },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH_FORMAT, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &sm, entry_point: Some("fs_custom"), compilation_options: Default::default(), targets: &targets }),
                multiview_mask: None,
                cache: None,
            }))
        })();
        if let Err(e) = &r {
            self.problems.push(format!("shader {}: {}", path, e));
        }
        self.custom.insert(path.to_string(), r);
    }

    fn tex(&self, path: &str, pixel: bool) -> &Tex {
        if path.is_empty() {
            return if pixel { &self.white_nearest } else { &self.white_linear };
        }
        if path == "@circle" {
            return &self.circle;
        }
        if path == "@glow" {
            return &self.glow;
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
                    .map(|k| mesh::Vertex { pos: p.positions[k].into(), normal: p.normals[k].into(), uv: p.uvs[k].into(), uv2: p.uvs2.get(k).copied().unwrap_or_default().into() })
                    .collect();
                let texture = match &p.texture {
                    None => String::new(),
                    Some(ModelTexture::Path(t)) => t.clone(),
                    Some(ModelTexture::Embedded { key, image }) => {
                        let k = (key.clone(), false);
                        if !self.textures.contains_key(&k) {
                            let t = make_tex(device, queue, &self.tex_layout, &self.linear, image.width, image.height, &image.pixels, self.mipmaps, TEX_FORMAT);
                            self.textures.insert(k, Ok(t));
                        }
                        key.clone()
                    }
                };
                let _ = i;
                parts.push(GpuPart { mesh: upload_mesh(device, &mesh::MeshData { vertices: verts, indices: p.indices.clone() }), texture, color: p.color });
            }
            // Baked light: stored as plain values (not colors), filtered smoothly, no mipmaps.
            let lightmap = data.lightmap.as_ref().map(|l| make_tex(device, queue, &self.tex_layout, &self.clamp, l.width, l.height, &l.pixels, false, wgpu::TextureFormat::Rgba8Unorm));
            GpuModel { parts, data, lightmap }
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

    /// Remakes what the graphics settings are baked into, when they change.
    fn apply_settings(&mut self, device: &wgpu::Device, g: &Graphics) {
        let want = (g.anisotropy, g.mipmaps, g.shadow_resolution);
        if want == self.applied {
            return;
        }
        if want.0 != self.applied.0 || want.1 != self.applied.1 {
            self.linear = make_linear(device, g.anisotropy);
            self.reload_textures();
        }
        if want.2 != self.applied.2 {
            let (v, bg, size) = make_shadow_map(device, g.shadow_resolution, &self.globals_layout, &self.globals, &self.shadow_samp);
            self.shadow_view = v;
            self.globals_bg = bg;
            self.shadow_size = size;
        }
        self.applied = want;
        self.mipmaps = g.mipmaps;
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
        let bind_of = |v: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.tex_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.clamp) },
                ],
            })
        };
        let extra = |w: u32, h: u32| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("gex pass"),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let v = t.create_view(&Default::default());
            let b = bind_of(&v);
            (v, b)
        };
        let color_bind = bind_of(&view);
        let post_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gex post"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let post_view = post_tex.create_view(&Default::default());
        let post = (bind_of(&post_view), post_view);
        let post = (post.1, post.0);
        let light = extra(size[0], size[1]);
        let bloom_a = extra(size[0] / 2, size[1] / 2);
        let bloom_b = extra(size[0] / 2, size[1] / 2);
        self.target = Some(Target { color, view, _depth: depth, depth_view, size, color_bind, post_tex, post, light, bloom_a, bloom_b });
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

    fn draw_sprites<'p>(&'p self, pass: &mut wgpu::RenderPass<'p>, batches: &[SpriteBatch]) {
        pass.set_vertex_buffer(0, self.quad.vb.slice(..));
        pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
        pass.set_index_buffer(self.quad.ib.slice(..), wgpu::IndexFormat::Uint32);
        let mut current = None;
        for (tex, px, blend, start, count) in batches {
            if current != Some(*blend) {
                pass.set_pipeline(match blend {
                    Blend::Normal => &self.sprite_pipe,
                    Blend::Add => &self.sprite_add_pipe,
                    Blend::Multiply => &self.sprite_mul_pipe,
                });
                current = Some(*blend);
            }
            pass.set_bind_group(1, &self.tex(tex, *px).bind, &[]);
            pass.draw_indexed(0..self.quad.count, 0, *start..start + count);
        }
    }

    fn geo(&self, g: &Geo) -> Option<(&wgpu::Buffer, &wgpu::Buffer, u32)> {
        match g {
            Geo::Prim(p) => self.meshes.get(p).map(|m| (&m.vb, &m.ib, m.count)),
            Geo::Part(path, i) => match self.models.get(path) {
                Some(Ok(m)) => m.parts.get(*i).map(|p| (&p.mesh.vb, &p.mesh.ib, p.mesh.count)),
                _ => None,
            },
            Geo::Skinned(object, path, i) => {
                let vb = self.skinned.get(&(*object, path.clone()))?.get(*i)?;
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
        let gs = &list.graphics;
        self.apply_settings(device, gs);
        let rs = gs.render_scale.clamp(0.25, 2.0);
        let size = [((size[0] as f32 * rs).round() as u32).max(1), ((size[1] as f32 * rs).round() as u32).max(1)];
        let new_target = self.ensure_target(device, size);
        let sprite_pixel = gs.sprite_filter != "smooth";
        let mesh_pixel = gs.texture_filter == "pixel";
        // Load what this frame needs.
        for s in &list.sprites {
            self.ensure_texture(device, queue, vfs, &s.texture, s.pixel.unwrap_or(sprite_pixel));
        }
        for l in &list.lights_2d {
            self.ensure_texture(device, queue, vfs, &l.texture, true);
        }
        if cams.draw_3d {
            for m in &list.meshes {
                self.ensure_texture(device, queue, vfs, &m.texture, false);
                self.ensure_normal(device, queue, vfs, &m.normal_map);
                self.ensure_custom(device, vfs, &m.shader);
                if gex_assets::model::is_model(&m.mesh) {
                    self.ensure_model(device, queue, vfs, &m.mesh);
                }
            }
        }
        let env = list.environment.clone();
        let time = self.clock.elapsed().as_secs_f32();
        let sky_tex = env.as_ref().map(|e| e.sky_texture.clone()).unwrap_or_default();
        if cams.draw_3d && !sky_tex.is_empty() {
            self.ensure_texture(device, queue, vfs, &sky_tex, false);
        }

        // ---- 3D draws ----
        let mut inst: Vec<Instance> = Vec::new();
        let mut draws: Vec<Draw3> = Vec::new();
        // The view's six planes, to skip what's off screen.
        let frustum = {
            let m = cams.view_proj_3d;
            let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
            [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| p / p.truncate().length().max(1e-6))
        };
        let in_view = |c: Vec3, r: f32| frustum.iter().all(|p| p.dot(c.extend(1.0)) > -r);
        // Camera-facing rotation for billboards.
        let fwd = (cams.focus - cams.eye).normalize_or(Vec3::NEG_Z);
        let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(fwd);
        let face_cam = Mat4::from_cols(right.extend(0.0), up.extend(0.0), (-fwd).extend(0.0), gex_core::glam::Vec4::W);
        if cams.draw_3d {
            for m in &list.meshes {
                let color = srgb_to_linear(m.color);
                let uv = [0.0, 0.0, m.uv_scale[0], m.uv_scale[1]];
                let mut m = m.clone();
                if m.billboard {
                    let (sc, _, t) = m.model.to_scale_rotation_translation();
                    m.model = Mat4::from_translation(t) * face_cam * Mat4::from_scale(sc);
                }
                let m = &m;
                let mat = [m.specular, m.emissive, m.unlit as i32 as f32, (!m.normal_map.is_empty()) as i32 as f32];
                let depth = m.model.w_axis.truncate().distance(cams.eye);
                if m.draw_distance > 0.0 && depth > m.draw_distance {
                    continue;
                }
                let uv = [m.uv_offset[0], m.uv_offset[1], uv[2], uv[3]];
                let ec = srgb_to_linear([m.emissive_color[0], m.emissive_color[1], m.emissive_color[2], 1.0]);
                let rc = srgb_to_linear([m.rim_color[0], m.rim_color[1], m.rim_color[2], 1.0]);
                let extra0 = [ec[0], ec[1], ec[2], m.rim];
                let extra1 = [rc[0], rc[1], rc[2], m.alpha_cut];
                let extra2 = [m.receive_shadows as i32 as f32, m.fog as i32 as f32, m.uv_scroll[0], m.uv_scroll[1]];
                let pixel = m.pixel.unwrap_or(mesh_pixel);
                let blend = m.blend_mode;
                let on_top = m.on_top;
                let (scale, _, centre) = m.model.to_scale_rotation_translation();
                if gex_assets::model::is_model(&m.mesh) {
                    let Some(Ok(model)) = self.models.get(&m.mesh) else { continue };
                    let data = model.data.clone();
                    // Source models sit in their rest pose already: pose them only when the bones move.
                    let skinned = data.is_skinned() && (!data.rest_is_bind || !m.animation.is_empty() || m.pose.is_some());
                    let has_lm = model.lightmap.is_some();
                    let mut mat = mat;
                    if has_lm {
                        mat[3] += 2.0;
                    }
                    let bc = m.model.transform_point3((data.min + data.max) * 0.5);
                    let br = (data.max - data.min).length() * 0.5 * scale.max_element() * if skinned { 1.5 } else { 1.0 };
                    let visible = in_view(bc, br);
                    if skinned && visible {
                        // Pose on the CPU and upload this object's own vertices.
                        let anim = data.animation(&m.animation);
                        let joints = if let Some(p) = &m.pose {
                            p.to_vec()
                        } else if m.blend < 1.0 && !m.prev_animation.is_empty() {
                            data.pose_blend(data.animation(&m.prev_animation), m.prev_time, anim, m.anim_time, m.blend)
                        } else {
                            data.pose(anim, m.anim_time)
                        };
                        let key = (m.object, m.mesh.clone());
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
                            let verts: Vec<mesh::Vertex> = (0..pos.len()).map(|k| mesh::Vertex { pos: pos[k].into(), normal: nor[k].into(), uv: p.uvs[k].into(), uv2: p.uvs2.get(k).copied().unwrap_or_default().into() }).collect();
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
                            geo: if skinned { Geo::Skinned(m.object, m.mesh.clone(), i) } else { Geo::Part(m.mesh.clone(), i) },
                            lightmap: if has_lm { m.mesh.clone() } else { String::new() },
                            texture,
                            normal: m.normal_map.clone(),
                            shader: m.shader.clone(),
                            pixel,
                            blend,
                            on_top,
                            inst: inst.len() as u32,
                            two_sided: m.two_sided.unwrap_or(data.two_sided),
                            transparent: c[3] < 0.999 || blend != Blend::Normal || on_top,
                            shadow: m.cast_shadows && !(skinned && !visible),
                            depth,
                            visible,
                        });
                        inst.push(Instance { model: m.model.to_cols_array_2d(), color: c, uv, mat, extra0, extra1, extra2 });
                    }
                } else {
                    let prim = mesh::primitive(&m.mesh);
                    let radius = match prim {
                        "cube" => 0.87,
                        "sphere" => 0.5,
                        _ => 0.71,
                    } * scale.max_element();
                    draws.push(Draw3 {
                        geo: Geo::Prim(prim),
                        lightmap: String::new(),
                        texture: m.texture.clone(),
                        normal: m.normal_map.clone(),
                        shader: m.shader.clone(),
                        pixel,
                        blend,
                        on_top,
                        inst: inst.len() as u32,
                        two_sided: m.two_sided.unwrap_or(prim == "plane" || prim == "quad"),
                        transparent: color[3] < 0.999 || blend != Blend::Normal || on_top,
                        shadow: m.cast_shadows,
                        depth,
                        visible: in_view(centre, radius),
                    });
                    inst.push(Instance { model: m.model.to_cols_array_2d(), color, uv, mat, extra0, extra1, extra2 });
                }
            }
            // Opaque front to back (less overdraw), transparent back to front, "on top" last.
            draws.sort_by(|a, b| a.on_top.cmp(&b.on_top).then(a.transparent.cmp(&b.transparent)).then(if a.transparent { b.depth.total_cmp(&a.depth) } else { a.depth.total_cmp(&b.depth) }));
        }

        // ---- lights, shadows, globals ----
        let mut lights = [LightU { pos: [0.0; 4], dir: [0.0; 4], color: [0.0; 4], extra: [0.0; 4] }; MAX_LIGHTS];
        let max_lights = (gs.max_lights as usize).min(MAX_LIGHTS);
        let mut ls: Vec<&gex_core::render::LightDraw> = list.lights.iter().collect();
        // The shadow-casting sun goes first (the shader shadows light 0).
        ls.sort_by_key(|l| !(l.kind == LightKind::Directional && l.shadows));
        let mut n = 0;
        for l in ls.iter().take(max_lights) {
            let kind = match l.kind {
                LightKind::Directional => 0.0,
                LightKind::Point => 1.0,
                LightKind::Spot => 2.0,
            };
            lights[n] = LightU {
                pos: [l.pos.x, l.pos.y, l.pos.z, kind],
                dir: [l.dir.x, l.dir.y, l.dir.z, l.angle.clamp(1.0, 89.0).to_radians().cos()],
                color: [l.color[0], l.color[1], l.color[2], l.range],
                extra: [l.softness, l.falloff, l.specular, l.shadow_bias],
            };
            n += 1;
        }
        let default_sun = list.lights.is_empty();
        if default_sun {
            // No lights in the blueprint: a soft default so things are visible.
            lights[0] = LightU { pos: [0.0; 4], dir: Vec3::new(-0.4, -1.0, -0.6).normalize().extend(0.0).into(), color: [0.85, 0.85, 0.85, 10.0], extra: [0.1, 2.0, 1.0, -1.0] };
            n = 1;
        }
        let sun_light = ls.first().filter(|l| l.kind == LightKind::Directional && l.shadows && max_lights > 0);
        let sun = sun_light.map(|l| l.dir).or(if default_sun { Some(Vec3::new(-0.4, -1.0, -0.6).normalize()) } else { None });
        let shadows = gs.shadows && cams.draw_3d && sun.is_some() && draws.iter().any(|d| d.shadow);
        let shadow_size = self.shadow_size;
        // Two shadow cascades: a sharp one near the camera, a coarser one out to the shadow distance.
        let far = (env.as_ref().map(|e| e.shadow_distance).unwrap_or(40.0) * gs.shadow_distance_scale).clamp(2.0, 2000.0);
        let near = (far * env.as_ref().map(|e| e.shadow_split).unwrap_or(0.25)).max(1.0);
        let cascade = |reach: f32| -> Mat4 {
            let Some(dir) = sun else { return Mat4::IDENTITY };
            let dir = dir.normalize_or(Vec3::NEG_Y);
            let lup = if dir.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
            // A sphere around the part of the view this cascade covers.
            let centre = cams.eye + fwd * reach * 0.5;
            let radius = reach * 0.6 + 1.0;
            let texel = radius * 2.0 / shadow_size as f32;
            let view = Mat4::look_at_rh(centre - dir * (radius + 80.0), centre, lup);
            // Snap to shadow texels so edges don't shimmer as the camera moves.
            let c = view.transform_point3(centre);
            let snap = Vec3::new((c.x / texel).round() * texel - c.x, (c.y / texel).round() * texel - c.y, 0.0);
            Mat4::orthographic_rh(-radius, radius, -radius, radius, 0.1, radius * 2.0 + 160.0) * Mat4::from_translation(snap) * view
        };
        let light_vp = cascade(near);
        let light_vp1 = cascade(far);
        let (ambient, fog_color, fog, sky_top, sky_horizon, ground) = match &env {
            Some(e) => (
                [e.ambient[0], e.ambient[1], e.ambient[2], 1.0],
                lin3(e.fog_color),
                [e.fog_start, e.fog_end, if gs.fog && ((!e.fog_exp && e.fog_end > e.fog_start) || (e.fog_exp && e.fog_density > 0.0)) { 1.0 } else { 0.0 }, shadows as i32 as f32],
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
            light_vp1: light_vp1.to_cols_array_2d(),
            shadow: [near * 0.9, far, shadows as i32 as f32, 0.0],
            post: [env.as_ref().map(|e| e.bloom).unwrap_or(0.0), env.as_ref().map(|e| e.bloom_threshold).unwrap_or(0.8), time, 0.0],
            eye: cams.eye.extend(1.0).into(),
            ambient,
            fog_color,
            fog,
            sky_top,
            sky_horizon,
            ground,
            counts: [n as f32, (!sky_tex.is_empty()) as i32 as f32, 1.0 / shadow_size as f32, 0.0],
            light_opts: [gs.exposure * env.as_ref().map(|e| e.exposure).unwrap_or(1.0), gs.specular as i32 as f32, gs.normal_maps as i32 as f32, gs.lighting as i32 as f32],
            shadow_opts: [
                if gs.shadow_softness <= 0.0 { 1.0 } else { (gs.shadow_taps() as f32).sqrt() },
                gs.shadow_softness,
                gs.shadow_bias,
                sun_light.map(|l| l.shadow_strength).unwrap_or(1.0),
            ],
            fog_opts: [
                env.as_ref().is_some_and(|e| e.fog_exp) as i32 as f32,
                env.as_ref().map(|e| e.fog_density).unwrap_or(0.0),
                env.as_ref().map(|e| e.sky_energy).unwrap_or(1.0),
                gs.tonemap_id() as f32,
            ],
            grade0: [
                gs.brightness,
                gs.contrast * env.as_ref().map(|e| e.contrast).unwrap_or(1.0),
                gs.saturation * env.as_ref().map(|e| e.saturation).unwrap_or(1.0),
                gs.gamma,
            ],
            grade1: {
                let et = env.as_ref().map(|e| e.tint).unwrap_or([1.0; 4]);
                [gs.tint[0] * et[0], gs.tint[1] * et[1], gs.tint[2] * et[2], 1.0]
            },
            grade2: [(gs.vignette + env.as_ref().map(|e| e.vignette).unwrap_or(0.0)).min(1.0), gs.grain, (gs.antialiasing == "fxaa") as i32 as f32, 0.0],
            screen: [size[0] as f32, size[1] as f32, 1.0 / size[0] as f32, 1.0 / size[1] as f32],
            lights,
        };
        let mut g = g;
        let env_neutral = env.as_ref().is_none_or(|e| e.contrast == 1.0 && e.saturation == 1.0 && e.tint == [1.0; 4] && e.vignette == 0.0);
        let grade = !(gs.color_neutral() && env_neutral);
        g.grade2[3] = grade as i32 as f32;
        let final_pass = grade || g.grade2[2] > 0.5;
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        queue.write_buffer(&self.shadow_globals, 0, bytemuck::bytes_of(&light_vp.to_cols_array_2d()));
        queue.write_buffer(&self.shadow_globals1, 0, bytemuck::bytes_of(&light_vp1.to_cols_array_2d()));

        // ---- sprites ----
        // Batches: (texture, pixel filtering, blend, first instance, count).
        let mut sprite_batches: Vec<SpriteBatch> = Vec::new();
        let mut unlit_batches: Vec<SpriteBatch> = Vec::new();
        let mut light_batches: Vec<SpriteBatch> = Vec::new();
        let lit_2d = gs.lights_2d && cams.draw_2d && list.ambient_2d[..3].iter().any(|c| *c < 0.999);
        if cams.draw_2d {
            // What the view shows, in world pixels (for repeating backgrounds).
            let inv = cams.view_proj_2d.inverse();
            let (mut vis_lo, mut vis_hi) = (gex_core::glam::Vec2::splat(f32::MAX), gex_core::glam::Vec2::splat(f32::MIN));
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let p = inv.transform_point3(Vec3::new(x, y, 0.0)).truncate();
                vis_lo = vis_lo.min(p);
                vis_hi = vis_hi.max(p);
            }
            let cam_c = list.camera2d.map(|c| c.center).unwrap_or_default();
            let push = |inst: &mut Vec<Instance>, batches: &mut Vec<SpriteBatch>, model: Mat4, color: [f32; 4], uv: [f32; 4], tex: &str, pixel: bool, blend: Blend| {
                let start = inst.len() as u32;
                inst.push(Instance::flat(model, color, uv));
                match batches.last_mut() {
                    Some(b) if b.0 == tex && b.1 == pixel && b.2 == blend => b.4 += 1,
                    _ => batches.push((tex.to_string(), pixel, blend, start, 1)),
                }
            };
            for s in &list.sprites {
                // Unit quad → sprite rectangle (optionally centered), then the object's transform.
                let off = if s.centered { Vec3::new(-0.5 * s.size.x, -0.5 * s.size.y, 0.0) } else { Vec3::ZERO };
                let local = Mat4::from_translation(off) * Mat4::from_scale(Vec3::new(s.size.x, s.size.y, 1.0));
                let pixel = s.pixel.unwrap_or(sprite_pixel);
                let tex_size = self.tex(&s.texture, pixel).size;
                // Unlit sprites draw after the 2D light map, so the dark doesn't touch them.
                let batches = if lit_2d && !s.lit { &mut unlit_batches } else { &mut sprite_batches };
                let mut uv = region_uv(s.region, tex_size);
                if s.flip[0] {
                    uv.swap(0, 2);
                }
                if s.flip[1] {
                    uv.swap(1, 3);
                }
                // Parallax: far things (below 1) follow the camera part of the way.
                let shift = cam_c * (gex_core::glam::Vec2::ONE - s.parallax);
                let model = Mat4::from_translation(shift.extend(0.0)) * s.model * local;
                let color = srgb_to_linear(s.color);
                if s.repeat[0] || s.repeat[1] {
                    let (mut lo, mut hi) = (gex_core::glam::Vec2::splat(f32::MAX), gex_core::glam::Vec2::splat(f32::MIN));
                    for c in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(1.0, 1.0, 0.0)] {
                        let p = model.transform_point3(c).truncate();
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                    let size = (hi - lo).max(gex_core::glam::Vec2::splat(1.0));
                    let range = |axis: usize, on: bool| -> (i32, i32) {
                        if !on {
                            return (0, 0);
                        }
                        let a = ((vis_lo[axis] - hi[axis]) / size[axis]).ceil() as i32;
                        let b = ((vis_hi[axis] - lo[axis]) / size[axis]).floor() as i32;
                        (a.max(-200), b.min(200))
                    };
                    let (x0, x1) = range(0, s.repeat[0]);
                    let (y0, y1) = range(1, s.repeat[1]);
                    for ky in y0..=y1 {
                        for kx in x0..=x1 {
                            let t = Mat4::from_translation(Vec3::new(kx as f32 * size.x, ky as f32 * size.y, 0.0));
                            push(&mut inst, batches, t * model, color, uv, &s.texture, pixel, s.blend);
                        }
                    }
                } else {
                    push(&mut inst, batches, model, color, uv, &s.texture, pixel, s.blend);
                }
            }
            if lit_2d {
                for l in &list.lights_2d {
                    let model = Mat4::from_translation(Vec3::new(l.pos.x - l.radius, l.pos.y - l.radius, 0.0)) * Mat4::from_scale(Vec3::new(l.radius * 2.0, l.radius * 2.0, 1.0));
                    let tex = if l.texture.is_empty() { "@glow" } else { l.texture.as_str() };
                    push(&mut inst, &mut light_batches, model, srgb_to_linear(l.color), [0.0, 0.0, 1.0, 1.0], tex, true, Blend::Add);
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
        // Material bind groups for this frame's draws.
        let keys: Vec<(String, String, String, bool)> = draws.iter().map(|d| (d.texture.clone(), d.normal.clone(), d.lightmap.clone(), d.pixel)).collect();
        for (t, n, l, px) in keys {
            self.material(device, &t, &n, &l, px);
        }
        // ---- shadow pass: two cascades side by side in one atlas ----
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
            pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
            for (k, bg) in [&self.shadow_bg, &self.shadow_bg1].into_iter().enumerate() {
                pass.set_viewport((k as u32 * shadow_size) as f32, 0.0, shadow_size as f32, shadow_size as f32, 0.0, 1.0);
                pass.set_bind_group(0, bg, &[]);
                for d in draws.iter().filter(|d| d.shadow && !d.transparent) {
                    if let Some((vb, ib, count)) = self.geo(&d.geo) {
                        pass.set_vertex_buffer(0, vb.slice(..));
                        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..count, 0, d.inst..d.inst + 1);
                    }
                }
            }
        }
        // ---- 2D light map: the ambient dark, with lights added in ----
        if lit_2d {
            let t = self.target.as_ref().unwrap();
            let a = list.ambient_2d;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gex 2d lights"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.light.0,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: a[0] as f64, g: a[1] as f64, b: a[2] as f64, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !light_batches.is_empty() {
                pass.set_pipeline(&self.light2d_pipe);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_vertex_buffer(0, self.quad.vb.slice(..));
                pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
                pass.set_index_buffer(self.quad.ib.slice(..), wgpu::IndexFormat::Uint32);
                for (tex, px, _, start, count) in &light_batches {
                    pass.set_bind_group(1, &self.tex(tex, *px).bind, &[]);
                    pass.draw_indexed(0..self.quad.count, 0, *start..start + count);
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
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: lc[0] as f64, g: lc[1] as f64, b: lc[2] as f64, a: lc[3] as f64 }),
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
            if cams.draw_3d && env.is_some() && gs.sky {
                pass.set_pipeline(&self.sky_pipe);
                pass.set_bind_group(1, &self.tex(&sky_tex, false).bind, &[]);
                pass.draw(0..3, 0..1);
            }
            if cams.draw_3d && !draws.is_empty() {
                pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
                let mut current: Option<usize> = None;
                let customs: Vec<&str> = {
                    let mut v: Vec<&str> = draws.iter().map(|d| d.shader.as_str()).filter(|s| !s.is_empty()).collect();
                    v.sort();
                    v.dedup();
                    v
                };
                for d in draws.iter().filter(|d| d.visible) {
                    let custom = match self.custom.get(&d.shader) {
                        Some(Ok(p)) => Some((customs.iter().position(|c| *c == d.shader).unwrap_or(0), p)),
                        _ => None,
                    };
                    let which = match custom {
                        Some((k, _)) => 10 + k,
                        None if d.on_top => 5,
                        None if d.blend == Blend::Add => 3,
                        None if d.blend == Blend::Multiply => 4,
                        None if d.transparent => 2,
                        None if d.two_sided => 1,
                        None => 0,
                    };
                    if current != Some(which) {
                        pass.set_pipeline(match (which, custom) {
                            (_, Some((_, p))) => p,
                            (0, _) => &self.mesh_pipe,
                            (1, _) => &self.mesh_pipe_2s,
                            (3, _) => &self.add_pipe,
                            (4, _) => &self.mul_pipe,
                            (5, _) => &self.top_pipe,
                            _ => &self.alpha_pipe,
                        });
                        current = Some(which);
                    }
                    let Some((vb, ib, count)) = self.geo(&d.geo) else { continue };
                    let Some(mat) = self.materials.get(&(d.texture.clone(), d.normal.clone(), d.lightmap.clone(), d.pixel)) else { continue };
                    pass.set_bind_group(1, mat, &[]);
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
                self.draw_sprites(&mut pass, &sprite_batches);
            }
            if lit_2d {
                pass.set_pipeline(&self.multiply_pipe);
                pass.set_bind_group(1, &t.light.1, &[]);
                pass.draw(0..3, 0..1);
            }
            if cams.draw_2d && !unlit_batches.is_empty() {
                self.draw_sprites(&mut pass, &unlit_batches);
            }
            if n2 > 0 {
                pass.set_pipeline(&self.line2_pipe);
                pass.set_vertex_buffer(0, self.lines.as_ref().unwrap().slice(..));
                pass.draw(n3..n3 + n2, 0..1);
            }
        }
        // ---- bloom: bright parts, blurred (more passes = wider), added back ----
        if gs.bloom && env.as_ref().is_some_and(|e| e.bloom > 0.0) {
            let t = self.target.as_ref().unwrap();
            let mut steps: Vec<(&wgpu::RenderPipeline, &wgpu::TextureView, &wgpu::BindGroup, bool)> = vec![(&self.bright_pipe, &t.bloom_a.0, &t.color_bind, false)];
            for _ in 0..gs.bloom_quality.clamp(1, 4) {
                steps.push((&self.blur_h_pipe, &t.bloom_b.0, &t.bloom_a.1, false));
                steps.push((&self.blur_v_pipe, &t.bloom_a.0, &t.bloom_b.1, false));
            }
            steps.push((&self.bloom_add_pipe, &t.view, &t.bloom_a.1, true));
            for (pipe, view, src, keep) in steps {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("gex bloom"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: if keep { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) }, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_bind_group(1, src, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        // ---- last pass: edge smoothing (FXAA) and the color settings ----
        if final_pass {
            let t = self.target.as_ref().unwrap();
            enc.copy_texture_to_texture(t.color.as_image_copy(), t.post_tex.as_image_copy(), wgpu::Extent3d { width: t.size[0], height: t.size[1], depth_or_array_layers: 1 });
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gex final"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.final_pipe);
            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_bind_group(1, &t.post.1, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
        // Forget skinned buffers of objects that weren't drawn.
        let live: std::collections::HashSet<(ObjectId, String)> = list.meshes.iter().map(|m| (m.object, m.mesh.clone())).collect();
        self.skinned.retain(|k, _| live.contains(k));
        new_target
    }
}

fn make_linear(device: &wgpu::Device, anisotropy: u32) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        // Anisotropic filtering needs linear filtering everywhere, and 1, 2, 4, 8 or 16.
        anisotropy_clamp: anisotropy.clamp(1, 16).next_power_of_two().min(16) as u16,
        ..Default::default()
    })
}

/// Blocky 3D textures that still repeat across surfaces.
fn make_nearest_repeat(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// The shadow atlas (two cascades of `size` side by side) and the globals bind group that reads it.
fn make_shadow_map(device: &wgpu::Device, size: u32, layout: &wgpu::BindGroupLayout, globals: &wgpu::Buffer, samp: &wgpu::Sampler) -> (wgpu::TextureView, wgpu::BindGroup, u32) {
    let max = device.limits().max_texture_dimension_2d;
    let size = size.clamp(64, max / 2);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow map"),
        size: wgpu::Extent3d { width: size * 2, height: size, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(samp) },
        ],
    });
    (view, bg, size)
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
#[allow(clippy::too_many_arguments)]
fn make_tex(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, w: u32, h: u32, rgba: &[u8], mips: bool, format: wgpu::TextureFormat) -> Tex {
    let levels = if mips { 32 - w.max(h).max(1).leading_zeros() } else { 1 };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
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
    Tex { bind, view, size: [w, h] }
}

#[cfg(test)]
mod tests {
    use egui_wgpu::wgpu;
    /// The shaders parse and validate (a bad shader would otherwise only show when drawing).
    #[test]
    fn shaders_are_valid() {
        for (name, src) in [("shaders.wgsl", include_str!("shaders.wgsl")), ("shadow.wgsl", include_str!("shadow.wgsl"))] {
            let module = wgpu::naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}: {}", name, e.emit_to_string(src)));
            wgpu::naga::valid::Validator::new(wgpu::naga::valid::ValidationFlags::all(), wgpu::naga::valid::Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{}: {}", name, e.emit_to_string(src)));
        }
    }
}
