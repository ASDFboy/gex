//! The full guide to the engine commands, for the editor's Command reference (View →
//! Commands, or F1): which group each one is in, how to use it, and an example. The short
//! form (inputs and a one-line summary) is in `commands::COMMANDS`; a test keeps the two in step
//! and checks that every example is valid .jlb.

/// (group, intro): the order groups appear in.
pub const GROUPS: &[(&str, &str)] = &[
    ("Objects", "Finding objects, reading and changing their properties, making and removing them."),
    ("Events and timers", "Calling events on other objects, doing things later, sliding values over time."),
    ("Physics", "Moving bodies, pushing them, and asking what's where."),
    ("Input", "Keys, mouse, gamepad."),
    ("Time and game flow", "Time, switching blueprints, pausing, the window, the camera."),
    ("Sound", "Sound effects, Sound objects and music."),
    ("Animation", "Sprite and model animations, Timeline clips, ragdolls, particles."),
    ("Tiles", "Reading and changing TileMap2D tiles, and finding paths through them."),
    ("Drawing", "Shapes, text and pictures drawn for one frame (call them from on_draw, or on_draw_gui for the screen), canvases and surfaces they draw into, and full-screen shaders."),
    ("Saving and remembering", "Keeping values between blueprints, and on disk between runs."),
    ("Settings", "Changing project display, graphics, physics and input settings while the game runs."),
    ("Maps", "Half-Life 2 maps (.bsp) shown by a Mesh3D: their entities and doors."),
    ("Multiplayer", "Hosting and joining games over the network, shared objects, and events between players' games."),
];

/// (name, group, how to use it, example).
pub const GUIDE: &[(&str, &str, &str, &str)] = &[
    // ---------------------------------------------------------------- objects
    ("OBJECT", "Objects",
     "Finds an object and gives its id (a number other commands take), or NULL when there's none.\n\n\
      The path is relative to the object running the script: \"Gun\" is a child, \"../Enemy\" a sibling, \"/Level/Door\" starts at the top of the blueprint. A bare name that isn't a child (\"Player\") is looked for in the whole blueprint, so you don't need the full path.\n\n\
      Most commands take a path text directly, so OBJECT is mainly for checking whether something exists or keeping its id.",
     "player = OBJECT(\"Player\")\nIF player !== NULL {\n\tPRINT(\"found it: \" + NAME_OF(player))\n}"),
    ("GET", "Objects",
     "Reads one property of another object: GET(object, \"property\").\n\n\
      object is an id, a path or a bare name (see OBJECT); NULL means this object. The property is any name you see in Properties (x, y, rotation, width, color, visible, opacity...) or one of your custom properties. Gives NULL when the object doesn't have it.\n\n\
      For this object, just use the name (speed) or self.speed.",
     "hp = GET(\"Player\", \"hp\")\npx = GET(\"Player\", \"x\")"),
    ("SET", "Objects",
     "Changes one property of another object: SET(object, \"property\", value).\n\n\
      The value takes the property's shape: a number for x, a color for color (a table [r:, g:, b:, a:] or a text like \"#ff8800\"), TRUE/FALSE for switches. Setting a name the object doesn't have makes a new custom property.\n\n\
      The change happens at once; physics bodies moved this way are teleported.",
     "SET(\"Door\", \"visible\", FALSE)\nSET(\"Hud/Score\", \"text\", \"Score: \" + GLOBAL.score)\nSET(\"Lamp\", \"color\", \"#ffd080\")"),
    ("OBJECTS", "Objects",
     "Every object in a group, as a table of ids (empty when none are).\n\n\
      Give objects groups in Properties → Groups (\"enemies\", \"coins\"). Loop over the table with FOR.",
     "FOR e IN OBJECTS(\"enemies\") {\n\tSET(e, \"speed\", 0)\n}\nleft = LEN(OBJECTS(\"coins\"))"),
    ("CHILDREN", "Objects",
     "An object's children, in order, as a table of ids. CHILDREN() or CHILDREN(NULL) gives this object's own.",
     "FOR c IN CHILDREN() {\n\tSET(c, \"visible\", TRUE)\n}"),
    ("PARENT", "Objects",
     "The id of the object's parent, or NULL for the top of the blueprint. PARENT() gives this object's parent.",
     "SEND(PARENT(), \"child_hit\", self.id)"),
    ("NAME_OF", "Objects",
     "An object's name as text. Handy in collision events, where you're given an id.",
     "DEF on_collide_start(other) {\n\tIF NAME_OF(other) == \"Spikes\" { DESTROY() }\n}"),
    ("PATH_OF", "Objects",
     "An object's full path from the top of the blueprint, like \"/Level/Enemies/Bat\". Mostly for PRINT when finding problems.",
     "PRINT(\"hit \" + PATH_OF(other))"),
    ("SPAWN", "Objects",
     "Makes a new instance of a blueprint file while the game runs and gives its id: SPAWN(\"res://bullet.jblueprint\", x, y, z, parent).\n\n\
      x, y, z place it (missing ones keep the blueprint's own position). parent is where it goes in the tree; leave it out (or NULL) to put it at the top of the blueprint, where its position is a world position.\n\n\
      Its scripts are loaded and its on_create() runs right away, before SPAWN gives back the id, so the next line can use what on_create set up (GET(id, ...), SEND).\n\n\
      Starting values: put a table last, SPAWN(\"res://bullet.jblueprint\", x, y, [speed: 600, owner: self.id]). Each value goes to the variable of that name in the new object's scripts (after their outside code, so it replaces the default written there, and before on_create, which already sees it), or, when no script has that variable, to its property of that name.",
     "DEF on_press_space() {\n\tb = SPAWN(\"res://bullet.jblueprint\", x, y, [speed: 600, angle: rotation])\n}"),
    ("DESTROY", "Objects",
     "Removes an object and everything inside it: DESTROY(object), or DESTROY() for this object.\n\n\
      It happens at the end of the frame, after on_destroy() runs on it, so code after DESTROY in the same event still runs.",
     "DEF on_collide_start(other) {\n\tIF NAME_OF(other) == \"Player\" {\n\t\tGLOBAL.score += 1\n\t\tDESTROY()\n\t}\n}"),

    // ---------------------------------------------------------------- events and timers
    ("SEND", "Events and timers",
     "Runs an event on one object: SEND(object, \"hurt\", 10) runs on_hurt(10) in that object's scripts.\n\n\
      The event name is without on_. Any inputs after it are passed along. Nothing happens if the object has no such event. It runs right away, before SEND returns.",
     "SEND(\"Player\", \"hurt\", 10)\n\n>> in the player's script:\nDEF on_hurt(amount) {\n\thp -= amount\n}"),
    ("EMIT", "Events and timers",
     "Runs an event on every object that has it: EMIT(\"level_won\") runs on_level_won() everywhere.\n\n\
      Inputs after the name are passed along. Use it for things many objects care about (the game ending, an alarm, the music changing).",
     "EMIT(\"alarm\", x, y)\n\n>> in each guard's script:\nDEF on_alarm(ax, ay) {\n\ttarget_x = ax\n}"),
    ("BROADCAST", "Events and timers",
     "Another name for EMIT: runs on_<event> on every object that has it.",
     "BROADCAST(\"game_over\")"),
    ("TIMER", "Events and timers",
     "Does something later: TIMER(\"spawn\", 2) runs on_timer(\"spawn\") and on_timer_spawn() on this object after 2 seconds.\n\n\
      seconds defaults to 1. With repeat TRUE it keeps going every that many seconds until TIMER_STOP. Starting a timer with a name that's already running starts it over.\n\n\
      Timers follow game time, so they wait while the game is paused and run faster or slower with TIME_SCALE.",
     "DEF on_create() {\n\tTIMER(\"spawn\", 1.5, TRUE)\n}\nDEF on_timer_spawn() {\n\tSPAWN(\"res://enemy.jblueprint\", RANDOM(0, 800), -40)\n}"),
    ("TIMER_STOP", "Events and timers",
     "Cancels one of this object's timers by name, so its event doesn't run.",
     "DEF on_press_escape() {\n\tTIMER_STOP(\"spawn\")\n}"),
    ("TWEEN", "Events and timers",
     "Slides a property smoothly to a new value: TWEEN(object, \"y\", 100, 0.5, \"bounce\").\n\n\
      Works on numbers (x, opacity, rotation, scale_x...), and on colors and vectors. seconds defaults to 0.5. ease is how it moves: \"linear\", \"in\" (starts slow), \"out\" (ends slow, the default), \"in_out\", \"back\" (goes a little past and comes back), \"bounce\", \"elastic\".\n\n\
      A new tween on the same property replaces the old one. When it finishes, on_tween_end(property) runs on that object.",
     "TWEEN(NULL, \"opacity\", 0, 1)\nTWEEN(\"Title\", \"y\", 60, 0.8, \"bounce\")\nTWEEN(\"Sky\", \"color\", \"#203050\", 3, \"in_out\")"),
    ("TWEEN_STOP", "Events and timers",
     "Stops an object's tweens where they are: TWEEN_STOP(object) stops all of them, TWEEN_STOP(object, \"x\") just that property's. on_tween_end doesn't run for a stopped tween.",
     "TWEEN_STOP(NULL, \"x\")"),

    // ---------------------------------------------------------------- physics
    ("MOVE", "Physics",
     "Moves a body by an amount, stopping at walls and sliding along them: MOVE(object, dx, dy, dz). Gives TRUE when the body is standing on something afterwards.\n\n\
      Use it for players and enemies (a kinematic Body2D/Body3D) in on_physics(dt) or on_step(dt), multiplying speeds by dt. A moving body also pushes dynamic bodies it walks into. In 2D, dz is left out. Plain objects without a body are just moved.",
     ">> fall: this script's own variable (outside the function, so it's kept)\nfall = 0\nDEF on_physics(dt) {\n\tfall += 900 * dt\n\tgrounded = MOVE(NULL, AXIS(\"left\", \"right\") * 250 * dt, fall * dt)\n\tIF grounded { fall = 0 }\n\tIF grounded AND KEY_PRESSED(\"space\") { fall = -450 }\n}"),
    ("IMPULSE", "Physics",
     "Kicks a dynamic body: adds (x, y, z) to its velocity at once, the same for heavy and light bodies. For jumps, explosions, knockback. 2D velocities are pixels per second; 3D metres per second. Kinematic and static bodies ignore it.",
     "DEF on_collide_start(other) {\n\tIMPULSE(other, 0, -400)\n}"),
    ("COLLIDING", "Physics",
     "The objects touching this one right now, as a table of ids. For things you check every frame (standing in water, on a button); for the moment of touching use on_collide_start(other).",
     "FOR o IN COLLIDING() {\n\tIF NAME_OF(o) == \"Lava\" { hp -= 50 * DELTA_TIME() }\n}"),
    ("RAYCAST", "Physics",
     "Looks along a line from (x, y) in direction (dx, dy) and gives the first 2D body or collider it hits, or NULL: RAYCAST(x, y, dx, dy, length, ignore, layers).\n\n\
      The result is a table: object (its id), x and y (where it hit), nx and ny (the surface's direction), distance. length defaults to very far. ignore is an object to look through (usually self.id, so it doesn't hit itself). layers limits what it can hit, like \"world, enemies\".",
     "hit = RAYCAST(x, y, 0, 1, 40, self.id)\nIF hit !== NULL {\n\tPRINT(\"ground \" + hit.distance + \" below\")\n}"),
    ("RAYCAST_3D", "Physics",
     "The same as RAYCAST, in 3D: RAYCAST_3D(x, y, z, dx, dy, dz, length, ignore, layers). length defaults to 1000 metres. The result also has z and nz.",
     "hit = RAYCAST_3D(x, y, z, 0, -1, 0, 2, self.id)\ngrounded = hit !== NULL"),
    ("DISTANCE", "Physics",
     "How far apart two objects are (pixels in 2D, metres in 3D), measured between their positions in the world. DISTANCE(\"Player\") measures from this object.",
     "IF DISTANCE(NULL, \"Player\") < 200 {\n\tchasing = TRUE\n}"),

    // ---------------------------------------------------------------- input
    ("KEY_DOWN", "Input",
     "TRUE while a key is held. Key names: \"a\"–\"z\", \"0\"–\"9\", \"space\", \"enter\", \"escape\", \"left\", \"right\", \"up\", \"down\", \"shift\", \"ctrl\", \"alt\", \"tab\", \"f1\"...; mouse buttons \"mouse_left\", \"mouse_right\", \"mouse_middle\"; gamepad buttons \"pad_a\", \"pad_b\", \"pad_start\"...\n\n\
      Input actions from Project settings work too: KEY_DOWN(\"jump\") is TRUE while any of jump's keys is held.",
     "IF KEY_DOWN(\"shift\") { speed = 400 } ELSE { speed = 200 }"),
    ("KEY_PRESSED", "Input",
     "TRUE only on the frame a key went down: one shot, for jumping, firing, opening a menu. Same names as KEY_DOWN. (Events like on_press_space() do the same without checking every frame.)",
     "IF KEY_PRESSED(\"e\") { SEND(\"Door\", \"use\") }"),
    ("KEY_RELEASED", "Input",
     "TRUE only on the frame a key was let go.",
     "IF KEY_RELEASED(\"space\") AND vy < 0 { vy = vy / 2 }"),
    ("AXIS", "Input",
     "Two ways:\n\n\
      AXIS(\"a\", \"d\"): a pair of keys as a number: -1 while the first is held, 1 while the second is, 0 for neither or both. The quickest way to write movement.\n\n\
      AXIS(\"left_x\"): a gamepad stick or trigger from -1 to 1: left_x, left_y, right_x, right_y, lt, rt.",
     "DEF on_step(dt) {\n\tx += AXIS(\"left\", \"right\") * 300 * dt\n\ty += AXIS(\"up\", \"down\") * 300 * dt\n}"),
    ("MOUSE_X", "Input",
     "The mouse's x in the game world (2D), following the camera. Use with MOUSE_Y to aim at the mouse.",
     ">> (JLB's angles are in degrees)\nrotation = ATAN(MOUSE_Y() - y, MOUSE_X() - x)"),
    ("MOUSE_Y", "Input",
     "The mouse's y in the game world (2D); y goes down.",
     "DRAW_CIRCLE(MOUSE_X(), MOUSE_Y(), 6, \"#ffffff\")"),
    ("MOUSE_WHEEL", "Input",
     "How far the mouse wheel turned this frame: positive away from you, 0 when it didn't.",
     "zoom += MOUSE_WHEEL() * 0.1"),
    ("MOUSE_LOCK", "Input",
     "MOUSE_LOCK(TRUE) hides the pointer and keeps it in the window, for mouse-look in first-person games; read movement with MOUSE_DX and MOUSE_DY. MOUSE_LOCK(FALSE) frees it. In the editor, Escape frees it too.",
     "DEF on_create() {\n\tMOUSE_LOCK(TRUE)\n}"),
    ("MOUSE_DX", "Input",
     "How far the mouse moved sideways this frame, in screen pixels. Works while the mouse is locked.",
     "rotation_y -= MOUSE_DX() * 0.15"),
    ("MOUSE_DY", "Input",
     "How far the mouse moved up or down this frame (down is positive). Works while locked.",
     "rotation_x = MAX(-89, MIN(89, rotation_x - MOUSE_DY() * 0.15))"),

    // ---------------------------------------------------------------- time and flow
    ("DELTA_TIME", "Time and game flow",
     "Seconds since the last frame (also given to on_step as dt). Multiply speeds by it so movement is the same at any frame rate.",
     "x += 100 * DELTA_TIME()"),
    ("GAME_TIME", "Time and game flow",
     "Seconds since the game started (game time: it stops while paused). Good for things that wave or pulse.",
     ">> SIN takes degrees: 180 a second is one wave every 2 seconds\ny = 200 + SIN(GAME_TIME() * 180) * 20"),
    ("CHANGE_BLUEPRINT", "Time and game flow",
     "Switches the game to another blueprint file: CHANGE_BLUEPRINT(\"res://level2.jblueprint\").\n\n\
      With fade (seconds), the screen fades to a color (black unless you give one) and back. Everything in the current blueprint is removed; GLOBAL variables and REMEMBER values carry over.",
     "DEF on_collide_start(other) {\n\tIF NAME_OF(other) == \"Player\" {\n\t\tCHANGE_BLUEPRINT(\"res://level2.jblueprint\", 0.5)\n\t}\n}"),
    ("PAUSE", "Time and game flow",
     "PAUSE(TRUE) stops the game: events, physics, timers and tweens wait. Objects in the group while_paused keep running, so put your pause menu in that group. PAUSE(FALSE) carries on.",
     "DEF on_press_escape() {\n\tPAUSE(!PAUSED())\n\tSET(\"PauseMenu\", \"visible\", PAUSED())\n}"),
    ("PAUSED", "Time and game flow",
     "TRUE while the game is paused with PAUSE.",
     "DEF on_step(dt) {\n\tIF PAUSED() { RETURN }\n\tx += 100 * dt\n}"),
    ("TIME_SCALE", "Time and game flow",
     "How fast game time runs: 1 normal, 0.3 slow motion, 2 double speed. dt, timers, tweens and physics all follow it. TIME_SCALE() with no input gives the current scale.",
     "TIME_SCALE(0.3)\nTIMER(\"normal_speed\", 0.5)"),
    ("SHAKE", "Time and game flow",
     "Shakes the camera: SHAKE(strength, seconds). strength is in pixels (2D; defaults to 8) and dies away over seconds (defaults to 0.4).",
     "SHAKE(12, 0.3)"),
    ("FULLSCREEN", "Time and game flow",
     "FULLSCREEN(TRUE) makes the game window full screen, FALSE puts it back. FULLSCREEN() with no input gives whether it is. (Alt+Enter and F11 switch too; SET_DISPLAY changes the resolution and how the picture is scaled.)",
     "FULLSCREEN(!FULLSCREEN())"),
    ("WINDOW_TITLE", "Time and game flow",
     "Changes the text in the game window's title bar.",
     "WINDOW_TITLE(\"My Game - Level \" + level)"),
    ("QUIT", "Time and game flow",
     "Stops the game (in the editor, like pressing Stop; in an exported game, closes it).",
     "DEF on_pressed_quit() {\n\tQUIT()\n}"),

    // ---------------------------------------------------------------- sound
    ("PLAY_SOUND", "Sound",
     "Plays a sound file once and gives its id: PLAY_SOUND(file, volume, pitch, pan, echo).\n\n\
      file is res://... (.wav, .ogg, .mp3, .flac, .opus, .m4a, .aiff, tracker music .mod/.xm/.s3m/.it and more) or hl2://.... volume 1 is normal; pitch 1 is normal (2 an octave up); pan -1 is left, 1 right; echo 0 to 1 adds an echo. Keep the id to stop it with STOP_SOUND.",
     "PLAY_SOUND(\"res://sounds/jump.wav\")\nPLAY_SOUND(\"res://sounds/hit.wav\", 0.6, RANDOM(0.9, 1.1))"),
    ("STOP_SOUND", "Sound",
     "Stops a sound PLAY_SOUND started, by its id. STOP_SOUND(NULL) stops every sound.",
     "engine = PLAY_SOUND(\"res://engine.ogg\")\nSTOP_SOUND(engine)"),
    ("PLAY", "Sound",
     "Starts a Sound object (one you placed in the blueprint, with its own file, volume, loop and 3D settings). Given a Timer object, starts it counting down from its wait_time again.",
     "PLAY(\"Alarm\")"),
    ("STOP", "Sound",
     "Stops a Sound object, or a Timer object (it won't run out until PLAY starts it again).",
     "STOP(\"Alarm\")"),
    ("MUSIC", "Sound",
     "Plays a music file on a loop, replacing whatever music was playing: MUSIC(file, volume). MUSIC(\"\") stops the music. Music carries on across CHANGE_BLUEPRINT.",
     "MUSIC(\"res://music/level1.ogg\", 0.7)"),
    ("VOLUME", "Sound",
     "The master volume for every sound and the music: 0 silent, 1 normal (up to 4).",
     "VOLUME(0.5)"),

    // ---------------------------------------------------------------- animation
    ("ANIMATE", "Animation",
     "Plays an animation: ANIMATE(object, \"run\").\n\n\
      For a Sprite2D, the animation comes from its animations property (frames on a sheet set up in Properties). For a Mesh3D, it's one of the model's own animations (see ANIMATIONS). If that animation is already playing it carries on, unless restart is TRUE. A non-looping one runs on_animation_end(name) when it finishes.",
     "IF AXIS(\"left\", \"right\") !== 0 {\n\tANIMATE(NULL, \"run\")\n} ELSE {\n\tANIMATE(NULL, \"idle\")\n}"),
    ("ANIMATIONS", "Animation",
     "The names of a Mesh3D model's animations, as a table of text.",
     "FOR a IN ANIMATIONS(\"Soldier\") { PRINT(a) }"),
    ("PLAY_CLIP", "Animation",
     "Makes an Animator play one of its clips (made in the Timeline) from the start: PLAY_CLIP(animator, \"open\"). When it ends, on_clip_end(clip) runs on the Animator's parent and on the Animator.",
     "PLAY_CLIP(\"Door/Animator\", \"open\")"),
    ("STOP_CLIP", "Animation",
     "Stops an Animator's clip where it is.",
     "STOP_CLIP(\"Door/Animator\")"),
    ("CLIP_PLAYING", "Animation",
     "The name of the clip an Animator is playing, or NULL when none is.",
     "IF CLIP_PLAYING(\"Door/Animator\") == NULL { PLAY_CLIP(\"Door/Animator\", \"close\") }"),
    ("BURST", "Animation",
     "Makes a Particles2D or Particles3D throw out a number of particles at once (default 20): explosions, puffs of dust, sparks. Works whether or not the particles are emitting on their own.",
     "BURST(\"Explosion\", 60)"),
    ("RAGDOLL", "Animation",
     "Makes a Mesh3D with a skinned model (a character) go limp and fall as a ragdoll from the pose it's in, starting at velocity (vx, vy, vz). IMPULSE shoves it afterwards.",
     "DEF on_died() {\n\tRAGDOLL(NULL, 0, 2, -3)\n}"),

    // ---------------------------------------------------------------- tiles
    ("TILE", "Tiles",
     "The tile id at a cell of a TileMap2D: TILE(tilemap, col, row). -1 means empty. Columns and rows count from 0 at the tilemap's top-left.",
     "IF TILE(\"Ground\", 3, 7) == -1 { PRINT(\"hole\") }"),
    ("SET_TILE", "Tiles",
     "Changes a tile: SET_TILE(tilemap, col, row, id). -1 (or leaving id out) clears it. Solid tiles' collisions follow the change.",
     "cell = TILE_AT(\"Ground\", x, y + 20)\nSET_TILE(\"Ground\", cell.col, cell.row, -1)"),
    ("TILE_AT", "Tiles",
     "The cell under a position in the world, as a table [col:, row:]: TILE_AT(tilemap, x, y). Use it with TILE and SET_TILE.",
     "c = TILE_AT(\"Ground\", MOUSE_X(), MOUSE_Y())\nSET_TILE(\"Ground\", c.col, c.row, 5)"),
    ("FIND_PATH", "Tiles",
     "A path through a TileMap2D's empty cells from one world position to another, going around solid tiles: FIND_PATH(tilemap, x1, y1, x2, y2). Gives a table of [x:, y:] points (cell centres) to walk through in order, or NULL when there's no way.",
     "path = FIND_PATH(\"Level\", x, y, GET(\"Player\", \"x\"), GET(\"Player\", \"y\"))\nIF path !== NULL AND LEN(path) > 1 {\n\tnext = path[2]\n}"),

    // ---------------------------------------------------------------- drawing
    ("DRAW_RECT", "Drawing",
     "Draws a rectangle for this frame: DRAW_RECT(x, y, w, h, color, filled). x, y is its top-left corner in the 2D world. filled defaults to TRUE; FALSE draws an outline. Call it from on_draw() every frame you want it shown.\n\n\
      Every DRAW_ command but DRAW_LINE takes a table of options last. layer: where it goes among the blueprint's sprites and labels (drawings go on top, at layer 1000, unless you say); rotation: degrees around x, y; blend: \"add\" or \"multiply\"; alpha.",
     "DEF on_draw() {\n\tDRAW_RECT(x - 20, y - 30, 40 * hp / 100, 4, \"#a6e22e\")\n}"),
    ("DRAW_LINE", "Drawing",
     "Draws a line for this frame from (x1, y1) to (x2, y2).",
     "DEF on_draw() {\n\tDRAW_LINE(x, y, MOUSE_X(), MOUSE_Y(), \"#f92672\")\n}"),
    ("DRAW_CIRCLE", "Drawing",
     "Draws a circle for this frame around (x, y). filled defaults to TRUE.",
     "DEF on_draw() {\n\tDRAW_CIRCLE(x, y, 64, \"#66d9ef\", FALSE)\n}"),
    ("DRAW_TEXT", "Drawing",
     "Draws text for this frame at (x, y): DRAW_TEXT(x, y, text, size, color). size defaults to 16. Anything given as text is shown as it would PRINT, and tags work as in a Label2D: [color=red]..[/color], [size=24], [i], [u], [s], [bg=#000000].\n\n\
      Options (a table last): layer, font (a .ttf/.otf of the project, or the name of a font installed on the computer like \"Times New Roman\"), align (\"left\", \"center\", \"right\": which point of the text is at x), valign (\"top\", \"middle\", \"bottom\"), wrap (break lines longer than this many pixels), outline (pixels) and outline_color, rotation, scale, alpha.",
     "DEF on_draw() {\n\tDRAW_TEXT(10, 10, \"Score: \" + GLOBAL.score, 24, \"#ffffff\")\n}"),
    ("DRAW_IMAGE", "Drawing",
     "Draws a picture for this frame: DRAW_IMAGE(file, x, y, w, h). w and h default to 64.",
     "DEF on_draw() {\n\tDRAW_IMAGE(\"res://ui/heart.png\", 10, 40, 24, 24)\n}"),
    ("DRAW_SPRITE", "Drawing",
     "Draws a picture for this frame at its own size, the way GameMaker's draw_sprite_ext does: DRAW_SPRITE(file, x, y, [options]).\n\n\
      Options: frame with hframes and vframes (a sheet of frames), or region: [x, y, w, h] (a part of the picture, in its pixels); w and h (the size to draw; its own size otherwise); scale, scale_x, scale_y; rotation (degrees); pivot_x and pivot_y (the point at x, y, and turned around: 0 the left/top edge, 0.5 the middle; 0 when left out); color and alpha; layer (1000 when left out: on top); blend; flip_h and flip_v; the effects brightness, hue, saturation, invert; shader and values: [a, b, c, d] (a custom sprite shader); filter (\"pixel\" or \"smooth\"); lit.",
     "DEF on_draw() {\n\tDRAW_SPRITE(\"res://cards.png\", x, y, [hframes: 8, frame: 3, pivot_x: 0.5, pivot_y: 0.5, rotation: 15, scale: 0.5, layer: 20])\n}"),
    ("PRELOAD", "Drawing",
     "Gets pictures ready in the background, so they show without a pause the first time they're drawn: PRELOAD(file, file...), a table of files, or a folder (every picture in it, and in its folders).\n\n\
      Pictures are always loaded in the background (one that isn't ready yet isn't drawn for a moment, instead of the game stopping while it loads); PRELOAD starts them early, after whatever is being drawn now. Pictures placed in a blueprint are preloaded when it starts. Big games: preload what the next screen needs while this one shows.",
     "DEF on_create() {\n\tPRELOAD(\"res://assets/cards\")\n}"),
    ("IMAGE_SIZE", "Drawing",
     "A picture's size in pixels: [w:, h:], or NULL when it can't be read. Read once per file and remembered.",
     "s = IMAGE_SIZE(\"res://cards.png\")\nPRINT(s.w + \" x \" + s.h)"),
    ("CLEAR_CANVAS", "Drawing",
     "Empties a Canvas2D or a surface: CLEAR_CANVAS(canvas, color). The color fills it (its clear_color when left out: see-through unless set). What was drawn into it earlier this frame goes too.\n\n\
      Drawing into one: give any DRAW_* the option target: and the Canvas2D (its name, path or id) or the surface. What's drawn stays, frame after frame, until it's cleared (like Scratch's pen), in the canvas's own pixels ((0, 0) its top-left).",
     "DEF on_step(dt) {\n\tIF KEY_DOWN(\"mouse_left\") {\n\t\tDRAW_CIRCLE(MOUSE_X() - 100, MOUSE_Y() - 100, 4, \"white\", TRUE, [target: \"Paper\"])\n\t}\n\tIF KEY_PRESSED(\"c\") { CLEAR_CANVAS(\"Paper\") }\n}"),
    ("SURFACE_CREATE", "Drawing",
     "Makes a picture to draw into, w by h pixels, and gives its name (like GameMaker's surfaces): draw into it with any DRAW_* and the option target: name; show it with DRAW_SPRITE(name, x, y), or as a Sprite2D's texture. It keeps what's drawn until CLEAR_CANVAS. Let go of it with SURFACE_FREE when it's not needed (at most 256 at once).\n\n\
      For one placed in the blueprint, use a Canvas2D object.",
     "minimap = SURFACE_CREATE(200, 150)\nDEF on_draw() {\n\tCLEAR_CANVAS(minimap, \"#000000\")\n\tDRAW_RECT(x / 10, y / 10, 3, 3, \"red\", TRUE, [target: minimap])\n\tDRAW_SPRITE(minimap, 10, 10, [screen: TRUE])\n}"),
    ("SURFACE_FREE", "Drawing",
     "Lets go of a surface made with SURFACE_CREATE (its picture is freed).",
     "SURFACE_FREE(minimap)"),
    ("POST_SHADER", "Drawing",
     "Runs a full-screen shader over the finished picture: POST_SHADER(file, [a, b, c, d]). The .wgsl file defines fn post(color: vec4<f32>, uv: vec2<f32>, time: f32, values: vec4<f32>) -> vec4<f32>: it gets each pixel's color and place (uv, 0 to 1 across the screen) and gives the new color; post_texture(uv) reads the picture anywhere (for waves and blurs). The four numbers arrive as values.x to values.w. NULL turns it off.\n\n\
      The project's graphics setting post_shader sets one for the whole game.",
     "POST_SHADER(\"res://shaders/hurt.wgsl\", [hurt_amount, 0, 0, 0])"),
    ("SET_CURSOR", "Input",
     "Draws a picture as the mouse pointer: SET_CURSOR(file, x, y), x and y being its hot spot (the pixel that points, from its top-left). \"hidden\" hides the pointer over the game; NULL brings the system's back.",
     "SET_CURSOR(\"res://assets/hand.png\", 6, 2)"),
    ("MOUSE_SCREEN_X", "Input",
     "Where the mouse is in screen space, across: the game window's pixels, (0, 0) its top-left, whatever the camera does. What on_draw_gui and screen: TRUE draw in.",
     "DEF on_draw_gui() {\n\tDRAW_CIRCLE(MOUSE_SCREEN_X(), MOUSE_SCREEN_Y(), 6, \"white\")\n}"),
    ("MOUSE_SCREEN_Y", "Input",
     "Where the mouse is in screen space, down (see MOUSE_SCREEN_X).",
     "sy = MOUSE_SCREEN_Y()"),
    ("TO_SCREEN", "Drawing",
     "Where a 2D world point is on screen: TO_SCREEN(x, y) gives [x:, y:] in screen space (the window's pixels). For putting a health bar drawn in on_draw_gui over an enemy.",
     "DEF on_draw_gui() {\n\tp = TO_SCREEN(x, y - 40)\n\tDRAW_RECT(p.x - 20, p.y, 40 * hp / 100, 4, \"red\")\n}"),
    ("TO_WORLD", "Drawing",
     "Where a screen-space point is in the 2D world: TO_WORLD(x, y) gives [x:, y:].",
     "w = TO_WORLD(MOUSE_SCREEN_X(), MOUSE_SCREEN_Y())"),
    ("TEXT_SIZE", "Drawing",
     "How big text is drawn: TEXT_SIZE(text, size, font, wrap) gives [w:, h:] in pixels, laid out exactly as a Label2D or DRAW_TEXT lays it out (size 16 and the built-in font when left out; wrap: the width it wraps at, 0 for none). For centering, boxes behind text and menus.",
     "s = TEXT_SIZE(\"Game over\", 48)\nDRAW_TEXT(400 - s.w / 2, 300, \"Game over\", 48, \"white\", [screen: TRUE])"),
    ("OBJECTS_AT", "Objects",
     "The 2D objects at a world point, the one drawn on top first: OBJECTS_AT(x, y, precise). Sprites count by their picture (precise: only its visible pixels), labels by their text, colliders and bodies by their shape. No physics needed (like GameMaker's instance_position).",
     "top = OBJECTS_AT(MOUSE_X(), MOUSE_Y(), TRUE)\nIF LEN(top) > 0 { PRINT(NAME_OF(top[1])) }"),
    ("OBJECTS_IN", "Objects",
     "The 2D objects whose box overlaps a world rectangle, the one on top first: OBJECTS_IN(x, y, w, h), x and y its top-left corner (like GameMaker's collision_rectangle). For box selection, area effects and checking a spot is free.",
     "FOR id IN OBJECTS_IN(x - 50, y - 50, 100, 100) {\n\tSEND(id, \"hit\", 10)\n}"),
    ("HAS_GLOBAL", "Saving and remembering",
     "TRUE once the GLOBAL variable of that name has been made (reading one that hasn't is an error): HAS_GLOBAL(\"name\").",
     "IF !HAS_GLOBAL(\"coins\") {\n\tGLOBAL coins = 0\n}"),
    ("GLOBAL_OR", "Saving and remembering",
     "A GLOBAL variable's value, or the fallback when it hasn't been made: GLOBAL_OR(\"name\", fallback).",
     "lives = GLOBAL_OR(\"lives\", 3)"),
    ("READ_FILE", "Saving and remembering",
     "Reads a text file of the project (res://...) as text, or NULL when it can't be read. For dialogue, level lists, any data you keep in files.",
     "story = READ_FILE(\"res://data/intro.txt\")"),
    ("READ_JSON", "Saving and remembering",
     "Reads a JSON file of the project into tables and values (objects become named tables, arrays lists), or NULL when it can't be read. Decimals come back exact.",
     "items = READ_JSON(\"res://data/items.json\")\nPRINT(items.sword.damage)"),
    ("POINT_ON", "Input",
     "TRUE when a point in the 2D world is on an object: POINT_ON(object, x, y, precise). A Sprite2D counts by its picture as it's drawn (turned, scaled, its pivot); with precise TRUE only where the picture isn't see-through. A Collider2D or Body2D counts by its shape, a Label2D by its text.\n\n\
      For the mouse there are events too: on_mouse_enter(), on_mouse_leave(), on_clicked(button), on_click_held(button), on_click_released(button).",
     "IF POINT_ON(\"Card\", MOUSE_X(), MOUSE_Y(), TRUE) {\n\tself.scale_x = 1.1\n}"),

    // ---------------------------------------------------------------- saving
    ("SAVE_DATA", "Saving and remembering",
     "Keeps a value on disk under a name, so it's still there the next time the game runs (high scores, settings, a save game). The value can be a number, text, TRUE/FALSE or a table of those. Each game keeps its own.",
     "SAVE_DATA(\"best\", GLOBAL.score)\nSAVE_DATA(\"save1\", [level: 3, hp: hp, coins: coins])"),
    ("LOAD_DATA", "Saving and remembering",
     "A value kept with SAVE_DATA, or NULL when nothing was saved under that name.",
     "best = LOAD_DATA(\"best\")\nIF best == NULL { best = 0 }"),
    ("DELETE_DATA", "Saving and remembering",
     "Forgets a value kept with SAVE_DATA.",
     "DELETE_DATA(\"save1\")"),
    ("REMEMBER", "Saving and remembering",
     "Keeps a value while the game runs, across CHANGE_BLUEPRINT (lives, the inventory, which door you came in by). It's gone when the game stops; use SAVE_DATA to keep it longer. (GLOBAL variables also carry over.)",
     "REMEMBER(\"lives\", lives)\nCHANGE_BLUEPRINT(\"res://level2.jblueprint\")"),
    ("RECALL", "Saving and remembering",
     "A value kept with REMEMBER, or NULL.",
     "lives = RECALL(\"lives\")\nIF lives == NULL { lives = 3 }"),

    // ---------------------------------------------------------------- settings
    ("SET_GRAPHICS", "Settings",
     "Changes one of the project's graphics settings while the game runs (the names in Project settings → Graphics and docs/SETTINGS.md): render_scale, shadows, bloom, tonemap, exposure, saturation, vignette, tint... For an options menu or a low-detail mode. It lasts until the game stops.",
     "SET_GRAPHICS(\"shadows\", FALSE)\nSET_GRAPHICS(\"render_scale\", 0.75)"),
    ("GRAPHICS", "Settings",
     "A graphics setting's current value; GRAPHICS() with no input gives a table of all of them.",
     "IF GRAPHICS(\"bloom\") { PRINT(\"bloom is on\") }"),
    ("SET_PHYSICS", "Settings",
     "Changes a physics setting while the game runs: gravity_2d ([x, y], pixels per second²), gravity_3d, tick_rate, solver_iterations, max_slope... Moon gravity, underwater sections.",
     "SET_PHYSICS(\"gravity_2d\", [0, 300])"),
    ("PHYSICS", "Settings",
     "A physics setting's current value; PHYSICS() with no input gives a table of all of them.",
     "g = PHYSICS(\"gravity_2d\")"),
    ("SET_DISPLAY", "Settings",
     "Changes one of the project's display settings while the game runs (Project settings → Display):\n\n\
      width, height: the resolution the game is drawn at (in a window, the window resizes to match).\n\
      window_scale: how big the window is compared to the resolution.\n\
      fullscreen: TRUE fills the screen (same as FULLSCREEN).\n\
      resizable: whether the player can resize the window.\n\
      scale_mode: \"fit\" (keeps the shape, bars around it), \"stretch\" (fills, may squash), \"pixel\" (whole-number scaling for pixel art) or \"expand\" (draws at the window's size and shows more).\n\
      scale_filter: \"pixel\" (default, sharp) or \"smooth\" (blends pixels when scaled).\n\
      bar_color: the color of the bars.\n\n\
      vsync is read when the game starts: change it, then SAVE_DISPLAY() keeps it for next time. Changes last until the game stops unless kept with SAVE_DISPLAY.",
     "SET_DISPLAY(\"width\", 1280)\nSET_DISPLAY(\"height\", 720)\nSET_DISPLAY(\"scale_mode\", \"pixel\")"),
    ("DISPLAY", "Settings",
     "A display setting's current value; DISPLAY() with no input gives a table of all of them.",
     "res = DISPLAY(\"width\") + \"x\" + DISPLAY(\"height\")"),
    ("SAVE_DISPLAY", "Settings",
     "Keeps the current display settings on this computer, so the exported game starts with them next time instead of the project's: the Apply button of an options menu. SAVE_DISPLAY(FALSE) forgets them and the project's settings are used again. (Only the built game keeps them: while playing in the editor, SAVE_DISPLAY does nothing and Project settings stay in charge.)",
     "DEF on_pressed_apply() {\n\tSET_DISPLAY(\"fullscreen\", GLOBAL.want_full)\n\tSET_DISPLAY(\"vsync\", GLOBAL.want_vsync)\n\tSAVE_DISPLAY()\n}"),
    ("SET_ACTION", "Settings",
     "Gives an input action (Project settings → Input) new keys while the game runs, for key rebinding: SET_ACTION(action, keys). keys is a list ([\"space\", \"w\"]) or a text (\"space, w\"); none removes the action. KEY_DOWN(\"jump\") and on_press_jump() use the new keys at once. It lasts until the game stops (keep choices with SAVE and set them again at the start).",
     "SET_ACTION(\"jump\", [\"space\", \"up\", \"pad_a\"])"),
    ("ACTION", "Settings",
     "An input action's keys as a list (NULL if there's no such action); ACTION() with no input gives a table of every action and its keys.",
     "FOR k IN ACTION(\"jump\") {\n\tPRINT(k)\n}"),

    // ---------------------------------------------------------------- maps
    ("MAP_ENTITIES", "Maps",
     "The entities in a Half-Life 2 map shown by a Mesh3D (a .bsp): MAP_ENTITIES(map, classname).\n\n\
      Gives a table of entries, each [classname:, name:, x:, y:, z:, rotation_y:, keys:, bounds:]: keys holds every setting as the map wrote it, bounds is a brush entity's box (a ladder, a trigger). classname picks one kind (\"info_player_start\"); ending it with * matches any that start that way (\"info_player*\").",
     "FOR e IN MAP_ENTITIES(\"Map\", \"info_player*\") {\n\tx = e.x\n\ty = e.y\n\tz = e.z\n}"),
    ("USE_DOOR", "Maps",
     "Opens (TRUE) or closes (FALSE) one of a map's doors; with no second input it toggles. A map's doors are in the group map_doors.",
     "FOR d IN OBJECTS(\"map_doors\") {\n\tIF DISTANCE(NULL, d) < 2 { USE_DOOR(d) }\n}"),
    // ---------------------------------------------------------------- multiplayer
    ("HOST_GAME", "Multiplayer",
     "Starts a multiplayer game on this computer that others can join with JOIN_GAME: HOST_GAME() uses port 7777, HOST_GAME(port) another.\n\n\
      You're player 1 straight away. Each player who joins runs on_player_joined(id) in your game. Players on the same network join with your MY_ADDRESS(); players over the internet need the port forwarded on your router (or a VPN like ZeroTier or Tailscale, which are free).\n\n\
      To try it alone: Build the game, start the built game and press Play in the editor, and join \"localhost\" from one of them.",
     "DEF on_pressed_host() {\n\tHOST_GAME()\n\tPRINT(\"hosting at \" + MY_ADDRESS())\n\tNET_SPAWN(\"res://player.jblueprint\", 100, 300)\n}"),
    ("JOIN_GAME", "Multiplayer",
     "Joins a game someone is hosting: JOIN_GAME(\"192.168.1.20\") (port 7777), JOIN_GAME(\"192.168.1.20\", 8000) or JOIN_GAME(\"host.example.com:8000\"). JOIN_GAME() joins this computer (\"localhost\").\n\n\
      It takes a moment: on_connected(id) runs once you're in (id is your player number), or on_join_failed(reason) if it didn't work. The host's shared objects appear in your game when you're in. Both games must be the same project.",
     "DEF on_pressed_join() {\n\tJOIN_GAME(address_box_text)\n}\nDEF on_connected(id) {\n\tNET_SPAWN(\"res://player.jblueprint\", 100 * id, 300)\n}\nDEF on_join_failed(reason) {\n\tPRINT(\"couldn't join: \" + reason)\n}"),
    ("LEAVE_GAME", "Multiplayer",
     "Leaves the multiplayer game: other players' objects disappear from yours, and yours from theirs. When the host leaves, the game ends for everyone (they get on_disconnected).",
     "DEF on_pressed_quit_to_menu() {\n\tLEAVE_GAME()\n\tCHANGE_BLUEPRINT(\"res://blueprints/menu.jblueprint\")\n}"),
    ("ONLINE", "Multiplayer",
     "TRUE while you're hosting or in someone's game (not while still joining).",
     "IF ONLINE() {\n\tPRINT(LEN(PLAYERS()) + \" players\")\n}"),
    ("IS_HOST", "Multiplayer",
     "TRUE in the host's game. Decisions only one game should make (spawning enemies, the score, who won) usually belong to the host.",
     "DEF on_timer_spawn_enemy() {\n\tIF IS_HOST() {\n\t\tNET_SPAWN(\"res://enemy.jblueprint\", RANDOM(0, 1280), 0)\n\t}\n}"),
    ("MY_ID", "Multiplayer",
     "Your player number: 1 for the host, then 2, 3... in the order players join. 0 when you're not online.",
     "self.text = \"You are player \" + MY_ID()"),
    ("PLAYERS", "Multiplayer",
     "Every player's number in the game, yours included, as a table.",
     "FOR p IN PLAYERS() {\n\tPRINT(\"player \" + p)\n}"),
    ("NET_EMIT", "Multiplayer",
     "Runs an event in every other player's game: NET_EMIT(\"exploded\", x, y) runs on_exploded(x, y) on every object that has it there (not in your own game; EMIT that too if you want it here).\n\n\
      Inputs can be numbers, text, TRUE/FALSE, NULL or tables of those. Object ids differ between games, so send a shared object's NET_ID instead. In the event, SENDER() is who sent it.",
     "DEF on_press_space() {\n\tNET_EMIT(\"chat\", \"hello!\")\n}\nDEF on_chat(text) {\n\tPRINT(\"player \" + SENDER() + \": \" + text)\n}"),
    ("NET_SEND", "Multiplayer",
     "Runs an event in one player's game: NET_SEND(player, \"event\", inputs...). The player is a number from PLAYERS(), SENDER() or on_player_joined(id).",
     "DEF on_player_joined(id) {\n\tNET_SEND(id, \"welcome\", \"hi, player \" + id)\n}"),
    ("SENDER", "Multiplayer",
     "Inside an event another player's game sent (NET_EMIT / NET_SEND): that player's number. NULL in any other event.",
     "DEF on_hit(damage) {\n\tPRINT(\"player \" + SENDER() + \" hit me for \" + damage)\n}"),
    ("NET_SPAWN", "Multiplayer",
     "SPAWN for everyone: NET_SPAWN(blueprint, x, y, z, parent) makes the instance in every player's game, and your game owns it: its position, rotation, size, visibility and properties (self.health, a sprite's flip, an animation...), and those of the objects inside it, are sent to the others 20 times a second when they change, and their copies glide there.\n\n\
      Its scripts run in every game, so code that reads keys should first check IS_MINE(), or every player's copy would follow your keyboard. Other players' copies of a body follow yours instead of falling, and their cameras stay off.\n\n\
      DESTROY on a shared object removes it for everyone. When a player leaves, the objects their game owned go too. Players who join later get every shared object that's still there.",
     "DEF on_connected(id) {\n\tme = NET_SPAWN(\"res://player.jblueprint\", 200, 300)\n}\n\n>> in player.jlb:\nDEF on_physics(dt) {\n\tIF !IS_MINE() { RETURN }\n\tx += AXIS(\"left\", \"right\") * 200 * dt\n}"),
    ("NET_SYNC", "Multiplayer",
     "Shares an object that's placed in the blueprint (a door, a moving platform, the ball), so it's the same in every player's game: NET_SYNC() in its own on_create, or NET_SYNC(object). The host's game moves it until SET_OWNER hands it to someone else; in the other games it follows.\n\n\
      Every game should NET_SYNC the same objects (the same script does that). Destroying it removes it for everyone.",
     "DEF on_create() {\n\tNET_SYNC()\n}\nDEF on_physics(dt) {\n\tIF IS_MINE() {\n\t\tself.rotation += 45 * dt\n\t}\n}"),
    ("OWNER", "Multiplayer",
     "The number of the player whose game moves a shared object (an object inside one counts as part of it). Your own number for objects that aren't shared.",
     "IF OWNER(\"Ball\") == SENDER() {\n\tPRINT(\"they have the ball\")\n}"),
    ("IS_MINE", "Multiplayer",
     "TRUE when your game is the one moving this object: a shared object you own (or an object inside one), or any object that isn't shared. FALSE on other players' copies. With no input, asks about this object. Always TRUE when not online, so the same scripts work in single player.",
     "DEF on_step(dt) {\n\tIF IS_MINE() AND KEY_PRESSED(\"jump\") {\n\t\tIMPULSE(NULL, 0, -400)\n\t}\n}"),
    ("SET_OWNER", "Multiplayer",
     "Hands a shared object to another player's game, which moves it from then on (picking up the ball, getting into a car): SET_OWNER(object, player).",
     "DEF on_collide_start(other) {\n\tIF IS_MINE(other) AND !IS_MINE() {\n\t\tSET_OWNER(NULL, MY_ID())\n\t}\n}"),
    ("NET_ID", "Multiplayer",
     "A shared object's name, the same in every player's game (object ids aren't): send it with NET_EMIT, and the other game finds the object with NET_OBJECT. NULL for objects that aren't shared.",
     "NET_EMIT(\"shot\", NET_ID(target), 10)"),
    ("NET_OBJECT", "Multiplayer",
     "The object in this game for a NET_ID from another game, or NULL when it isn't here.",
     "DEF on_shot(who, damage) {\n\tt = NET_OBJECT(who)\n\tIF t !== NULL AND IS_MINE(t) {\n\t\tSET(t, \"health\", GET(t, \"health\") - damage)\n\t}\n}"),
    ("MY_ADDRESS", "Multiplayer",
     "This computer's address for friends to type into JOIN_GAME: its ZeroTier address when it's on a ZeroTier network (or Tailscale, Hamachi), otherwise its local network one, like \"192.168.1.20\". NULL if there's no network.\n\n\
      On Windows, friends on ZeroTier can only get in once GEX is allowed through the firewall on every network type (File → Work together → Allow GEX through the firewall does it for the editor; a built game asks Windows itself: tick Public too).",
     "self.text = \"Hosting at \" + MY_ADDRESS()"),
];

/// The basics every command relies on, shown first.
pub const BASICS: &str = "Commands are the ALL-CAPS words a script calls to work with the game: COMMAND(inputs). Inputs in the list that you leave out at the end take their defaults.\n\n\
Objects: where a command takes an object, give its id (a number: self.id, or what OBJECT, SPAWN, OBJECTS, collision events give you), its path from this object (\"Gun\", \"../Enemy\", \"/Level/Door\"), its bare name (\"Player\", found anywhere in the blueprint), or NULL for this object itself.\n\n\
This object: inside its scripts, its properties are plain names (x, y, speed) or self.x, self.speed. Change them by assigning: x += 10.\n\n\
Colors: a table [r: 1, g: 0.5, b: 0, a: 1] (0 to 1) or a text like \"#ff8800\".\n\n\
Files: res://... is your project, hl2://... a mounted Half-Life 2.\n\n\
Keys: \"a\"-\"z\", \"0\"-\"9\", \"space\", \"enter\", \"escape\", \"left\", \"right\", \"up\", \"down\", \"shift\", \"ctrl\", \"alt\", \"tab\", \"f1\"..., mouse buttons \"mouse_left\", \"mouse_right\", \"mouse_middle\", gamepad \"pad_a\", \"pad_b\", \"pad_x\", \"pad_y\", \"pad_lb\", \"pad_rb\", \"pad_start\", \"pad_select\", \"pad_up\"... and input actions named in Project settings.\n\n\
Variables for the whole game: GLOBAL score = 0 makes one; every script reads and changes it as GLOBAL.score, even inside tables (GLOBAL.save.decks[2] = 7). They last for the whole run, across CHANGE_BLUEPRINT (a script that runs GLOBAL score = 0 again sets it back to 0).";

/// (event, when it runs): the functions the engine calls in your scripts.
pub const EVENTS: &[(&str, &str)] = &[
    ("(code outside functions)", "when the script is attached, before on_create"),
    ("on_create()", "once the whole blueprint is loaded, or right after SPAWN"),
    ("on_step(dt)", "every frame; dt is the seconds since the last one"),
    ("on_physics(dt)", "60 times a second, before physics moves bodies: the place for MOVE"),
    ("on_draw()", "every frame after on_step: the place for DRAW_ commands"),
    ("on_draw_gui()", "every frame after on_draw: DRAW_ commands here are in screen space (the window's pixels, over everything, whatever the camera does), like GameMaker's Draw GUI"),
    ("on_destroy()", "just before the object is removed"),
    ("on_key_pressed(key) / on_key_released(key)", "any key went down / up"),
    ("on_press_<key>() / on_release_<key>()", "one key or input action: on_press_space(), on_press_a(), on_press_jump()"),
    ("on_mouse_pressed(button, x, y)", "a click: button is \"left\", \"right\" or \"middle\""),
    ("on_mouse_moved(x, y) / on_mouse_wheel(amount)", "the mouse moved / the wheel turned"),
    ("on_mouse_enter() / on_mouse_leave()", "a 2D object (a Sprite2D's picture, a Collider2D, Body2D or Label2D): the mouse went over it / off it. A Sprite2D with mouse_precise counts only its visible pixels"),
    ("on_clicked(button) / on_click_held(button) / on_click_released(button)", "a 2D object: a mouse button went down / is held / came up while the mouse is over it. Every object under the mouse gets them, not just the top one"),
    ("on_collide_start(other)", "started touching another body or collider (other is its id)"),
    ("on_collide(other)", "every frame while touching"),
    ("on_collide_end(other)", "stopped touching"),
    ("on_timer(name) / on_timer_<name>()", "a TIMER finished"),
    ("on_tween_end(property)", "a TWEEN on this object finished"),
    ("on_animation_end(name)", "a non-looping sprite or model animation finished"),
    ("on_clip_end(clip)", "an Animator's clip finished (on the Animator and its parent)"),
    ("on_pressed()", "on a Button's own scripts: it was clicked"),
    ("on_pressed_<button name>()", "on every script: a Button with that name was clicked (Start Game -> on_pressed_start_game)"),
    ("on_text_changed(text) / on_text_submitted(text)", "on a TextInput's scripts: typed in / Enter pressed"),
    ("on_toggled(checked)", "on a Checkbox's scripts: clicked (checked: whether it's ticked now); on_toggled_<checkbox name>(checked) runs on every script"),
    ("on_value_changed(value)", "on a Slider's scripts: its handle moved; on_value_changed_<slider name>(value) runs on every script"),
    ("on_timeout(name)", "a Timer object reached zero (on the Timer and its parent); on_timeout_<timer name>() runs on every script"),
    ("on_<event>(...)", "your own: SEND(object, \"<event>\", ...) or EMIT(\"<event>\", ...)"),
    ("on_connected(id) / on_join_failed(reason)", "multiplayer: JOIN_GAME got you in (id is your player number) / couldn't"),
    ("on_player_joined(id) / on_player_left(id)", "multiplayer: another player joined / left"),
    ("on_disconnected(reason)", "multiplayer: the host left or the connection broke"),
    ("when_<name>() -> on_<name>()", "a condition: on_<name> runs once each time when_<name> turns TRUE"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_guide_with_a_valid_example() {
        for (name, ..) in crate::commands::COMMANDS {
            assert!(GUIDE.iter().any(|g| g.0 == *name), "{} has no guide entry", name);
        }
        crate::commands::register();
        for (name, group, details, example) in GUIDE {
            assert!(crate::commands::COMMANDS.iter().any(|c| c.0 == *name), "{} isn't a command", name);
            assert!(GROUPS.iter().any(|g| g.0 == *group), "{}: no group {}", name, group);
            assert!(!details.is_empty() && !example.is_empty(), "{}", name);
            let toks = jlb::lexer::lex(example).unwrap_or_else(|e| panic!("{}: {}", name, e.msg));
            if let Err(e) = jlb::parser::Parser::new(toks).program() {
                panic!("{}'s example doesn't parse: line {}: {}\n{}", name, e.line, e.msg, example);
            }
        }
    }
}
