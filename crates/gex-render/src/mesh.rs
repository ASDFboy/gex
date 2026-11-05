//! Built-in primitive meshes. Unit sized and centered: scale the node to resize them.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

pub fn cube() -> MeshData {
    let mut v = Vec::new();
    let mut i = Vec::new();
    // (normal, u axis, v axis)
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    for (n, u, w) in faces {
        let base = v.len() as u32;
        for (su, sv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = [0, 1, 2].map(|k| 0.5 * (n[k] + su * u[k] + sv * w[k]));
            v.push(Vertex { pos: p, normal: n, uv: [(su + 1.0) / 2.0, 1.0 - (sv + 1.0) / 2.0] });
        }
        i.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    MeshData { vertices: v, indices: i }
}

pub fn plane() -> MeshData {
    let n = [0.0, 1.0, 0.0];
    let vertices = vec![
        Vertex { pos: [-0.5, 0.0, 0.5], normal: n, uv: [0.0, 1.0] },
        Vertex { pos: [0.5, 0.0, 0.5], normal: n, uv: [1.0, 1.0] },
        Vertex { pos: [0.5, 0.0, -0.5], normal: n, uv: [1.0, 0.0] },
        Vertex { pos: [-0.5, 0.0, -0.5], normal: n, uv: [0.0, 0.0] },
    ];
    MeshData { vertices, indices: vec![0, 1, 2, 0, 2, 3] }
}

/// A square standing up, facing +Z (sprites in 3D, particles).
pub fn quad() -> MeshData {
    let n = [0.0, 0.0, 1.0];
    let vertices = vec![
        Vertex { pos: [-0.5, -0.5, 0.0], normal: n, uv: [0.0, 1.0] },
        Vertex { pos: [0.5, -0.5, 0.0], normal: n, uv: [1.0, 1.0] },
        Vertex { pos: [0.5, 0.5, 0.0], normal: n, uv: [1.0, 0.0] },
        Vertex { pos: [-0.5, 0.5, 0.0], normal: n, uv: [0.0, 0.0] },
    ];
    MeshData { vertices, indices: vec![0, 1, 2, 0, 2, 3] }
}

pub fn sphere(rings: u32, segments: u32) -> MeshData {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for r in 0..=rings {
        let phi = std::f32::consts::PI * r as f32 / rings as f32;
        for s in 0..=segments {
            let theta = 2.0 * std::f32::consts::PI * s as f32 / segments as f32;
            let n = [phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin()];
            vertices.push(Vertex { pos: n.map(|c| c * 0.5), normal: n, uv: [s as f32 / segments as f32, r as f32 / rings as f32] });
        }
    }
    let row = segments + 1;
    for r in 0..rings {
        for s in 0..segments {
            let a = r * row + s;
            let b = a + row;
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    MeshData { vertices, indices }
}

/// The primitive a Mesh3D's "mesh" property names (unknown names show as a cube).
pub fn primitive(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "sphere" | "ball" => "sphere",
        "plane" | "floor" => "plane",
        "quad" => "quad",
        _ => "cube",
    }
}
