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

## Sprite shaders

A Sprite2D's `shader` property (and DRAW_SPRITE's `shader:` option) takes a `.wgsl` file with
one function that decides each pixel's color:

```wgsl
fn sprite(base: vec4<f32>, uv: vec2<f32>, world: vec2<f32>, time: f32, values: vec4<f32>) -> vec4<f32> {
    // A white flash that fades in and out (values.x says how strong).
    let k = (sin(time * 6.0) * 0.5 + 0.5) * values.x;
    return vec4<f32>(mix(base.rgb, vec3<f32>(1.0), k), base.a);
}
```

| Input | |
|---|---|
| `base` | the picture's pixel times the sprite's color and opacity, with its effects (brightness, hue...), as it would be drawn |
| `uv` | where in the texture this pixel is (0 to 1; inside the frame, for sprite sheets) |
| `world` | where this pixel is in the 2D world |
| `time` | seconds since the game started |
| `values` | the sprite's `value_1` to `value_4` (or DRAW_SPRITE's `values: [a, b, c, d]`): numbers scripts can change every frame |

Colors are as they look on screen, 0 to 1. `sprite_texture(uv)` reads the picture anywhere
(without the sprite's color), for outlines, blurs or a shader that ignores the tint. What
`sprite` returns is drawn with the sprite's blend; an alpha near 0 draws nothing.

GameMaker shaders port straight over: `texture2D(gm_BaseTexture, v_vTexcoord)` is
`sprite_texture(uv)`, `v_vColour * texture2D(...)` is `base`, and a uniform is one of `values`.

## Sprite effects without a shader

Sprite2D (and DRAW_SPRITE) also have built-in effects that cost nothing extra: `brightness`
(-1 to 1, added), `hue` (degrees around the color wheel: Scratch's color effect is hue =
color / 200 × 360), `saturation` (1 normal, 0 grey) and `invert` (0 to 1).
