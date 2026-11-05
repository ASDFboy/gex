# GEX

A 2D and 3D game engine with a docked editor, Godot-style scenes, and game logic written in
[.jlb](https://github.com/ASDFboy/jlb-rust), using GameMaker-style events.

By ASDFboy and Claude.

## Run it

```
gex                       the project library (new / open / demo)
gex <project folder>      open a project in the editor
gex --play <folder>       run a project's main scene, no editor
```

Build from source (Rust 1.85+):

```
cargo run --release -p gex-editor              # the editor
cargo test --workspace                         # everything's tests, including the demo game
```

## What's in it

- **Scenes all the way down.** A scene is a node tree saved as a `.jscene` file. Any node can be
  an instance of another scene, with overrides on top; a scene whose root is an instance is an
  inherited scene. Instances are saved by reference, so editing `enemy.jscene` updates every
  enemy everywhere. Right-click a branch → **Save branch as scene** to make one.
- **2D:** sprites with sprite-sheet animation, text, cameras, trigger areas, physics bodies
  (dynamic / kinematic / static, boxes, circles, capsules), tilemaps painted in the editor with
  automatic collision and A* pathfinding, and `DRAW_*` commands for quick shapes.
- **3D:** cubes, spheres, planes and models (glTF/GLB with skeletal animation, OBJ, Source
  `.mdl` and `.bsp`), directional / point / spot lights with shadows, sky, fog, physics bodies
  and mesh collision.
- **UI:** panels, labels, buttons, text inputs, progress bars and images, anchored to the screen.
- **Sound:** sound effects, music, Sound nodes (wav, ogg, mp3, flac).
- **Input:** keyboard, mouse and gamepads, with named input actions set in Project settings.
- **Particles:** a Particles2D node for sparks, smoke and trails (previewed live in the editor).
- **Scripting in .jlb.** Any number of scripts per node. A script's outside variables are that
  node's instance variables, `self` is the node, and events are functions with fixed names
  (`on_create`, `on_step(dt)`, `on_collide_start(other)`, `on_press_space`, `on_timer_<name>`,
  `on_pressed_<button>`, custom `on_<event>` via `SEND`/`EMIT`, and condition events:
  `when_x()` turning TRUE runs `on_x()`). Save data between runs with `SAVE_DATA`.
  See [docs/SCRIPTING.md](docs/SCRIPTING.md).
- **The editor:** dockable panels (hierarchy, inspector, events, assets, console), scene tabs,
  2D/3D view with move / rotate / scale tools and box select, undo/redo, copy/paste, a Neovim-style
  `:` command line, optional Vim keys in the script editor, completion, go to definition,
  find/replace, breakpoints with a debugger, Play/Stop with live values in the inspector, and
  scripts that reload while the game runs.
- **Project files:** the Assets panel makes folders, scenes and scripts, imports files dropped on
  the window, and renames (fixing every scene and script that uses the file) or deletes (into a
  hidden `.trash` folder) with a right-click.
- **Export:** File → Export game makes a folder with the game and a player that runs it.
- **Half-Life 2 assets**, read in place from your own Steam install (`hl2://`): VPK archives,
  VTF textures, VMT materials, models, maps and sounds. They are never copied into exports.
  See [docs/HL2.md](docs/HL2.md).

## Editor keys

| Key | |
|---|---|
| F5 / Shift+F5 | play / stop |
| F8 / F10 | continue / step one line (paused at a breakpoint) |
| Ctrl+S | save everything |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Ctrl+C / Ctrl+V / Ctrl+D | copy / paste / duplicate nodes |
| Delete | delete the selected nodes |
| W / E / R | move / rotate / scale tool |
| `:` | the command line (`:w`, `:q`, `:play`, `:add Sprite2D`, `:e scenes/level.jscene`, `:branch`, `:vim`, `:2d`, `:3d`…) |
| Right-drag / middle-drag (or Shift+right-drag) / wheel | look around (3D) or pan (2D) / pan / zoom in the scene view |
| F12 or Ctrl+click | go to a function's definition (script editor) |
| Ctrl+F | find and replace (script editor) |

## Learn it

1. [Tutorial 1: the basics](docs/TUTORIAL-1-BASICS.md): nodes, scripts, collisions, reusable
   scenes, a score, a menu, export.
2. [Tutorial 2: going further](docs/TUTORIAL-2-ADVANCED.md): physics, tilemaps and pathfinding,
   animation, cameras, tweens and particles, UI, input actions, saving, 3D, first person,
   Half-Life 2 assets, debugging.
3. Reference: [every node](docs/NODES.md), [every script event and command](docs/SCRIPTING.md).

## Layout

| Crate | What it is |
|---|---|
| `crates/gex-core` | node tree, scene files and instancing, projects, input state, render extraction |
| `crates/gex-script` | runs .jlb scripts: events, engine commands, the game loop, the game thread |
| `crates/gex-assets` | the virtual file system: `res://`, mounted game installs, VPK, VTF, VMT, images |
| `crates/gex-render` | wgpu renderer: 2D sprites, lit 3D meshes, lines; draws to a texture |
| `crates/gex-editor` | the `gex` program: editor and player, built with egui |
| `third_party/jlb` | the .jlb interpreter (jlb-rust), with an embedding API added |

How the pieces fit: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
