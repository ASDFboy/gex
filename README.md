<img src="crates/gex-editor/assets/logo.png" width="64" height="64" alt="GEX logo">

# GEX

A 2D and 3D game engine with a GameMaker-style editor (a tab per blueprint), Godot-style blueprints, and game logic written in
[.jlb](https://github.com/ASDFboy/jlb-rust), using GameMaker-style events.

By ASDFboy. **[Website](https://asdfboy.github.io/gex/) · [Download](https://github.com/ASDFboy/gex/releases/latest)**

## Run it

On Windows, download `GEX.zip` from [Releases](https://github.com/ASDFboy/gex/releases/latest), unzip it and run
`GEX.exe`. It's one file with no installer; the docs, templates and demo game are inside it.

```
gex                       the project library (new / open / demo)
gex <project folder>      open a project in the editor
gex --play <folder>       run a project's main blueprint, no editor
```

Build from source (Rust 1.85+):

```
cargo run --release -p gex-editor              # the editor
cargo test --workspace                         # everything's tests, including the demo game
```

To export games for other systems, put their GEX players in a `players` folder next to the
`gex` program: `gex-windows.exe`, `gex-linux`, `gex-macos` (build each with `cargo build
--release -p gex-editor` on, or for, that system and rename it). The Linux player is attached
to each release as `gex-linux`.

GEX keeps itself up to date: when the editor starts (and every 6 hours) it checks this
repository's latest release; a newer one is downloaded, checked against the release's SHA-256
list, put in place and a restart offered. View → Updates chooses whether it installs by itself,
asks first or doesn't check; the old version is removed once the new one starts. It uses the system's `curl`,
which Windows 10 and later have.

New releases are built by GitHub: raise the version in `Cargo.toml` and push to `main` (or push the
`release` branch, or run Actions → Release) and it builds Windows and Linux, runs the tests, and
publishes `GEX.zip`, `gex-windows.exe`, `gex-linux` and `SHA256SUMS` under that version.

If the window stays blank or GEX closes straight away, the graphics driver may not like the
default graphics API: set the `WGPU_BACKEND` environment variable to `vulkan`, `dx12` or `gl`
and start it again.

## What's in it

- **Blueprints all the way down.** A blueprint is an object tree saved as a `.jblueprint` file. Any object can be
  an instance of another blueprint, with overrides on top; a blueprint whose root is an instance is an
  inherited blueprint. Instances are saved by reference, so editing `enemy.jblueprint` updates every
  enemy everywhere. Right-click a branch → **Save branch as blueprint** to make one.
- **2D:** sprites with sprite-sheet animation, text, cameras, trigger areas, physics bodies
  (dynamic / kinematic / static, boxes, circles, capsules), tilemaps painted in the editor with
  automatic collision and A* pathfinding, and `DRAW_*` commands for quick shapes.
- **3D:** cubes, spheres, planes and models (glTF/GLB with skeletal animation, FBX, COLLADA,
  OBJ, 3DS, STL, PLY, MagicaVoxel, Source `.mdl` and `.bsp`), directional / point / spot lights with shadows, sky, fog, physics bodies
  and mesh collision.
- **UI:** panels, labels, buttons, text inputs, progress bars and images.
- **Sound:** sound effects, music, Sound objects (WAV, MP3, OGG, Opus, FLAC, AIFF, M4A, WebM and
  tracker music: MOD, XM, S3M, IT).
- **Pictures:** PNG, JPEG, GIF, WebP, BMP, TGA, TIFF, SVG, Photoshop PSD, DDS, EXR, HDR and more
  ([every format](docs/OBJECTS.md#file-formats)).
- **Input:** keyboard, mouse and gamepads, with named input actions set in Project settings.
- **Particles:** a Particles2D object for sparks, smoke and trails (previewed live in the editor).
- **Scripting in .jlb.** Any number of scripts per object. A script's outside variables are that
  object's instance variables, `self` is the object, and events are functions with fixed names
  (`on_create`, `on_step(dt)`, `on_collide_start(other)`, `on_press_space`, `on_timer_<name>`,
  `on_pressed_<button>`, custom `on_<event>` via `SEND`/`EMIT`, and condition events:
  `when_x()` turning TRUE runs `on_x()`). Save data between runs with `SAVE_DATA`.
  See [docs/SCRIPTING.md](docs/SCRIPTING.md).
- **The editor:** a GameMaker-style tab per blueprint for placing and editing instances, with its
  scripts in small windows wired to their objects; a Monokai Black look,
  2D/3D view with move / rotate / scale tools, undo/redo, copy/paste, a Neovim-style
  `:` command line, optional Vim keys in the script editor, completion, go to definition,
  find/replace, find in files, breakpoints (with conditions and watches) and a debugger, a
  profiler, an animation Timeline, Play/Stop with live values in the inspector, and scripts that
  reload while the game runs.
- **Project files:** the Assets panel makes folders, blueprints and scripts, imports files dropped on
  the window, and renames (fixing every blueprint and script that uses the file) or deletes (into a
  hidden `.trash` folder) with a right-click; every one of those can be undone. It previews
  pictures, models and sounds.
- **Export:** File → Export game makes a folder with the game and a player that runs it, for
  Windows, Linux or macOS (an .app), named as you like and with your icon on the Windows
  program.
- **Multiplayer games:** `HOST_GAME()` and `JOIN_GAME(address)` in scripts; shared objects
  (`NET_SPAWN`, `NET_SYNC`) stay in step between players' games, and `NET_EMIT` runs events in
  everyone's. See [Multiplayer](docs/SCRIPTING.md#multiplayer).
- **Work together, live:** File → Work together shares the open project; others join with your
  address and invite code and get a copy. Blueprints sync property by property, scripts letter
  by letter (two people can type in one script), saved files sync; everyone sees the others'
  tabs, pointers, selections and text cursors, and can chat, follow someone and play together.
  Encrypted, with view-only and remove for the host. See [docs/COLLABORATION.md](docs/COLLABORATION.md).
- **Half-Life 2 assets**, read in place from your own Steam install (`hl2://`): VPK archives,
  VTF textures, VMT materials, models, maps and sounds. They are never copied into exports.
  See [docs/HL2.md](docs/HL2.md).

## Editor keys

| Key | |
|---|---|
| F5 | play the game from its main blueprint (again: stop) |
| F6 | play the blueprint you're editing (again: stop) |
| F1 | the Command reference (on a command in a script: its page) |
| F8 / F10 | continue / step one line (paused at a breakpoint) |
| Ctrl+S | save everything |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Ctrl+C / Ctrl+V / Ctrl+D | copy / paste / duplicate objects |
| Delete | delete the selected objects |
| 1 / 2 / 3 | move / rotate / scale tool |
| W A S D (Q / E) | move the editor camera (Shift: fast) |
| `:` | the command line (`:w`, `:q`, `:play`, `:add Sprite2D`, `:e blueprints/level.jblueprint`, `:branch`, `:vim`, `:2d`, `:3d`…) |
| Right-drag / middle-drag (or Shift+right-drag) / wheel | look around (3D) or pan (2D) / pan / zoom in the blueprint view |
| F12 or Ctrl+click | go to a function's definition (script editor) |
| Ctrl+H | find and replace (script editor) |
| Ctrl+F | find in every script and blueprint |

## Learn it

1. [Tutorial 1: the basics](docs/TUTORIAL-1-BASICS.md): objects, scripts, collisions, reusable
   blueprints, a score, a menu, export.
2. [Tutorial 2: going further](docs/TUTORIAL-2-ADVANCED.md): physics, tilemaps and pathfinding,
   animation, cameras, tweens and particles, UI, input actions, saving, 3D, first person,
   Half-Life 2 assets, debugging.
3. Reference: [every object](docs/OBJECTS.md), [every script event and command](docs/SCRIPTING.md),
   [graphics and physics settings](docs/SETTINGS.md) (all changeable from scripts while playing),
   [the built-in placeholder art](docs/PLACEHOLDERS.md) (`gex://`: 700 shapes, patterns, icons
   and more, ready to drag into a blueprint), [working on a project together](docs/COLLABORATION.md).

## Layout

| Crate | What it is |
|---|---|
| `crates/gex-core` | object tree, blueprint files and instancing, projects, input state, render extraction |
| `crates/gex-script` | runs .jlb scripts: events, engine commands, the game loop, the game thread |
| `crates/gex-assets` | the virtual file system: `res://`, mounted game installs, VPK, VTF, VMT, images |
| `crates/gex-render` | wgpu renderer: 2D sprites, lit 3D meshes, lines; draws to a texture |
| `crates/gex-editor` | the `gex` program: editor and player, built with egui |
| `third_party/jlb` | the .jlb interpreter (jlb-rust), with an embedding API added |

How the pieces fit: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Credits

Icons: [Silk](http://www.famfamfam.com/lab/icons/silk/) by Mark James and [Silk Companion](https://damieng.com/creative/icons/silk-companion-1-icons) by Damien Guard, Creative Commons Attribution 2.5.
