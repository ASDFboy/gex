# Architecture

## The stack, and why

| Need | Choice | Why |
|---|---|---|
| Language | **Rust** | The JLB interpreter (jlb-rust) is Rust, so it embeds directly with no FFI layer. |
| Rendering | **wgpu** | One API over DirectX 12, Vulkan and Metal (plus GL as a fallback). Free, no SDK installs. |
| Editor UI | **egui + egui_dock** | Immediate-mode, native, small. The viewport is a wgpu texture inside the UI, so the editor and game share one renderer. Idle cost is near zero: egui only repaints on input (or while a game runs). |
| Math | glam | The usual for Rust games. |
| Files | RON | Readable, diff-friendly text for scenes and projects. |
| Valve formats | own readers (VPK, VTF, VMT) | Small, no dependencies, tested against synthetic files. `vbsp`/`vmdl` crates are lined up for maps and models. |

Considered and passed over: **Bevy** (its ECS fights a Godot-style node tree and a single-threaded
interpreter, and its editor story isn't there yet); **Tauri/web UI** for the editor (heavier at
runtime, and the 3D viewport would need a second renderer or a WebGPU bridge); **Godot as a
base** (its own scripting and scene formats would compete with JLB everywhere).

## Data flow

```
 .jscene files ──scene::load──▶ SceneTree ◀── inspector, hierarchy, viewport edits (undo = tree snapshots)
                                    │
                    Play: clone ────┤
                                    ▼
                    game thread: Game { SceneTree + Interp + one Env per script }
                         ▲   input each frame          │ RenderList + tree copy + log lines
                         └──────── editor ◀────────────┘
                                    │
                    render::extract(tree) ─▶ RenderList ─▶ gex-render ─▶ texture ─▶ egui viewport
```

- **One building block.** `Node { name, kind, transform, props, scripts, groups, children }`.
  The kind decides what the engine does with it; everything else is the same for all nodes.
  Properties are `Variant`s, which the inspector, scene files and scripts all share.
- **Scenes are trees, instances are references.** `scene::build` expands an instance by
  loading its file; the instance root remembers `instance_of`, and nodes inside are marked
  `from_instance`. `scene::describe` saves only what differs from the instanced file.
- **The game runs on its own thread.** The interpreter is single-threaded (`Rc`), so a game
  owns its own `Interp` and tree on a thread with a large stack. A stuck script never freezes
  the editor: Stop sets a cancel flag the interpreter checks between statements.
- **The renderer only sees a `RenderList`**: sprites, labels, meshes, lights, colliders,
  cameras. Text is drawn by egui on top. The same path serves the editor's view, play mode and
  the standalone player.

## Scripting bridge

`third_party/jlb` is jlb-rust plus a small embedding layer (`src/embed.rs`, `src/host.rs`,
`src/lib.rs`), kept as a patch (`third_party/jlb-rust-embed.patch`) so it can go upstream:

- `host::register(names)` lets a host add ALL-CAPS commands the lexer accepts.
- `Interp::host` receives those calls; it may also take over a built-in (the engine answers
  `KEY_DOWN` from its own input), falling back to the built-in otherwise.
- `Interp::load_module(path, preset)` runs a file in a fresh set of outside variables;
  `call_value` calls a function in it; `set_output` redirects PRINT; `cancel` stops code.

Each script attached to a node is loaded once per node, so its outside variables are that node's
instance variables. Before each event the engine writes the node into a `self` table; after it,
whatever the script changed in `self` is written back (custom keys become node properties and
show in the inspector). Engine commands that change other nodes (`SET`, `SPAWN`, ...) work on the
tree directly; anything that runs more script code (`SEND`, `EMIT`, `DESTROY`) is queued and run
right after, so scripts never re-enter each other.

## What's next

1. **Physics**: rapier2d/3d behind `Body2D`/`Body3D` nodes; collisions feed the same events.
2. **Audio**: a `Sound` node and `PLAY_SOUND`, reading .wav/.ogg/.mp3 from any mount (HL2's included).
3. **Source models and maps**: `.mdl` via `vmdl` (mesh, skeleton, animations), `.bsp` via `vbsp`
   as a scene you can instance; materials through the existing VMT path.
4. **Editor**: Vim-style modal editing in the script editor, completion of engine commands,
   breakpoints through jlb-rust's debugger, gizmos for rotate/scale, scene tabs.
5. **Export**: a standalone game folder (player + `res://` files only; mounted files are never
   included).
