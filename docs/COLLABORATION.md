# Working on a project together

Several people can work on one GEX project at the same time, each on their own computer, and
see each other's changes as they happen.

## Starting

**The one sharing:** open the project, then File → Work together → **Start sharing**. The window
shows the **address** (like `192.168.1.20:7778`) and an **invite code** (like `7K2P-9QXM-4HDR-T6WN`).
**Copy invite** copies both; send them to the people you want.

**Everyone else:** on the project library, **Join someone…** (or File → Work together in the
editor). Paste the invite into Address (it fills in the code too), and **Join**. The project is
copied into a folder named after it, inside the folder shown (the one new projects go in); files
already there that match aren't sent again, so joining the same project later is quick. A bar
shows the copy's progress, and the project opens when it's all there.

On the same network (home Wi-Fi, a LAN) that's all. Over the internet the person sharing
forwards the port (TCP 7778) on their router to their computer, or everyone joins the same free
virtual network (ZeroTier, Tailscale) and uses its addresses. Windows asks whether GEX may use
the network the first time: allow it.

## What stays in step

- **Blueprints, property by property.** Moving, adding, renaming, re-parenting, deleting or
  changing an object shows up for everyone within a tenth of a second. Two people changing
  different properties of the same object (one moves it, one recolors it) both keep their
  change; only when two change the same property at the same moment does the last one win.
  Renamed and moved objects stay the same objects, so nobody loses their selection. Undo takes
  back your own changes, not the others' (for your last 64 steps).
- **Scripts, letter by letter.** Two people can type in the same script at once, even on the
  same line: each one's typing moves past the other's.
- **Files.** Saving, importing, renaming or deleting a file does the same on everyone's copy
  (within two seconds). Project settings follow when they're saved. Files over 256 MB, hidden
  folders (like the editor's own `.gex`) and `target` folders stay on each computer.

Unsaved changes are shared too; saving writes them to everyone's disk.

## Seeing each other

Everyone has a color, and is a colored square with their initial at the top right: hover it to
see what they're doing, click it for the Work together window.

- A tab someone else is in has their colored mark.
- In the blueprint view their mouse pointer is an arrow with their name (in 3D, on whatever
  they point at), and what they've selected has a box in their color.
- In a script, where they're typing is a bar in their color with their name, and their line is
  tinted.
- **Follow** (in the Work together window) takes you where someone is: their blueprint, their
  view, and the script they're typing in. Moving your view stops following.
- **Chat** at the bottom of the Work together window. New messages show as "1 new" at the top
  right.

## Playing together

The one sharing can press **Play together**: their game starts and hosts a multiplayer game,
and everyone else's GEX starts playing the same blueprint and joins it. Scripts see it like any
multiplayer game (`IS_HOST()`, `MY_ID()`, `on_player_joined`...; see
[Multiplayer](SCRIPTING.md#multiplayer)), and their own `HOST_GAME` / `JOIN_GAME` calls are
skipped. Each person stops their own game.

## Who can do what

The one sharing sees everyone in the Work together window:

- **Can edit** (on by default): untick it to make someone view-only. They still see everything
  live, but their changes are put back and not shared.
- **Remove** takes someone out; their computer can't come back in for the rest of the session.

## If the connection drops

A joined GEX tries to get back in by itself for a minute ("reconnecting…" at the top right).
Changes made meanwhile are kept and shared once it's back. After that, the session ends and
your copy of the project stays as it is.

## Security

- Only someone with the invite code can join. Each sharing gets a new, random code (80 bits:
  it can't be guessed), and a computer that gets it wrong five times is shut out for the session.
- After the first hello, everything (project files, edits, chat) is encrypted and checked
  (ChaCha20-Poly1305, with keys made from the code and fresh random numbers on both sides), so
  nobody else on the network can read it or slip changes in, and nothing can be replayed.
- Paths sent by others are checked to stay inside the project, so a joined GEX only ever writes
  in the project's folder.
- Anyone you let edit can change every file in the project, scripts included, and scripts run
  when you play. Share the code only with people you trust, and make others view-only.

## Ending

**Leave** (or **Stop sharing**) in the Work together window. When the one sharing stops, the
others are told and keep working on their own copy. Closing GEX also ends it. Everyone needs
the same version of GEX (it updates itself when it starts).
