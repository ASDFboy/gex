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
| `Sound` | a sound you can start and stop | `stream` (a .wav/.ogg/.mp3/.flac file), `volume`, `pitch`, `loop`, `autoplay` |

## 2D

| Kind | What it is | Main properties |
|---|---|---|
| `Node2D` | an empty 2D node to group others | |
| `Sprite2D` | a picture or a colored rectangle | `texture`, `width`, `height`, `color` (tint), `layer` (higher draws on top), `flip_h`, `flip_v` |
| `Label2D` | text in the world (moves with the camera) | `text`, `size`, `color` |
| `Camera2D` | what the game shows; follows its parent | `zoom`, `current` |
| `Collider2D` | an area that reports overlaps (pickups, triggers) | `width`, `height` |
| `Body2D` | a physics object | `body` (dynamic / kinematic / static), `shape` (box / circle), `width`, `height`, `radius`, `friction`, `bounce`, `density`, `gravity_scale`, `fixed_rotation`; live: `vx`, `vy`, `on_floor` |
| `TileMap2D` | a grid of tiles from one tileset picture | `tileset`, `tile_width`, `tile_height`, `solid`, `solid_tiles` |

**Sprite animation.** A sprite sheet is one picture cut into `hframes` × `vframes` cells; `frame`
picks one. Name animations in `animations`, like `walk: 0-5, idle: 6, jump: 7-9`, then
`ANIMATE(NULL, "walk")` (speed from `anim_fps`, looping from `anim_loop`).

**Tilemaps.** Select a TileMap2D and the Inspector shows its tileset as a palette: click a tile,
then paint in the scene view (right-drag erases). Tile ids count left to right, top to bottom,
from 0. Solid tiles collide with bodies; `solid_tiles` like `"0-3, 7"` limits which ones.
`FIND_PATH` walks around the solid tiles.

## UI

UI nodes are drawn on top of the game at fixed screen positions (they ignore cameras). Position
is the top-left corner; `anchor` (`top_left`, `top`, `top_right`, `left`, `center`, `right`,
`bottom_left`, `bottom`, `bottom_right`) pins it to a side of the screen so it stays put when the
window changes size.

| Kind | What it is | Main properties | Events |
|---|---|---|---|
| `Panel` | a box to put other UI on | `width`, `height`, `color`, `border_color`, `border`, `radius` | |
| `Label` | text | `text`, `size`, `color`, `align`, `width` | |
| `Button` | a clickable button | `text`, `width`, `height`, `size`, `color`, `text_color`, `disabled` | `on_pressed()`, `on_pressed_<name>()` |
| `TextInput` | a box to type in | `text`, `placeholder`, `width`, `height` | `on_text_changed(text)`, `on_text_submitted(text)` |
| `ProgressBar` | a bar (health, loading) | `value`, `max`, `color`, `back_color` | |
| `Image` | a picture | `texture`, `width`, `height`, `color` | |

## 3D

| Kind | What it is | Main properties |
|---|---|---|
| `Node3D` | an empty 3D node to group others | |
| `Mesh3D` | a shape or a model | `mesh` (`cube`, `sphere`, `plane`, or a model file: .glb/.gltf, .obj, and from a Source game .mdl or a .bsp map), `color`, `texture`, `uv_scale_x/y`, `cast_shadows`, `collision`; models: `animation`, `anim_speed`, `anim_loop` |
| `Camera3D` | what the game shows | `fov`, `near`, `far`, `current` |
| `Light3D` | a light | `type` (directional / point / spot), `color`, `energy`, `range`, `angle`, `shadows` (the first directional light casts shadows) |
| `Body3D` | a physics object | like Body2D, plus `depth`, `vz`; `shape` box / sphere / capsule |
| `Environment` | the sky, ambient light and fog | `sky_top`, `sky_horizon`, `ground`, `sky_texture` (a panorama), `ambient`, `ambient_energy`, `fog_color`, `fog_start`, `fog_end` |

Drag a model, picture or sound from the Assets panel into the scene view to make the right node
for it.

## Scenes inside scenes

Save any branch as its own `.jscene` (right-click → **Save branch as scene**), then drop that file
into other scenes as many times as you like: every copy follows the original file. Right-click an
instance → **Editable children** to change things inside one copy only (only the differences are
saved). `SPAWN("res://scenes/enemy.jscene", x, y)` adds one while the game runs.
