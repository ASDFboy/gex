# Tutorial 1: the basics

This walks through making a tiny game from an empty project: a player that moves, a coin to
pick up, a score, and a menu. It takes about half an hour. Every step uses things that are in
the editor today.

If you'd rather read working code first, the launcher's **Create the demo project** makes a
project with five small games in it (coins, a platformer, a 3D physics toy, first person, and a
menu that ties them together). Every script in it is commented.

## 1. Make a project

1. Start `gex`. The launcher shows your recent projects on the left.
2. Under **NEW PROJECT**, type a name and a folder, then **Create empty project**.
3. The editor opens on `scenes/main.jscene`, which holds one node: `Main` (a Node2D).

The editor's panels:

| Panel | What it's for |
|---|---|
| Hierarchy (left) | the scene's node tree. Drag to re-parent; right-click for more |
| Scene view (middle) | see and move things. 2D / 3D switch at the top left; W / E / R pick move / rotate / scale |
| Inspector (right) | the selected node's position and properties |
| Events (right, second tab) | the scripts on the selected node and the events they handle |
| Console / Assets (bottom) | messages and errors; your project's files |

Every panel can be dragged somewhere else. **View → Reset layout** puts them back.

## 2. Nodes: make a player

Everything in a game is a node, and a scene is a tree of nodes.

1. Select `Main`, then **+ Node → 2D → Sprite2D** in the Hierarchy. Rename it by typing
   `Player` in the name field at the top of the Inspector.
2. In the Inspector, set **Position** to x 640, y 360 (the middle of a 1280×720 window), and in
   **Properties** set `width` and `height` to 40 and `color` to something bright.
3. Want a picture instead of a colored box? Drop a PNG on the editor window (it's copied into
   the project), then drag it from the Assets panel onto the `texture` field.

The grey frame in the scene view is the game window. 2D positions are in pixels, with y going
**down**.

## 3. Scripts: make it move

1. With `Player` selected, open the **Events** tab and press **+ New script**. A script tab opens
   with `scripts/player.jlb`.
2. Replace its contents with:

```
>> Outside variables belong to this node.
speed = 300

DEF on_step(dt) {
	IF KEY_DOWN("left") OR KEY_DOWN("a") { self.x -= speed * dt }
	IF KEY_DOWN("right") OR KEY_DOWN("d") { self.x += speed * dt }
	IF KEY_DOWN("up") OR KEY_DOWN("w") { self.y -= speed * dt }
	IF KEY_DOWN("down") OR KEY_DOWN("s") { self.y += speed * dt }
}
```

3. Press **F5** (or ▶ Play). Click the scene view and use the arrow keys. **Shift+F5** stops.

What's going on:

- A script is a list of **events**: functions with fixed names that the engine calls.
  `on_step(dt)` runs every frame; `dt` is the seconds since the last one.
- `self` is the node the script is on. `self.x`, `self.y`, `self.rotation`, `self.visible`, and
  every property in the Inspector (`self.color`, `self.width`...) can be read and changed.
- Variables written at the top of the file (`speed`) belong to this node. Ten players would each
  have their own `speed`. Inside a function, change one with `GLOBAL speed = 400`.
- ALL-CAPS words like `KEY_DOWN` are engine commands. Type a few letters and the editor offers
  them, with their inputs. The full list is in [SCRIPTING.md](SCRIPTING.md).

The Events tab lists the events a script handles; **+ Event** adds a new one with its inputs
filled in. A node can have several scripts, and each one gets the events.

## 4. Collisions: a coin to collect

1. Add a child to `Main`: **Sprite2D**, named `Coin`, 20×20, yellow, at x 300, y 250.
2. Right-click `Coin` → **Add child → Collider2D**, size 20×20. Do the same for `Player` (40×40).
3. Add a script to `Player`'s list of events (in `player.jlb`):

```
DEF on_collide_start(other) {
	PRINT("touched " + NAME_OF(other))
	DESTROY(other)
}
```

Play: touching the coin prints a line in the Console and removes it. A Collider2D speaks for its
parent (unless it has scripts of its own), so `Player` hears about its collider's touches, and
`other` is the Coin.

If a script has a mistake, the Console shows the file and line in red. Click it to jump there.
That script stops until you play again, and everything else keeps running.

## 5. Reusable scenes: many coins

1. Right-click `Coin` → **Save branch as scene**. It becomes `scenes/coin.jscene`, and the
   `Coin` in your level becomes a copy of that file (the 🎬 mark).
2. Drag `scenes/coin.jscene` from the Assets panel into the scene view a few times.
3. Open the coin scene (right-click a copy → **Open its scene**; it opens in a tab), make it
   orange and save with **Ctrl+S**: every coin everywhere changes. Changing one copy in the
   level (select a node inside it, edit it in the Inspector) changes only that copy.

To spawn more while the game runs, use `SPAWN("res://scenes/coin.jscene", x, y)`.

## 6. Talking between nodes: a score

1. Add a **Label** to `Main` (under **UI**), named `Score`, at x 20, y 16, `text` = `Score: 0`.
   UI nodes stay put on the screen while the camera moves.
2. Add a script to `Main` (select it, **+ New script**):

```
score = 0

DEF on_coin_taken() {
	GLOBAL score += 1
	SET("Score", "text", "Score: " + score)
}
```

3. In `player.jlb`, announce pickups instead of printing:

```
DEF on_collide_start(other) {
	DESTROY(other)
	EMIT("coin_taken")
}
```

`EMIT("coin_taken")` runs `on_coin_taken()` on every script that has it. `SEND(node,
"coin_taken")` runs it on one node only. You can pass values: `EMIT("hurt", 10)` →
`DEF on_hurt(amount)`.

Paths like `"Score"` are relative to the node running the script: `"Score"` is a child of
`Main`; from the player it would be `"../Score"`, and `"/Main/Score"` works from anywhere.

## 7. Timers

```
DEF on_create() {
	TIMER("spawn", 2, TRUE)
}

DEF on_timer_spawn() {
	SPAWN("res://scenes/coin.jscene", RANDOM(40, 1240), RANDOM(80, 680))
}
```

`on_create()` runs once when the scene starts. `TIMER(name, seconds, repeat)` calls
`on_timer_<name>()` later.

## 8. A menu and a second scene

1. **File → New scene → with a Node2D root**. It opens in a new tab as
   `scenes/new_scene.jscene`; right-click that file in the Assets panel → **Rename…** →
   `menu.jscene`.
2. Add a **Button** (UI) named `Play`, and a script on the root:

```
DEF on_pressed_play() {
	CHANGE_SCENE("res://scenes/main.jscene", 0.5)
}
```

Clicking a Button named `Play` runs `on_pressed_play()` (a Button named `Start Game` would be
`on_pressed_start_game()`). The `0.5` fades to black and back over half a second.

3. **File → Save scene as main scene** so the game starts on the menu.

To keep a value across scenes (lives, an inventory), `REMEMBER("lives", 3)` and later
`RECALL("lives")`. To keep it after the game closes, `SAVE_DATA("best", score)` and
`LOAD_DATA("best")`.

## 9. Share it

**File → Export game…** makes a folder with your game and a small player program. Double-click
the program in it to play, or zip the folder up and send it.

In the export window, **For** picks the system (Windows, Linux or macOS; GEX looks for the other
systems' players in the `players` folder that comes with it) and **Program name** names the
program. Set an icon (a .png) in **File → Project settings** and it shows on the game's window and on
the Windows program. A macOS export is a `Name.app`; since it isn't signed, the first time you
open it, right-click it → Open.

## Handy keys

| | |
|---|---|
| F5 / Shift+F5 | play / stop |
| Ctrl+S | save everything |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Ctrl+C, Ctrl+V, Ctrl+D, Delete | copy, paste, duplicate, delete nodes |
| W / E / R | move / rotate / scale in the scene view |
| `:` | the command line: `:w`, `:play`, `:add Sprite2D`, `:e scenes/menu.jscene`, `:vim`… |

Next: [Tutorial 2: going further](TUTORIAL-2-ADVANCED.md).
