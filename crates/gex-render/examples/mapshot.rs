//! Renders a map from a point: `cargo run -p gex-render --example mapshot -- <dir> <map.bsp> <x> <y> <z> <yaw> <out.png>`
//! (position in Source units, as an entity's origin). For checking map loading by eye.

use egui_wgpu::wgpu;
use gex_core::glam::Vec3;
use gex_core::render::extract;
use gex_core::{Node, NodeKind, SceneTree, Variant};
use gex_render::{Cameras, Overlay, Renderer};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (dir, map) = (&a[0], &a[1]);
    let f = |i: usize| a[i].parse::<f32>().unwrap();
    let (x, y, z, yaw) = (f(2), f(3), f(4), f(5));
    let out = &a[6];
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.add(None, Node::new("R", NodeKind::Node3D));
    let mut m = Node::new("Map", NodeKind::Mesh3D);
    m.props.insert("mesh".into(), Variant::Text(format!("res://{}", map)));
    tree.add(Some(root), m);
    let mut c = Node::new("Cam", NodeKind::Camera3D);
    c.transform.position = Vec3::new(x, z + 64.0, -y) * 0.0254;
    c.transform.rotation = Vec3::new(-5.0, yaw - 90.0, 0.0);
    tree.add(Some(root), c);
    let mut env = Node::new("Env", NodeKind::Environment);
    env.props.insert("bloom".into(), Variant::Num(std::env::var("BLOOM").ok().and_then(|b| b.parse().ok()).unwrap_or(0.0)));
    tree.add(Some(root), env);
    let list = extract(&tree);
    let size = [960u32, 540u32];
    let cams = Cameras::for_game(&list, size, [960, 540]);
    let vfs = gex_assets::Vfs::new(std::path::Path::new(dir), &Default::default());
    let mut r = Renderer::new(&device, &queue);
    let t = std::time::Instant::now();
    r.render(&device, &queue, &vfs, size, &list, &cams, &Overlay::default());
    eprintln!("first frame (load + draw): {:?}", t.elapsed());
    let t = std::time::Instant::now();
    r.render(&device, &queue, &vfs, size, &list, &cams, &Overlay::default());
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    eprintln!("next frame: {:?}", t.elapsed());
    for p in r.problems.iter().take(5) {
        eprintln!("problem: {}", p);
    }
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
    let data = buf.slice(..).get_mapped_range().unwrap().to_vec();
    let mut px = Vec::new();
    for y in 0..size[1] {
        px.extend_from_slice(&data[(y * row) as usize..(y * row + size[0] * 4) as usize]);
    }
    image::RgbaImage::from_raw(size[0], size[1], px).unwrap().save(out).unwrap();
}
