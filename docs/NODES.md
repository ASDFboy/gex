# Nodes

Every scene is a tree of nodes. Add one with the **+** button in the Hierarchy (or `:add Kind`),
and change its properties in the Inspector. Any property can be read and changed from a script
(`self.width = 64`, `SET("Door", "color", "#ff0000")`); a property you invent (`self.hp = 3`)
just becomes a new property on that node.

Positions: 2D nodes use pixels with y going down; 3D nodes use metres with y going up.
Rotations are in degrees. A child moves, turns and scales with its parent.

## General

| Kind | What it is | Main properties |
|---|---|---|
| `Node` | an empty node: a folder, or a holder for scripts (game managers) | |
| `Sound` | a sound you can start and stop | `stream` (a .wav/.ogg/.mp3/.flac file), `volume`, `pitch`, `loop`, `autoplay`; `spatial` makes it quieter the farther it is from the camera, silent at `range` (0 = 25 m in 3D, 1000 px in 2D). Put it under the thing making the noise. |

## 2D

| Kind | What it is | Main properties |
|---|---|---|
| `Node2D` | an empty 2D node to group others | |
| `Sprite2D` | a picture or a colored rectangle | `texture`, `width`, `height`, `color` (tint), `layer` (higher draws on top), `flip_h`, `flip_v`, `parallax_x/y`, `repeat_x/y` |
| `Label2D` | text in the world (moves with the camera) | `text`, `size`, `color`, `font` |
| `Camera2D` | what the game shows; follows its parent | `zoom`, `current`, `smoothing` (0 = locked on, higher = catches up faster), `limits` with `limit_left/top/right/bottom` (the view stays inside), `ambient` (below white darkens the scene for Light2Ds) |
| `Light2D` | a light in a dark 2D scene | `color`, `energy`, `radius`, `texture` (a soft glow if empty) |
| `Collider2D` | an area that reports overlaps (pickups, triggers) with other colliders and bodies | `width`, `height`, `layer`, `mask` |
| `Body2D` | a physics object | `body` (dynamic / kinematic / static), `shape` (box / circle), `width`, `height`, `radius`, `friction`, `bounce`, `density`, `gravity_scale`, `fixed_rotation`; live: `vx`, `vy`, `on_floor` |
| `TileMap2D` | a grid of tiles from one tileset picture | `tileset`, `tile_width`, `tile_height`, `solid`, `solid_tiles` |
| `Particles2D` | sparks, smoke, dust, trails | `emitting`, `rate` (per second), `lifetime`, `direction` (degrees, -90 = up), `spread`, `speed`, `speed_random`, `gravity`, `size` → `size_end`, `color` → `color_end`, `spin`, `area` (radius they start in), `texture` (a soft dot if empty) |

**Sprite animation.** A sprite sheet is one picture cut into `hframes` × `vframes` cells; `frame`
picks one. Name animations in `animations`, like `idle=0; walk=1-5; jump=6,7,8`, then
`ANIMATE(NULL, "walk")` (speed from `anim_fps`, looping from `anim_loop`).

**Backgrounds.** `parallax_x` below 1 makes a sprite (or tilemap) follow the camera only part
of the way, so it looks far off: 0.5 for distant hills, 0 for a sky that never moves. `repeat_x`
tiles it sideways forever, which is how scrolling backgrounds are made.

**2D lighting.** Set the Camera2D's `ambient` to a dark color (night blue, say) and add Light2D
nodes: each one lights a soft circle. UI and text aren't darkened.

**Particles.** They play in the editor too, so you can tune them by eye. Leave `emitting` on for
a steady stream (smoke, a trail behind the player), or turn it off and call `BURST(node, 40)`
for one-off effects (an explosion, a coin pickup). Particles stay where they were thrown out, so a
moving emitter leaves a trail.

**Tilemaps.** Select a TileMap2D and the Inspector shows its tileset as a palette: click a tile,
then paint in the scene view (Shift-drag erases). Tile ids count left to right, top to bottom,
from 0. Solid tiles collide with bodies; `solid_tiles` like `"0-3, 7"` limits which ones.
`FIND_PATH` walks around the solid tiles.

## UI

UI nodes are drawn on top of the game at fixed screen positions (they ignore cameras). Position
is the top-left corner; `anchor` (`top_left`, `top`, `top_right`, `left`, `center`, `right`,
`bottom_left`, `bottom`, `bottom_right`) pins it to a side of the screen so it stays put when the
window changes size.

| Kind | What it is | Main properties | Events |
|---|---|---|---|
| `Panel` | a box to put other UI on | `width`, `height`, `color`, `border_color`, `border`, `radius`, `texture`, `slice` | |
| `Label` | text | `text`, `size`, `color`, `align`, `width`, `font` | |
| `Button` | a clickable button | `text`, `width`, `height`, `size`, `color`, `text_color`, `disabled`, `font`, `texture`, `slice` | `on_pressed()`, `on_pressed_<name>()` |
| `TextInput` | a box to type in | `text`, `placeholder`, `width`, `height` | `on_text_changed(text)`, `on_text_submitted(text)` |
| `ProgressBar` | a bar (health, loading) | `value`, `max`, `color`, `back_color` | |
| `Image` | a picture | `texture`, `width`, `height`, `color`, `slice` | |

**Fonts.** Drop a `.ttf` or `.otf` into the project and onto a Label's, Button's, TextInput's or
Label2D's `font`.

**Rich text.** Labels, buttons and Label2Ds understand tags: `[color=#ff8800]hot[/color]`,
`[color=red]`, `[size=32]big[/size]`, `[i]italic[/i]`, `[u]underline[/u]`, `[s]struck[/s]`,
`[bg=#000000]highlight[/bg]`.

**Picture panels (nine-slice).** Give a Panel, Button or Image a `texture` and a `slice` in
pixels: the corners and edges of the picture keep their size and only the middle stretches, so
one small frame image makes boxes of any size.

## 3D

| Kind | What it is | Main properties |
|---|---|---|
| `Node3D` | an empty 3D node to group others | |
| `Mesh3D` | a shape or a model | `mesh` (`cube`, `sphere`, `plane`, `quad`, or a model file: .glb/.gltf, .obj, and from a Source game .mdl or a .bsp map), `color`, `texture`, `uv_scale_x/y`, `cast_shadows`, `collision`; material: `normal_map`, `specular` (0 matte to 1 glossy), `emissive` (glows; with bloom, it shines), `unlit`, `shader` (see [SHADERS.md](SHADERS.md)); models: `animation`, `anim_speed`, `anim_loop`, `anim_blend` (seconds to crossfade into a new animation) |
| `Sprite3D` | a picture in 3D: faces the camera (`billboard`) or stands like a sign | `texture`, `width`, `height`, `color`, `billboard`, `unlit` |
| `Label3D` | text over a point in 3D (names over heads, signs) | `text`, `size` (screen pixels), `color`, `font` |
| `Particles3D` | fire, smoke, sparks, dust in 3D (plays in the editor too) | like Particles2D, with `direction_x/y/z`, metres and metres per second, and `glow` |
| `Camera3D` | what the game shows | `fov`, `near`, `far`, `current` |
| `Light3D` | a light | `type` (directional / point / spot), `color`, `energy`, `range`, `angle`, `shadows` (the first directional light with shadows on casts them, sharp near the camera and softer out to the Environment's `shadow_distance`) |
| `Body3D` | a physics object | like Body2D, plus `depth`, `vz`; `shape` box / sphere / capsule / hull (the outline of its first Mesh3D child's model: a barrel, a chair) |
| `Collider3D` | a 3D area that reports what enters it (triggers, pickups, damage zones) | `shape` (box / sphere), `width`, `height`, `depth`, `radius`, `layer`, `mask` |
| `Environment` | the sky, ambient light, fog, shadows and glow | `sky_top`, `sky_horizon`, `ground`, `sky_texture` (a panorama), `ambient`, `ambient_energy`, `fog_color`, `fog_start`, `fog_end`, `shadow_distance` (how far shadows reach, in metres), `bloom` (glow around bright things, 0 = off), `bloom_threshold` |

Drag a model, picture or sound from the Assets panel into the scene view to make the right node
for it.

## Collision layers

Bodies and colliders have `layer` (what they are) and `mask` (what they bump into or notice),
both as names: `layer` = `player`, `mask` = `world, enemies`. An empty `layer` is `default`
(which is also what tilemaps and level geometry are), and an empty `mask` means everything. Two
things touch only when each is in the other's mask. Some uses:

- enemies that walk through each other: their `layer` = `enemies`, `mask` = `default, player`;
- a ghost that floats through walls: `mask` = `player`;
- a pickup that only the player can take: the Collider2D's `mask` = `player`.

`RAYCAST` and `RAYCAST_3D` take the same kind of list as a last input to choose what a ray can hit.

## Scenes inside scenes

Save any branch as its own `.jscene` (right-click → **Save branch as scene**), then drop that file
into other scenes as many times as you like (drag it from the Assets panel into the scene view,
or **Instance into scene** in its preview): every copy follows the original file. Nodes inside a
copy show greyed in the Hierarchy; select one and change it in the Inspector to change that copy
only (only the differences are saved). Right-click a copy → **Open its scene** to change them all,
or **Make local** to cut it loose from the file. `SPAWN("res://scenes/enemy.jscene", x, y)` adds one while the game runs.
