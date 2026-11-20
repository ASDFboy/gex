**Windows:** download `GEX.zip`, unzip it and run `GEX.exe`. It's one file with no installer; the docs, templates and a demo game are built in. GEX updates itself when it starts (View → Updates).

**Exporting for Linux:** put `gex-linux` in a `players` folder next to `GEX.exe`.

New in 0.3.2:
- Working together shares mounted assets: files from Half-Life 2 or mounted folders that the project uses reach everyone (only what's used, never all of Half-Life 2), fetched as soon as anyone's GEX needs one. Each computer keeps its own mounts.
- Images, models and sounds changed by others show the new version at once.

New in 0.3.1:
- Work together and multiplayer games use ZeroTier: the ZeroTier address is listed first (each address has its own Copy invite), MY_ADDRESS() gives it, and Work together can allow GEX through the Windows Firewall on every network type (ZeroTier networks count as Public).

New in 0.3.0:
- Work together: invite codes and encrypted sessions; blueprints merge property by property and objects keep who they are through renames; chat, follow someone, play together; view-only and remove for the host; reconnects by itself, keeping changes made meanwhile; a progress bar when joining.
- Updates: checked against the release's SHA-256 list; install by itself, ask first, or don't check; go back to the version before; checks every 6 hours; download progress.

Website: https://asdfboy.github.io/gex/
