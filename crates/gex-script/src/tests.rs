use std::collections::HashSet;
use std::path::PathBuf;

use gex_core::project::Project;
use gex_core::Variant;

use crate::world::{Game, LogKind};

fn project(name: &str, files: &[(&str, &str)]) -> Project {
    let dir = std::env::temp_dir().join(format!("gex-script-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (p, text) in files {
        let path: PathBuf = dir.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::write(dir.join("project.jproj"), "(name: \"test\")").unwrap();
    Project::open(&dir).unwrap()
}

const MAIN: &str = r#"(root: (name: "Root", kind: Some(Node2D), children: [
    (name: "Player", kind: Some(Sprite2D), scripts: ["res://player.jlb"], children: [(name: "Box", kind: Some(Collider2D))]),
    (name: "Wall", kind: Some(Sprite2D), transform: Some((position: (100.0, 0.0, 0.0))), scripts: ["res://wall.jlb"],
        children: [(name: "Box", kind: Some(Collider2D))]),
]))"#;

const PLAYER: &str = r#"
speed = 100
hits = 0
DEF on_create() {
    self.ticks = 0
    TIMER("tick", 0.1, TRUE)
}
DEF on_step(dt) {
    IF KEY_DOWN("right") { self.x += speed * dt }
}
DEF on_collide_start(other) {
    GLOBAL hits += 1
    PRINT("hit " + NAME_OF(other))
    SEND(other, "bumped", 5)
}
DEF on_timer_tick() {
    self.ticks += 1
}
DEF when_far() {
    RETURN self.x > 50
}
DEF on_far() {
    PRINT("far!")
    EMIT("player_far")
}
"#;

const WALL: &str = r#"
DEF on_bumped(n) {
    self.bumps = n
}
DEF on_player_far() {
    SPAWN("res://bullet.jscene", 1, 2)
}
"#;

const BULLET_SCENE: &str = r#"(root: (name: "Bullet", kind: Some(Sprite2D), scripts: ["res://bullet.jlb"]))"#;
const BULLET: &str = "DEF on_create() {\n    PRINT(\"bullet at \" + self.x + \",\" + self.y)\n    DESTROY()\n}\nDEF on_destroy() {\n    PRINT(\"bullet gone\")\n}\n";

#[test]
fn a_small_game_runs() {
    let p = project(
        "game",
        &[("scenes/main.jscene", MAIN), ("player.jlb", PLAYER), ("wall.jlb", WALL), ("bullet.jscene", BULLET_SCENE), ("bullet.jlb", BULLET)],
    );
    let mut g = Game::from_scene(p, "res://scenes/main.jscene", None).unwrap();
    let right: HashSet<String> = ["right".to_string()].into();
    for _ in 0..60 {
        g.frame(1.0 / 60.0, right.clone(), [0.0, 0.0], HashSet::new());
    }
    let logs = g.take_logs();
    let errors: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert!(errors.is_empty(), "{:#?}", errors);
    let printed: Vec<_> = logs.iter().map(|l| l.text.as_str()).collect();
    assert!(printed.contains(&"hit Wall"), "{:?}", printed);
    assert_eq!(printed.iter().filter(|t| **t == "far!").count(), 1, "conditions fire once on the way up: {:?}", printed);
    assert!(printed.contains(&"bullet at 1,2") && printed.contains(&"bullet gone"), "{:?}", printed);

    let t = g.tree();
    let player = t.find_path(None, "Root/Player").unwrap();
    let wall = t.find_path(None, "Root/Wall").unwrap();
    assert!((t.nodes[player].transform.position.x - 100.0).abs() < 0.01);
    assert_eq!(t.nodes[wall].props.get("bumps"), Some(&Variant::Num(5.0)));
    let ticks = t.nodes[player].num("ticks", 0.0);
    assert!((9.0..=10.0).contains(&ticks), "ticks {}", ticks);
    assert!(t.find_named("Bullet").is_none(), "destroyed");
}

#[test]
fn script_errors_are_reported_with_a_location() {
    let p = project(
        "err",
        &[
            ("main.jscene", r#"(root: (name: "R", kind: Some(Node2D), scripts: ["res://bad.jlb"]))"#),
            ("bad.jlb", "DEF on_step(dt) {\n    a = 1\n    y = GET(\"Nope\", \"x\")\n}\n"),
        ],
    );
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    let logs = g.take_logs();
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).collect();
    assert_eq!(errs.len(), 1, "a failing script stops instead of repeating: {:#?}", logs);
    assert_eq!(errs[0].line, 3);
    assert!(errs[0].file.as_deref().unwrap_or("").ends_with("bad.jlb"));
}

#[test]
fn handlers_are_found() {
    let h = crate::world::scan_handlers_text("DEF on_step(dt) {}\n  FUNC when_x() {}\nDEF helper() {}\nGLOBAL DEF on_y() {}\n");
    let mut v: Vec<_> = h.into_iter().collect();
    v.sort();
    assert_eq!(v, ["on_step", "on_y", "when_x"]);
}

#[test]
fn physics_bodies_fall_collide_and_move() {
    let scene = r#"(root: (name: "R", kind: Some(Node2D), children: [
        (name: "Floor", kind: Some(Body2D), transform: Some((position: (400.0, 500.0, 0.0))),
            props: {"body": Text("static"), "width": Num(800.0), "height": Num(40.0)}),
        (name: "Wall", kind: Some(Body2D), transform: Some((position: (600.0, 400.0, 0.0))),
            props: {"body": Text("static"), "width": Num(40.0), "height": Num(160.0)}),
        (name: "Crate", kind: Some(Body2D), transform: Some((position: (200.0, 100.0, 0.0))), scripts: ["res://crate.jlb"]),
        (name: "Hero", kind: Some(Body2D), transform: Some((position: (500.0, 464.0, 0.0))),
            props: {"body": Text("kinematic")}, scripts: ["res://hero.jlb"]),
    ]))"#;
    let crate_jlb = "landed = 0\nDEF on_collide_start(other) {\n    IF NAME_OF(other) == \"Floor\" { GLOBAL landed += 1 }\n}\nDEF on_step(dt) {\n    self.landed = landed\n}\n";
    let hero_jlb = "DEF on_step(dt) {\n    self.floor = MOVE(NULL, 300 * dt, 50 * dt)\n    hit = RAYCAST(self.x, self.y, 0, 1, 500, self.id)\n    IF hit !== NULL { self.below = NAME_OF(hit.node) }\n}\n";
    let p = project("phys", &[("main.jscene", scene), ("crate.jlb", crate_jlb), ("hero.jlb", hero_jlb)]);
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    for _ in 0..180 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let logs = g.take_logs();
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    let c = t.find_path(None, "R/Crate").unwrap();
    let y = t.nodes[c].transform.position.y;
    // The crate (32 tall) rests on the floor's top at y = 480.
    assert!((y - 464.0).abs() < 2.0, "crate y {}", y);
    assert_eq!(t.nodes[c].props.get("on_floor"), Some(&Variant::Bool(true)));
    assert_eq!(t.nodes[c].props.get("landed"), Some(&Variant::Num(1.0)), "collide_start once");
    let h = t.find_path(None, "R/Hero").unwrap();
    let hx = t.nodes[h].transform.position.x;
    // Walking right is stopped by the wall (its left side is at 580; hero half width 16).
    assert!(hx < 566.0 && hx > 540.0, "hero x {}", hx);
    assert_eq!(t.nodes[h].props.get("floor"), Some(&Variant::Bool(true)));
    assert_eq!(t.nodes[h].props.get("below"), Some(&Variant::Text("Floor".into())));
}

#[test]
fn physics_3d_ball_rests_on_ground() {
    let scene = r#"(root: (name: "R", kind: Some(Node3D), children: [
        (name: "Ground", kind: Some(Body3D), props: {"body": Text("static"), "width": Num(20.0), "height": Num(1.0), "depth": Num(20.0)}),
        (name: "Ball", kind: Some(Body3D), transform: Some((position: (0.0, 5.0, 0.0))), props: {"shape": Text("sphere")}),
    ]))"#;
    let p = project("phys3", &[("main.jscene", scene)]);
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    for _ in 0..240 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let t = g.tree();
    let b = t.find_path(None, "R/Ball").unwrap();
    let y = t.nodes[b].transform.position.y;
    assert!((y - 1.0).abs() < 0.05, "ball y {}", y);
    assert_eq!(t.nodes[b].props.get("on_floor"), Some(&Variant::Bool(true)));
}

/// A tiny 8-bit mono WAV file.
fn wav() -> String {
    let data: Vec<u8> = (0..800).map(|i| (128.0 + 60.0 * ((i as f32) * 0.3).sin()) as u8).collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    // Stored as Latin-1 text so the helper can write it; every byte maps to one char.
    b.iter().map(|c| *c as char).collect()
}

#[test]
fn sounds_play_or_report_problems() {
    let w = wav();
    let scene = r#"(root: (name: "R", kind: Some(Node), scripts: ["res://s.jlb"], children: [
        (name: "Beep", kind: Some(Sound), props: {"stream": Res("res://beep.wav"), "autoplay": Bool(true)}),
    ]))"#;
    let s = "DEF on_create() {\n    PLAY_SOUND(\"res://beep.wav\", 0.5)\n    PLAY(\"Beep\")\n    MUSIC(\"res://beep.wav\")\n    MUSIC(\"\")\n    TRY { PLAY_SOUND(\"res://bad.wav\") } FAIL e { PRINT(\"bad sound\") }\n}\n";
    let p = project("audio", &[("main.jscene", scene), ("s.jlb", s), ("bad.wav", "not a sound")]);
    // The WAV has bytes >= 0x80, so write it as raw bytes.
    std::fs::write(p.dir.join("beep.wav"), w.chars().map(|c| c as u8).collect::<Vec<u8>>()).unwrap();
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    let logs = g.take_logs();
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    assert!(logs.iter().any(|l| l.text == "bad sound"), "{:?}", logs.iter().map(|l| &l.text).collect::<Vec<_>>());
}

#[test]
fn tiles_animation_drawing_and_ui() {
    let scene = r#"(root: (name: "R", kind: Some(Node2D), scripts: ["res://r.jlb"], children: [
        (name: "Map", kind: Some(TileMap2D), transform: Some((position: (0.0, 200.0, 0.0))),
            props: {"tiles": Text("1,1,1,1,1,1,1,1,1,1")}),
        (name: "Ball", kind: Some(Body2D), transform: Some((position: (100.0, 0.0, 0.0))), props: {"shape": Text("circle")}),
        (name: "Hero", kind: Some(Sprite2D), props: {"hframes": Num(4.0), "animations": Text("idle=0; walk=1-3"), "animation": Text("walk"), "anim_fps": Num(10.0)}),
        (name: "Go", kind: Some(Button), scripts: ["res://go.jlb"]),
    ]))"#;
    let r = "pressed = 0\nDEF on_step(dt) {\n    DRAW_RECT(0, 0, 10, 10, \"#ff0000\")\n    DRAW_CIRCLE(5, 5, 3, [r: 0, g: 1, b: 0, a: 1], FALSE)\n    DRAW_TEXT(1, 2, \"hi\", 12)\n}\nDEF on_pressed_go() {\n    GLOBAL pressed += 1\n    self.pressed = pressed\n    SET_TILE(\"Map\", 0, 0, -1)\n}\n";
    let go = "DEF on_pressed() {\n    self.text = \"pressed!\"\n}\n";
    let p = project("twod", &[("main.jscene", scene), ("r.jlb", r), ("go.jlb", go)]);
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    for _ in 0..90 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let go_id = g.tree().find_path(None, "R/Go").unwrap();
    g.step(crate::FrameInput { dt: 1.0 / 60.0, ui: vec![crate::UiEvent::Pressed(go_id)], ..Default::default() });
    let logs = g.take_logs();
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let list = g.render_list();
    let t = g.tree();
    let ball = t.find_path(None, "R/Ball").unwrap();
    // The ball (radius 16) rests on the tile row whose top is at y = 200.
    assert!((t.nodes[ball].transform.position.y - 184.0).abs() < 2.0, "ball y {}", t.nodes[ball].transform.position.y);
    let hero = t.find_path(None, "R/Hero").unwrap();
    let f = t.nodes[hero].num("frame", -1.0);
    assert!((1.0..=3.0).contains(&f), "frame {}", f);
    assert_eq!(t.nodes[go_id].text("text"), "pressed!");
    let root = t.root.unwrap();
    assert_eq!(t.nodes[root].props.get("pressed"), Some(&Variant::Num(1.0)));
    let map = t.find_path(None, "R/Map").unwrap();
    assert!(t.nodes[map].text("tiles").starts_with("-1,1"));
    // 9 tiles + hero + the drawn rect and filled... and the outline circle as lines.
    assert!(list.sprites.iter().any(|s| s.layer == 1000.0));
    assert!(list.lines_2d.len() >= 32);
    assert!(list.labels.iter().any(|l| l.text == "hi"));
    assert_eq!(list.ui.len(), 1);
}

#[test]
fn model_animation_and_mesh_collision() {
    let scene = r#"(root: (name: "R", kind: Some(Node3D), scripts: ["res://r.jlb"], children: [
        (name: "Guy", kind: Some(Mesh3D), props: {"mesh": Text("res://guy.glb"), "anim_loop": Bool(false)}),
        (name: "Floor", kind: Some(Mesh3D), props: {"mesh": Text("plane"), "collision": Bool(true)}, transform: Some((scale: (10.0, 1.0, 10.0)))),
        (name: "Ball", kind: Some(Body3D), transform: Some((position: (0.0, 3.0, 0.0))), props: {"shape": Text("sphere")}),
    ]))"#;
    let r = "ended = \"\"\nDEF on_create() {\n    names = ANIMATIONS(\"Guy\")\n    self.count = LEN(names)\n    ANIMATE(\"Guy\", names[1])\n}\n";
    let guy = "";
    let p = project("model", &[("main.jscene", scene), ("r.jlb", r), ("guy.jlb", guy)]);
    std::fs::write(p.dir.join("guy.glb"), gex_assets::model::tiny_skinned_glb()).unwrap();
    std::fs::write(p.dir.join("end.jlb"), "DEF on_animation_end(name) {\n    self.done = name\n}\n").unwrap();
    // Attach the end-of-animation handler to Guy.
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    {
        let mut st = g.st.borrow_mut();
        let id = st.tree.find_path(None, "R/Guy").unwrap();
        st.tree.nodes[id].scripts.push("res://end.jlb".into());
    }
    for _ in 0..150 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    let root = t.root.unwrap();
    assert_eq!(t.nodes[root].props.get("count"), Some(&Variant::Num(1.0)));
    let guy = t.find_path(None, "R/Guy").unwrap();
    assert_eq!(t.nodes[guy].props.get("anim_time"), Some(&Variant::Num(1.0)), "stopped at the end");
    assert_eq!(t.nodes[guy].props.get("done"), Some(&Variant::Text("lift".into())));
    let ball = t.find_path(None, "R/Ball").unwrap();
    let y = t.nodes[ball].transform.position.y;
    assert!((y - 0.5).abs() < 0.05, "the ball rests on the collision plane: {}", y);
}

#[test]
fn breakpoints_pause_and_scripts_reload() {
    use crate::thread::{FromGame, GameHandle};
    let scene = r#"(root: (name: "R", kind: Some(Node2D), scripts: ["res://s.jlb"]))"#;
    let p = project("debug", &[("main.jscene", scene), ("s.jlb", "count = 0\nDEF on_step(dt) {\n    x = 41\n    GLOBAL count += 1\n}\n")]);
    let text = p.read_text("res://main.jscene").unwrap();
    let tree = gex_core::scene::load(&text, &|q: &str| p.read_text(q)).unwrap();
    let mut h = GameHandle::start(p.clone(), tree.clone());
    let file = p.res_to_path("res://s.jlb").unwrap().canonicalize().unwrap();
    h.breakpoints.set(file, [4].into());
    h.request_frame(crate::FrameInput { dt: 0.016, ..Default::default() });
    let mut paused = None;
    for _ in 0..200 {
        for m in h.poll() {
            if let FromGame::Paused(p) = m {
                paused = Some(p);
            }
        }
        if paused.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let p2 = paused.expect("paused at the breakpoint");
    assert_eq!(p2.line, 4);
    assert!(p2.locals.iter().any(|(k, v)| k == "x" && v == "41"), "{:?}", p2.locals);
    assert!(p2.outside.iter().any(|(k, v)| k == "count" && v == "0"), "{:?}", p2.outside);
    assert_eq!(p2.stack.first().map(|s| s.0.as_str()), Some("on_step"));
    h.breakpoints.set(p.res_to_path("res://s.jlb").unwrap().canonicalize().unwrap(), Default::default());
    h.resume();
    let mut got_frame = false;
    for _ in 0..200 {
        if h.poll().iter().any(|m| matches!(m, FromGame::Frame { .. })) {
            got_frame = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(got_frame);
    h.stop();

    // Reloading keeps outside variables that still exist.
    let mut g = Game::new(p.clone(), tree, None);
    for _ in 0..3 {
        g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    }
    std::fs::write(p.dir.join("s.jlb"), "count = 0\nDEF on_step(dt) {\n    GLOBAL count += 10\n    self.seen = count\n}\n").unwrap();
    assert_eq!(g.reload_script("res://s.jlb"), 1);
    g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    let t = g.tree();
    assert_eq!(t.nodes[t.root.unwrap()].props.get("seen"), Some(&Variant::Num(13.0)));
}

#[test]
fn paths_and_saves() {
    // A wall with one gap: the path must go through it.
    let tiles = "-1,-1,-1,-1,-1\n-1,-1,1,-1,-1\n-1,-1,1,-1,-1\n-1,-1,-1,-1,-1";
    let scene = format!(
        r#"(root: (name: "R", kind: Some(Node2D), scripts: ["res://r.jlb"], children: [
        (name: "Map", kind: Some(TileMap2D), props: {{"tiles": Text("{}")}}),
    ]))"#,
        tiles.replace('\n', "\\n")
    );
    let r = "DEF on_create() {\n    p = FIND_PATH(\"Map\", 16, 48, 144, 48)\n    self.steps = LEN(p)\n    self.blocked = FIND_PATH(\"Map\", 16, 48, 80, 48)\n    SAVE_DATA(\"slot_1\", [score: 7, names: [\"a\", \"b\"], done: TRUE])\n    back = LOAD_DATA(\"slot_1\")\n    self.score = back.score\n    self.second = back.names[2]\n    self.missing = LOAD_DATA(\"nothing_here\")\n    DELETE_DATA(\"slot_1\")\n}\n";
    let p = project("paths", &[("main.jscene", &scene), ("r.jlb", r)]);
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    let root = &t.nodes[t.root.unwrap()];
    let steps = root.num("steps", 0.0);
    // (0,1) → (4,1) around the wall at column 2 (rows 1-2): through row 0 or row 3.
    assert!((5.0..=7.0).contains(&steps), "steps {}", steps);
    assert_eq!(root.props.get("blocked"), Some(&Variant::Null), "the goal is inside the wall");
    assert_eq!(root.props.get("score"), Some(&Variant::Num(7.0)));
    assert_eq!(root.props.get("second"), Some(&Variant::Text("b".into())));
    assert_eq!(root.props.get("missing"), Some(&Variant::Null));
}

#[test]
fn particles_emit_burst_fade_and_draw() {
    let scene = r#"(root: (name: "Root", kind: Some(Node2D), scripts: ["res://boom.jlb"], children: [
        (name: "Smoke", kind: Some(Particles2D), transform: Some((position: (100.0, 100.0, 0.0))),
            props: {"rate": Num(60.0), "lifetime": Num(0.5), "gravity": Num(200.0)}),
        (name: "Boom", kind: Some(Particles2D), props: {"emitting": Bool(false), "speed": Num(0.0)}),
    ]))"#;
    let script = "DEF on_press_space() {\n    BURST(\"Boom\", 50)\n}\n";
    let p = project("particles", &[("main.jscene", scene), ("boom.jlb", script)]);
    let mut g = Game::from_scene(p, "res://main.jscene", None).unwrap();
    for _ in 0..30 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let smoke = g.tree().find_path(None, "Root/Smoke").unwrap();
    let boom = g.tree().find_path(None, "Root/Boom").unwrap();
    let count = |g: &Game, id| g.st.borrow().particles.get(&id).map(|e| e.count()).unwrap_or(0);
    let n = count(&g, smoke);
    assert!((20..=32).contains(&n), "about rate * min(time, lifetime): {}", n);
    assert_eq!(count(&g, boom), 0, "not emitting");
    g.frame(1.0 / 60.0, ["space".to_string()].into(), [0.0; 2], HashSet::new());
    assert_eq!(count(&g, boom), 50);
    let list = g.render_list();
    assert!(list.sprites.iter().filter(|s| s.node == boom).count() == 50);
    // Smoke fades towards color_end as it ages.
    assert!(list.sprites.iter().filter(|s| s.node == smoke).any(|s| s.color[3] < 0.7));
    // Everything is gone a lifetime after the burst.
    for _ in 0..80 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert_eq!(count(&g, boom), 0);
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:?}", errs);
}
