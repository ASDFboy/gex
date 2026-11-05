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
| `on_physics(dt)` | 60 times a second |
| `on_destroy()` | before the node is removed |
| `on_key_pressed(key)` / `on_key_released(key)` | any key |
| `on_press_<key>()` / `on_release_<key>()` | one key, e.g. `on_press_space()`, `on_press_a()`, `on_press_left()` |
| `on_mouse_pressed(button, x, y)` | a click (`"left"`, `"right"`, `"middle"`) |
| `on_collide_start(other)` / `on_collide(other)` / `on_collide_end(other)` | Collider2D overlaps |
| `on_timer(name)` / `on_timer_<name>()` | a `TIMER` finishing |
| `on_<event>(...)` | `SEND(node, "<event>", ...)` or `EMIT("<event>", ...)` |
| `when_<name>()` → `on_<name>()` | a condition: `on_<name>` runs once each time `when_<name>` turns TRUE |

Collisions: a `Collider2D` reports for the node that has scripts, itself or its parent, so a
Sprite2D with a Collider2D child gets the events.

Key names match .jlb's graphics: `"a"`–`"z"`, `"0"`–`"9"`, `"f1"`…, `"space"`, `"enter"`,
`"escape"`, `"tab"`, `"left"`, `"right"`, `"up"`, `"down"`, `"shift"`, `"ctrl"`, `"alt"`, …

## Engine commands

Where a command takes a node, give an id (a number from `self.id`, `NODE`, `SPAWN`...), a path
(`"Gun"`, `"../Enemy"`, `"/Level/Door"`, relative to this node), or `NULL` for this node.

| Command | |
|---|---|
| `NODE(path)` | the node at a path, or NULL |
| `GET(node, "prop")` / `SET(node, "prop", value)` | read or change another node |
| `NODES(group)` | every node in a group (set in the inspector) |
| `CHILDREN(node)`, `PARENT(node)`, `NAME_OF(node)`, `PATH_OF(node)` | the tree |
| `SPAWN(scene, x, y, z, parent)` | adds an instance of a .jscene; gives its id |
| `DESTROY(node)` | removes a node at the end of the frame |
| `SEND(node, event, ...)` / `EMIT(event, ...)` | custom events, to one node or all |
| `TIMER(name, seconds, repeat)` / `TIMER_STOP(name)` | |
| `COLLIDING(node)` | ids touching a node now |
| `DISTANCE(a, b)` | |
| `KEY_DOWN(k)`, `KEY_PRESSED(k)`, `KEY_RELEASED(k)`, `MOUSE_X()`, `MOUSE_Y()` | input |
| `DELTA_TIME()`, `GAME_TIME()` | time |
| `CHANGE_SCENE(scene)`, `QUIT()` | |

Everything else in .jlb works as usual: tables, IMPORT/LOAD (relative to the script's folder),
TRY/FAIL. `PRINT` goes to the editor's console; a script error shows there with its line (click
it to jump), and that script stops until the game restarts while the rest keeps running. `END`
stops the game.

Not yet in the engine: `START`/tasks inside scripts, and .jlb's own windows and drawing commands
(`WINDOW`, `RECT`...): the engine draws nodes instead.
