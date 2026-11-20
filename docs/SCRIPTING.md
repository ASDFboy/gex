# Scripting objects with .jlb

Every object can have any number of scripts (Events → **Add event**, **New script** or **Attach
existing**). A script is an ordinary `.jlb` file:

```
>> Outside variables belong to this object: each Player gets its own copy.
speed = 300

DEF on_step(dt) {
	IF KEY_DOWN("right") { self.x += speed * dt }
}

DEF on_collide_start(other) {
	PRINT("touched " + NAME_OF(other))
	SEND(other, "hit", 10)          >> runs on_hit(10) on the other object
}
```

## `self`

`self` is the object the script is on. Change it and the object changes.

| Entry | Meaning |
|---|---|
| `self.x`, `self.y`, `self.z` | position (2D: pixels, y down, and z is the drawing order: higher draws on top; 3D: metres, y up) |
| `self.rotation` | 2D rotation in degrees (same as `rotation_z`); 3D also has `rotation_x`, `rotation_y` |
| `self.scale_x`, `self.scale_y`, `self.scale_z` | scale |
| `self.visible`, `self.name` | |
| `self.id`, `self.kind` | the object's handle (a number) and kind, read-only |
| any property | `self.color`, `self.width`, `self.text`, ... (colors are `[r:, g:, b:, a:]` tables, 0 to 1) |
| anything new | `self.hp = 3` makes a property: it shows in the inspector while the game runs |

All scripts on one object share the object, so they can talk through `self`; each keeps its own
outside variables.

### Without `self.`

A property can be named by itself: `x += speed * dt` is `self.x += self.speed * dt`. The bare
name means the property only when nothing else has that name. The script's own variables, a
function's inputs and its `LOCAL`s come first:

```
count = 0                 >> the script's own variable (a first value given outside functions)
DEF on_step(dt) {
	x += 200 * dt         >> the object's x
	visible = FALSE       >> the object's visible
	LOCAL y = 10          >> a variable of this function, not the object's y
	GLOBAL count += 1     >> the script's count
}
```

So inside a function, giving a value to a name the object has changes the object; write
`LOCAL` for a throwaway variable that happens to share a property's name. Only properties the
object actually has are found this way (`self.anything` still reads NULL for one it hasn't).

## Variables for the whole game: `GLOBAL`

A variable made with `GLOBAL` is one variable for the whole game. Every script can use it as
`GLOBAL.name`, whichever object made it:

```
GLOBAL score = 0              >> in any script: makes it (or joins the one already there)

DEF on_collide_start(other) {
	GLOBAL.score += 1         >> in any other script
	SET("Hud", "text", "Score " + GLOBAL.score)
}
```

`GLOBAL.name = value` also works without declaring it first. A `GLOBAL.name` nobody has set
yet isn't defined. Globals start over each time the game runs.

## Variables set per instance: `$name`

A line with just `$name` at the top of a script declares a variable whose value isn't in the
script: every object using the script gets its own, set in Properties under **Script
variables** (a number, text, switch or color; 0 until set).

```
$speed
$label

DEF on_step(dt) {
	self.x += $speed * dt
}
```

`$speed` is the object's `speed` property, so it's the same thing as `self.speed`, and other
scripts can read it with `GET(object, "speed")`.

## Events

| Function | When it runs |
|---|---|
| top-level code | when the script is attached (before `on_create`) |
| `on_create()` | once the whole blueprint is loaded, or after `SPAWN` |
| `on_step(dt)` | every frame |
| `on_physics(dt)` | 60 times a second, before physics moves bodies (use it for movement) |
| `on_draw()` | every frame after `on_step`: the place for `DRAW_*` commands |
| `on_destroy()` | before the object is removed |
| `on_key_pressed(key)` / `on_key_released(key)` | any key |
| `on_press_<key>()` / `on_release_<key>()` | one key, e.g. `on_press_space()`, `on_press_a()`, `on_press_pad_a()` |
| `on_mouse_pressed(button, x, y)` | a click (`"left"`, `"right"`, `"middle"`) |
| `on_mouse_moved(x, y)`, `on_mouse_wheel(amount)` | |
| `on_collide_start(other)` / `on_collide(other)` / `on_collide_end(other)` | a Collider2D or a body touches another |
| `on_timer(name)` / `on_timer_<name>()` | a `TIMER` finishing |
| `on_pressed()` | on a Button's own scripts: it was clicked |
| `on_pressed_<button name>()` | on every script: a Button with that name was clicked (`Start Game` → `on_pressed_start_game`) |
| `on_text_changed(text)` / `on_text_submitted(text)` | on a TextInput's scripts |
| `on_clip_end(clip)` | on an Animator's parent (and the Animator): its clip finished |
| `on_tween_end(property)` | a `TWEEN` on this object finished |
| `on_animation_end(name)` | a non-looping sprite or model animation finished |
| `on_<event>(...)` | `SEND(object, "<event>", ...)` or `EMIT("<event>", ...)` (or `NET_EMIT` / `NET_SEND` from another player's game) |
| `on_connected(id)` / `on_join_failed(reason)` | multiplayer: `JOIN_GAME` got you in, as player `id` / couldn't |
| `on_player_joined(id)` / `on_player_left(id)` | multiplayer: another player joined / left |
| `on_disconnected(reason)` | multiplayer: the host left, or the connection broke |
| `when_<name>()` → `on_<name>()` | a condition: `on_<name>` runs once each time `when_<name>` turns TRUE |

The Events panel lists these; click one to add it to a script.

Collisions: a `Collider2D` reports for its parent (or for itself, if it has scripts), so a
Sprite2D with a Collider2D child gets the events. Bodies (`Body2D`, `Body3D`) report for
themselves.

Key names: `"a"`–`"z"`, `"0"`–`"9"`, `"f1"`…, `"space"`, `"enter"`, `"escape"`, `"tab"`,
`"backspace"`, `"left"`, `"right"`, `"up"`, `"down"`, `"shift"`, `"ctrl"`, `"alt"`. Mouse buttons
work with `KEY_DOWN` too: `"mouse_left"`, `"mouse_right"`, `"mouse_middle"`. Gamepad buttons are
keys named `"pad_a"`, `"pad_b"`, `"pad_x"`, `"pad_y"`, `"pad_lb"`, `"pad_rb"`, `"pad_lt"`,
`"pad_rt"`, `"pad_start"`, `"pad_select"`, `"pad_up"`/`"pad_down"`/`"pad_left"`/`"pad_right"`;
sticks are `AXIS("left_x")` and friends.

**Input actions.** In File → Project settings, give an action a name and some keys (`jump` =
`space, w, pad_a`). Then `KEY_DOWN("jump")`, `KEY_PRESSED("jump")` and `on_press_jump()` work
with any of them, and you can change the keys later without touching scripts.

## Engine commands

Where a command takes an object, give an id (a number from `self.id`, `OBJECT`, `SPAWN`...), a path
(`"Gun"`, `"../Enemy"`, `"/Level/Door"`, relative to this object), or `NULL` for this object.
Colors are tables `[r: 1, g: 0.5, b: 0, a: 1]` (0 to 1) or a hex text like `"#ff8800"`.
Files are `res://...` (your project) or `hl2://...` (a mounted Source game, see
[HL2.md](HL2.md)).

The script editor completes these as you type and shows each one's inputs. **View → Commands**
(or **F1**) opens the Command reference in a tab of its own: every command explained, with an example to copy, plus
the events. F1 with the cursor on a command in a script opens that command's page.

### Objects and the tree

| Command | |
|---|---|
| `OBJECT(path)` | the object at a path (relative to this one), or NULL |
| `GET(object, property)` | a property of another object |
| `SET(object, property, value)` | changes a property of another object |
| `OBJECTS(group)` | every object in a group, as a table of ids |
| `CHILDREN(object)` | an object's children, as a table of ids |
| `PARENT(object)` | an object's parent, or NULL |
| `NAME_OF(object)` | an object's name |
| `PATH_OF(object)` | an object's full path |
| `SPAWN(blueprint, x, y, z, parent)` | adds an instance of a blueprint file; gives its id |
| `DESTROY(object)` | removes an object (at the end of the frame); runs on_destroy |

### Events and timers

| Command | |
|---|---|
| `SEND(object, event, inputs...)` | runs on_<event>(inputs...) on one object |
| `EMIT(event, inputs...)` | runs on_<event>(inputs...) on every object that has it |
| `BROADCAST(event, inputs...)` | another name for EMIT |
| `TIMER(name, seconds, repeat)` | runs on_timer(name) and on_timer_<name>() later |
| `TWEEN(object, property, to, seconds, ease)` | slides a number, position or color to a new value; ease: linear, in, out (default), in_out, back, bounce, elastic; then on_tween_end(property) |
| `TWEEN_STOP(object, property)` | stops an object's tweens (just that property's, if given), leaving the value where it is |
| `TIMER_STOP(name)` | cancels this object's timer |

### Physics and queries

| Command | |
|---|---|
| `COLLIDING(object)` | the objects touching this one now, as a table of ids |
| `MOVE(object, dx, dy, dz)` | moves a body, sliding along walls; gives TRUE when it's on the floor after |
| `IMPULSE(object, x, y, z)` | adds to a dynamic body's velocity at once (a kick or a jump), whatever its mass |
| `RAYCAST(x, y, dx, dy, length, ignore, layers)` | the first 2D body along a line: [object:, x:, y:, nx:, ny:, distance:] or NULL; layers (like "world, enemies") limits what it can hit |
| `RAYCAST_3D(x, y, z, dx, dy, dz, length, ignore, layers)` | the same in 3D |
| `DISTANCE(a, b)` | how far apart two objects are |
| `FIND_PATH(tilemap, x1, y1, x2, y2)` | a walkable path through a TileMap2D: a table of [x:, y:] points, or NULL |

### Sound

| Command | |
|---|---|
| `PLAY_SOUND(file, volume, pitch, pan, echo)` | plays a sound file once (res:// or hl2://...); pan -1 left to 1 right, echo 0 to 1; gives its id |
| `STOP_SOUND(id)` | stops a sound from PLAY_SOUND (NULL stops every sound) |
| `PLAY(object)` | starts a Sound object |
| `STOP(object)` | stops a Sound object |
| `MUSIC(file, volume)` | plays a looping music track, replacing the last one ("" stops it) |
| `VOLUME(level)` | the master volume, 0 to 1 |

### Animation, particles and tiles

| Command | |
|---|---|
| `BURST(object, count)` | a Particles2D or Particles3D throws out this many particles at once (an explosion, a puff) |
| `RAGDOLL(object, vx, vy, vz)` | a Mesh3D with a skinned model (a character) goes limp and falls as a ragdoll, starting at that velocity; IMPULSE shoves it |
| `USE_DOOR(door, open)` | opens or closes one of a map's doors (in the group map_doors); with no second input, toggles it |
| `MAP_ENTITIES(map, classname)` | the entities in a map (a Mesh3D showing a .bsp): a table of [classname:, name:, x:, y:, z:, rotation_y:, keys:, bounds:] (keys holds every setting as written; bounds is a brush entity's box, like a ladder's); classname picks one kind, "info_player*" any that start so |
| `PLAY_CLIP(animator, clip)` | an Animator plays one of its clips (made in the Timeline) from the start; on_clip_end(clip) runs on its parent when it ends |
| `STOP_CLIP(animator)` | stops an Animator's clip where it is |
| `CLIP_PLAYING(animator)` | the name of the clip an Animator is playing, or NULL |
| `ANIMATE(object, name, restart)` | plays an animation: a Sprite2D's (from its animations property) or a model's; the one already playing carries on unless restart is TRUE |
| `ANIMATIONS(object)` | the names of a Mesh3D model's animations |
| `TILE(object, col, row)` | a TileMap2D's tile id at a cell (-1 = empty) |
| `SET_TILE(object, col, row, id)` | changes a tile (-1 clears it) |
| `TILE_AT(object, x, y)` | the cell under a world position: [col:, row:] |

### Saving and remembering

| Command | |
|---|---|
| `SAVE_DATA(name, value)` | keeps a value (a table, number, text...) on disk between runs |
| `LOAD_DATA(name)` | a value kept with SAVE_DATA, or NULL |
| `DELETE_DATA(name)` | forgets a value kept with SAVE_DATA |
| `REMEMBER(name, value)` | keeps a value while the game runs, across CHANGE_BLUEPRINT (lives, score, inventory) |
| `RECALL(name)` | a value kept with REMEMBER, or NULL |

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
| `MOUSE_LOCK(on)` | hides the mouse and keeps it in the window, for mouse-look (Escape frees it in the editor) |
| `MOUSE_DX()` | how far the mouse moved sideways this frame, in screen pixels (works while locked) |
| `MOUSE_DY()` | how far the mouse moved up or down this frame (down is positive) |

### Game flow and camera

| Command | |
|---|---|
| `CHANGE_BLUEPRINT(blueprint, fade, color)` | switches to another blueprint file; with fade (seconds) the screen fades to color (default black) and back |
| `QUIT()` | stops the game |
| `FULLSCREEN(on)` | switches the game window to full screen (TRUE) or back (FALSE); with no input, gives whether it is |
| `WINDOW_TITLE(text)` | changes the game window's title |
| `PAUSE(on)` | pauses the game (TRUE) or carries on (FALSE): only objects in the group while_paused keep running, for a pause menu |
| `PAUSED()` | TRUE while the game is paused |
| `TIME_SCALE(scale)` | how fast game time runs: 1 normal, 0.3 slow motion, 2 double; with no input, gives the current scale |
| `SHAKE(strength, seconds)` | shakes the camera: strength in pixels (2D), dying away over seconds |

### Settings while playing

Every project display, graphics and physics setting (listed in [SETTINGS.md](SETTINGS.md)) and
every input action can change while the game runs: an options menu, key rebinding, a low-detail
mode, moon gravity.

| Command | What it does |
|---|---|
| `SET_GRAPHICS(setting, value)` | changes a graphics setting: `SET_GRAPHICS("shadows", FALSE)`, `SET_GRAPHICS("tonemap", "aces")`, `SET_GRAPHICS("tint", "#ffd0a0")` |
| `GRAPHICS(setting)` | a graphics setting's value; `GRAPHICS()` gives a table of all of them |
| `SET_PHYSICS(setting, value)` | changes a physics setting: `SET_PHYSICS("gravity_2d", [0, 400])`, `SET_PHYSICS("tick_rate", 120)` |
| `PHYSICS(setting)` | a physics setting's value; `PHYSICS()` gives a table of all of them |
| `SET_DISPLAY(setting, value)` | changes a display setting: `SET_DISPLAY("width", 640)`, `SET_DISPLAY("scale_mode", "stretch")`, `SET_DISPLAY("fullscreen", TRUE)` |
| `DISPLAY(setting)` | a display setting's value; `DISPLAY()` gives a table of all of them |
| `SAVE_DISPLAY(keep)` | keeps the current display settings for the next time the exported game starts; `SAVE_DISPLAY(FALSE)` forgets them |
| `SET_ACTION(action, keys)` | rebinds an input action: `SET_ACTION("jump", ["space", "w"])`; no keys removes it |
| `ACTION(action)` | an action's keys as a list; `ACTION()` gives a table of every action |

Each object's own look and physics properties change like any other: `self.opacity = 0.4`,
`SET("Crate", "linear_damping", 3)`.

## Multiplayer

One player hosts and the others join; every game runs the same project. Everything goes
through the host's game, which passes it on.

| Command | What it does |
|---|---|
| `HOST_GAME(port)` | starts a game others can join (port 7777 when left out); you're player 1 |
| `JOIN_GAME(address, port)` | joins one: `JOIN_GAME("192.168.1.20")`, `JOIN_GAME("localhost")`, `JOIN_GAME("example.com:8000")`; then `on_connected(id)` or `on_join_failed(reason)` |
| `LEAVE_GAME()` | leaves (when the host leaves, the game ends for everyone) |
| `ONLINE()`, `IS_HOST()`, `MY_ID()`, `PLAYERS()` | whether you're in a game, whether you host it, your player number (0 offline), every player's number |
| `NET_EMIT(event, inputs...)` | runs `on_<event>(inputs...)` on every object in every *other* player's game |
| `NET_SEND(player, event, inputs...)` | the same in one player's game |
| `SENDER()` | in such an event: who sent it |
| `NET_SPAWN(blueprint, x, y, z, parent)` | `SPAWN` for everyone; your game owns the instance |
| `NET_SYNC(object)` | shares an object placed in the blueprint (the host owns it) |
| `OWNER(object)`, `IS_MINE(object)`, `SET_OWNER(object, player)` | who moves a shared object; hand it to someone else |
| `NET_ID(object)`, `NET_OBJECT(net_id)` | a shared object's name in every game, and back (object ids differ between games) |
| `MY_ADDRESS()` | this computer's address for friends to join: its ZeroTier one when it has one, else its local network one |

**Shared objects.** The owner's game sends a shared object's position, rotation, size,
visibility and properties (`self.health`, a sprite's `flip_x`, ...), and those of the objects
inside it, 20 times a second when they change; the other games glide their copies there.
`DESTROY` removes it for everyone, and a player's objects go when they leave. Players who
join later get every shared object still there; objects only appear for players in the same
blueprint.

**Scripts run in every game**, on every copy. Code that reads keys (or decides things only one
game should) checks first: `IF !IS_MINE() { RETURN }`, `IF IS_HOST() { ... }`. Not online,
`IS_MINE()` is always TRUE, so the same scripts work in single player. Other players' copies of
a body follow instead of falling, and their cameras stay off.

```
>> main.jlb, on the menu:
DEF on_pressed_host() {
    HOST_GAME()
    NET_SPAWN("res://player.jblueprint", 200, 300)
}
DEF on_pressed_join() {
    JOIN_GAME("localhost")
}
DEF on_connected(id) {
    NET_SPAWN("res://player.jblueprint", 200 + 80 * id, 300)
}

>> player.jlb:
DEF on_physics(dt) {
    IF !IS_MINE() { RETURN }
    x += AXIS("left", "right") * 250 * dt
}
```

To try it on one computer, Build the game, run it and host, then press Play in the editor and
join `"localhost"`. Over the internet everyone joins the same ZeroTier network (free; Tailscale
works too) and uses the host's ZeroTier address (`MY_ADDRESS()` gives it), or the host forwards
the port (TCP 7777) on their router. Windows counts ZeroTier networks as Public: when it asks
about the firewall the first time a game hosts, tick Public too (in the editor, File → Work
together → Allow GEX through the firewall does it).

## Waiting: tasks

An event has to finish within its frame, so it can't `WAIT`. For anything that happens over
time (a cutblueprint, an enemy's attack pattern, a countdown), put it in its own function and run
it as a task with `START`:

```
DEF on_create() {
	START intro()
}

DEF intro() {
	SET("Caption", "text", "Long ago...")
	WAIT(2)
	TWEEN("Caption", "color", "#ffffff00", 1)
	WAIT(1)
	SET("Caption", "text", "Press space")
}
```

- `WAIT(seconds)` counts game time: it slows with `TIME_SCALE` and holds while the game is paused.
- `t = START f()` gives back the task; `AWAIT(t)` (inside another task) waits for it and gives
  back what it returned; `DONE(t)` says whether it has finished; `CANCEL(t)` stops it.
- A task belongs to the object whose event started it: `self` and `NULL` mean that object, and
  destroying the object stops its tasks.
- Tasks take turns with the game: each runs until its next `WAIT` (or a thousand statements),
  then the game carries on and the task picks up next frame.

## Pausing

`PAUSE(TRUE)` freezes the game: frame events (`on_step`, `on_physics`, `on_draw`), input
events, timers, tweens, animations, particles, physics and collisions all stop. Objects in the
group **`while_paused`** (add it in the Inspector's Groups) carry on, so put your pause menu's
script there:

```
>> On an object in the while_paused group.
DEF on_press_escape() {
	PAUSE(!PAUSED())
	SET("PauseMenu", "visible", PAUSED())
}
```

Custom events (`SEND`, `EMIT`), `on_create` and `on_destroy` still run while paused.
`TIME_SCALE(0.3)` gives slow motion: `dt`, timers, tweens, animations and physics all slow down.

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

## First-person controls in short

`MOUSE_LOCK(TRUE)` hides the mouse and holds it; then `MOUSE_DX()` / `MOUSE_DY()` say how far it
moved each frame. Turn the body with `self.rotation_y` and tilt a Camera3D child with its
`rotation_x`; move with `MOVE(NULL, dx, dy, dz)` on a kinematic capsule Body3D. The demo's
`scripts/fps.jlb` is a complete example. In the editor, Escape frees the mouse; click the game
view to grab it again.

## Debugging

Click the gutter next to a line in the script editor to set a breakpoint. When the game reaches it, it pauses: the Console's debugger shows the call
stack, the function's locals and the script's outside variables; F8 continues, F10 runs one
line. Right-click the gutter for a conditional breakpoint (it stops only when an expression like
`hp < 10` is TRUE), and add watch expressions under the debugger: they're worked out, in the
stopped function, each time the game stops. A script error shows in the Console with its file and line (click to jump there); that
script stops until the game restarts while the rest keeps running. Saving a script while the
game runs reloads it in place, keeping its outside variables.

## Everything else

Everything else in .jlb works as usual: tables, IMPORT/LOAD (relative to the script's folder),
TRY/FAIL, the maths and text builtins. `PRINT` goes to the editor's console. `END` stops the
game. Note that "not equal" in .jlb is `!==`.

Not in the engine: .jlb's own windows and drawing commands (`WINDOW`, `RECT`...): use objects and
the `DRAW_*` commands instead.

## Older names

`NODE`, `NODES` and `CHANGE_SCENE` are the old names of `OBJECT`, `OBJECTS` and
`CHANGE_BLUEPRINT`; scripts using them still run. Projects with `.jscene` files are converted
to `.jblueprint` when opened.
