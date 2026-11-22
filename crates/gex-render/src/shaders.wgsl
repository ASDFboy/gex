// GEX shaders: lit meshes (with shadows and fog), shadow maps, sky, sprites, lines.

struct Light {
    // xyz = position, w = kind (0 directional, 1 point, 2 spot)
    pos: vec4<f32>,
    // xyz = direction it shines, w = cos(half cone) for spots
    dir: vec4<f32>,
    // rgb = color * energy, w = range
    color: vec4<f32>,
    // x = spot edge softness, y = falloff, z = specular strength, w = own shadow bias (< 0: project's)
    extra: vec4<f32>,
};

struct Globals {
    view_proj_3d: mat4x4<f32>,
    view_proj_2d: mat4x4<f32>,
    inv_view_proj_3d: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    // The far shadow cascade.
    light_view_proj1: mat4x4<f32>,
    // x = near cascade's reach, y = far cascade's reach, z = on
    shadow: vec4<f32>,
    // x = bloom strength, y = bloom threshold, z = time (seconds)
    post: vec4<f32>,
    eye: vec4<f32>,
    ambient: vec4<f32>,
    fog_color: vec4<f32>,
    // x = start, y = end, z = fog on, w = shadows on
    fog: vec4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    ground: vec4<f32>,
    // x = number of lights, y = sky texture on, z = shadow texel size
    counts: vec4<f32>,
    // x = exposure, y = specular on, z = normal maps on, w = lighting on
    light_opts: vec4<f32>,
    // x = shadow samples across, y = softness, z = bias, w = sun shadow strength
    shadow_opts: vec4<f32>,
    // x = exponential fog, y = fog density, z = sky brightness, w = tonemap
    fog_opts: vec4<f32>,
    // brightness, contrast, saturation, gamma
    grade0: vec4<f32>,
    // tint
    grade1: vec4<f32>,
    // x = vignette, y = grain, z = fxaa on, w = color grading on
    grade2: vec4<f32>,
    // width, height, 1/width, 1/height
    screen: vec4<f32>,
    lights: array<Light, 16>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_samp: sampler_comparison;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
// Meshes only: the normal map (flat blue when there's none), and baked light (maps).
@group(1) @binding(2) var normal_tex: texture_2d<f32>;
@group(1) @binding(3) var lightmap_tex: texture_2d<f32>;

fn encode(c: vec4<f32>) -> vec4<f32> {
    let lo = c.rgb * 12.92;
    let hi = 1.055 * pow(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(hi, lo, c.rgb <= vec3<f32>(0.0031308)), c.a);
}

struct InstanceIn {
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) color: vec4<f32>,
    @location(8) uv: vec4<f32>,
    // x = specular, y = emissive, z = unlit, w = flags (1 = normal map, 2 = lightmap)
    @location(9) mat: vec4<f32>,
    // rgb = glow color, a = rim strength
    @location(11) extra0: vec4<f32>,
    // rgb = rim color, a = alpha cut
    @location(12) extra1: vec4<f32>,
    // x = receives shadows, y = fogged, zw = texture scroll per second
    @location(13) extra2: vec4<f32>,
};

// Exposure, then the chosen tonemap (3D light can be brighter than the screen shows).
fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let x = max(c * g.light_opts.x, vec3<f32>(0.0));
    let t = i32(g.fog_opts.w + 0.5);
    if (t == 1) {
        // ACES (Narkowicz's fit).
        return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (t == 2) {
        return x / (x + vec3<f32>(1.0));
    }
    if (t == 3) {
        // Hable / Uncharted 2 filmic.
        let a = 0.15; let b = 0.50; let cc = 0.10; let d = 0.20; let e = 0.02; let f = 0.30;
        let y = x * 2.0;
        let m = ((y * (a * y + cc * b) + d * e) / (y * (a * y + b) + d * f)) - e / f;
        let w = vec3<f32>(11.2);
        let wm = ((w * (a * w + cc * b) + d * e) / (w * (a * w + b) + d * f)) - e / f;
        return m / wm;
    }
    return x;
}

fn finish3(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(tonemap(c.rgb), c.a);
}

// ---------- meshes ----------

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(10) uv2: vec2<f32>,
};

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world: vec3<f32>,
    @location(4) mat: vec4<f32>,
    @location(5) uv2: vec2<f32>,
    @location(6) extra0: vec4<f32>,
    @location(7) extra1: vec4<f32>,
    @location(8) extra2: vec4<f32>,
};

@vertex
fn vs_mesh(v: MeshIn, i: InstanceIn) -> MeshOut {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: MeshOut;
    let w = model * vec4<f32>(v.pos, 1.0);
    o.clip = g.view_proj_3d * w;
    o.world = w.xyz;
    o.normal = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    o.uv = v.uv * i.uv.zw + i.uv.xy + i.extra2.zw * g.post.z;
    o.color = i.color;
    o.mat = i.mat;
    o.uv2 = v.uv2;
    o.extra0 = i.extra0;
    o.extra1 = i.extra1;
    o.extra2 = i.extra2;
    return o;
}

fn shadow_tap(c: f32, uv: vec2<f32>, z: f32) -> f32 {
    // The atlas holds the two cascades side by side; stay inside this one's half.
    let t = g.counts.z;
    let u = clamp(uv.x, t, 1.0 - t) * 0.5 + c * 0.5;
    return textureSampleCompareLevel(shadow_map, shadow_samp, vec2<f32>(u, uv.y), z);
}

fn shadow_factor(world: vec3<f32>, n: vec3<f32>, own_bias: f32) -> f32 {
    if (g.shadow.z < 0.5) {
        return 1.0;
    }
    let d = distance(g.eye.xyz, world);
    if (d > g.shadow.y) {
        return 1.0;
    }
    var c = 0.0;
    var lc = g.light_view_proj * vec4<f32>(world + n * 0.04, 1.0);
    if (d > g.shadow.x) {
        c = 1.0;
        lc = g.light_view_proj1 * vec4<f32>(world + n * 0.12, 1.0);
    }
    let p = lc.xyz / lc.w;
    let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || p.z > 1.0) {
        return 1.0;
    }
    // A grid of samples, spread by the softness setting.
    let t = g.counts.z * 2.0 * g.shadow_opts.y;
    let bias = select(g.shadow_opts.z, own_bias, own_bias >= 0.0);
    let z = p.z - bias;
    let side = i32(g.shadow_opts.x + 0.5);
    let mid = f32(side - 1) * 0.5;
    var s = 0.0;
    for (var i = 0; i < side; i = i + 1) {
        for (var j = 0; j < side; j = j + 1) {
            s = s + shadow_tap(c, uv + vec2<f32>(f32(i) - mid, f32(j) - mid) * t, z);
        }
    }
    s = s / f32(side * side);
    s = mix(1.0, s, g.shadow_opts.w);
    // Fade out at the far edge instead of stopping in a line.
    let fade = smoothstep(g.shadow.y * 0.85, g.shadow.y, d);
    return mix(s, 1.0, fade);
}

// The surface normal, bent by the normal map (tangents worked out from screen derivatives).
fn surface_normal(o: MeshOut, nm: vec4<f32>) -> vec3<f32> {
    let n = normalize(o.normal);
    let dp1 = dpdx(o.world);
    let dp2 = dpdy(o.world);
    let duv1 = dpdx(o.uv);
    let duv2 = dpdy(o.uv);
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-20));
    let tbn = mat3x3<f32>(t * inv, b * inv, n);
    let mapped = normalize(tbn * (nm.xyz * 2.0 - 1.0));
    let has_normal = (u32(o.mat.w + 0.5) & 1u) != 0u && g.light_opts.z > 0.5;
    return select(n, mapped, has_normal && all(mapped == mapped));
}

fn shade(o: MeshOut, base: vec4<f32>, nm: vec4<f32>, lm: vec4<f32>) -> vec4<f32> {
    var n = surface_normal(o, nm);
    let baked = (u32(o.mat.w + 0.5) & 2u) != 0u;
    let view = normalize(g.eye.xyz - o.world);
    // Two-sided surfaces: light the side we're looking at.
    if (dot(n, view) < 0.0) {
        n = -n;
    }
    var light = g.ambient.rgb;
    if (baked) {
        // A map's baked light (stored at half strength) replaces the sun and the ambient.
        light = lm.rgb * 2.0;
    }
    let count = i32(g.counts.x);
    for (var k = 0; k < count; k = k + 1) {
        let l = g.lights[k];
        let kind = i32(l.pos.w);
        if (baked && kind == 0) {
            continue;
        }
        var to_light: vec3<f32>;
        var atten = 1.0;
        if (kind == 0) {
            to_light = -l.dir.xyz;
            if (k == 0 && o.extra2.x > 0.5) {
                atten = shadow_factor(o.world, n, l.extra.w);
            }
        } else {
            let d = l.pos.xyz - o.world;
            let dist = length(d);
            to_light = d / max(dist, 0.0001);
            let r = max(l.color.w, 0.001);
            let f = clamp(1.0 - pow(dist / r, max(l.extra.y, 0.1)), 0.0, 1.0);
            atten = f * f;
            if (kind == 2) {
                let c = dot(-to_light, normalize(l.dir.xyz));
                atten = atten * smoothstep(l.dir.w, mix(l.dir.w, 1.0, max(l.extra.x, 0.002)), c);
            }
        }
        let diff = max(dot(n, to_light), 0.0);
        let h = normalize(to_light + view);
        let spec = pow(max(dot(n, h), 0.0), 8.0 + 120.0 * o.mat.x) * o.mat.x * l.extra.z * g.light_opts.y;
        light = light + l.color.rgb * (diff + spec) * atten;
    }
    let glow = base.rgb * o.mat.y * o.extra0.rgb;
    var c = base.rgb * light + glow;
    if (o.mat.z > 0.5 || g.light_opts.w < 0.5) {
        // Unlit (or lighting turned off): the texture as it is (plus glow).
        c = base.rgb + glow;
    }
    // Rim light: brightest where the surface turns away from the camera.
    if (o.extra0.a > 0.0) {
        c = c + o.extra1.rgb * o.extra0.a * pow(1.0 - max(dot(n, view), 0.0), 3.0);
    }
    if (g.fog.z > 0.5 && o.extra2.y > 0.5) {
        let d = distance(g.eye.xyz, o.world);
        var f = clamp((d - g.fog.x) / max(g.fog.y - g.fog.x, 0.001), 0.0, 1.0);
        if (g.fog_opts.x > 0.5) {
            f = 1.0 - exp(-g.fog_opts.y * d);
        }
        c = mix(c, g.fog_color.rgb, f);
    }
    return vec4<f32>(c, base.a);
}

@fragment
fn fs_mesh(o: MeshOut) -> @location(0) vec4<f32> {
    let base = textureSample(tex, samp, o.uv) * o.color;
    let nm = textureSample(normal_tex, samp, o.uv);
    let lm = textureSample(lightmap_tex, samp, o.uv2);
    // Cut-out textures (fences, leaves): treat mostly clear texels as holes.
    if (base.a < o.extra1.a) {
        discard;
    }
    return encode(finish3(shade(o, vec4<f32>(base.rgb, 1.0), nm, lm)));
}

@fragment
fn fs_mesh_alpha(o: MeshOut) -> @location(0) vec4<f32> {
    let base = textureSample(tex, samp, o.uv) * o.color;
    let nm = textureSample(normal_tex, samp, o.uv);
    let lm = textureSample(lightmap_tex, samp, o.uv2);
    return encode(finish3(shade(o, base, nm, lm)));
}

// Multiply blending: the color comes out premultiplied, so clear parts leave things as they are.
@fragment
fn fs_mesh_mul(o: MeshOut) -> @location(0) vec4<f32> {
    let base = textureSample(tex, samp, o.uv) * o.color;
    let nm = textureSample(normal_tex, samp, o.uv);
    let lm = textureSample(lightmap_tex, samp, o.uv2);
    let c = encode(finish3(shade(o, base, nm, lm)));
    return vec4<f32>(c.rgb * c.a, c.a);
}

// ---------- sky ----------

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var o: SkyOut;
    o.clip = vec4<f32>(p, 1.0, 1.0);
    o.ndc = p;
    return o;
}

@fragment
fn fs_sky(o: SkyOut) -> @location(0) vec4<f32> {
    let a = g.inv_view_proj_3d * vec4<f32>(o.ndc, 0.0, 1.0);
    let b = g.inv_view_proj_3d * vec4<f32>(o.ndc, 1.0, 1.0);
    let dir = normalize(b.xyz / b.w - a.xyz / a.w);
    var c: vec3<f32>;
    if (g.counts.y > 0.5) {
        // An equirectangular panorama.
        let u = atan2(dir.x, -dir.z) / 6.2831853 + 0.5;
        let v = acos(clamp(dir.y, -1.0, 1.0)) / 3.14159265;
        c = textureSampleLevel(tex, samp, vec2<f32>(u, v), 0.0).rgb;
    } else if (dir.y >= 0.0) {
        c = mix(g.sky_horizon.rgb, g.sky_top.rgb, pow(dir.y, 0.6));
    } else {
        c = mix(g.sky_horizon.rgb, g.ground.rgb, pow(-dir.y, 0.4));
    }
    return encode(vec4<f32>(tonemap(c * g.fog_opts.z), 1.0));
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
    // brightness (added), hue shift (turns), toward grey, inverted
    @location(2) fx: vec4<f32>,
    // a custom sprite shader's four values
    @location(3) values: vec4<f32>,
    // where the pixel is in the 2D world
    @location(4) world: vec2<f32>,
};

@vertex
fn vs_sprite(v: SpriteIn, i: InstanceIn) -> SpriteOut {
    let model = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: SpriteOut;
    let w = model * vec4<f32>(v.pos, 0.0, 1.0);
    o.clip = g.view_proj_2d * w;
    o.uv = mix(i.uv.xy, i.uv.zw, v.uv);
    o.color = i.color;
    o.fx = i.mat;
    o.values = i.extra0;
    o.world = w.xy;
    return o;
}

// Screen colors (sRGB) back to linear light.
fn decode(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow(max((c + 0.055) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4<f32>(c.bg, k.wz), vec4<f32>(c.gb, k.xy), step(c.b, c.g));
    let q = mix(vec4<f32>(p.xyw, c.r), vec4<f32>(c.r, p.yzx), step(p.x, c.r));
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3<f32>(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let k = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    let p = abs(fract(c.xxx + k.xyz) * 6.0 - k.www);
    return c.z * mix(k.xxx, clamp(p - k.xxx, vec3<f32>(0.0), vec3<f32>(1.0)), c.y);
}

// A sprite's color effects, on its screen (sRGB) color.
fn sprite_fx(c0: vec3<f32>, fx: vec4<f32>) -> vec3<f32> {
    var c = c0;
    if (fx.y != 0.0) {
        var h = rgb_to_hsv(c);
        h.x = fract(h.x + fx.y);
        c = hsv_to_rgb(h);
    }
    if (fx.z != 0.0) {
        let l = dot(c, vec3<f32>(0.299, 0.587, 0.114));
        c = mix(c, vec3<f32>(l), fx.z);
    }
    c = c + vec3<f32>(fx.x);
    c = mix(c, vec3<f32>(1.0) - c, fx.w);
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The texture times the color, with the effects: in screen (sRGB) color.
fn sprite_base(o: SpriteOut) -> vec4<f32> {
    let t = textureSample(tex, samp, o.uv) * o.color;
    var c = encode(t);
    if (any(o.fx != vec4<f32>(0.0))) {
        c = vec4<f32>(sprite_fx(c.rgb, o.fx), c.a);
    }
    return c;
}

// A custom sprite shader can read the texture anywhere with this (screen colors).
fn sprite_texture(uv: vec2<f32>) -> vec4<f32> {
    return encode(textureSample(tex, samp, uv));
}

@fragment
fn fs_sprite(o: SpriteOut) -> @location(0) vec4<f32> {
    if (all(o.fx == vec4<f32>(0.0))) {
        let c = textureSample(tex, samp, o.uv) * o.color;
        if (c.a < 0.004) {
            discard;
        }
        return encode(c);
    }
    let c = sprite_base(o);
    if (c.a < 0.004) {
        discard;
    }
    return c;
}

@fragment
fn fs_sprite_mul(o: SpriteOut) -> @location(0) vec4<f32> {
    let c = sprite_base(o);
    return vec4<f32>(c.rgb * c.a, c.a);
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

// ---------- full-screen passes (2D light, bloom) ----------

fn screen_uv(ndc: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

// The 2D light map, multiplied over the picture.
@fragment
fn fs_copy(o: SkyOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(tex, samp, screen_uv(o.ndc), 0.0);
}

// Bloom: the bright parts of the picture.
@fragment
fn fs_bright(o: SkyOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(tex, samp, screen_uv(o.ndc), 0.0).rgb;
    let l = max(c.r, max(c.g, c.b));
    let k = smoothstep(g.post.y, g.post.y + 0.25, l);
    return vec4<f32>(c * k, 1.0);
}

fn blur(uv: vec2<f32>, dir: vec2<f32>) -> vec4<f32> {
    let px = dir / vec2<f32>(textureDimensions(tex, 0));
    var c = textureSampleLevel(tex, samp, uv, 0.0).rgb * 0.227027;
    c = c + textureSampleLevel(tex, samp, uv + px * 1.3846, 0.0).rgb * 0.3162162;
    c = c + textureSampleLevel(tex, samp, uv - px * 1.3846, 0.0).rgb * 0.3162162;
    c = c + textureSampleLevel(tex, samp, uv + px * 3.2308, 0.0).rgb * 0.0702703;
    c = c + textureSampleLevel(tex, samp, uv - px * 3.2308, 0.0).rgb * 0.0702703;
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_blur_h(o: SkyOut) -> @location(0) vec4<f32> {
    return blur(screen_uv(o.ndc), vec2<f32>(1.0, 0.0));
}

@fragment
fn fs_blur_v(o: SkyOut) -> @location(0) vec4<f32> {
    return blur(screen_uv(o.ndc), vec2<f32>(0.0, 1.0));
}

// Bloom added back on top, scaled by its strength.
@fragment
fn fs_bloom_add(o: SkyOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(tex, samp, screen_uv(o.ndc), 0.0).rgb * g.post.x;
    return vec4<f32>(c, 1.0);
}

// ---------- the last pass: FXAA, then the color settings ----------

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

fn px(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(tex, samp, uv, 0.0).rgb;
}

// FXAA (the small "console" version): blur along edges, found by comparing neighbours.
fn fxaa(uv: vec2<f32>) -> vec3<f32> {
    let r = g.screen.zw;
    let nw = luma(px(uv + vec2<f32>(-1.0, -1.0) * r));
    let ne = luma(px(uv + vec2<f32>(1.0, -1.0) * r));
    let sw = luma(px(uv + vec2<f32>(-1.0, 1.0) * r));
    let se = luma(px(uv + vec2<f32>(1.0, 1.0) * r));
    let m = luma(px(uv));
    let lo = min(m, min(min(nw, ne), min(sw, se)));
    let hi = max(m, max(max(nw, ne), max(sw, se)));
    var dir = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
    let reduce = max((nw + ne + sw + se) * 0.25 * 0.125, 1.0 / 128.0);
    let k = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
    dir = clamp(dir * k, vec2<f32>(-8.0), vec2<f32>(8.0)) * r;
    let a = 0.5 * (px(uv + dir * (1.0 / 3.0 - 0.5)) + px(uv + dir * (2.0 / 3.0 - 0.5)));
    let b = a * 0.5 + 0.25 * (px(uv - dir * 0.5) + px(uv + dir * 0.5));
    let lb = luma(b);
    if (lb < lo || lb > hi) {
        return a;
    }
    return b;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs_final(o: SkyOut) -> @location(0) vec4<f32> {
    let uv = screen_uv(o.ndc);
    var c = px(uv);
    if (g.grade2.z > 0.5) {
        c = fxaa(uv);
    }
    if (g.grade2.w > 0.5) {
        c = c + vec3<f32>(g.grade0.x);
        c = (c - vec3<f32>(0.5)) * g.grade0.y + vec3<f32>(0.5);
        let l = luma(c);
        c = mix(vec3<f32>(l), c, g.grade0.z);
        c = pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / max(g.grade0.w, 0.01)));
        c = c * g.grade1.rgb;
        let d = distance(uv, vec2<f32>(0.5)) * 1.41421;
        c = c * (1.0 - g.grade2.x * smoothstep(0.3, 1.0, d));
        if (g.grade2.y > 0.0) {
            let n = hash(uv * g.screen.xy + vec2<f32>(fract(g.post.z * 7.13) * 100.0)) - 0.5;
            c = c + vec3<f32>(n * g.grade2.y * 0.25);
        }
    }
    // Alpha is kept: the editor draws the view over its own backdrop.
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), textureSampleLevel(tex, samp, uv, 0.0).a);
}

// Custom material shaders: a project .wgsl defines
//     fn material(base: vec4<f32>, uv: vec2<f32>, world: vec3<f32>, normal: vec3<f32>, time: f32) -> vec4<f32>
// and the engine appends fs_custom (below) after it.
