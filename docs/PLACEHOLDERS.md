# Built-in placeholder art (gex://)

GEX comes with 708 simple pictures you can use straight away while a game has no art of its own: shapes, tiling patterns, solid colors, UI pieces, icons, stand-in characters, particles and skies. They're inside the GEX program, so every project has them and exported games include them. They're original and free to use in anything, no credit needed.

## Using them

- In the **Assets** panel, click **gex:// (built-in)**. Folders of pictures show as thumbnails.
- Drag a picture into the blueprint view to make a Sprite2D, or onto a `texture` field. With an object selected, **Use on selected** sets its texture.
- In scripts, use the path like any other: `self.texture = "gex://icons/coin.png"`.
- Right-click one → **Copy into project** to get an editable copy in `res://assets/placeholders/`.
- `shapes/white/`, `shapes/outline/`, `icons/white/` and `particles/` are white on purpose: set the object's `color` to tint them any color.
- Patterns tile seamlessly: use `repeat_x`/`repeat_y` on a Sprite2D, or `uv_scale_x/y` on a Mesh3D.
- The `ui/panel_*` and `ui/button_*` pictures are made for nine-slice: give a Panel, Button or Image the picture as `texture` and `slice` = 16.

## What's in it

**shapes/** — 128×128. Folders: `white`, `outline`, and one per color (black, blue, brown, cyan, gray, green, lime, orange, pink, purple, red, teal, yellow). In each: arrow, burst, capsule, check, chevron, circle, crescent, cross, diamond, drop, half_circle, heart, hexagon, lightning, octagon, parallelogram, pentagon, plus, right_triangle, ring, rounded_square, square, star, star6, trapezoid, triangle. (A few have no outline version.)

**patterns/** — tiling textures (128×128 unless noted):

- `bricks/`: dark, gray, red, white, yellow
- `checker/`: blue_2, blue_4, blue_8, bw_2, bw_4, bw_8, dark_2, dark_4, dark_8, gray_2, gray_4, gray_8, green_2, green_4, green_8, orange_2, orange_4, orange_8, purple_2, purple_4, purple_8, red_2, red_4, red_8
- `diamonds/`: blue, mono, teal, warm
- `dots/`: blue, bw, dark, gray, green, orange, purple, red
- `gradients/`: candy_horizontal, candy_radial, candy_vertical, fire_horizontal, fire_radial, fire_vertical, forest_horizontal, forest_radial, forest_vertical, mono_horizontal, mono_radial, mono_vertical, night_horizontal, night_radial, night_vertical, ocean_horizontal, ocean_radial, ocean_vertical, sky_horizontal, sky_radial, sky_vertical, sunset_horizontal, sunset_radial, sunset_vertical
- `grid/`: blue, bw, dark, gray, green, orange, purple, red
- `hexagons/`: blue, mono, teal, warm
- `metal/`: gold, rust, steel
- `noise/`: dirt, grass, gray, lava, moss, sand, snow, stone, water
- `prototype/`: blue, dark, gray, green, orange, purple, red, white
- `scales/`: blue, mono, teal, warm
- `stripes/`: blue_diagonal, blue_horizontal, blue_vertical, bw_diagonal, bw_horizontal, bw_vertical, dark_diagonal, dark_horizontal, dark_vertical, gray_diagonal, gray_horizontal, gray_vertical, green_diagonal, green_horizontal, green_vertical, orange_diagonal, orange_horizontal, orange_vertical, purple_diagonal, purple_horizontal, purple_vertical, red_diagonal, red_horizontal, red_vertical
- `tiles/`: black, blue, terracotta, white
- `waves/`: blue, mono, teal, warm
- `wood/`: dark, light, red
- `zigzag/`: blue, mono, teal, warm

  `prototype/` are 256×256 grid textures with a "1m x 1m" label, for blocking out 3D levels. `gradients/` are 8×256 strips (vertical), 256×8 (horizontal) and 256×256 (radial).

**colors/** — 16×16 solid swatches: beige, black, blue, brown, coral, crimson, cyan, dark_gray, forest, gold, gray, green, indigo, lavender, light_gray, lime, magenta, maroon, mint, navy, olive, orange, peach, pink, purple, red, sky, slate, tan, teal, white, yellow.

**ui/** — bar_back, bar_fill_blue, bar_fill_green, bar_fill_purple, bar_fill_red, bar_fill_yellow, button_blue_hover, button_blue_normal, button_blue_pressed, button_dark_hover, button_dark_normal, button_dark_pressed, button_green_hover, button_green_normal, button_green_pressed, button_light_hover, button_light_normal, button_light_pressed, button_red_hover, button_red_normal, button_red_pressed, button_wood_hover, button_wood_normal, button_wood_pressed, checkbox_off, checkbox_on, crosshair_circle, crosshair_cross, crosshair_dot, crosshair_gap, cursor, panel_blue, panel_dark, panel_glass, panel_green, panel_light, panel_red, panel_wood, radio_off, radio_on, slider_knob.

**icons/** — 64×64, in color (and white in `icons/white/`): arrow_down, arrow_left, arrow_right, arrow_up, bolt, bomb, check, clock, close, coin, drop, eye, flag, flame, gear, gem, heart, home, info, key, lock, minus, music, mute, pause, play, plus, potion, shield, skull, sound, star, stop, sword, trophy, warning.

**characters/** — 128×128 stand-ins: blob, box_buddy, bush, chest, crate, door, enemy, ghost, player, robot, rock, ship, slime, smiley, tree, ufo.

**particles/** — white, for Particles2D/3D `texture` or glows: dot, glow, ring, shard, smoke, soft_circle, sparkle, square, star, streak.

**skies/** — 512×256 backdrops (alien, day, night, overcast, sunset) and 512×160 hill strips that tile sideways for parallax (hills_blue, hills_dark, hills_green, hills_purple, hills_sand).

## Adding more

The pack is made by `tools/make_placeholders.py` (Python with Pillow). Add to it there and run it; the next build of GEX picks the files up.
