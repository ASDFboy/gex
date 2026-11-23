**Windows:** download `GEX.zip`, unzip it and run `GEX.exe`. It's one file with no installer; the docs and templates are built in. GEX updates itself when it starts (Settings → Updates).

**Exporting for Linux:** put `gex-linux` in a `players` folder next to `GEX.exe`.

New in 0.5.5:
- No more pop-in or pauses the first time a menu opens: when a game starts, every picture in the project loads in the background (as many as fit in about 768 MB of video memory) and its sprite shaders are built on another thread, so they're ready before they're needed. Pictures on screen still go first, and preloads wait while plenty is already waiting to upload, so memory doesn't pile up.
- A picture something on screen is waiting for now jumps ahead of preloads and uploads faster.

New in 0.5.4:
- No more soft edges by default. With scale_mode fit, the game is drawn at the size it's shown instead of being drawn small and stretched, so sprites and text stay sharp in a big or full-screen window. scale_filter is now pixel and antialiasing (FXAA) is off unless a project turns them on.

New in 0.5.3:
- Open scripts and blueprints changed by another program (another editor, files copied in) are noticed: read again when they have no unsaved changes, and never written over with the old copy. With unsaved changes, yours are kept and the other version goes to .trash.

New in 0.5.2:
- Text (Label2D, DRAW_TEXT) is drawn pixel for pixel at the size it's shown, as sharp as text in a browser: it used to be made at a nearby size and stretched, which blurred it.

New in 0.5.1:
- Label2D, DRAW_TEXT and TEXT_SIZE can use a font installed on the computer by its name (font: "Times New Roman"), with look-alikes where it's missing (Liberation Serif, Arimo...).

New in 0.5.0:
Drawing
- on_draw_gui(), and screen: TRUE on any DRAW_*: drawing in the window's pixels over everything, whatever the camera does (GameMaker's Draw GUI). MOUSE_SCREEN_X/Y, TO_SCREEN and TO_WORLD go between the two.
- Canvas2D: a picture placed in the blueprint that scripts draw into (DRAW_* with target:), keeping what's drawn until CLEAR_CANVAS, like Scratch's pen. SURFACE_CREATE / SURFACE_FREE make ones in scripts, like GameMaker's surfaces.
- Full-screen shaders: the graphics setting post_shader, or POST_SHADER(file, values) from scripts (fn post(color, uv, time, values), post_texture(uv) to read the picture anywhere).
- SET_CURSOR: a picture as the mouse pointer, or the pointer hidden.
- Pictures can be made into sprites in Assets (Make a sprite…): frames, origin, speed and mouse mask kept with the picture, used by every Sprite2D and DRAW_SPRITE showing it, playing by itself.
- Text colors take short hex too ([color=#f80]).
Scripting
- SPAWN runs the new object's on_create right away, so the next line can use what it set up.
- Persistent objects (a tick in the Inspector) come along into the next blueprint with their variables, like GameMaker's.
- OBJECTS_AT and OBJECTS_IN find objects by place without physics; TEXT_SIZE measures text exactly as it's drawn; HAS_GLOBAL and GLOBAL_OR; READ_FILE and READ_JSON read the project's data files.
- Clicking labels (on_clicked, POINT_ON, picking them in the editor) uses the text's real size.
Editor and engine
- The Profiler shows every event's time by script over the last second, and the slowest frames.
- Find in files: a blueprint result selects the object it's about.
- Pictures loading in the editor show as a faint box; pictures not drawn for a while are let go when video memory runs short; text that changes every frame no longer piles up.
- Each computer's linked folders (Half-Life 2, asset folders) are kept on that computer (.gex/mounts.ron), not in the shared project file.
- A test build stopped from the editor closes properly (its saves are written).
- A picture changed while the game runs is measured again (sizes, click masks).

New in 0.4.4 (checking today's changes together):
- A short decimal read back from a property or a save is exact again: SET(obj, "speed", 0.3) then GET, or SAVE_DATA then LOAD_DATA, gives 0.3, so `== 0.3` holds (it used to come back as a near miss).
- Clicking a Label2D (on_clicked, POINT_ON, and picking it in the editor) uses its real block: [color=...] tags take no room and wrap_width wraps it. The editor picks labels and sprites by layer, as they're drawn.
- A new test runs objects being made and removed, a script reloading with a new event, SET, saves and a blueprint change together, so the lookups GEX now keeps between frames are checked against all of them.

New in 0.4.3:
- Saving writes only the files whose contents changed. A blueprint, script or project settings that weren't changed are left exactly as they are, even when they're laid out differently from how GEX writes them, so a project synced with GitHub (or shared with Work together) sends only what you really changed, and saving can't put an old copy back over a file another computer changed. Files are replaced in one step, so a sync never picks up a half-written file.

New in 0.4.2 (speed):
- Pictures load in the background: a big one showing for the first time no longer freezes the game (it used to stall a frame for up to a tenth of a second or more). Pictures placed in a blueprint load as it starts; PRELOAD(files or a folder) loads others early.
- Scripts' arithmetic is 2-3 times faster: fractions like 2.5 or 1/3 no longer use the big-number code (results are exactly the same).
- Events cost far less per object (1000 objects with on_step: about 3x faster), and events most objects don't have cost nothing.
- Drawing thousands of sprites, tile maps and particles takes about half the time; off-screen sprites are skipped.
- SAVE_DATA writes in the background; a built game no longer copies its whole object tree a few times a second.
- Moving to another blueprint reuses scripts already read.
- The selected tab is drawn in front of the tabs next to it (their slanted edges used to cover it).

New in 0.4.1 (what remaking a Scratch game and a GameMaker game needed):
- Label2D text is drawn in layer order with sprites (under or between them), turns and scales with its object, and has align, valign, wrap_width and an outline.
- Sprite2D: pivot_x / pivot_y (the point it sits at and turns around), color effects (brightness, hue, saturation, invert) and custom sprite shaders (a .wgsl `fn sprite(...)`, with value_1–value_4 from scripts); GameMaker shaders port line for line.
- DRAW_SPRITE(file, x, y, [options]): GameMaker's draw_sprite_ext and more (frames, regions, scale, rotation, pivot, tint, alpha, layer, blend, effects, shaders). Every DRAW_ command takes an options table, including layer.
- Mouse events on 2D objects: on_mouse_enter, on_mouse_leave, on_clicked, on_click_held, on_click_released (mouse_precise counts only visible pixels); POINT_ON and IMAGE_SIZE.
- SPAWN takes starting values: SPAWN(file, x, y, [hp: 5]) sets the new object's variables before its on_create.
- GLOBAL variables last across CHANGE_BLUEPRINT, as the docs always said; `LOCAL d @= GLOBAL.save` works; events written in an IMPORTed file run.
- Particles2D: animated textures (sprite sheets, over each particle's life or at a rate, from a random frame), a starting angle (start_rotation), facing the way they fly, three colors, acceleration, damping, gravity direction, wiggle, a spawn rectangle, random size and lifetime, non-square particles.

New in 0.4.0 (GitHub for the whole of GEX):
- GEX settings (Settings on the start screen, File → GEX settings in a project): sign in to GitHub once, for every project; and the update settings, which used to be in the View menu.
- New projects can be kept on GitHub: tick "Keep it on GitHub" (on by default once signed in; changeable in GEX settings). An existing project: Project settings → GitHub. A project kept on GitHub stays linked and saves to and loads from it by itself whenever it's open.
- Projects on your GitHub that aren't on this computer yet are listed on the start screen with your own; one click gets and opens them. The separate "From GitHub" window is gone.
- New projects start empty: the sample 2D game, 3D game and demo games are gone.

New in 0.3.9 (polish):
- Tabs: the one showing is taller and darker, the others shorter and lighter. The 2D/3D buttons are one switch (blueprints with only 2D things in them stay in 2D).
- Arrows (→) and other symbols show everywhere instead of empty boxes.
- Start screen: projects listed by when they were last worked on.
- Inspector: property names in words ("Border color"; scripts' names in the tooltip), numbers without stray decimals, rotation as "45°", colors with an edge and their hex value. Selected UI objects are outlined. The object list gets more room.
- Rename objects with F2 or a double-click; Cut, Copy, Paste and Select all in the Edit menu and the right-click menu, with their shortcuts. Shortcuts (Delete, F2, Ctrl+C...) work right after clicking in the object list.
- File → Open project (recent ones, or a folder); closing or switching projects saves first.
- Right-clicking an object selects it. The Timeline and Profiler open in different places and stay off the Commands page; windows close with their project.
- Assets: picture names under their thumbnails, a play button on sounds, "+ Blueprint / + Script / + Folder / More".
- Project settings: one Save and a Cancel that puts everything back; Updates moved out of it.
- Work together: a Browse button for where to put the copy (Windows).
- While playing, the editing tools step aside, and the view says which blueprint the game is in when it has moved on.

New in 0.3.8 (Sync with GitHub, gone over):
- Works with Work together: while you share a linked project, changes from GitHub wait until what others are typing is saved; a joined copy never syncs on its own.
- Line endings are kept exactly as GEX writes them (no Windows CRLF conversion), and file names of any kind come through.
- A sign-in that's needed shows as "Sync: sign in" with a button, instead of windows popping up in the background; turned-down sign-ins are forgotten so Git asks again.
- Offline, sign-in and deleted-repository problems are told apart; after one, it tries again soon.
- If both computers send at once, it brings in the other's changes and sends again instead of failing.
- Closing a project sends its last changes in the background (no freeze); quitting GEX waits briefly only when there's something to send.
- "Syncing…" only shows for real syncs (no flicker every 20 seconds); Git is looked for once, not every frame.
- From GitHub opens the copy already on this computer instead of making a second one; a failed download leaves nothing behind.
- Stopping and linking again reuses the same repository; conflict copies keep their file endings ("player (other computer).jlb").

New in 0.3.7:
- Sync with GitHub (File menu): keeps a project the same on all your computers through a private repository on your GitHub account. What you save goes up on its own; what you did elsewhere comes in when the project opens, every two minutes and when it closes, and open scripts and blueprints update in place. From GitHub on the start screen gets a synced project onto another computer. Conflicting changes keep the newer version and put the other aside; each computer keeps its own Half-Life 2 location. Uses Git (GEX can install it on Windows); the GitHub sign-in stays with Git.

New in 0.3.6:
- Timer object: counts down from wait_time and runs on_timeout(name) on itself and its parent (and on_timeout_<name>() everywhere); repeats or runs once; PLAY / STOP start it over or stop it.
- UI layout: Row, Column and Grid place the UI under them (sized to fit, so anchors center whole groups); ScrollBox stacks UI in a box you scroll with the wheel or its bar, clipping what's outside.
- Checkbox (on_toggled) and Slider (on_value_changed, with min, max and step).
- Selected UI objects are outlined in the editor (they used to be hidden behind the UI), and layout objects show a faint outline.

New in 0.3.5:
- Updating no longer keeps old copies of GEX: the replaced version (and any kept by earlier updates) is removed when the new one starts. The "Go back to…" button is gone with it.

New in 0.3.4:
- REX, the assistant panel, is gone from the editor.

New in 0.3.3 (Work together, gone over end to end):
- Joining never copies over another project with the same name; joining again updates your earlier copy (files removed by the sharer go, your new files are shared, and files you changed are kept aside in "Name (your changes)" before being updated).
- When the sharer stops (or closes GEX), everyone's told at once instead of trying to reconnect for a minute.
- Play together: people who join mid-game join the game, and when the sharer stops their game everyone's stops.
- Scripts and blueprints changed on the sharer's computer by another program reach everyone.
- Mounted-folder files that change are sent again; the window shows files still on their way; textures reload once per batch instead of per file.
- Security: others' project settings can't mount folders on your computer; paths can't leave a mount (drive letters, backslashes); connections that never prove the code are dropped after 20 s, and only a few may try at once; incoming messages only take memory as their data arrives.
- Faster with big blueprints: your blueprint is compared with the shared one often only while you're working on it.
- "Stop sharing" asks first while others are there; joining saves the project you had open.

New in 0.3.2:
- Working together shares mounted assets: files from Half-Life 2 or mounted folders that the project uses reach everyone (only what's used, never all of Half-Life 2), fetched as soon as anyone's GEX needs one. Each computer keeps its own mounts.
- Images, models and sounds changed by others show the new version at once.

New in 0.3.1:
- Work together and multiplayer games use ZeroTier: the ZeroTier address is listed first (each address has its own Copy invite), MY_ADDRESS() gives it, and Work together can allow GEX through the Windows Firewall on every network type (ZeroTier networks count as Public).

New in 0.3.0:
- Work together: invite codes and encrypted sessions; blueprints merge property by property and objects keep who they are through renames; chat, follow someone, play together; view-only and remove for the host; reconnects by itself, keeping changes made meanwhile; a progress bar when joining.
- Updates: checked against the release's SHA-256 list; install by itself, ask first, or don't check; checks every 6 hours; download progress.

Website: https://asdfboy.github.io/gex/
