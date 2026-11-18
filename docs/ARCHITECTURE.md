# Architecture

## The stack, and why

| Need | Choice | Why |
|---|---|---|
| Language | **Rust** | The JLB interpreter (jlb-rust) is Rust, so it embeds directly with no FFI layer. |
| Rendering | **wgpu** | One API over DirectX 12, Vulkan and Metal (plus GL as a fallback). Free, no SDK installs. |
| Editor UI | **egui** | Immediate-mode, native, small. The viewport is a wgpu texture inside the UI, so the editor and game share one renderer. Idle cost is near zero: egui only repaints on input (or while a game runs). |
| Math | glam | The usual for Rust games. |
| Files | RON | Readable, diff-friendly text for blueprints and projects. |
| Valve formats | own readers (VPK, VTF, VMT) | Small, no dependencies, tested against synthetic files. `vbsp`/`vmdl` crates are lined up for maps and models. |

Considered and passed over: **Bevy** (its ECS fights a Godot-style object tree and a single-threaded
interpreter, and its editor story isn't there yet); **Tauri/web UI** for the editor (heavier at
runtime, and the 3D viewport would need a second renderer or a WebGPU bridge); **Godot as a
base** (its own scripting and blueprint formats would compete with JLB everywhere).

## Data flow

```
 .jblueprint files ──blueprint::load──▶ BlueprintTree ◀── inspector, hierarchy, viewport edits (undo = tree snapshots)
                                    │
                    Play: clone ────┤
                                    ▼
                    game thread: Game { BlueprintTree + Interp + one Env per script }
                         ▲   input each frame          │ RenderList + tree copy + log lines
                         └──────── editor ◀────────────┘
                                    │
                    render::extract(tree) ─▶ RenderList ─▶ gex-render ─▶ texture ─▶ egui viewport
```

- **One building block.** `Object { name, kind, transform, props, scripts, groups, children }`.
  The kind decides what the engine does with it; everything else is the same for all objects.
  Properties are `Variant`s, which the inspector, blueprint files and scripts all share.
- **Blueprints are trees, instances are references.** `blueprint::build` expands an instance by
  loading its file; the instance root remembers `instance_of`, and objects inside are marked
  `from_instance`. `blueprint::describe` saves only what differs from the instanced file.
- **The game runs on its own thread.** The interpreter is single-threaded (`Rc`), so a game
  owns its own `Interp` and tree on a thread with a large stack. A stuck script never freezes
  the editor: Stop sets a cancel flag the interpreter checks between statements.
- **The renderer only sees a `RenderList`**: sprites, labels, meshes, lights, colliders,
  cameras, particles, the fade color. Text and UI objects are drawn by egui on top. The same path serves the editor's view, play mode and
  the standalone player.

## Scripting bridge

`third_party/jlb` is jlb-rust plus a small embedding layer (`src/embed.rs`, `src/host.rs`,
`src/lib.rs`), kept as a patch (`third_party/jlb-rust-embed.patch`) so it can go upstream:

- `host::register(names)` lets a host add ALL-CAPS commands the lexer accepts.
- `Interp::host` receives those calls; it may also take over a built-in (the engine answers
  `KEY_DOWN` from its own input), falling back to the built-in otherwise.
- `Interp::load_module(path, preset)` runs a file in a fresh set of outside variables;
  `call_value` calls a function in it; `set_output` redirects PRINT; `cancel` stops code.

Each script attached to an object is loaded once per object, so its outside variables are that object's
instance variables. Before each event the engine writes the object into a `self` table; after it,
whatever the script changed in `self` is written back (custom keys become object properties and
show in the inspector). Engine commands that change other objects (`SET`, `SPAWN`, ...) work on the
tree directly; anything that runs more script code (`SEND`, `EMIT`, `DESTROY`) is queued and run
right after, so scripts never re-enter each other.

## Where things live

| Area | Files |
|---|---|
| Objects, properties, blueprint files, instancing | `gex-core/src/{object,blueprint,tree,variant}.rs` |
| What gets drawn | `gex-core/src/render.rs` (`extract`), `gex-render/src/lib.rs`, `shaders.wgsl` |
| The game loop and events | `gex-script/src/world.rs` (`Game::step`) |
| Engine commands | `gex-script/src/commands.rs` (the `COMMANDS` list feeds completion and a docs test) |
| Physics | `gex-script/src/physics.rs` (rapier2d / rapier3d) |
| Particles, tweens, audio, saves, paths | `gex-script/src/{particles,tween,audio,extras}.rs` |
| The game thread | `gex-script/src/thread.rs` |
| Files and mounts, Source formats, models | `gex-assets/src/{vfs,vpk,vtf,vmt,source,model}.rs` |
| Editor panels, blueprint view, script editor | `gex-editor/src/{app,panels,viewport,code,vim}.rs` |
| Editor state, undo, play mode, export | `gex-editor/src/state.rs` |
| Project file operations | `gex-editor/src/files.rs` |
| The built-in demo | `gex-editor/src/demo/` (its test plays every blueprint) |
