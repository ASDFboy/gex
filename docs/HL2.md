# Half-Life 2 assets

The engine can use the files of your own Steam install of Half-Life 2 (with Episode One and
Two, which ship inside the same install folder) as an asset library at `hl2://`.

## Setting it up

1. **File → Project settings & mounts…** (or **⚙ Mounts** in the Assets panel).
2. Tick **Mount Half-Life 2 at hl2://**, then **Detect** (reads Steam's library list) or type
   the folder, e.g. `C:\Program Files (x86)\Steam\steamapps\common\Half-Life 2`.
3. **Game folders** sets the search order; the default `ep2, episodic, hl2` means Episode Two's
   version of a file wins over Episode One's, which wins over Half-Life 2's.
4. **Apply.** Browse it under Assets → `hl2://`.

The setting is saved in the project's `project.jproj`:

```
mounts: [(scheme: "hl2", kind: SourceGame(install: "C:\\...\\Half-Life 2", games: ["ep2", "episodic", "hl2"]))]
```

## What can be used

| Files | How |
|---|---|
| `.vpk` archives and loose files | read in place; browse them in Assets → `hl2://` |
| `.vtf` textures | DXT1/3/5 and the 8-bit RGB(A)/BGR(A)/I8/IA88 formats; drop one on a texture field |
| `.vmt` materials | resolved through `$basetexture`; use them anywhere a texture goes |
| `.wav` / `.mp3` sounds | `PLAY_SOUND("hl2://sound/weapons/crowbar/crowbar_impact1.wav")`, `MUSIC(...)`, or a Sound node |
| `.mdl` models | set a Mesh3D's `mesh` to the `.mdl` (or drag it into the 3D view); textures come from its materials, and `ANIMATIONS(node)` / `ANIMATE(node, name)` play its sequences |
| `.bsp` maps | a Mesh3D with `mesh` = `hl2://maps/d1_trainstation_01.bsp` draws the map's brushes, displacements and static props; turn on its `collision` to walk on it |

## Walking around a map

1. Make a 3D scene. Add a Mesh3D, set `mesh` to a map (browse `hl2://maps/`), and tick
   `collision`.
2. Source maps are measured in inches; GEX converts them to metres, so the first-person player
   from the demo (`scenes/fps.jscene`, `scripts/fps.jlb`) fits. Copy its Player node (Ctrl+C,
   Ctrl+V into your scene) and put it somewhere above the floor.
3. Add a Light3D (directional) and an Environment, then press F5.

Big maps take a few seconds to load the first time; the model cache keeps them after that.

The Source readers are tested against small files built in the tests, not against every file
Valve shipped. If a model, map or texture looks wrong or won't load, the Console says which file
and why.

## The rules it follows

- **Nothing is copied.** Files are read from the install folder each time; projects store only
  paths like `hl2://materials/brick/brickwall001a.vmt`.
- **Nothing leaves your machine.** `Vfs::redistributable()` is false for every mounted path, and
  File → Export game leaves them out (and warns about scripts that mention them). A game that uses `hl2://` assets runs only
  where Half-Life 2 is installed.
- This is for your own, non-commercial use of a game you own. Valve's licence allows using
  Half-Life 2 content in Source-engine mods; using it in another engine, even privately, is a
  grey area, and sharing or selling a game built on it is not allowed. Keep `hl2://` projects
  to yourself, and use your own `res://` assets in anything you publish.
