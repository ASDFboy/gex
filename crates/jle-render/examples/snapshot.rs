//! Renders a scene headless to a PNG: `cargo run -p jle-render --example snapshot -- <out.png> [3d|2d]`.
//! Used to check the renderer without a window.

use egui_wgpu::wgpu;
use jle_core::glam::Vec3;
use jle_core::render::extract;
use jle_core::{Node, NodeKind, SceneTree, Variant};
use jle_render::{Cameras, EditorCamera2D, EditorCamera3D, Overlay, Renderer, COLOR_FORMAT};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().cloned().unwrap_or_else(|| "snapshot.png".into());
    let mode_3d = args.get(1).map(|s| s == "3d").unwrap_or(true);
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("no adapter");
    eprintln!("adapter: {:?}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("no device");

    let mut tree = SceneTree::new();
    let root = tree.add(None, Node::new("R", NodeKind::Node3D));
    let mut cube = Node::new("Cube", NodeKind::Mesh3D);
    cube.transform.position = Vec3::new(0.0, 0.5, 0.0);
    tree.add(Some(root), cube);
    let mut s = Node::new("S", NodeKind::Sprite2D);
    s.transform.position = Vec3::new(100.0, 100.0, 0.0);
    s.props.insert("color".into(), Variant::Color([1.0, 0.3, 0.2, 1.0]));
    tree.add(Some(root), s);
    let list = extract(&tree);

    let size = [640u32, 360u32];
    let cams = Cameras::for_editor(mode_3d, &EditorCamera2D::default(), &EditorCamera3D::default(), size);
    let mut o = Overlay::default();
    for i in -20..=20 {
        let f = i as f32;
        o.line3(Vec3::new(f, 0.0, -20.0), Vec3::new(f, 0.0, 20.0), [0.5, 0.6, 0.7, 0.6]);
        o.line3(Vec3::new(-20.0, 0.0, f), Vec3::new(20.0, 0.0, f), [0.5, 0.6, 0.7, 0.6]);
    }
    let vfs = jle_assets::Vfs::new(std::path::Path::new("."), &Default::default());
    let mut r = Renderer::new(&device, &queue);
    r.render(&device, &queue, &vfs, size, &list, &cams, &o);

    // Read the target back.
    let tex = r.target_texture().unwrap();
    let row = (size[0] * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: (row * size[1]) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
        wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = buf.slice(..).get_mapped_range().expect("mapped").to_vec();
    let mut px = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for y in 0..size[1] {
        px.extend_from_slice(&data[(y * row) as usize..(y * row + size[0] * 4) as usize]);
    }
    assert_eq!(COLOR_FORMAT, wgpu::TextureFormat::Rgba8Unorm);
    image::RgbaImage::from_raw(size[0], size[1], px).unwrap().save(&out).unwrap();
    eprintln!("wrote {}", out);
}
