# Scripting nodes with .jlb

Every node can have any number of scripts (Events panel → **+ New script** or **Attach
existing**). A script is an ordinary `.jlb` file:

```
>> Outside variables belong to this node: each Player gets its own copy.
speed = 300

DEF on_step(dt) {
	IF KEY_DOWN("right") { self.x += speed * dt }
}

DEF on_collide_start(other) {
	PRINT("touched " + NAME_OF(other))
	SEND(other, "hit", 10)          >> runs on_hit(10) on the other node
}
```

## `self`

`self` is the node the script is on. Change it and the node changes.

| Entry | Meaning |
|---|---|
| `self.x`, `self.y`, `self.z` | position (2D: pixels, y down; 3D: metres, y up) |
| `self.rotation` | 2D rotation in degrees (same as `rotation_z`); 3D also has `rotation_x`, `rotation_y` |
| `self.scale_x`, `self.scale_y`, `self.scale_z` | scale |
| `self.visible`, `self.name` | |
| `self.id`, `self.kind` | the node's handle (a number) and kind, read-only |
| any property | `self.color`, `self.width`, `self.text`, ... (colors are `[r:, g:, b:, a:]` tables, 0 to 1) |
| anything new | `self.hp = 3` makes a property: it shows in the inspector while the game runs |

All scripts on one node share the node, so they can talk through `self`; each keeps its own
outside variables.

## Events

| Function | When it runs |
|---|---|
| top-level code | when the script is attached (before `on_create`) |
| `on_create()` | once the whole scene is loaded, or after `SPAWN` |
| `on_step(dt)` | every frame |
| `on_physics(dt)` | 60 times a second, before physics moves bodies (use it for movement) |
| `on_draw()` | every frame after `on_step`: the place for `DRAW_*` commands |
| `on_destroy()` | before the node is removed |
| `on_key_pressed(key)` / `on_key_released(key)` | any key |
| `on_press_<key>()` / `on_release_<key>()` | one key, e.g. `on_press_space()`, `on_press_a()`, `on_press_pad_a()` |
| `on_mouse_pressed(button, x, y)` | a click (`"left"`, `"right"`, `"middle"`) |
| `on_mouse_moved(x, y)`, `on_mouse_wheel(amount)` | |
| `on_collide_start(other)` / `on_collide(other)` / `on_collide_end(other)` | a Collider2D or a body touches another |
| `on_timer(name)` / `on_timer_<name>()` | a `TIMER` finishing |
| `on_pressed()` | on a Button's own scripts: it was clicked |
| `on_pressed_<button name>()` | on every script: a Button with that name was clicked (`Start Game` → `on_pressed_start_game`) |
| `on_text_changed(text)` / `on_text_submitted(text)` | on a TextInput's scripts |
| `on_animation_end(name)` | a non-looping sprite or model animation finished |
| `on_<event>(...)` | `SEND(node, "<event>", ...)` or `EMIT("<event>", ...)` |
| `when_<name>()` → `on_<name>()` | a condition: `on_<name>` runs once each time `when_<name>` turns TRUE |

The Events panel lists these; click one to add it to a script.

Collisions: a `Collider2D` reports for the nearest node with scripts (itself or its parent), so a
Sprite2D with a Collider2D child gets the events. Bodies (`Body2D`, `Body3D`) report for
themselves.

Key names: `"a"`–`"z"`, `"0"`–`"9"`, `"f1"`…, `"space"`, `"enter"`, `"escape"`, `"tab"`,
`"backspace"`, `"left"`, `"right"`, `"up"`, `"down"`, `"shift"`, `"ctrl"`, `"alt"`. Mouse buttons
work with `KEY_DOWN` too: `"mouse_left"`, `"mouse_right"`, `"mouse_middle"`. Gamepad buttons are
keys named `"pad_a"`, `"pad_b"`, `"pad_x"`, `"pad_y"`, `"pad_lb"`, `"pad_rb"`, `"pad_lt"`,
`"pad_rt"`, `"pad_start"`, `"pad_select"`, `"pad_up"`/`"pad_down"`/`"pad_left"`/`"pad_right"`;
sticks are `AXIS("left_x")` and friends.

## Engine commands

Where a command takes a node, give an id (a number from `self.id`, `NODE`, `SPAWN`...), a path
(`"Gun"`, `"../Enemy"`, `"/Level/Door"`, relative to this node), or `NULL` for this node.
Colors are tables `[r: 1, g: 0.5, b: 0, a: 1]` (0 to 1) or a hex text like `"#ff8800"`.
Files are `res://...` (your project) or `hl2://...` (a mounted Source game, see
[HL2.md](HL2.md)).

The script editor completes these as you type and shows each one's inputs.

### Nodes and the tree

| Command | |
|---|---|
| `NODE(path)` | the node at a path (relative to this one), or NULL |
| `GET(node, property)` | a property of another node |
| `SET(node, property, value)` | changes a property of another node |
| `NODES(group)` | every node in a group, as a table of ids |
| `CHILDREN(node)` | a node's children, as a table of ids |
| `PARENT(node)` | a node's parent, or NULL |
| `NAME_OF(node)` | a node's name |
| `PATH_OF(node)` | a node's full path |
| `SPAWN(scene, x, y, z, parent)` | adds an instance of a scene file; gives its id |
| `DESTROY(node)` | removes a node (at the end of the frame); runs on_destroy |

### Events and timers

| Command | |
|---|---|
| `SEND(node, event, inputs...)` | runs on_<event>(inputs...) on one node |
| `EMIT(event, inputs...)` | runs on_<event>(inputs...) on every node that has it |
| `TIMER(name, seconds, repeat)` | runs on_timer(name) and on_timer_<name>() later |
| `TIMER_STOP(name)` | cancels this node's timer |

### Physics and queries

| Command | |
|---|---|
| `COLLIDING(node)` | the nodes touching this one now, as a table of ids |
| `MOVE(node, dx, dy, dz)` | moves a body, sliding along walls; gives TRUE when it's on the floor after |
| `IMPULSE(node, x, y, z)` | adds to a dynamic body's velocity at once (a kick or a jump), whatever its mass |
| `RAYCAST(x, y, dx, dy, length, ignore)` | the first 2D body along a line: [node:, x:, y:, nx:, ny:, distance:] or NULL |
| `RAYCAST_3D(x, y, z, dx, dy, dz, length, ignore)` | the same in 3D |
| `DISTANCE(a, b)` | how far apart two nodes are |
| `FIND_PATH(tilemap, x1, y1, x2, y2)` | a walkable path through a TileMap2D: a table of [x:, y:] points, or NULL |

### Sound

| Command | |
|---|---|
| `PLAY_SOUND(file, volume, pitch)` | plays a sound file once (res:// or hl2://...); gives its id |
| `STOP_SOUND(id)` | stops a sound from PLAY_SOUND (NULL stops every sound) |
| `PLAY(node)` | starts a Sound node |
| `STOP(node)` | stops a Sound node |
| `MUSIC(file, volume)` | plays a looping music track, replacing the last one ("" stops it) |
| `VOLUME(level)` | the master volume, 0 to 1 |

### Animation, particles and tiles

| Command | |
|---|---|
| `BURST(node, count)` | a Particles2D throws out this many particles at once (an explosion, a puff) |
| `ANIMATE(node, name)` | plays an animation: a Sprite2D's (from its animations property) or a model's |
| `ANIMATIONS(node)` | the names of a Mesh3D model's animations |
| `TILE(node, col, row)` | a TileMap2D's tile id at a cell (-1 = empty) |
| `SET_TILE(node, col, row, id)` | changes a tile (-1 clears it) |
| `TILE_AT(node, x, y)` | the cell under a world position: [col:, row:] |

### Saving

| Command | |
|---|---|
| `SAVE_DATA(name, value)` | keeps a value (a table, number, text...) on disk between runs |
| `LOAD_DATA(name)` | a value kept with SAVE_DATA, or NULL |
| `DELETE_DATA(name)` | forgets a value kept with SAVE_DATA |

### Drawing

| Command | |
|---|---|
| `DRAW_RECT(x, y, w, h, color, filled)` | draws a rectangle this frame (2D world) |
| `DRAW_LINE(x1, y1, x2, y2, color)` | draws a line this frame |
| `DRAW_CIRCLE(x, y, radius, color, filled)` | draws a circle this frame |
| `DRAW_TEXT(x, y, text, size, color)` | draws text this frame |
| `DRAW_IMAGE(file, x, y, w, h)` | draws an image this frame |

### Input and time

| Command | |
|---|---|
| `KEY_DOWN(key)` | TRUE while a key is held (mouse buttons: "mouse_left", "mouse_right", "mouse_middle") |
| `KEY_PRESSED(key)` | TRUE on the frame a key went down |
| `KEY_RELEASED(key)` | TRUE on the frame a key went up |
| `MOUSE_X()` | mouse x in game coordinates |
| `MOUSE_Y()` | mouse y in game coordinates |
| `MOUSE_WHEEL()` | how far the wheel turned this frame |
| `AXIS(name)` | a gamepad stick or trigger, -1 to 1: left_x, left_y, right_x, right_y, lt, rt |
| `DELTA_TIME()` | seconds since the last frame |
| `GAME_TIME()` | seconds since the game started |

### Game flow

| Command | |
|---|---|
| `CHANGE_SCENE(scene)` | switches to another scene file |
| `QUIT()` | stops the game |

## Physics in short

- `Body2D` / `Body3D` with `body` = `"dynamic"` falls and gets pushed around. Steer it with
  `IMPULSE` or by setting `self.vx` / `self.vy`; read `self.on_floor`.
- `body` = `"kinematic"` moves only when you move it. `MOVE(NULL, dx, dy)` slides along walls
  and gives TRUE when standing on something: the usual way to make a player.
- `body` = `"static"` never moves (walls, floors). TileMap2D tiles are static automatically (all
  of them, or only the ids in `solid_tiles`, like `"0-3, 7"`). A `Mesh3D` with `collision` on
  gives a 3D model a static collision shape.
- 2D physics counts 50 pixels as a metre; the project's gravity settings are in the
  `project.jproj` file (`gravity_2d`, `gravity_3d`).

## Debugging

Click the gutter next to a line in the script editor to set a breakpoint. When the game reaches it, it pauses: the Console's debugger shows the call
stack, the function's locals and the script's outside variables; F8 continues, F10 runs one
line. A script error shows in the Console with its file and line (click to jump there); that
script stops until the game restarts while the rest keeps running. Saving a script while the
game runs reloads it in place, keeping its outside variables.

## Everything else

Everything else in .jlb works as usual: tables, IMPORT/LOAD (relative to the script's folder),
TRY/FAIL, the maths and text builtins. `PRINT` goes to the editor's console. `END` stops the
game. Note that "not equal" in .jlb is `!==`.

Not in the engine: `START`/tasks inside scripts, and .jlb's own windows and drawing commands
(`WINDOW`, `RECT`...): use nodes and the `DRAW_*` commands instead.
