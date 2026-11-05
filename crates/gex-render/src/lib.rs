//! gex-render: draws a `RenderList` into an offscreen texture with wgpu. The editor
//! shows that texture in its viewport; the game player shows it full-window. Labels
//! (text) are left to the caller, who draws them with egui on top.

pub mod camera;
pub mod mesh;

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use gex_assets::Vfs;
use gex_core::glam::{Mat4, Vec3};
use gex_core::render::RenderList;
use wgpu::util::DeviceExt;

pub use camera::{Cameras, EditorCamera2D, EditorCamera3D};

/// The viewport target holds sRGB-encoded values (the shaders encode), which is what
/// egui expects of the textures it shows.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Image assets: sampled as linear color.
const TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightU {
    dir: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    vp3: [[f32; 4]; 4],
    vp2: [[f32; 4]; 4],
    ambient: [f32; 4],
    counts: [f32; 4],
    lights: [LightU; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    /// Texture rectangle: u0, v0, u1, v1.
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

pub struct Renderer {
    mesh_pipe: wgpu::RenderPipeline,
    sprite_pipe: wgpu::RenderPipeline,
    line3_pipe: wgpu::RenderPipeline,
    line2_pipe: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    meshes: HashMap<&'static str, GpuMesh>,
    quad: GpuMesh,
    /// Path → texture, or the reason it couldn't load.
    textures: HashMap<(String, bool), Result<Tex, String>>,
    white_linear: Tex,
    white_nearest: Tex,
    missing: Tex,
    circle: Tex,
    instances: Option<wgpu::Buffer>,
    lines: Option<wgpu::Buffer>,
    target: Option<Target>,
    /// Texture problems not reported yet (the caller logs them once).
    pub problems: Vec<String>,
}

fn srgb_to_linear(c: [f32; 4]) -> [f32; 4] {
    let f = |x: f32| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gex shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
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

        let blend = Some(wgpu::ColorTargetState {
            format: COLOR_FORMAT,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        });
        let depth = |write: bool, cmp: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(cmp),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let pipe = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    cull: Option<wgpu::Face>,
                    ds: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                primitive: wgpu::PrimitiveState { topology, cull_mode: cull, ..Default::default() },
                depth_stencil: ds,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[blend.clone()],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let mesh_pipe = pipe(
            "meshes",
            &layout,
            "vs_mesh",
            "fs_mesh",
            &[Some(mesh_layout), Some(instance_layout.clone())],
            tri,
            Some(wgpu::Face::Back),
            depth(true, wgpu::CompareFunction::Less),
        );
        let sprite_pipe = pipe(
            "sprites",
            &layout,
            "vs_sprite",
            "fs_sprite",
            &[Some(sprite_layout), Some(instance_layout)],
            tri,
            None,
            depth(false, wgpu::CompareFunction::Always),
        );
        let lines = wgpu::PrimitiveTopology::LineList;
        let line3_pipe = pipe("lines 3d", &line_layout, "vs_line", "fs_line", &[Some(line_vlayout.clone())], lines, None, depth(false, wgpu::CompareFunction::LessEqual));
        let line2_pipe = pipe("lines 2d", &line_layout, "vs_line", "fs_line", &[Some(line_vlayout)], lines, None, depth(false, wgpu::CompareFunction::Always));

        let sampler = |f: wgpu::FilterMode, a: wgpu::AddressMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: a,
                address_mode_v: a,
                address_mode_w: a,
                mag_filter: f,
                min_filter: f,
                ..Default::default()
            })
        };
        let linear = sampler(wgpu::FilterMode::Linear, wgpu::AddressMode::Repeat);
        let nearest = sampler(wgpu::FilterMode::Nearest, wgpu::AddressMode::ClampToEdge);

        let upload = |m: mesh::MeshData| GpuMesh {
            vb: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&m.vertices), usage: wgpu::BufferUsages::VERTEX }),
            ib: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(&m.indices), usage: wgpu::BufferUsages::INDEX }),
            count: m.indices.len() as u32,
        };
        let mut meshes = HashMap::new();
        meshes.insert("cube", upload(mesh::cube()));
        meshes.insert("plane", upload(mesh::plane()));
        meshes.insert("sphere", upload(mesh::sphere(16, 24)));
        let quad_v: [f32; 16] = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let quad = GpuMesh {
            vb: device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("quad"), contents: bytemuck::cast_slice(&quad_v), usage: wgpu::BufferUsages::VERTEX }),
            ib: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("quad"),
                contents: bytemuck::cast_slice(&[0u32, 1, 2, 0, 2, 3]),
                usage: wgpu::BufferUsages::INDEX,
            }),
            count: 6,
        };

        let mut check = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                check.extend_from_slice(if (x + y) % 2 == 0 { &[255, 0, 255, 255] } else { &[20, 20, 20, 255] });
            }
        }
        let white_linear = make_tex(device, queue, &tex_layout, &linear, 1, 1, &[255; 4]);
        let white_nearest = make_tex(device, queue, &tex_layout, &nearest, 1, 1, &[255; 4]);
        let missing = make_tex(device, queue, &tex_layout, &nearest, 8, 8, &check);
        // "@circle": a soft-edged disc for DRAW_CIRCLE.
        let mut disc = Vec::with_capacity(64 * 64 * 4);
        for y in 0..64 {
            for x in 0..64 {
                let d = ((x as f32 - 31.5).powi(2) + (y as f32 - 31.5).powi(2)).sqrt();
                let a = ((32.0 - d) * 1.5).clamp(0.0, 1.0);
                disc.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
            }
        }
        let circle = make_tex(device, queue, &tex_layout, &linear, 64, 64, &disc);
        Renderer {
            mesh_pipe,
            sprite_pipe,
            line3_pipe,
            line2_pipe,
            globals,
            globals_bg,
            tex_layout,
            linear,
            nearest,
            meshes,
            quad,
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

    /// Forgets loaded textures (after files change on disk).
    pub fn reload_textures(&mut self) {
        self.textures.clear();
    }

    /// Loads (once) the texture at an asset path. Pixel textures use nearest filtering.
    fn ensure_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vfs: &Vfs, path: &str, pixel: bool) {
        let key = (path.to_string(), pixel);
        if self.textures.contains_key(&key) {
            return;
        }
        let sampler = if pixel { &self.nearest } else { &self.linear };
        let t = gex_assets::load_image(vfs, path).map(|img| make_tex(device, queue, &self.tex_layout, sampler, img.width, img.height, &img.pixels));
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

    /// Draws a frame. Returns true when the target texture was (re)created, so a texture
    /// registered with egui must be updated to the new view.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, vfs: &Vfs, size: [u32; 2], list: &RenderList, cams: &Cameras, overlay: &Overlay) -> bool {
        let new_target = self.ensure_target(device, size);
        // Textures first (loading needs &mut self).
        for s in &list.sprites {
            if !s.texture.is_empty() && !s.texture.starts_with('@') {
                self.ensure_texture(device, queue, vfs, &s.texture, true);
            }
        }
        for m in &list.meshes {
            if !m.texture.is_empty() {
                self.ensure_texture(device, queue, vfs, &m.texture, false);
            }
        }

        // Globals.
        let mut lights = [LightU { dir: [0.0; 4], color: [0.0; 4] }; 4];
        let mut n = 0;
        for l in list.lights.iter().take(4) {
            lights[n] = LightU { dir: [l.dir.x, l.dir.y, l.dir.z, 0.0], color: [l.color[0], l.color[1], l.color[2], 1.0] };
            n += 1;
        }
        if n == 0 {
            // No lights in the scene: a soft default so things are visible.
            lights[0] = LightU { dir: Vec3::new(-0.4, -1.0, -0.6).normalize().extend(0.0).into(), color: [0.85, 0.85, 0.85, 1.0] };
            n = 1;
        }
        let g = Globals {
            vp3: cams.view_proj_3d.to_cols_array_2d(),
            vp2: cams.view_proj_2d.to_cols_array_2d(),
            ambient: [0.18, 0.18, 0.22, 1.0],
            counts: [n as f32, 0.0, 0.0, 0.0],
            lights,
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));

        // Instances: meshes grouped by (mesh, texture), then sprites in layer order.
        let mut inst: Vec<Instance> = Vec::new();
        let mut mesh_batches: Vec<(&'static str, String, u32, u32)> = Vec::new();
        let mut order: Vec<usize> = (0..list.meshes.len()).collect();
        order.sort_by(|a, b| {
            let (x, y) = (&list.meshes[*a], &list.meshes[*b]);
            mesh::primitive(&x.mesh).cmp(mesh::primitive(&y.mesh)).then(x.texture.cmp(&y.texture))
        });
        for i in order {
            let m = &list.meshes[i];
            let prim = mesh::primitive(&m.mesh);
            let start = inst.len() as u32;
            inst.push(Instance { model: m.model.to_cols_array_2d(), color: srgb_to_linear(m.color), uv: [0.0, 0.0, m.uv_scale[0], m.uv_scale[1]] });
            match mesh_batches.last_mut() {
                Some(b) if b.0 == prim && b.1 == m.texture => b.3 += 1,
                _ => mesh_batches.push((prim, m.texture.clone(), start, 1)),
            }
        }
        let mut sprite_batches: Vec<(String, u32, u32)> = Vec::new();
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
        if !inst.is_empty() {
            upload(device, queue, &mut self.instances, bytemuck::cast_slice(&inst), wgpu::BufferUsages::VERTEX);
        }
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

        let t = self.target.as_ref().unwrap();
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("gex frame") });
        {
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
            if cams.draw_3d && !mesh_batches.is_empty() {
                pass.set_pipeline(&self.mesh_pipe);
                pass.set_vertex_buffer(1, self.instances.as_ref().unwrap().slice(..));
                for (prim, tex, start, count) in &mesh_batches {
                    let m = &self.meshes[prim];
                    pass.set_bind_group(1, &self.tex(tex, false).bind, &[]);
                    pass.set_vertex_buffer(0, m.vb.slice(..));
                    pass.set_index_buffer(m.ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..m.count, 0, *start..start + count);
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
        new_target
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

fn make_tex(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, w: u32, h: u32, rgba: &[u8]) -> Tex {
    let tex = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEX_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        Default::default(),
        rgba,
    );
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
