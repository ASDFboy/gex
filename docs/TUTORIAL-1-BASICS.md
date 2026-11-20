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
3. The project starts with nothing in it. Press **New 2D blueprint**: it makes `main.jblueprint`
   (the first blueprint is where the game starts) and opens it. It holds one object, `Main`.

The editor works like GameMaker's. Along the top is a tab for each open blueprint (drag one sideways to
move it):

| Tab | What it's for |
|---|---|
| A blueprint (`main`) | the blueprint itself, like a room. On the left: **Blueprint** (the object tree; drag to re-parent, right-click for more) and under it the selected one's **Properties** and **Events**. In the middle: the view. Scripts open as small windows over the view, each wired to its object; an object's scripts share one window as tabs |

**Assets** on the right lists what the project has, grouped by kind (blueprints, scripts, sprites,
sounds...). **+Blueprint**, **+Script** and **+Folder** make one, double-click opens one, right-click has
the rest (rename, a highlight color, delete), and files dropped on the window are imported. The
Console (messages and errors) comes up along the bottom when something is printed; double-click
it to fold it down to a bar.

## 2. Objects: make a player

Everything in a game is an object, and a blueprint is a tree of objects.

1. **+ Object → Sprite2D**. It appears in the middle of the view. Rename it by typing `Player`
   in the name field at the top of Properties.
2. Set `width` and `height` to 40 and `color` to something bright.
3. Want a picture instead of a colored box? Drop a picture (PNG, JPEG, WebP, SVG...) on the editor window (it's copied into
   the project), then drag it from Assets onto the `texture` field.

The frame in the blueprint view is the game window. 2D positions are in pixels, with y going
**down**.

Moving things in the view: **1 / 2 / 3** pick move, rotate and scale (click the view first; **W A S D** then move the camera, faster with Shift). Drag an arrow (or a
ring, when rotating) to change one axis, a small two-colored square to change two, or the
object itself to move it freely as you look at it. Hold Ctrl to snap.

## 3. Scripts: make it move

1. With `Player` selected, switch **Properties** to **Events** and press **Add event → Step**.
   That makes `player_step.jlb` (object + event) next to the blueprint, attaches it, and opens it in a small window beside the object with the
   cursor inside the new event.
2. Make the script read:

```
>> Outside variables belong to this object.
speed = 300

DEF on_step(dt) {
	x += AXIS("left", "right") * speed * dt
	y += AXIS("up", "down") * speed * dt
}
```

3. Press **F6** (or ▶ Play). The editor shows the game; use the arrow keys. **F6** again stops
   and brings you back to the script.

What's going on:

- A script is a list of **events**: functions with fixed names that the engine calls.
  `on_step(dt)` runs every frame; `dt` is the seconds since the last one.
- `self` is the object the script is on. `self.x`, `self.y`, `self.rotation`, `self.visible`, and
  every property (`self.color`, `self.width`...) can be read and changed.
- Variables written at the top of the file (`speed`) belong to this object. Ten players would each
  have their own `speed`. Inside a function, change one with `GLOBAL speed = 400`.
- ALL-CAPS words like `KEY_DOWN` are engine commands. Type a few letters and the editor offers
  them, with their inputs. The full list is in [SCRIPTING.md](SCRIPTING.md).

**Add event** has every event the engine sends: create, destroy, step, draw, keys, mouse,
collisions, timers, and **Custom** for your own (section 6). An object can have several scripts,
and each one gets the events.

## 4. Collisions: a coin to collect

1. **+ Object → Sprite2D**, named `Coin`, 20×20, yellow. Drag it somewhere away from the player.
2. With `Coin` selected, **+ Object → Collider2D**: it goes on the coin, already the coin's size.
   Do the same for `Player`.
3. Select `Player`, then **Events → Add event → Collision → Start**, and fill it in:

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

## 5. Reusable blueprints: many coins

1. Right-click `Coin` → **Save branch as blueprint**. It becomes `blueprints/coin.jblueprint`, and the
   `Coin` in your level becomes a copy of that file (the 🎬 mark).
2. Drag `blueprints/coin.jblueprint` from the Assets panel into the blueprint view a few times.
3. Open the coin blueprint (right-click a copy → **Open its blueprint**; it opens in a tab), make it
   orange and save with **Ctrl+S**: every coin everywhere changes. Changing one copy in the
   level (select an object inside it, edit it in the Inspector) changes only that copy.

To spawn more while the game runs, use `SPAWN("res://blueprints/coin.jblueprint", x, y)`.

## 6. Talking between objects: a score

1. Add a **Label** to `Main` (under **UI**), named `Score`, at x 20, y 16, `text` = `Score: 0`.
   UI objects stay put on the screen while the camera moves.
2. Select `Main`, then **Events → Add event → Custom**, type `coin_taken`, **Add**:

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

`EMIT("coin_taken")` is a broadcast, like Scratch's: it runs `on_coin_taken()` on every script
that has it (`BROADCAST` is another name for the same command), and once a name is broadcast
somewhere, **Add event → Custom** lists it. `SEND(object,
"coin_taken")` runs it on one object only. You can pass values: `EMIT("hurt", 10)` →
`DEF on_hurt(amount)`.

Paths like `"Score"` are relative to the object running the script: `"Score"` is a child of
`Main`; from the player it would be `"../Score"`, and `"/Main/Score"` works from anywhere.

## 7. Timers

```
DEF on_create() {
	TIMER("spawn", 2, TRUE)
}

DEF on_timer_spawn() {
	SPAWN("res://blueprints/coin.jblueprint", RANDOM(40, 1240), RANDOM(80, 680))
}
```

`on_create()` runs once when the blueprint starts. `TIMER(name, seconds, repeat)` calls
`on_timer_<name>()` later.

## 8. A menu and a second blueprint

1. In Assets, **+ → New blueprint → UI / menu**, and name it `menu`.
2. Add a **Button** (UI) named `Play`, and a script on the root:

```
DEF on_pressed_play() {
	CHANGE_BLUEPRINT("res://blueprints/main.jblueprint", 0.5)
}
```

Clicking a Button named `Play` runs `on_pressed_play()` (a Button named `Start Game` would be
`on_pressed_start_game()`). The `0.5` fades to black and back over half a second.

3. **File → Set as main blueprint** so the game starts on the menu.

To keep a value across blueprints (lives, an inventory), `REMEMBER("lives", 3)` and later
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
| F5 | play the game from its main blueprint (again: stop) |
| F6 | play the blueprint you're editing (again: stop) |
| F1 | the Command reference (on a command in a script: its page) |
| Ctrl+S | save everything |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Ctrl+C, Ctrl+V, Ctrl+D, Delete | copy, paste, duplicate, delete objects |
| 1 / 2 / 3 | move / rotate / scale in the blueprint view |
| W A S D (Q / E) | move the editor camera (Shift: fast) |
| `:` | the command line: `:w`, `:play`, `:add Sprite2D`, `:e blueprints/menu.jblueprint`, `:vim`… |

Next: [Tutorial 2: going further](TUTORIAL-2-ADVANCED.md).
