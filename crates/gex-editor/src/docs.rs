//! Middle-click (or the mouse's back button) on anything: a window explaining it.
//!
//! Panels and widgets `mark` the area they cover with a topic as they're drawn; a click finds
//! the smallest marked area under the pointer (in the layer on top there) and shows its topic.

use eframe::egui::{self, Rect, RichText};
use gex_core::ObjectKind;

use crate::theme;

#[derive(Clone)]
struct Mark {
    rect: Rect,
    layer: egui::LayerId,
    topic: String,
}

#[derive(Clone, Default)]
struct Marks(Vec<Mark>);

#[derive(Clone)]
struct Open {
    topic: String,
    at: egui::Pos2,
}

fn marks_id() -> egui::Id {
    egui::Id::new("gex doc marks")
}

/// Says "this area is about `topic`" for this frame.
pub fn mark(ui: &egui::Ui, rect: Rect, topic: impl Into<String>) {
    let m = Mark { rect: rect.intersect(ui.clip_rect()), layer: ui.layer_id(), topic: topic.into() };
    ui.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).0.push(m));
}

/// The same for a widget just added.
pub fn on(r: egui::Response, ui: &egui::Ui, topic: impl Into<String>) -> egui::Response {
    mark(ui, r.rect, topic);
    r
}

/// Hover text that is also the control's documentation (middle-click or mouse 4 on it).
pub trait Tip {
    /// The same as `on_hover_text`, and the text is what the documentation window says.
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self;
    /// Documentation with a title of its own, for controls whose label says little.
    fn named(self, title: &str, text: &str) -> Self;
}

impl Tip for egui::Response {
    fn tip(self, text: impl Into<egui::WidgetText>) -> Self {
        let text: egui::WidgetText = text.into();
        let m = Mark { rect: self.rect, layer: self.layer_id, topic: format!("tip:\u{1}{}", text.text()) };
        self.ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).0.push(m));
        self.on_hover_text(text)
    }
    fn named(self, title: &str, text: &str) -> Self {
        let m = Mark { rect: self.rect, layer: self.layer_id, topic: format!("tip:{}\u{1}{}", title, text) };
        self.ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).0.push(m));
        self.on_hover_text(text)
    }
}

/// What the menus are for, by label.
const MENUS: &[(&str, &str)] = &[
    ("File", "Saving, opening and making blueprints, project settings, exporting the game, closing the project."),
    ("Edit", "Undo and redo, and duplicating or deleting the selected objects."),
    ("Object", "Adds an object to the blueprint being edited."),
    ("View", "Which panels show, tool windows (Timeline, Profiler), find in files, and the layout."),
    ("Open blueprint", "Opens one of the project's blueprints in its own tab."),
    ("New blueprint", "Makes a new blueprint file with a 2D, 3D or plain root object."),
    ("+ Object", "Adds an object at the top of the blueprint (a collider goes on the selected object)."),
    ("+ Instance", "Places an instance of another blueprint in this one. Instances follow their file when it changes."),
    ("Add child", "Adds an object inside this one: it follows this one's position and visibility."),
    ("Add event", "Adds a function the engine calls (when the object is created, every step, on a key, a collision...) to this object. Each event gets a script of its own named after the object and the event, like bullet_step."),
    ("Mouse", "Events for mouse buttons, movement and the wheel."),
    ("Collision", "Events for when this object's collider or body starts, keeps or stops touching another."),
    ("Other", "Timers, UI buttons and text boxes, animations and tweens finishing."),
    ("Custom", "Your own events: BROADCAST(\"name\") runs on_name() on every object that has it."),
    ("Attach", "Attaches an existing script to this object as well."),
    ("+", "Import files, open the project folder, undo the last file change."),
    ("+Blueprint", "Makes a new blueprint (2D, 3D or a menu) in the selected folder."),
    ("Key pressed", "Runs once when a key goes down."),
    ("Key released", "Runs once when a key comes up."),
];

/// A menu button that explains itself like the other controls.
pub fn menu<R>(ui: &mut egui::Ui, label: impl Into<egui::WidgetText>, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<Option<R>> {
    let label: egui::WidgetText = label.into();
    let text = MENUS.iter().find(|m| m.0 == label.text()).map(|m| m.1);
    let title = label.text().to_string();
    let mut r = ui.menu_button(label, add);
    if let Some(t) = text {
        r.response = r.response.named(&title, t);
    }
    r
}

/// True on the frame the documentation button was clicked (middle, or mouse 4).
pub fn clicked(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Middle) || i.pointer.button_clicked(egui::PointerButton::Extra1))
}

/// Opens a topic directly (the script editor knows the word under the pointer).
pub fn open(ctx: &egui::Context, topic: impl Into<String>) {
    let at = ctx.pointer_latest_pos().unwrap_or_default();
    // Read before taking the lock on the context's data: asking the context for anything
    // while holding it would wait forever.
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new("gex doc direct frame"), frame);
        d.insert_temp(egui::Id::new("gex doc open"), Some(Open { topic: topic.into(), at }));
    });
}

/// Call once a frame after everything is drawn.
pub fn show(ctx: &egui::Context) {
    let marks = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<Marks>(marks_id()))).0;
    let open_id = egui::Id::new("gex doc open");
    let direct_id = egui::Id::new("gex doc direct frame");
    if clicked(ctx) {
        // A direct open this frame (a word in a script) wins over the areas.
        let direct = ctx.data(|d| d.get_temp::<u64>(direct_id)) == Some(ctx.cumulative_frame_nr());
        if let (false, Some(p)) = (direct, ctx.pointer_latest_pos()) {
            let top = ctx.layer_id_at(p);
            let under = |tips: bool| marks.iter().filter(|m| m.rect.contains(p) && Some(m.layer) == top && m.topic.starts_with("tip:") == tips).min_by(|a, b| a.rect.area().total_cmp(&b.rect.area()));
            // A control's own text first, then what it's part of.
            let part_of = under(false).map(|m| m.topic.clone()).unwrap_or_else(|| "ui:editor".into());
            let topic = match under(true) {
                Some(t) => format!("{}\u{1}{}", t.topic, part_of),
                None => part_of,
            };
            ctx.data_mut(|d| d.insert_temp(open_id, Some(Open { topic, at: p })));
        }
    }
    let Some(o) = ctx.data(|d| d.get_temp::<Option<Open>>(open_id)).flatten() else { return };
    let (title, kind, body) = lookup(&o.topic);
    let mut keep = true;
    let screen = ctx.content_rect();
    let pos = egui::pos2(o.at.x.min(screen.right() - 440.0).max(8.0), (o.at.y + 14.0).min(screen.bottom() - 260.0).max(8.0));
    let w = egui::Window::new("Documentation").id(egui::Id::new(("gex doc window", o.at.x as i32, o.at.y as i32))).open(&mut keep).collapsible(false).resizable(false).default_pos(pos).order(egui::Order::Tooltip).show(ctx, |ui| {
        ui.set_width(410.0);
        theme::fade_in(ui);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&title).size(16.0).strong().color(theme::ACCENT));
            ui.label(RichText::new(kind).small().color(theme::TEXT_DIM));
        });
        ui.separator();
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for line in body.lines() {
                if let Some(code) = line.strip_prefix("    ") {
                    ui.label(RichText::new(code).monospace().color(theme::MK_GREEN));
                } else if let Some(h) = line.strip_prefix("# ") {
                    ui.add_space(4.0);
                    theme::title(ui, h);
                } else if let Some((name, rest)) = line.strip_prefix("- ").and_then(|l| l.split_once(": ")) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(name).monospace().color(theme::C_EVENT));
                        ui.label(RichText::new(rest).color(theme::TEXT));
                    });
                } else if line.is_empty() {
                    ui.add_space(4.0);
                } else {
                    ui.label(line);
                }
            }
        });
        ui.add_space(2.0);
        ui.label(RichText::new("Middle-click or mouse 4 on anything for its documentation. Esc closes.").small().color(theme::TEXT_DIM));
    });
    let outside = w.is_some_and(|w| ctx.input(|i| i.pointer.primary_clicked()) && !ctx.pointer_latest_pos().is_some_and(|p| w.response.rect.contains(p)));
    if !keep || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.insert_temp::<Option<Open>>(open_id, None));
    }
}

fn kind_doc(k: ObjectKind) -> &'static str {
    use ObjectKind::*;
    match k {
        Object => "The plainest object: a name, scripts and children, no position. Good for managers (score, spawning) and as the root of menus.",
        Sound => "Plays a sound file. PLAY() starts it; `autoplay` starts it with the blueprint; `loop` repeats it.",
        Animator => "Plays clips made in the Timeline: property changes over time on this object's siblings and children.",
        Object2D => "An empty 2D object: a position, rotation and scale that its children follow. Use it to group and move things together.",
        Sprite2D => "A picture or colored rectangle in the 2D world. Give it a texture, or just a width, height and color.",
        Label2D => "Text placed in the 2D world (it moves with the camera, unlike the UI Label).",
        Camera2D => "What the player sees of the 2D world. The blueprint's first Camera2D is used; with none, the view sits at the top-left.",
        Collider2D => "A shape that notices touching. It speaks for its parent: the parent's scripts get on_collide_start(other). It doesn't push things; a Body2D does.",
        Body2D => "A physics body in 2D: `dynamic` falls and bounces, `static` never moves (floors, walls), `kinematic` is moved by your script and pushes others.",
        TileMap2D => "A grid of tiles painted from one tileset picture. Select it and pick a tile in Properties to paint in the blueprint view.",
        Particles2D => "Many small pictures emitted over time: smoke, sparks, rain.",
        Light2D => "A light in a 2D blueprint. Sprites with `lit` on are lit by it.",
        Panel => "A UI rectangle on the screen (not in the world): a background for other UI.",
        Button => "A UI button. Its scripts get on_pressed(); every script gets on_pressed_<name>().",
        Label => "UI text on the screen: score, messages. Change it with SET(label, \"text\", ...).",
        TextInput => "A UI box the player types in. Sends on_text_changed and on_text_submitted.",
        ProgressBar => "A UI bar filled from `value` out of `max`: health, loading.",
        Image => "A UI picture on the screen.",
        Object3D => "An empty 3D object: a position, rotation and scale that its children follow.",
        Mesh3D => "A 3D shape: a cube, sphere or plane, or a model file. A cube can have a texture per face (Advanced texture settings).",
        Camera3D => "What the player sees of the 3D world. The blueprint's first one is used.",
        Light3D => "A 3D light: `sun` lights everything from one direction, `point` shines around a spot, `spot` is a cone.",
        Body3D => "A physics body in 3D: `dynamic`, `static` or `kinematic`, with a box, sphere or capsule shape.",
        Collider3D => "A 3D shape that notices touching and tells its parent's scripts (on_collide_start). It doesn't push things.",
        Sprite3D => "A flat picture in the 3D world, by default always facing the camera.",
        Label3D => "Text placed in the 3D world.",
        Particles3D => "Many small pictures emitted over time in 3D.",
        Environment => "The 3D blueprint's sky, ambient light, fog and screen effects. One per blueprint.",
    }
}

const UI: &[(&str, &str, &str)] = &[
    ("editor", "GEX", "Blueprints are trees of objects; scripts attached to objects handle events.\n\nTabs along the top: one tab per open blueprint; scripts open as small windows over its view, wired to their object.\n\nMiddle-click or mouse 4 on any panel, button, object, property, event, file or word in a script to read about it."),
    ("assets", "Assets", "The project's folders and files, as they are on disk.\n\n- +Blueprint, +Script, +Folder: make one in the selected folder\n- +: import files, open the project folder\n- Double-click: open a blueprint (in a tab) or a script (in the tab showing)\n- Drag onto a folder: move it there (everything using it is updated)\n- Drag into the blueprint view or onto a property: use it\n- Right-click: rename, color, delete, more\n\nUnder the project: the art that comes with GEX and connected game installs. Those are read in place; right-click a built-in file to copy it into the project."),
    ("folder", "Folder", "A folder in the project. Click to select it (new things from + go here) and open or close it. Drag files or folders onto it to move them in; drag it onto another folder to move it."),
    ("instances", "Blueprint", "The objects of the blueprint, as a tree. A child follows its parent's position and visibility.\n\n- Click: select (Ctrl+click: several)\n- Drag onto another object: make it its child\n- Right-click: add a child, duplicate, color, save as a blueprint, delete\n- + Object: a new object at the top of the blueprint (a collider goes on the selected object)\n- + Instance: places another blueprint here as an instance"),
    ("properties", "Properties", "The selected object's values: its name, where it is (Transform), what its kind has (Properties, Look...), its scripts' $variables, and custom properties scripts read as self.<name>.\n\nDrag a number sideways to change it, or double-click to type. Drag a file from Assets onto a texture, sound or font field. Click the icon by the name to change the object's icon."),
    ("events", "Events", "What the selected object's scripts respond to. A script is a list of events: functions the engine calls.\n\n- Add event: adds the function to the object's script (making the script if there is none) and opens it there\n- Click an event: opens it in the script\n- Custom: your own events. BROADCAST(\"name\") runs on_name() on every object that has it, SEND(object, \"name\") on one.\n\nUnder the events: the scripts attached to the object. An object can have several."),
    ("console", "Console", "What the game PRINTs, and errors. Click an error to open the script at that line. It appears when there is something to show. Double-click it to fold it down to a bar; click the bar to unfold it. While the game is paused at a breakpoint the debugger shows here."),
    ("blueprint_view", "Blueprint view", "The blueprint as the game will show it. The frame is the game's window.\n\n- Click: select. Drag: move, rotate or scale with the current tool\n- Arrows / rings / boxes: one axis. Two-colored squares: two axes\n- W A S D: move the camera (Q / E: down / up in 3D), Shift: fast\n- Right-drag: look around (3D) or pan (2D). Wheel: zoom\n- Drag a picture, model or blueprint in from Assets to place it"),
    ("2d3d", "2D / 3D", "Which world the view shows and edits. A blueprint can hold both: 3D objects with UI on top, for example."),
    ("tools", "Move, Rotate, Scale", "What dragging does to the selected objects. Keys: 1, 2, 3.\n\nGrab an arrow, ring or box end to change one axis; a small two-colored square to change two; the object itself to change it as you look at it."),
    ("snap", "Snap", "Dragging goes in steps: the first field for moving and scaling (pixels in 2D, meters in 3D), the second for rotating (degrees). 0 turns it off. Holding Ctrl while dragging flips snapping."),
    ("grid", "Grid", "Shows or hides the grid in the blueprint view. It isn't part of the game."),
    ("colliders", "Colliders", "Outlines colliders and physics bodies, in the editor and while the game runs. Useful to see why things do or don't touch."),
    ("game_camera", "Game camera", "While playing: on shows the game through its own camera; off lets you fly around the running game with the editor camera."),
    ("tabs", "Tabs", "One tab per open blueprint, and the Commands tab when the Command reference is open (View → Commands, F1). An orange dot means unsaved changes (Ctrl+S saves everything). The cross closes a tab (saving it). Drag a tab sideways to move it."),
    ("menubar", "Menu bar", "File (save, project settings, export), Edit (undo, redo), Object (add an object), View (panels, tools, layout). Drag the empty part to move the window; double-click it to maximize."),
    ("build", "Build", "Builds the game and runs it in a window of its own, starting in the blueprint you're editing (like Play), with the window and graphics settings from Project settings: what players get. Everything is saved first. What the game prints shows in the console. The build is temporary: it's deleted when the game closes (File > Export makes one to keep). While it runs the button closes it."),
    ("play", "Play / Stop", "Runs the blueprint you're editing (F6). F5 runs the game from its start (the main blueprint) and comes back here when it stops. Everything is saved first. The editor shows the game and gives it the keyboard; Stop (F5 or F6 again) returns to where you were."),
    ("script_editor", "Script editor", "A .jlb script. ALL-CAPS words are commands; type a few letters for suggestions. Functions named on_... are events the engine calls; `self` is the object; `$name` is a variable set per instance in Properties.\n\n- Ctrl+S: save. Ctrl+H: find and replace. Ctrl+F: find in all files\n- F1: the Command reference (on a command: its page)\n- Click a line number: a breakpoint\n- Ctrl+click a name: go to where it's defined\n- Middle-click a command: its documentation"),
    ("script_vars", "Script variables", "Variables the object's scripts declare with a line like\n    $speed\nEach instance gets its own value here; the script reads and writes it as $speed (the same thing as self.speed)."),
    ("custom_props", "Custom properties", "Values of your own on this object. Scripts read and change them as self.<name>; other objects with GET and SET."),
    ("transform", "Transform", "Where the object is, how it's turned and how big, relative to its parent. In 2D: pixels, with y going down, and one rotation in degrees. In 3D: meters, y up, rotation around x, y and z."),
    ("groups", "Groups", "Names this object goes by together with others (\"enemies\"). OBJECTS(\"enemies\") gives them all; colliders can ask whether the other is in a group."),
    ("icon", "Icon", "What stands for this object in lists. Automatic shows a sprite's or mesh's own picture and the kind's icon for the rest. A blueprint's root icon is the blueprint's icon in Assets."),
    ("find", "Find in files", "Searches every script and blueprint in the project (Ctrl+F). Click a result to go there."),
    ("rex", "REX", "The assistant. Ask it about GEX or your project (\"why doesn't my coin disappear?\"), or tell it what to make.\n\nIt runs Claude Code in the project's folder, using the Claude Code you have installed and are signed in to: GEX holds no key and it counts against your own Claude plan.\n\n- May change files: off, it only reads and answers; on, it can create and change blueprints and scripts\n- New chat: forgets the conversation\n- Stop: ends what it's doing\n\nView > REX shows or hides it."),
    ("status", "Status bar", "The last thing the editor did, and at the right the blueprint and selected object. Click `commands` (or press :) for the command line."),
];

/// (title, what kind of thing it is, the text) for a topic.
fn lookup(topic: &str) -> (String, &'static str, String) {
    let (group, name) = topic.split_once(':').unwrap_or(("ui", topic));
    match group {
        "tip" => {
            // title, the control's text, and the topic of what it's in.
            let mut parts = name.splitn(3, '\u{1}');
            let (title, text, within) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or("ui:editor"));
            let (in_title, _, in_body) = lookup(within);
            let mut text = text.to_string();
            if let Some(c) = text.chars().next().filter(|c| c.is_lowercase()) {
                text = format!("{}{}", c.to_uppercase(), &text[c.len_utf8()..]);
            }
            let title = if title.is_empty() { in_title.clone() } else { title.to_string() };
            (title, "control", format!("{}\n\n# In: {}\n{}", text, in_title, in_body))
        }
        "object" => {
            let Some(k) = ObjectKind::from_name(name) else { return (name.to_string(), "object", String::new()) };
            let mut body = format!("{}\n\n# Properties\n", kind_doc(k));
            for (p, _) in k.default_props() {
                let d = gex_core::props::info(k, p).doc;
                if !d.is_empty() {
                    body.push_str(&format!("- {}: {}\n", p, d));
                }
            }
            (k.name().to_string(), "object", body)
        }
        "prop" => {
            let (kind, p) = name.split_once('.').unwrap_or(("", name));
            let k = ObjectKind::from_name(kind).unwrap_or(ObjectKind::Object);
            let info = gex_core::props::info(k, p);
            let mut body = if info.doc.is_empty() { "A property of this object.".to_string() } else { info.doc.to_string() };
            if !info.choices.is_empty() {
                body.push_str(&format!("\n\nOne of: {}", info.choices.join(", ")));
            }
            if let Some((a, b)) = info.range {
                body.push_str(&format!("\n\nFrom {} to {}", a, b));
            }
            body.push_str(&format!("\n\n# In a script\n    self.{p} = ...\n    SET(object, \"{p}\", ...)\n    GET(object, \"{p}\")", p = p));
            (p.to_string(), "property", body)
        }
        "cmd" => {
            if let Some((n, args, doc)) = gex_script::commands::COMMANDS.iter().find(|c| c.0 == name) {
                return (n.to_string(), "engine command", format!("    {}{}\n\n{}{}", n, args, doc[..1].to_uppercase(), &doc[1..]));
            }
            const WORDS: &[(&str, &str, &str)] = &[
                ("DEF", "DEF name(inputs) {\n        ...\n    }", "Defines a function. Functions named on_... are events the engine calls."),
                ("RETURN", "RETURN value", "Leaves the function, giving a value back (or nothing)."),
                ("IF", "IF speed > 10 {\n        ...\n    } ELIF speed > 5 {\n        ...\n    } ELSE {\n        ...\n    }", "Runs the block only when the condition is true."),
                ("ELIF", "} ELIF other_condition {", "After an IF: tried when the conditions before it were false."),
                ("ELSE", "} ELSE {", "After an IF: runs when none of its conditions were true."),
                ("WHILE", "WHILE lives > 0 {\n        ...\n    }", "Repeats the block for as long as the condition stays true."),
                ("REPEAT", "REPEAT 5 {\n        ...\n    }", "Runs the block a number of times."),
                ("FOR", "FOR i IN 1 TO 10 {\n        ...\n    }\n    FOR enemy IN OBJECTS(\"enemies\") {\n        ...\n    }", "Runs the block once for each number in a range, or each thing in a table."),
                ("IN", "FOR i IN 1 TO 10 { }", "Part of FOR: what to go through."),
                ("TO", "FOR i IN 1 TO 10 { }", "Part of FOR: the last number of the range."),
                ("STEP", "FOR i IN 0 TO 100 STEP 10 { }", "Part of FOR: how much to count by."),
                ("BREAK", "BREAK", "Leaves the loop it's in right away."),
                ("SKIP", "SKIP", "Jumps to the loop's next turn."),
                ("GLOBAL", "GLOBAL score += 1", "Inside a function: changes a variable written at the top of the script instead of making a new one that's forgotten when the function ends."),
                ("LOCAL", "LOCAL n = 0", "Makes a variable that exists only inside this block."),
                ("CONST", "CONST max_speed = 400", "A variable that can't be changed afterwards."),
                ("TRUE", "alive = TRUE", "Yes / on."),
                ("FALSE", "alive = FALSE", "No / off."),
                ("NULL", "target = NULL", "Nothing: no value."),
                ("AND", "IF a AND b { }", "True when both sides are."),
                ("OR", "IF a OR b { }", "True when either side is."),
                ("NOT", "IF NOT ready { }", "Turns true into false and false into true."),
                ("TRY", "TRY {\n        ...\n    } FAIL {\n        ...\n    }", "Runs the block; if something in it goes wrong, runs the FAIL block instead of stopping the script."),
                ("START", "START task_name()", "Starts a function running alongside the rest (it can WAIT without holding anything up)."),
            ];
            if let Some((n, example, what)) = WORDS.iter().find(|w| w.0 == name) {
                return (n.to_string(), "JLB keyword", format!("{}\n\n    {}", what, example.replace("\n    ", "\n    ")));
            }
            if jlb::lexer::KEYWORDS.contains(&name) {
                return (name.to_string(), "JLB keyword", "Part of the JLB language itself (flow control, definitions). See the language reference in docs/SCRIPTING.md.".into());
            }
            (name.to_string(), "JLB built-in", "A command built into the JLB language (math, text, tables...). See docs/SCRIPTING.md.".into())
        }
        "event" => {
            let base = name.split('(').next().unwrap_or(name);
            let hit = crate::panels::EVENTS.iter().find(|e| e.0.split('(').next() == Some(base)).or_else(|| {
                // on_press_a, on_timer_spawn...: the documented example of the same family.
                ["on_press_", "on_release_", "on_timer_", "on_pressed_"].iter().find(|p| base.starts_with(**p)).and_then(|p| crate::panels::EVENTS.iter().find(|e| e.0.starts_with(p)))
            });
            match hit {
                Some((sig, doc)) => (base.to_string(), "event", format!("Runs when {}.\n\n    DEF {} {{\n        ...\n    }}", doc, if sig.starts_with(base) { sig.to_string() } else { format!("{}()", base) })),
                None => (base.to_string(), "custom event", format!("Your own event. It runs when a script calls\n    BROADCAST(\"{n}\")\non every object that has it, or\n    SEND(object, \"{n}\")\non one object.", n = base.trim_start_matches("on_"))),
            }
        }
        "file" => {
            let n = name.rsplit('/').next().unwrap_or(name).to_string();
            let low = n.to_ascii_lowercase();
            let what = if low.ends_with(".jblueprint") {
                "A blueprint: a saved tree of objects. Open it to edit it; drag it into another blueprint to place an instance of it (instances follow the file when it changes)."
            } else if low.ends_with(".jlb") {
                "A script. Attach it to an object (Events) and the engine calls its on_... functions."
            } else if gex_assets::image_load::is_image(&low) {
                "A picture. Drag it into the 2D view to make a sprite, or onto a texture property."
            } else if gex_assets::model::is_model(&low) {
                "A 3D model. Drag it into the 3D view to make a Mesh3D."
            } else if gex_assets::formats::is_audio(&low) {
                "A sound. Select it to hear it; use it on a Sound object or with PLAY_SOUND."
            } else {
                "A file in the project."
            };
            (n, "file", format!("{}\n\n    {}", what, name))
        }
        _ => match UI.iter().find(|u| u.0 == name) {
            Some((_, title, body)) => (title.to_string(), "editor", body.to_string()),
            None => lookup("ui:editor"),
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_kind_of_topic_has_text() {
        for t in ["object:Sprite2D", "prop:Mesh3D.texture", "cmd:SPAWN", "cmd:IF", "event:on_step", "event:on_press_a", "event:on_coin_taken", "file:res://a.jblueprint", "ui:assets", "ui:nonsense"] {
            let (title, _, body) = super::lookup(t);
            assert!(!title.is_empty() && !body.is_empty(), "{}", t);
        }
        for k in gex_core::object::ALL_KINDS {
            assert!(!super::kind_doc(*k).is_empty());
        }
    }
}
