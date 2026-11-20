# Working on a project together

Several people can work on one GEX project at the same time, each on their own computer, and
see each other's changes as they happen.

## Starting

**The one sharing:** open the project, then File → Work together → **Start sharing**. The window
shows the address the others type in (like `192.168.1.20:7778`) with a Copy button.

**Everyone else:** on the project library, **Join someone…** (or File → Work together in the
editor). Type the address and **Join**. The project is copied into a folder named after it,
inside the folder shown (the one new projects go in); files already there that match aren't
sent again, so joining the same project later is quick. When everything has arrived the project
opens.

On the same network (home Wi-Fi, a LAN) that's all. Over the internet the person sharing
forwards the port (TCP 7778) on their router to their computer, or everyone joins the same free
virtual network (ZeroTier, Tailscale) and uses its addresses. Windows asks whether GEX may use
the network the first time: allow it.

## What stays in step

- **Blueprints, object by object.** Moving, adding, renaming, deleting or changing an object
  shows up for everyone within a tenth of a second. Two people changing different objects both
  keep their changes; when two change the same object at the same moment, the last one wins.
  Undo takes back your own changes, not the others'.
- **Scripts, letter by letter.** Two people can type in the same script at once, even on the
  same line: each one's typing moves past the other's.
- **Files.** Saving, importing, renaming or deleting a file does the same on everyone's copy
  (within two seconds). Project settings follow too. Files over 256 MB, hidden folders (like the
  editor's own `.gex`) and `target` folders stay on each computer.

Unsaved changes are shared too; saving writes them to everyone's disk.

## Seeing each other

Everyone has a color (and is a colored square with their initial at the top right; hover it to
see what they're doing, click it for the Work together window).

- A tab someone else is in has their colored mark.
- In the blueprint view their mouse pointer is an arrow with their name, and what they've
  selected has a box in their color (in 2D the object's outline; in 3D a square at its middle).
- In a script, where they're typing is a bar in their color with their name, and their line is
  tinted.

## Ending

**Leave** (or **Stop sharing**) in the Work together window. When the one sharing stops, the
others get a message and keep working on their own copy. Closing GEX also ends it.

## Good to know

- Everyone needs the same version of GEX (it updates itself when it starts).
- Playing the game is each person's own: pressing Play doesn't start it for the others.
- Your editor layout (open tabs, windows) is your own.
- Anyone you let join can change every file in the project, including scripts, so only share
  with people you trust.
