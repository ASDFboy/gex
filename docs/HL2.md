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

## What can be used now

| Files | Status |
|---|---|
| `.vpk` archives and loose files | read in place, browsable |
| `.vtf` textures | DXT1/3/5 and the 8-bit RGB(A)/BGR(A)/I8/IA88 formats; first frame, full-size mip |
| `.vmt` materials | resolved through `$basetexture`; drop one on a Sprite2D or Mesh3D texture field |
| `.wav`/`.mp3` sounds | listed; playback comes with the audio milestone |
| `.mdl` models, `.bsp` maps | listed; loading comes in a later milestone (via the `vmdl` / `vbsp` crates) |

## The rules it follows

- **Nothing is copied.** Files are read from the install folder each time; projects store only
  paths like `hl2://materials/brick/brickwall001a.vmt`.
- **Nothing leaves your machine.** `Vfs::redistributable()` is false for every mounted path, and
  the future export tool will refuse to package them. A game that uses `hl2://` assets runs only
  where Half-Life 2 is installed.
- This is for your own, non-commercial use of a game you own. Valve's licence allows using
  Half-Life 2 content in Source-engine mods; using it in another engine, even privately, is a
  grey area, and sharing or selling a game built on it is not allowed. Keep `hl2://` projects
  to yourself, and use your own `res://` assets in anything you publish.
