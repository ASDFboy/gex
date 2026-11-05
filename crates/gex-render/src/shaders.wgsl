// One shader file for the three pipelines: meshes (3D, lit), sprites (2D) and lines.

struct Light {
    dir: vec4<f32>,
    color: vec4<f32>,
};

struct Globals {
    view_proj_3d: mat4x4<f32>,
    view_proj_2d: mat4x4<f32>,
    ambient: vec4<f32>,
    // x = number of lights
    counts: vec4<f32>,
    lights: array<Light, 4>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct InstanceIn {
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    @location(8) uv: vec4<f32>,
};

fn encode(c: vec4<f32>) -> vec4<f32> {
    let lo = c.rgb * 12.92;
    let hi = 1.055 * pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(hi, lo, c.rgb <= vec3<f32>(0.0031308)), c.a);
}

// ---------- meshes ----------

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_mesh(v: MeshIn, i: InstanceIn) -> MeshOut {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: MeshOut;
    o.clip = g.view_proj_3d * model * vec4<f32>(v.pos, 1.0);
    o.normal = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    o.uv = v.uv * i.uv.zw + i.uv.xy;
    o.color = i.color;
    return o;
}

@fragment
fn fs_mesh(o: MeshOut) -> @location(0) vec4<f32> {
    let base = textureSample(tex, samp, o.uv) * o.color;
    var light = g.ambient.rgb;
    let n = normalize(o.normal);
    for (var k = 0; k < i32(g.counts.x); k = k + 1) {
        light = light + g.lights[k].color.rgb * max(dot(n, -g.lights[k].dir.xyz), 0.0);
    }
    return encode(vec4<f32>(base.rgb * light, base.a));
}

// ---------- sprites ----------

struct SpriteIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct SpriteOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_sprite(v: SpriteIn, i: InstanceIn) -> SpriteOut {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: SpriteOut;
    o.clip = g.view_proj_2d * model * vec4<f32>(v.pos, 0.0, 1.0);
    o.uv = mix(i.uv.xy, i.uv.zw, v.uv);
    o.color = i.color;
    return o;
}

@fragment
fn fs_sprite(o: SpriteOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, o.uv) * o.color;
    if (c.a < 0.004) {
        discard;
    }
    return encode(c);
}

// ---------- lines (grid, gizmos, collider outlines) ----------

struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
    // 0 = 3D world, 1 = 2D world
    @location(2) space: f32,
};

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var o: LineOut;
    if (v.space > 0.5) {
        o.clip = g.view_proj_2d * vec4<f32>(v.pos, 1.0);
    } else {
        o.clip = g.view_proj_3d * vec4<f32>(v.pos, 1.0);
    }
    o.color = v.color;
    return o;
}

@fragment
fn fs_line(o: LineOut) -> @location(0) vec4<f32> {
    return o.color;
}
