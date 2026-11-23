# Project settings

Project settings (**File → Project settings**) has **Display**, **Graphics** and **Physics** pages.
Each setting shows its help when you hover its name. Graphics changes show in the view at once;
**Save** keeps them in `project.jproj`.

Scripts change any of them while the game runs, and read them back:

```
SET_GRAPHICS("bloom", FALSE)          >> a low-detail mode
SET_GRAPHICS("render_scale", 0.75)
SET_PHYSICS("gravity_2d", [0, 400])    >> moon gravity
SET_DISPLAY("scale_mode", "pixel")     >> crisp whole-number scaling
SET_DISPLAY("width", 1920)             >> a resolution option
SAVE_DISPLAY()                         >> keep display choices for next time
IF GRAPHICS("shadows") { ... }
all = PHYSICS()                        >> every physics setting, as a table
```

These are the project-wide settings. Each object also has its own (see [OBJECTS.md](OBJECTS.md)):
a sprite's `blend` and `filter`, a mesh's `rim` and `fog`, a light's `softness`, a body's
`linear_damping`... and the Environment object's own color settings multiply the project's.

## Display

The game is drawn at its resolution (`width` × `height`) and then scaled to fill the window or
the screen, the way `scale_mode` says. Full screen always covers the whole screen; F11 or Alt+Enter switch.

Settings scripts change with SET_GRAPHICS, SET_PHYSICS, SET_DISPLAY and SET_ACTION last until the game
stops, across CHANGE_BLUEPRINT. SAVE_DISPLAY() keeps the display settings on the player's computer: the
exported game starts with them next time (playing in the editor always uses the project's).

### Resolution

| Setting | Default | What it does |
|---|---|---|
| `width` | `1280` | How many pixels across the game is drawn at (its resolution). Positions in a 2D game with no Camera2D go from 0 to this. (64 to 16384) |
| `height` | `720` | How many pixels down the game is drawn at. (64 to 16384) |

### Window

| Setting | Default | What it does |
|---|---|---|
| `window_scale` | `1.0` | How big the window opens compared to the resolution: 3 opens a 320×180 game in a 960×540 window (kept inside the screen). (0.25 to 8.0) |
| `fullscreen` | `false` | Fills the whole screen (borderless). F11 or Alt+Enter switches while playing. |
| `resizable` | `true` | The player can resize the window (the picture scales to fit, see scale_mode). |
| `vsync` | `true` | Waits for the screen to refresh before showing each frame: no tearing, frames capped to the screen's rate. Takes effect when the game starts (SAVE_DISPLAY keeps a change for next time). |

### Scaling

| Setting | Default | What it does |
|---|---|---|
| `scale_mode` | `"fit"` | How the picture fills a window or screen that isn't the resolution's size. fit: as big as it goes keeping its shape, with bars filling the rest (drawn at that size, so it stays sharp). stretch: fills everything, squashed if the shapes differ. pixel: scaled by whole numbers only (2×, 3×…) so pixel art stays even. expand: drawn at the window's own size, showing more of the world instead of scaling. (fit, stretch, pixel, expand) |
| `scale_filter` | `"pixel"` | When the picture is scaled: smooth blends neighbouring pixels; pixel keeps them sharp squares (pixel art). (smooth, pixel) |
| `bar_color` | `[0.0, 0.0, 0.0, 1.0]` | The color of the bars around the picture (fit and pixel). |

## Graphics

### Picture

| Setting | Default | What it does |
|---|---|---|
| `render_scale` | `1.0` | How many pixels are drawn for each pixel on screen: below 1 is faster and softer, above 1 is sharper (and slower). (0.25 to 2.0) |
| `antialiasing` | `"off"` | Smooths jagged edges: fxaa is a fast blur along edges; off keeps every pixel crisp. One of: off, fxaa. |
| `fps_cap` | `0` | The most frames drawn each second (0 = no limit beyond vsync). Lower saves power. (0 to 1000) |

### Textures

| Setting | Default | What it does |
|---|---|---|
| `texture_filter` | `"smooth"` | 3D textures: smooth blends between texels; pixel keeps them blocky (retro look). One of: smooth, pixel. |
| `sprite_filter` | `"pixel"` | 2D sprites and tiles: pixel keeps pixel art sharp; smooth blends (for painted art). One of: pixel, smooth. |
| `mipmaps` | `true` | Smaller copies of each texture for far-away surfaces: less shimmering at a distance. |
| `anisotropy` | `8` | Sharpness of textures seen at a slant, like a floor stretching away (1 = off, 16 = sharpest). (1 to 16) |

### Lighting

| Setting | Default | What it does |
|---|---|---|
| `lighting` | `true` | 3D lights and ambient light. Off draws every 3D surface at its plain color. |
| `max_lights` | `8` | The most 3D lights used at once (the shadow-casting sun counts first). (0 to 16) |
| `specular` | `true` | Shiny highlights on 3D surfaces. |
| `normal_maps` | `true` | Surface bumps from normal-map textures. |
| `lights_2d` | `true` | 2D lighting (a Camera2D's ambient dark and Light2D objects). |

### Shadows

| Setting | Default | What it does |
|---|---|---|
| `shadows` | `true` | Shadows from the sun (the first directional Light3D with shadows on). |
| `shadow_resolution` | `2048` | Pixels across each shadow map: higher is sharper and uses more memory. (256 to 8192) |
| `shadow_quality` | `"medium"` | How many samples soften each shadow edge: 4, 9 or 16. One of: low, medium, high. |
| `shadow_softness` | `1.0` | How blurry shadow edges are (0 = hard). (0.0 to 6.0) |
| `shadow_bias` | `0.002` | Pushes shadows back a little so surfaces don't shadow themselves (acne). Too much and shadows float. (0.0 to 0.05) |
| `shadow_distance_scale` | `1.0` | Multiplies every Environment's shadow distance: lower is sharper and shorter. (0.1 to 4.0) |

### Effects

| Setting | Default | What it does |
|---|---|---|
| `sky` | `true` | Draws the Environment's sky (off shows the background color). |
| `fog` | `true` | Fog from Environment objects. |
| `bloom` | `true` | Glow around bright things (each Environment sets how much). |
| `bloom_quality` | `1` | How many blur passes bloom uses: more is wider and smoother. (1 to 4) |
| `particles` | `1.0` | How many particles emitters make, 0 to 1 of their rate (lower is faster). (0.0 to 1.0) |

### Color

| Setting | Default | What it does |
|---|---|---|
| `tonemap` | `"none"` | How very bright 3D light is squeezed into the screen's range: aces and filmic look like film, reinhard is gentle. One of: none, aces, reinhard, filmic. |
| `exposure` | `1.0` | Brightness of 3D light before tonemapping (like a camera's exposure). (0.0 to 8.0) |
| `brightness` | `0.0` | Added to the whole picture. (-1.0 to 1.0) |
| `contrast` | `1.0` | Pushes darks and lights apart (1 = unchanged). (0.0 to 3.0) |
| `saturation` | `1.0` | Color strength: 0 is black and white, 1 unchanged. (0.0 to 3.0) |
| `gamma` | `1.0` | Brightens (above 1) or darkens (below 1) the middle tones. (0.2 to 3.0) |
| `tint` | `white` | Multiplies the whole picture by a color. |
| `vignette` | `0.0` | Darkens the corners of the screen. (0.0 to 1.0) |
| `grain` | `0.0` | Moving film grain over the picture. (0.0 to 1.0) |
| `post_shader` | none | A full-screen shader (a .wgsl file of the project) run over the finished picture: it defines `fn post(color, uv, time, values)` (see SHADERS.md). Scripts: `POST_SHADER(file, [values])`. |

## Physics

### World

| Setting | Default | What it does |
|---|---|---|
| `gravity_2d` | `[0.0, 980.0]` | Gravity for 2D bodies, in pixels per second² (y grows downward). |
| `gravity_3d` | `[0.0, -9.81, 0.0]` | Gravity for 3D bodies, in metres per second². |
| `linear_damping` | `0.0` | Air resistance on every body's movement (added to each body's own). (0.0 to 50.0) |
| `angular_damping` | `0.0` | Air resistance on every body's spinning (added to each body's own). (0.0 to 50.0) |
| `sleeping` | `true` | Bodies that stop moving go to sleep (cheaper) until something touches them. |

### Timing

| Setting | Default | What it does |
|---|---|---|
| `tick_rate` | `60.0` | Physics steps each second: higher is more accurate (fast objects, stacks) and slower. (10.0 to 480.0) |
| `max_steps` | `5` | The most physics steps one frame may catch up on when the game falls behind. (1 to 30) |

### Solver

| Setting | Default | What it does |
|---|---|---|
| `solver_iterations` | `8` | How hard the solver works each step: more keeps stacks and joints steadier. (1 to 32) |
| `ccd` | `true` | Continuous collision: fast bodies can't pass through thin walls (a body's own ccd setting can turn it off). |
| `ccd_substeps` | `1` | Extra checks per step for fast bodies. (1 to 8) |
| `contact_stiffness` | `30.0` | How hard touching bodies push apart: higher is stiffer, lower is squishier. (1.0 to 120.0) |

### Characters

| Setting | Default | What it does |
|---|---|---|
| `max_slope` | `45.0` | The steepest slope (degrees) a body moved with MOVE can walk up. (0.0 to 89.0) |
| `step_height` | `0.0` | Steps up ledges this high when moved with MOVE (3D: metres, 2D: x100 pixels; 0 = off). (0.0 to 2.0) |
| `snap_to_ground` | `0.1` | Keeps a body moved with MOVE stuck to the ground going down slopes and steps (3D: metres, 2D: x100 pixels). (0.0 to 2.0) |
