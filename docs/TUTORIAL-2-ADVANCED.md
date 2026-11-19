# Tutorial 2: going further

Each section here stands on its own: pick the ones your game needs. They assume you've done
[Tutorial 1](TUTORIAL-1-BASICS.md). The demo project (launcher → **Create the demo project**)
has a working version of most of them.

- [Physics and a platformer character](#physics-and-a-platformer-character)
- [Tilemaps and pathfinding](#tilemaps-and-pathfinding)
- [Sprite animation](#sprite-animation)
- [Cameras](#cameras)
- [Making it feel good: tweens, particles, shake, sound](#making-it-feel-good)
- [UI](#ui)
- [Input actions and gamepads](#input-actions-and-gamepads)
- [Condition events](#condition-events)
- [Saving and remembering](#saving-and-remembering)
- [3D](#3d)
- [First person](#first-person)
- [Half-Life 2 assets](#half-life-2-assets)
- [Debugging](#debugging)
- [Working faster in the editor](#working-faster-in-the-editor)

## Physics and a platformer character

A **Body2D** is a physics object. Its `body` property decides what kind:

| `body` | Moves how | Use it for |
|---|---|---|
| `dynamic` | gravity, bounces, gets pushed | crates, balls, ragdolls, debris |
| `kinematic` | only when a script moves it, with `MOVE` | the player, moving platforms, enemies |
| `static` | never | walls and floors (tilemaps are static on their own) |

`MOVE(object, dx, dy)` moves a kinematic body by that much, sliding along anything solid, and
gives back TRUE when the body ends up standing on something. A player, then, is a few lines:

```
speed = 260
gravity = 1400
jump = 560
vy = 0
on_floor = FALSE

DEF on_physics(dt) {
	dx = 0
	IF KEY_DOWN("left") { dx = -1 }
	IF KEY_DOWN("right") { dx = 1 }
	IF KEY_PRESSED("space") AND on_floor {
		GLOBAL vy = 0 - jump
	}
	GLOBAL vy = MIN(vy + gravity * dt, 900)
	GLOBAL on_floor = MOVE(NULL, dx * speed * dt, vy * dt)
	IF on_floor AND vy > 0 { GLOBAL vy = 0 }
}
```

Put movement in `on_physics(dt)` (a steady 60 times a second) rather than `on_step(dt)`.

Dynamic bodies: shove one with `IMPULSE(object, x, y)`, or set `self.vx` / `self.vy` on it
directly. Their `on_floor` property says whether they're resting on something. `bounce`,
`friction`, `density` and `gravity_scale` are in the Inspector; `fixed_rotation` stops a body
tipping over.

Look before you leap: `RAYCAST(x, y, dx, dy, length, ignore)` gives the first body along a line,
as `[object:, x:, y:, nx:, ny:, distance:]`, or NULL:

```
>> Is there ground just ahead of this enemy's feet?
ahead = RAYCAST(self.x + 20 * direction, self.y + 20, 0, 1, 30, self.id)
IF ahead == NULL { GLOBAL direction = 0 - direction }
```

## Tilemaps and pathfinding

1. Make a tileset picture: tiles in a grid, all the same size (32×32 is the default).
2. Add a **TileMap2D**, drop the picture on its `tileset`, and set `tile_width`/`tile_height`.
3. With the tilemap selected, the Inspector shows the tileset as a palette. Click a tile, then
   paint in the blueprint view. Shift-drag erases.

Tiles are solid by default; `solid_tiles` like `"0-3, 7"` makes only those ids solid (so grass
decorations can be walked through). `solid` off turns collision off for the whole map.

From scripts: `TILE_AT(map, x, y)` gives the cell under a point as `[col:, row:]`;
`TILE(map, col, row)` reads a tile id (-1 = empty); `SET_TILE(map, col, row, id)` changes one
(and its collision). Breakable blocks:

```
DEF on_mouse_pressed(button, x, y) {
	cell = TILE_AT("Ground", x, y)
	SET_TILE("Ground", cell.col, cell.row, -1)
}
```

`FIND_PATH(map, x1, y1, x2, y2)` walks around solid tiles and gives a list of `[x:, y:]` points
(cell centres), or NULL when there's no way through. An enemy that chases the player:

```
speed = 120
path = NULL
step = 1

DEF on_create() {
	TIMER("think", 0.5, TRUE)
}

DEF on_timer_think() {
	GLOBAL path = FIND_PATH("../Ground", self.x, self.y, GET("../Player", "x"), GET("../Player", "y"))
	GLOBAL step = 2
}

DEF on_step(dt) {
	IF path == NULL OR step > LEN(path) { RETURN }
	target = path[step]
	dx = target.x - self.x
	dy = target.y - self.y
	d = (dx * dx + dy * dy) !^ 2
	IF d < 4 {
		GLOBAL step += 1
	} ELSE {
		self.x += dx / d * speed * dt
		self.y += dy / d * speed * dt
	}
}
```

(Lists count from 1 in .jlb, and `path[1]` is where the enemy already is, so it heads for
`path[2]`. `x !^ 2` is the square root.)

## Sprite animation

A sprite sheet is one picture cut into equal frames. On the Sprite2D set `hframes` (frames
across) and `vframes` (frames down); `frame` picks one, counting from 0. Name the animations
in `animations`:

```
idle=0; run=1-6; jump=7,8
```

then play them from a script, and flip the sprite to face the other way:

```
IF dx !== 0 {
	ANIMATE(NULL, "run")
	self.flip_h = dx < 0
} ELSE {
	ANIMATE(NULL, "idle")
}
```

`anim_fps` sets the speed and `anim_loop` whether it repeats. A one-shot animation runs
`on_animation_end(name)` when it finishes. Calling `ANIMATE` with the animation that's already
playing carries on rather than restarting it, so it's fine every frame; `ANIMATE(NULL, "attack",
TRUE)` starts over.

## Animating anything: the Timeline

Sprite sheets cover frames; for everything else (a door sliding, a light pulsing, a title
bouncing in, a boss's attack pattern) use an **Animator**.

1. Select the object to animate and add a child **Animator** (+ Object → General). In the
   Inspector, press **Edit clips in the Timeline**.
2. **+ Clip** makes a clip. Set its `length` and whether it loops.
3. Under **+ Track**, pick an object (the Animator's parent, or anything under it) and a property
   (`x`, `y`, `rotation`, `color`, `visible`, any number...), then **Add**.
4. Move the playhead (drag the slider or click in a track), set the value in the Inspector or
   the blueprint view, and press **+key** on the track. Do that at a few times and press ▶: the
   values blend between keys. Drag a key (the yellow diamond) to move it; right-click it to
   change its easing (`linear`, `in`, `out`, `in_out`, `step`) or delete it.
5. Tick **autoplay** to start the clip with the blueprint, or play it from a script:

```
DEF on_pressed_open() {
	PLAY_CLIP("Door/Animator", "open")
}

DEF on_clip_end(clip) {
	PRINT(clip + " finished")
}
```

Clips are saved in `animations/` as `.anim` files, so several Animators can share one.

## Cameras

A **Camera2D** shows the world from where it is, so make it a child of the player to follow
them. On the camera:

- `smoothing`: 0 sticks to the player exactly; around 5 eases after them.
- `limits` with `limit_left`, `limit_top`, `limit_right`, `limit_bottom`: the view never shows
  past the edges of your level.
- `zoom`: 2 is twice as close.

A **Camera3D** works the same way in 3D. With several cameras, the first one with `current`
ticked is used; switch by setting `current` from a script.

## Making it feel good

**Tweens** slide a property to a new value over time, with an easing curve:

```
TWEEN("Title", "y", 170, 0.8, "bounce")        >> drop a title in
TWEEN(NULL, "scale_x", 1.2, 0.1, "out")        >> a little squash
TWEEN("Door", "color", "#ff000000", 0.5)       >> fade a sprite out
```

Eases: `linear`, `in`, `out` (the default), `in_out`, `back`, `bounce`, `elastic`. When one
finishes, the object's scripts get `on_tween_end(property)`.

**Particles**: add a Particles2D. It plays right away in the editor, so tune it by eye:
`rate`, `lifetime`, `speed`, `spread` (degrees each side of `direction`), `gravity`, `size` →
`size_end`, `color` → `color_end`. Leave `emitting` on for smoke or a trail; for a burst, turn
it off and call `BURST("Sparks", 40)` (move it first with `SET("Sparks", "x", ...)`).

**Screen shake**: `SHAKE(10, 0.4)`: ten pixels, dying away over 0.4 seconds.

**Sound**:

```
PLAY_SOUND("res://sounds/jump.wav")                  >> a one-off
id = PLAY_SOUND("res://sounds/engine.ogg", 0.5, 1.2) >> quieter, higher
STOP_SOUND(id)
MUSIC("res://music/level1.ogg", 0.6)                 >> loops; MUSIC("") stops it
VOLUME(0.8)                                          >> everything
```

A **Sound** object holds a sound you start with `PLAY(object)` (or `autoplay`). Tick `spatial` and
it gets quieter the farther it is from the camera: put it under a waterfall or an engine.

## UI

UI objects (Panel, Label, Button, TextInput, ProgressBar, Image) are drawn over the game in
window pixels and ignore cameras. `anchor` is its origin: the point of it that sits at
its position (`top_left`, `top`, `center`, `bottom_right`...). A `center` button at the middle
of the screen is centred whatever its size.

| Object | Its events |
|---|---|
| Button | `on_pressed()` on its own scripts; `on_pressed_<name>()` on every script |
| TextInput | `on_text_changed(text)`, `on_text_submitted(text)` (Enter) |

A health bar is a ProgressBar: `SET("HUD/Health", "value", hp / max_hp)` (or set its `max`).
Hide or show a whole menu by setting `visible` on the Panel that holds it.

## Input actions and gamepads

In **File → Project settings**, name an action and list its keys:

| Action | Keys |
|---|---|
| jump | space, w, up, pad_a |
| fire | mouse_left, ctrl, pad_rt |

Then `KEY_DOWN("jump")`, `KEY_PRESSED("jump")` and `on_press_jump()` work with every one of
them. Gamepad buttons are keys named `pad_a`, `pad_b`, `pad_x`, `pad_y`, `pad_lb`, `pad_rb`,
`pad_lt`, `pad_rt`, `pad_start`, `pad_select` and `pad_up/down/left/right`; sticks are
`AXIS("left_x")`, `AXIS("left_y")`, `AXIS("right_x")`, `AXIS("right_y")`, triggers
`AXIS("lt")` / `AXIS("rt")`.

## Condition events

A function named `when_<something>()` is a condition: the engine checks it every frame, and
the moment it turns TRUE it runs `on_<something>()` once (and again only after it's been FALSE
in between).

```
DEF when_dead() {
	RETURN self.hp <= 0
}

DEF on_dead() {
	BURST("../Sparks", 60)
	SHAKE(12, 0.5)
	DESTROY()
}
```

## Saving and remembering

| | Lasts | Use for |
|---|---|---|
| outside variables | while the object exists | an object's own state |
| `REMEMBER(name, value)` / `RECALL(name)` | until the game closes, across `CHANGE_BLUEPRINT` | lives, score, inventory |
| `SAVE_DATA(name, value)` / `LOAD_DATA(name)` / `DELETE_DATA(name)` | on disk, between runs | save games, settings, high scores |

Values can be numbers, text, TRUE/FALSE and tables (nested ones too):

```
SAVE_DATA("slot1", [level: 3, hp: 80, items: ["key", "lamp"]])
s = LOAD_DATA("slot1")
IF s !== NULL { CHANGE_BLUEPRINT("res://blueprints/level" + s.level + ".jblueprint") }
```

## 3D

3D positions are in metres with y up. A 3D blueprint needs:

1. A **Camera3D**: point it with its rotation (x tilts up/down, y turns left/right).
2. A **Light3D**: `directional` for the sun (the first one casts shadows), `point` for a lamp,
   `spot` for a torch. Point and spot lights reach `range` metres.
3. An **Environment** (optional): sky colors or a panorama picture, ambient light and fog.
4. Things to look at: **Mesh3D** with `mesh` = `cube`, `sphere` or `plane`, or a model file.
   Drag a `.glb`, `.gltf` or `.obj` from the Assets panel into the 3D view.

In the 3D view, right-drag orbits, right-drag with WASD flies, middle-drag (or Shift+right-drag)
pans, and the wheel zooms.

Animated models (glTF with skins): `ANIMATIONS(object)` lists their animations, and
`ANIMATE(object, "Run")` plays one. `anim_speed` and `anim_loop` are on the Mesh3D.

Physics in 3D is the same as 2D: **Body3D** (`box`, `sphere` or `capsule`) with
`MOVE(object, dx, dy, dz)`, `IMPULSE(object, x, y, z)`, and `RAYCAST_3D(x, y, z, dx, dy, dz,
length, ignore)`. A Mesh3D with `collision` ticked becomes solid ground, model or map
included. To show a dynamic box, make a Body3D with a Mesh3D child: the body moves, and the
mesh goes along.

## First person

The demo's `blueprints/fps.jblueprint` is a complete first-person setup; copy its Player into your own
blueprints. How it's put together:

- The player is a **Body3D**, `kinematic`, `shape` = `capsule`, with a **Camera3D** child
  ("Head") 0.7 m up.
- `MOUSE_LOCK(TRUE)` hides the mouse and holds it in the window. Each frame `MOUSE_DX()` and
  `MOUSE_DY()` say how far it moved.
- Mouse sideways turns the body (`self.rotation_y`); up/down tilts only the head
  (`SET("Head", "rotation_x", pitch)`), kept between -85 and 85.
- Walking turns the keys by where you face, then `MOVE`s:

```
mx = fx * COS(yaw) + fz * SIN(yaw)
mz = fz * COS(yaw) - fx * SIN(yaw)
GLOBAL on_floor = MOVE(NULL, mx * speed * dt, vy * dt, mz * speed * dt)
```

(`SIN` and `COS` take degrees.) In the editor, Escape frees the mouse; click the game view to
grab it again.

## Half-Life 2 assets

With Half-Life 2 installed through Steam, **File → Project settings & mounts…** → tick
**Mount Half-Life 2**, **Detect**, **Apply**. Its files show in the Assets panel under
`hl2://`, read straight from your install:

- textures and materials (`.vtf`, `.vmt`) go anywhere a picture does;
- sounds: `PLAY_SOUND("hl2://sound/ambient/alarms/klaxon1.wav")`;
- models: a Mesh3D with `mesh` = `hl2://models/props_c17/oildrum001.mdl`;
- maps: a Mesh3D with `mesh` = `hl2://maps/d1_trainstation_01.bsp` and `collision` on, then
  the first-person player on top.

Exported games never include these files, and scripts that use `hl2://` get a warning at
export. Keep `hl2://` projects for yourself. More in [HL2.md](HL2.md).

## Debugging

- `PRINT(anything)` writes to the Console.
- While the game runs, select an object: the Inspector shows its live values, including the
  properties your scripts made (`self.hp`).
- Click in a script's left margin to set a breakpoint (a red dot). When the game reaches that
  line it stops, opens the script there, and the Console's debugger shows the call stack, the
  function's variables and the script's outside variables. **F8** carries on, **F10** runs one
  line.
- Right-click the margin for a breakpoint with a condition (a dot with `?`): `hp < 10` stops only
  when that's TRUE. Handy inside `on_step`, which runs every frame.
- Under the debugger, **+ watch** takes any expression (`self.x + 5`, `LEN(enemies)`); every
  watch is worked out each time the game stops.
- **View → Profiler** (or the FPS in the status bar) shows each frame's time, split into
  scripts, physics and the rest, and which script was slowest.
- Save a script while the game runs (Ctrl+S) and it reloads in place, keeping its outside
  variables. Great for tuning numbers.
- A script with an error stops (the rest of the game doesn't); click the red Console line to
  jump to it.

## Working faster in the editor

- **Ctrl+F** finds text in every script and blueprint; click a result to open it there.
- The Assets panel previews models (drag to turn them; it lists their animations) and plays
  sounds. Its **↶** button (or Edit → Undo file change) undoes the last rename, delete, new file
  or import.

- **The command line**: press `:` anywhere outside a text box. `:w` saves, `:play`, `:stop`,
  `:add Sprite2D`, `:e blueprints/level2.jblueprint`, `:find Player`, `:rename Hero`, `:branch` (save
  the selection as a blueprint), `:vim` (toggle Vim keys in the script editor), `:2d`, `:3d`,
  `:layout` (reset the panels).
- **Vim keys** in the script editor (`:vim`): normal mode `hjkl w b e 0 $ gg G`, `dd yy p P x
  D`, `dw cw`, `i a I A o O`, `u` / Ctrl+R, and `:` for the command line.
- **Script editor**: completion as you type (Tab or Enter takes it), F12 or Ctrl+click jumps to
  a function's definition, Ctrl+H finds and replaces.
- **Files**: drop files on the window to import them; right-click in the Assets panel to
  rename (every blueprint and script that uses the file is fixed) or delete (into the project's
  hidden `.trash` folder, so nothing's lost).
- **Blueprints**: several open as tabs (Ctrl+W closes one). Drag to box-select in 2D; Ctrl+click
  to add to a selection; Ctrl+C / Ctrl+V copies objects between blueprints.
