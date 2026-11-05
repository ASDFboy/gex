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

## What works today (milestone 0)

- **Scenes all the way down.** A scene is a node tree saved as a `.jscene` file. Any node can be
  an instance of another scene, with overrides on top; a scene whose root is an instance is an
  inherited scene. Saving writes instances by reference, so editing `enemy.jscene` updates every
  enemy everywhere.
- **2D and 3D nodes** in one tree: `Node2D`, `Sprite2D`, `Label2D`, `Camera2D`, `Collider2D`,
  `Node3D`, `Mesh3D` (cube / sphere / plane), `Camera3D`, `Light3D`.
- **Scripting in .jlb.** Any number of scripts per node. A script's outside variables are that
  node's instance variables, `self` is the node, and events are functions with fixed names
  (`on_create`, `on_step(dt)`, `on_collide_start(other)`, `on_press_space`, `on_timer_<name>`,
  custom `on_<event>` via `SEND`/`EMIT`, and condition events: `when_x()` turning TRUE runs
  `on_x()`). See [docs/SCRIPTING.md](docs/SCRIPTING.md).
- **The editor:** dockable panels (hierarchy with drag-to-reparent, inspector, events, assets,
  console, script tabs), 2D/3D scene view with picking and dragging, undo/redo, a Neovim-style
  `:` command line, Play/Stop in the editor with live values in the inspector, and a script
  editor with JLB highlighting and live syntax checks from the interpreter itself.
- **Assets:** `res://` for project files, plus mounts. A Source-engine install (Half-Life 2 and
  its episodes) mounts at `hl2://` and is read in place, loose files and `.vpk` archives, with
  `.vtf` textures and `.vmt` materials usable on sprites and meshes. See [docs/HL2.md](docs/HL2.md).

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
