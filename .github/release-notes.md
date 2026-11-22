**Windows:** download `GEX.zip`, unzip it and run `GEX.exe`. It's one file with no installer; the docs, templates and a demo game are built in. GEX updates itself when it starts (View → Updates).

**Exporting for Linux:** put `gex-linux` in a `players` folder next to `GEX.exe`.

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
