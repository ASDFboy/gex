// Depth-only pass from the sun, for shadows.

@group(0) @binding(0) var<uniform> shadow_vp: mat4x4<f32>;

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct InstanceIn {
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    @location(8) uv: vec4<f32>,
};

@vertex
fn vs_shadow(v: MeshIn, i: InstanceIn) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    return shadow_vp * model * vec4<f32>(v.pos, 1.0);
}
