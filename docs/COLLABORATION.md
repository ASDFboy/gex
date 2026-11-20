# Working on a project together

Several people can work on one GEX project at the same time, each on their own computer, and
see each other's changes as they happen.

## Starting

**The one sharing:** open the project, then File → Work together → **Start sharing**. The window
shows the **address** (like `192.168.1.20:7778`) and an **invite code** (like `7K2P-9QXM-4HDR-T6WN`).
**Copy invite** copies both; send them to the people you want.

**Everyone else:** on the project library, **Join someone…** (or File → Work together in the
editor). Paste the invite into Address (it fills in the code too), and **Join**. The project is
copied into a folder named after it, inside the folder shown (the one new projects go in). A bar
shows the copy's progress, and the project opens when it's all there (the project open until then
is saved first).

- **Never over another project:** if a different project of yours already has that name, the copy
  goes beside it ("Name (joined)"). The same happens when joining from the computer that shares.
- **Joining the same project again** updates the copy you made before: only what changed comes
  over, files the sharer removed are removed, and files you made in the copy meanwhile are
  shared. Files you *changed* in the copy meanwhile are replaced by the sharer's, but yours are
  kept beside the project in "Name (your changes)", and the chat says so.

The window lists every address of the computer sharing, labelled by network, with the
**ZeroTier** one first: friends on your ZeroTier network use that one (each address has its own
**Copy invite**), friends on the same Wi-Fi the LAN one. Joined ZeroTier after sharing started?
**Look again** finds it. Without ZeroTier (or Tailscale, Hamachi), the person sharing forwards
the port (TCP 7778) on their router.

**Windows Firewall:** Windows counts ZeroTier, Tailscale and Hamachi networks as Public and blocks
programs there unless they're allowed (its first-time prompt only ticks Private). The Work
together window says whether GEX is allowed; **Allow GEX through the firewall** fixes it on every
network type after one Windows admin prompt, and removes any block Windows added if its prompt
was cancelled. Only the person sharing needs it (and whoever hosts when playing together).

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

Unsaved changes are shared too; saving writes them to everyone's disk. Scripts and blueprints
changed on the sharer's computer by another program (or a `git pull`) reach everyone who has
them open.

### Assets from outside the project (Half-Life 2, mounted folders)

Files a project uses from a mount (`hl2://...`, or a folder of art mounted as `art://...`) live
on the computer that mounted them, not in the project, so they don't come with the copy. GEX
fetches them instead, so everyone sees and hears the same thing:

- The person sharing offers every mounted file the project names (in blueprints, scripts,
  animations, settings, unsaved changes included); each computer takes what it doesn't have.
- Anything else a computer tries to read and doesn't have (a model's textures, a map's
  materials, a sound whose name a script builds) is fetched the moment it's missed, and shows up
  a moment later.
- **Half-Life 2 is never sent whole**: only the files the project actually uses, and the ones
  those need. Someone who has Half-Life 2 themselves (mounted under Settings → Half-Life 2) uses
  their own copy and fetches nothing.
- Files can come from anyone in the session, not only the one sharing: if a joiner has a folder
  mounted that others don't, its files reach them through the host.
- Files in a mounted folder that change (a picture redrawn) are offered again, within a few
  seconds.
- While files are on their way, the Work together window says how many; textures and models
  show as they arrive.
- Fetched files are kept in the project's hidden `.gex/remote` folder, so joining again is
  quick; they're never shared on or put into exported games.
- Each computer keeps its own mounts in the project settings: someone else's Half-Life 2
  folder doesn't replace yours (or get set on a computer that doesn't have it).

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
skipped. Anyone who joins the session while the game runs joins the game too, and a game someone
was playing on their own gives way to it. Each person can stop their own game; when the one
sharing stops theirs, everyone's stops.

## Who can do what

The one sharing sees everyone in the Work together window:

- **Can edit** (on by default): untick it to make someone view-only. They still see everything
  live, but their changes are put back and not shared.
- **Remove** takes someone out; their computer can't come back in for the rest of the session.

## If the connection drops

A joined GEX tries to get back in by itself for a minute ("reconnecting…" at the top right).
Changes made meanwhile in open blueprints and scripts are kept and shared once it's back
(files saved meanwhile that the sharer changed too are kept aside, as when joining again). After
a minute, the session ends and your copy of the project stays as it is.

## Security

- Only someone with the invite code can join. Each sharing gets a new, random code (80 bits:
  it can't be guessed), and a computer that gets it wrong five times is shut out for the session.
- After the first hello, everything (project files, edits, chat) is encrypted and checked
  (ChaCha20-Poly1305, with keys made from the code and fresh random numbers on both sides), so
  nobody else on the network can read it or slip changes in, and nothing can be replayed.
- Paths sent by others are checked to stay inside the project, so a joined GEX only ever writes
  in the project's folder; files from mounts are only read from inside the mount.
- Others never get to read a folder on your computer they name: their project settings can't
  mount folders here (only Half-Life 2, where you have it at the same place).
- Connections that don't prove the code within 20 seconds are dropped, and only a few may try
  at once.
- Anyone you let edit can change every file in the project, scripts included, and scripts run
  when you play. Share the code only with people you trust, and make others view-only.

## Ending

**Leave** (or **Stop sharing**, which asks first while others are there) in the Work together
window. When the one sharing stops, the others are told and keep working on their own copy. Closing GEX also ends it. Everyone needs
the same version of GEX (it updates itself when it starts).
