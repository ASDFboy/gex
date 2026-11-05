# Custom shaders

A Mesh3D's `shader` property takes a `.wgsl` file from your project. In it, write one function
that decides each pixel's color before lighting:

```wgsl
fn material(base: vec4<f32>, uv: vec2<f32>, world: vec3<f32>, normal: vec3<f32>, time: f32) -> vec4<f32> {
    // Blue stripes that scroll up the object.
    let stripe = step(0.5, fract(world.y * 4.0 + time));
    return vec4<f32>(mix(base.rgb, vec3<f32>(0.1, 0.4, 1.0), stripe), base.a);
}
```

| Input | |
|---|---|
| `base` | the texture times the mesh's `color` (what you'd normally see before lighting) |
| `uv` | the texture position (already scaled by `uv_scale_x/y`) |
| `world` | where this pixel is in the world, in metres |
| `normal` | which way the surface faces |
| `time` | seconds since the game (or editor) started, for animation |

What it returns is then lit, shadowed and fogged like any other surface (or shown as it is, if
the mesh is `unlit`). An alpha below 0.01 cuts a hole.

The language is [WGSL](https://www.w3.org/TR/WGSL/). A few useful pieces: `sin`, `cos`,
`fract`, `step`, `smoothstep`, `mix`, `clamp`, `length`, `dot`, `vec3<f32>(r, g, b)`.

If the shader has a mistake, the Console shows the error with its line, and the mesh is drawn
normally until the file is fixed (saving it reloads it).
