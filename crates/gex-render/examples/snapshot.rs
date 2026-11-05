//! Renders a scene headless to a PNG: `cargo run -p gex-render --example snapshot -- <out.png> [3d|2d]`.
//! Used to check the renderer without a window.

use egui_wgpu::wgpu;
use gex_core::glam::Vec3;
use gex_core::render::extract;
use gex_core::{Node, NodeKind, SceneTree, Variant};
use gex_render::{Cameras, EditorCamera2D, EditorCamera3D, Overlay, Renderer, COLOR_FORMAT};

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
    let add = |t: &mut SceneTree, kind, pos: Vec3, f: &dyn Fn(&mut Node)| {
        let mut n = Node::new("n", kind);
        n.transform.position = pos;
        f(&mut n);
        t.add(Some(root), n);
    };
    add(&mut tree, NodeKind::Environment, Vec3::ZERO, &|n| {
        n.props.insert("fog_start".into(), Variant::Num(8.0));
        n.props.insert("fog_end".into(), Variant::Num(30.0));
        n.props.insert("bloom".into(), Variant::Num(std::env::var("GEX_SNAP_BLOOM").ok().and_then(|b| b.parse().ok()).unwrap_or(0.0)));
    });
    add(&mut tree, NodeKind::Sprite3D, Vec3::new(-1.0, 2.0, 0.5), &|n| {
        n.props.insert("color".into(), Variant::Color([0.2, 1.0, 0.4, 1.0]));
    });
    add(&mut tree, NodeKind::Mesh3D, Vec3::new(0.0, 2.2, -1.5), &|n| {
        n.props.insert("mesh".into(), Variant::Text("sphere".into()));
        n.props.insert("emissive".into(), Variant::Num(3.0));
        n.props.insert("color".into(), Variant::Color([1.0, 0.8, 0.3, 1.0]));
        if let Ok(sh) = std::env::var("GEX_SNAP_SHADER") {
            n.props.insert("shader".into(), Variant::Res(sh));
        }
    });
    add(&mut tree, NodeKind::Light3D, Vec3::ZERO, &|n| n.transform.rotation = Vec3::new(-50.0, 30.0, 0.0));
    add(&mut tree, NodeKind::Light3D, Vec3::new(2.0, 1.0, 1.5), &|n| {
        n.props.insert("type".into(), Variant::Text("point".into()));
        n.props.insert("color".into(), Variant::Color([1.0, 0.2, 0.1, 1.0]));
        n.props.insert("energy".into(), Variant::Num(3.0));
        n.props.insert("range".into(), Variant::Num(4.0));
    });
    add(&mut tree, NodeKind::Mesh3D, Vec3::ZERO, &|n| {
        n.props.insert("mesh".into(), Variant::Text("plane".into()));
        n.transform.scale = Vec3::new(40.0, 1.0, 40.0);
    });
    add(&mut tree, NodeKind::Mesh3D, Vec3::new(0.0, 0.5, 0.0), &|_| {});
    add(&mut tree, NodeKind::Mesh3D, Vec3::new(2.0, 1.0, 0.0), &|n| n.props.insert("mesh".into(), Variant::Text("sphere".into())).map(|_| ()).unwrap_or(()));
    add(&mut tree, NodeKind::Mesh3D, Vec3::new(-2.0, 0.6, 1.0), &|n| {
        n.props.insert("color".into(), Variant::Color([0.3, 0.6, 1.0, 0.4]));
    });
    // GEX_SNAP_MODEL=dir:file.glb:animation adds a model (from that folder) to the scene.
    if let Ok(spec) = std::env::var("GEX_SNAP_MODEL") {
        let parts: Vec<&str> = spec.split(':').collect();
        let file = format!("res://{}", parts.get(1).copied().unwrap_or(""));
        let anim = parts.get(2).copied().unwrap_or("").to_string();
        add(&mut tree, NodeKind::Mesh3D, Vec3::new(0.0, 0.0, 2.0), &|n| {
            n.props.insert("mesh".into(), Variant::Text(file.clone()));
            n.props.insert("animation".into(), Variant::Text(anim.clone()));
            n.props.insert("anim_time".into(), Variant::Num(0.35));
            n.transform.scale = Vec3::splat(0.02);
            n.transform.rotation.y = 60.0;
        });
    }
    let mut s = Node::new("S", NodeKind::Sprite2D);
    s.transform.position = Vec3::new(100.0, 100.0, 0.0);
    s.props.insert("color".into(), Variant::Color([1.0, 0.3, 0.2, 1.0]));
    tree.add(Some(root), s);
    // 2D lighting: a dark camera ambient and one light.
    if std::env::var("GEX_SNAP_DARK").is_ok() {
        let mut c = Node::new("Cam", NodeKind::Camera2D);
        c.transform.position = Vec3::new(320.0, 180.0, 0.0);
        c.props.insert("ambient".into(), Variant::Color([0.15, 0.15, 0.25, 1.0]));
        tree.add(Some(root), c);
        let mut bg = Node::new("BG", NodeKind::Sprite2D);
        bg.transform.position = Vec3::new(320.0, 180.0, 0.0);
        bg.props.insert("width".into(), Variant::Num(200.0));
        bg.props.insert("height".into(), Variant::Num(100.0));
        bg.props.insert("repeat_x".into(), Variant::Bool(true));
        bg.props.insert("parallax_x".into(), Variant::Num(0.5));
        bg.props.insert("layer".into(), Variant::Num(-1.0));
        bg.props.insert("color".into(), Variant::Color([0.5, 0.6, 0.7, 1.0]));
        tree.add(Some(root), bg);
        let mut l = Node::new("L", NodeKind::Light2D);
        l.transform.position = Vec3::new(120.0, 120.0, 0.0);
        tree.add(Some(root), l);
    }
    let list = extract(&tree);

    let size = [640u32, 360u32];
    let cams = Cameras::for_editor(mode_3d, &EditorCamera2D::default(), &EditorCamera3D::default(), size);
    let o = Overlay::default();
    let dir = std::env::var("GEX_SNAP_MODEL").ok().and_then(|s| s.split(':').next().map(|d| d.to_string())).unwrap_or_else(|| ".".into());
    let vfs = gex_assets::Vfs::new(std::path::Path::new(&dir), &Default::default());
    let mut r = Renderer::new(&device, &queue);
    r.render(&device, &queue, &vfs, size, &list, &cams, &o);
    for p in &r.problems {
        eprintln!("problem: {}", p);
    }

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
