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

const MAIN: &str = r#"(root: (name: "Root", kind: Some(Object2D), children: [
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
    SPAWN("res://bullet.jblueprint", 1, 2)
}
"#;

const BULLET_BLUEPRINT: &str = r#"(root: (name: "Bullet", kind: Some(Sprite2D), scripts: ["res://bullet.jlb"]))"#;
const BULLET: &str = "DEF on_create() {\n    PRINT(\"bullet at \" + self.x + \",\" + self.y)\n    DESTROY()\n}\nDEF on_destroy() {\n    PRINT(\"bullet gone\")\n}\n";

#[test]
fn a_small_game_runs() {
    let p = project(
        "game",
        &[("blueprints/main.jblueprint", MAIN), ("player.jlb", PLAYER), ("wall.jlb", WALL), ("bullet.jblueprint", BULLET_BLUEPRINT), ("bullet.jlb", BULLET)],
    );
    let mut g = Game::from_blueprint(p, "res://blueprints/main.jblueprint", None).unwrap();
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
    assert!((t.objects[player].transform.position.x - 100.0).abs() < 0.01);
    assert_eq!(t.objects[wall].props.get("bumps"), Some(&Variant::Num(5.0)));
    let ticks = t.objects[player].num("ticks", 0.0);
    assert!((9.0..=10.0).contains(&ticks), "ticks {}", ticks);
    assert!(t.find_named("Bullet").is_none(), "destroyed");
}

#[test]
fn script_errors_are_reported_with_a_location() {
    let p = project(
        "err",
        &[
            ("main.jblueprint", r#"(root: (name: "R", kind: Some(Object2D), scripts: ["res://bad.jlb"]))"#),
            ("bad.jlb", "DEF on_step(dt) {\n    a = 1\n    y = GET(\"Nope\", \"x\")\n}\n"),
        ],
    );
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
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
    let blueprint = r#"(root: (name: "R", kind: Some(Object2D), children: [
        (name: "Floor", kind: Some(Body2D), transform: Some((position: (400.0, 500.0, 0.0))),
            props: {"body": Text("static"), "width": Num(800.0), "height": Num(40.0)}),
        (name: "Wall", kind: Some(Body2D), transform: Some((position: (600.0, 400.0, 0.0))),
            props: {"body": Text("static"), "width": Num(40.0), "height": Num(160.0)}),
        (name: "Crate", kind: Some(Body2D), transform: Some((position: (200.0, 100.0, 0.0))), scripts: ["res://crate.jlb"]),
        (name: "Hero", kind: Some(Body2D), transform: Some((position: (500.0, 464.0, 0.0))),
            props: {"body": Text("kinematic")}, scripts: ["res://hero.jlb"]),
    ]))"#;
    let crate_jlb = "landed = 0\nDEF on_collide_start(other) {\n    IF NAME_OF(other) == \"Floor\" { GLOBAL landed += 1 }\n}\nDEF on_step(dt) {\n    self.landed = landed\n}\n";
    let hero_jlb = "DEF on_step(dt) {\n    self.floor = MOVE(NULL, 300 * dt, 50 * dt)\n    hit = RAYCAST(self.x, self.y, 0, 1, 500, self.id)\n    IF hit !== NULL { self.below = NAME_OF(hit.object) }\n}\n";
    let p = project("phys", &[("main.jblueprint", blueprint), ("crate.jlb", crate_jlb), ("hero.jlb", hero_jlb)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    for _ in 0..180 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let logs = g.take_logs();
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    let c = t.find_path(None, "R/Crate").unwrap();
    let y = t.objects[c].transform.position.y;
    // The crate (32 tall) rests on the floor's top at y = 480.
    assert!((y - 464.0).abs() < 2.0, "crate y {}", y);
    assert_eq!(t.objects[c].props.get("on_floor"), Some(&Variant::Bool(true)));
    assert_eq!(t.objects[c].props.get("landed"), Some(&Variant::Num(1.0)), "collide_start once");
    let h = t.find_path(None, "R/Hero").unwrap();
    let hx = t.objects[h].transform.position.x;
    // Walking right is stopped by the wall (its left side is at 580; hero half width 16).
    assert!(hx < 566.0 && hx > 540.0, "hero x {}", hx);
    assert_eq!(t.objects[h].props.get("floor"), Some(&Variant::Bool(true)));
    assert_eq!(t.objects[h].props.get("below"), Some(&Variant::Text("Floor".into())));
}

#[test]
fn solid_sprites_key_pair_axis_and_bare_object_names() {
    let blueprint = r#"(root: (name: "R", kind: Some(Object2D), children: [
        (name: "Floor", kind: Some(Sprite2D), transform: Some((position: (400.0, 500.0, 0.0))),
            props: {"solid": Bool(true), "width": Num(800.0), "height": Num(40.0)}),
        (name: "Deco", kind: Some(Sprite2D), transform: Some((position: (520.0, 464.0, 0.0)))),
        (name: "Box", kind: Some(Object2D), children: [
            (name: "Wall", kind: Some(Sprite2D), transform: Some((position: (600.0, 400.0, 0.0))),
                props: {"solid": Bool(true), "width": Num(40.0), "height": Num(160.0)}),
        ]),
        (name: "Hero", kind: Some(Body2D), transform: Some((position: (500.0, 464.0, 0.0))),
            props: {"body": Text("kinematic")}, scripts: ["res://hero.jlb"]),
    ]))"#;
    let hero = "DEF on_step(dt) {\n    self.floor = MOVE(NULL, AXIS(\"a\", \"d\") * 300 * dt, 50 * dt)\n    self.wall_x = GET(\"Wall\", \"x\")\n    self.found = OBJECT(\"Wall\") !== NULL\n    self.both = AXIS(\"a\", \"a\")\n}\n";
    let p = project("solid", &[("main.jblueprint", blueprint), ("hero.jlb", hero)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    let d: HashSet<String> = ["d".to_string()].into();
    for _ in 0..120 {
        g.frame(1.0 / 60.0, d.clone(), [0.0; 2], HashSet::new());
    }
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    let h = t.find_path(None, "R/Hero").unwrap();
    let hx = t.objects[h].transform.position.x;
    // Walked right through the plain sprite, stopped by the solid one (left side 580, half width 16).
    assert!(hx < 566.0 && hx > 540.0, "hero x {}", hx);
    assert_eq!(t.objects[h].props.get("floor"), Some(&Variant::Bool(true)), "stands on the solid floor sprite");
    assert_eq!(t.objects[h].props.get("wall_x"), Some(&Variant::Num(600.0)), "GET finds Wall by its bare name");
    assert_eq!(t.objects[h].props.get("found"), Some(&Variant::Bool(true)));
    assert_eq!(t.objects[h].props.get("both"), Some(&Variant::Num(0.0)));
}

#[test]
fn import_and_load_work_on_every_object() {
    let blueprint = r#"(root: (name: "R", kind: Some(Object2D), children: [
        (name: "A", kind: Some(Sprite2D), scripts: ["res://user.jlb"]),
        (name: "B", kind: Some(Sprite2D), scripts: ["res://user.jlb"]),
    ]))"#;
    let helpers = "DEF double(n) { RETURN n * 2 }\n";
    let user = "IMPORT \"helpers.jlb\"\nLOAD \"helpers.jlb\"\nDEF on_create() {\n    self.a = double(21)\n    self.b = helpers.double(5)\n}\n";
    let p = project("import", &[("main.jblueprint", blueprint), ("helpers.jlb", helpers), ("user.jlb", user)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    for name in ["R/A", "R/B"] {
        let o = &t.objects[t.find_path(None, name).unwrap()];
        assert_eq!(o.props.get("a"), Some(&Variant::Num(42.0)), "{}", name);
        assert_eq!(o.props.get("b"), Some(&Variant::Num(10.0)), "{}", name);
    }
}

#[test]
fn physics_3d_ball_rests_on_ground() {
    let blueprint = r#"(root: (name: "R", kind: Some(Object3D), children: [
        (name: "Ground", kind: Some(Body3D), props: {"body": Text("static"), "width": Num(20.0), "height": Num(1.0), "depth": Num(20.0)}),
        (name: "Ball", kind: Some(Body3D), transform: Some((position: (0.0, 5.0, 0.0))), props: {"shape": Text("sphere")}),
    ]))"#;
    let p = project("phys3", &[("main.jblueprint", blueprint)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    for _ in 0..240 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let t = g.tree();
    let b = t.find_path(None, "R/Ball").unwrap();
    let y = t.objects[b].transform.position.y;
    assert!((y - 1.0).abs() < 0.05, "ball y {}", y);
    assert_eq!(t.objects[b].props.get("on_floor"), Some(&Variant::Bool(true)));
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
    let blueprint = r#"(root: (name: "R", kind: Some(Object), scripts: ["res://s.jlb"], children: [
        (name: "Beep", kind: Some(Sound), props: {"stream": Res("res://beep.wav"), "autoplay": Bool(true)}),
    ]))"#;
    let s = "DEF on_create() {\n    PLAY_SOUND(\"res://beep.wav\", 0.5)\n    PLAY(\"Beep\")\n    MUSIC(\"res://beep.wav\")\n    MUSIC(\"\")\n    TRY { PLAY_SOUND(\"res://bad.wav\") } FAIL e { PRINT(\"bad sound\") }\n}\n";
    let p = project("audio", &[("main.jblueprint", blueprint), ("s.jlb", s), ("bad.wav", "not a sound")]);
    // The WAV has bytes >= 0x80, so write it as raw bytes.
    std::fs::write(p.dir.join("beep.wav"), w.chars().map(|c| c as u8).collect::<Vec<u8>>()).unwrap();
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    let logs = g.take_logs();
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    assert!(logs.iter().any(|l| l.text == "bad sound"), "{:?}", logs.iter().map(|l| &l.text).collect::<Vec<_>>());
}

#[test]
fn tiles_animation_drawing_and_ui() {
    let blueprint = r#"(root: (name: "R", kind: Some(Object2D), scripts: ["res://r.jlb"], children: [
        (name: "Map", kind: Some(TileMap2D), transform: Some((position: (0.0, 200.0, 0.0))),
            props: {"tiles": Text("1,1,1,1,1,1,1,1,1,1")}),
        (name: "Ball", kind: Some(Body2D), transform: Some((position: (100.0, 0.0, 0.0))), props: {"shape": Text("circle")}),
        (name: "Hero", kind: Some(Sprite2D), props: {"hframes": Num(4.0), "animations": Text("idle=0; walk=1-3"), "animation": Text("walk"), "anim_fps": Num(10.0)}),
        (name: "Go", kind: Some(Button), scripts: ["res://go.jlb"]),
    ]))"#;
    let r = "pressed = 0\nDEF on_step(dt) {\n    DRAW_RECT(0, 0, 10, 10, \"#ff0000\")\n    DRAW_CIRCLE(5, 5, 3, [r: 0, g: 1, b: 0, a: 1], FALSE)\n    DRAW_TEXT(1, 2, \"hi\", 12)\n}\nDEF on_pressed_go() {\n    GLOBAL pressed += 1\n    self.pressed = pressed\n    SET_TILE(\"Map\", 0, 0, -1)\n}\n";
    let go = "DEF on_pressed() {\n    self.text = \"pressed!\"\n}\n";
    let p = project("twod", &[("main.jblueprint", blueprint), ("r.jlb", r), ("go.jlb", go)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
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
    assert!((t.objects[ball].transform.position.y - 184.0).abs() < 2.0, "ball y {}", t.objects[ball].transform.position.y);
    let hero = t.find_path(None, "R/Hero").unwrap();
    let f = t.objects[hero].num("frame", -1.0);
    assert!((1.0..=3.0).contains(&f), "frame {}", f);
    assert_eq!(t.objects[go_id].text("text"), "pressed!");
    let root = t.root.unwrap();
    assert_eq!(t.objects[root].props.get("pressed"), Some(&Variant::Num(1.0)));
    let map = t.find_path(None, "R/Map").unwrap();
    assert!(t.objects[map].text("tiles").starts_with("-1,1"));
    // 9 tiles + hero + the drawn rect and filled... and the outline circle as lines.
    assert!(list.sprites.iter().any(|s| s.layer == 1000.0));
    assert!(list.lines_2d.len() >= 32);
    assert!(list.labels.iter().any(|l| l.text == "hi"));
    assert_eq!(list.ui.len(), 1);
}

#[test]
fn model_animation_and_mesh_collision() {
    let blueprint = r#"(root: (name: "R", kind: Some(Object3D), scripts: ["res://r.jlb"], children: [
        (name: "Guy", kind: Some(Mesh3D), props: {"mesh": Text("res://guy.glb"), "anim_loop": Bool(false)}),
        (name: "Floor", kind: Some(Mesh3D), props: {"mesh": Text("plane"), "collision": Bool(true)}, transform: Some((scale: (10.0, 1.0, 10.0)))),
        (name: "Ball", kind: Some(Body3D), transform: Some((position: (0.0, 3.0, 0.0))), props: {"shape": Text("sphere")}),
    ]))"#;
    let r = "ended = \"\"\nDEF on_create() {\n    names = ANIMATIONS(\"Guy\")\n    self.count = LEN(names)\n    ANIMATE(\"Guy\", names[1])\n}\n";
    let guy = "";
    let p = project("model", &[("main.jblueprint", blueprint), ("r.jlb", r), ("guy.jlb", guy)]);
    std::fs::write(p.dir.join("guy.glb"), gex_assets::model::tiny_skinned_glb()).unwrap();
    std::fs::write(p.dir.join("end.jlb"), "DEF on_animation_end(name) {\n    self.done = name\n}\n").unwrap();
    // Attach the end-of-animation handler to Guy.
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    {
        let mut st = g.st.borrow_mut();
        let id = st.tree.find_path(None, "R/Guy").unwrap();
        st.tree.objects[id].scripts.push("res://end.jlb".into());
    }
    for _ in 0..150 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    let root = t.root.unwrap();
    assert_eq!(t.objects[root].props.get("count"), Some(&Variant::Num(1.0)));
    let guy = t.find_path(None, "R/Guy").unwrap();
    assert_eq!(t.objects[guy].props.get("anim_time"), Some(&Variant::Num(1.0)), "stopped at the end");
    assert_eq!(t.objects[guy].props.get("done"), Some(&Variant::Text("lift".into())));
    let ball = t.find_path(None, "R/Ball").unwrap();
    let y = t.objects[ball].transform.position.y;
    assert!((y - 0.5).abs() < 0.05, "the ball rests on the collision plane: {}", y);
}

#[test]
fn breakpoints_pause_and_scripts_reload() {
    use crate::thread::{FromGame, GameHandle};
    let blueprint = r#"(root: (name: "R", kind: Some(Object2D), scripts: ["res://s.jlb"]))"#;
    let p = project("debug", &[("main.jblueprint", blueprint), ("s.jlb", "count = 0\nDEF on_step(dt) {\n    LOCAL x = 41\n    GLOBAL count += 1\n}\n")]);
    let text = p.read_text("res://main.jblueprint").unwrap();
    let tree = gex_core::blueprint::load(&text, &|q: &str| p.read_text(q)).unwrap();
    let mut h = GameHandle::start(p.clone(), tree.clone());
    let file = p.res_to_path("res://s.jlb").unwrap().canonicalize().unwrap();
    h.breakpoints.set(file.clone(), [3, 4].into());
    h.breakpoints.set_condition(file, 3, "count > 5");
    h.breakpoints.set_watches(vec!["x * 2".into(), "count +".into()]);
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
    assert_eq!(p2.watches[0], ("x * 2".to_string(), "82".to_string()), "{:?}", p2.watches);
    assert!(p2.watches[1].1.starts_with('⚠'), "a watch that can't be worked out says why: {:?}", p2.watches);
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
    assert_eq!(t.objects[t.root.unwrap()].props.get("seen"), Some(&Variant::Num(13.0)));
}

#[test]
fn paths_and_saves() {
    // A wall with one gap: the path must go through it.
    let tiles = "-1,-1,-1,-1,-1\n-1,-1,1,-1,-1\n-1,-1,1,-1,-1\n-1,-1,-1,-1,-1";
    let blueprint = format!(
        r#"(root: (name: "R", kind: Some(Object2D), scripts: ["res://r.jlb"], children: [
        (name: "Map", kind: Some(TileMap2D), props: {{"tiles": Text("{}")}}),
    ]))"#,
        tiles.replace('\n', "\\n")
    );
    let r = "DEF on_create() {\n    p = FIND_PATH(\"Map\", 16, 48, 144, 48)\n    self.steps = LEN(p)\n    self.blocked = FIND_PATH(\"Map\", 16, 48, 80, 48)\n    SAVE_DATA(\"slot_1\", [score: 7, names: [\"a\", \"b\"], done: TRUE])\n    back = LOAD_DATA(\"slot_1\")\n    self.score = back.score\n    self.second = back.names[2]\n    self.missing = LOAD_DATA(\"nothing_here\")\n    DELETE_DATA(\"slot_1\")\n}\n";
    let p = project("paths", &[("main.jblueprint", &blueprint), ("r.jlb", r)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    g.frame(0.016, HashSet::new(), [0.0; 2], HashSet::new());
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let t = g.tree();
    let root = &t.objects[t.root.unwrap()];
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
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), scripts: ["res://boom.jlb"], children: [
        (name: "Smoke", kind: Some(Particles2D), transform: Some((position: (100.0, 100.0, 0.0))),
            props: {"rate": Num(60.0), "lifetime": Num(0.5), "gravity": Num(200.0)}),
        (name: "Boom", kind: Some(Particles2D), props: {"emitting": Bool(false), "speed": Num(0.0)}),
    ]))"#;
    let script = "DEF on_press_space() {\n    BURST(\"Boom\", 50)\n}\n";
    let p = project("particles", &[("main.jblueprint", blueprint), ("boom.jlb", script)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
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
    assert!(list.sprites.iter().filter(|s| s.object == boom).count() == 50);
    // Smoke fades towards color_end as it ages.
    assert!(list.sprites.iter().filter(|s| s.object == smoke).any(|s| s.color[3] < 0.7));
    // Everything is gone a lifetime after the burst.
    for _ in 0..80 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert_eq!(count(&g, boom), 0);
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:?}", errs);
}

#[test]
fn camera_follow_limits_shake_and_fades() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), scripts: ["res://go.jlb"], children: [
        (name: "Hero", kind: Some(Object2D), transform: Some((position: (100.0, 100.0, 0.0))), children: [
            (name: "Cam", kind: Some(Camera2D), props: {"smoothing": Num(5.0), "limits": Bool(true), "limit_right": Num(2000.0)}),
        ]),
    ]))"#;
    let other = r#"(root: (name: "Other", kind: Some(Object2D), scripts: ["res://other.jlb"]))"#;
    let go = "DEF on_create() {\n    REMEMBER(\"lives\", 3)\n}\nDEF on_press_space() {\n    CHANGE_BLUEPRINT(\"res://other.jblueprint\", 1)\n}\nDEF on_press_s() {\n    SHAKE(10, 0.5)\n}\n";
    let oth = "DEF on_create() {\n    PRINT(\"lives \" + RECALL(\"lives\"))\n}\n";
    let p = project("camera", &[("main.jblueprint", blueprint), ("other.jblueprint", other), ("go.jlb", go), ("other.jlb", oth)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    // The hero is near the top-left corner: the limits keep the view inside (0,0)-(2000,720).
    let c = g.render_list().camera2d.unwrap().center;
    assert_eq!(c, gex_core::glam::Vec2::new(640.0, 360.0));
    // Smoothing: after a jump right, the camera eases over.
    g.st.borrow_mut().tree.objects.iter_mut().find(|(_, n)| n.name == "Hero").unwrap().1.transform.position.x = 1200.0;
    g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    let c1 = g.render_list().camera2d.unwrap().center.x;
    assert!(c1 > 640.0 && c1 < 700.0, "eases: {}", c1);
    for _ in 0..120 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert!((g.render_list().camera2d.unwrap().center.x - 1200.0).abs() < 1.0);
    g.frame(1.0 / 60.0, ["s".to_string()].into(), [0.0; 2], HashSet::new());
    assert!((g.render_list().camera2d.unwrap().center.x - 1200.0).abs() > 0.01, "shaking");
    for _ in 0..40 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert!((g.render_list().camera2d.unwrap().center.x - 1200.0).abs() < 1.0, "the shake died away");
    // A fading blueprint change: half the time fading out, then the change.
    g.frame(1.0 / 60.0, ["space".to_string()].into(), [0.0; 2], HashSet::new());
    for _ in 0..10 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let f = g.render_list().fade;
    assert!(f[3] > 0.1 && f[3] < 0.9, "{:?}", f);
    assert!(g.st.borrow().change_blueprint.is_none());
    for _ in 0..25 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        if g.st.borrow().change_blueprint.is_some() {
            break;
        }
    }
    assert_eq!(g.st.borrow().change_blueprint.as_deref(), Some("res://other.jblueprint"));
}

#[test]
fn blueprint_changes_keep_remembered_values_and_fade_in() {
    let a = r#"(root: (name: "A", kind: Some(Object2D), scripts: ["res://a.jlb"]))"#;
    let b = r#"(root: (name: "B", kind: Some(Object2D), scripts: ["res://b.jlb"]))"#;
    let sa = "DEF on_create() {\n    REMEMBER(\"inv\", [\"sword\", gold: 5])\n    CHANGE_BLUEPRINT(\"res://b.jblueprint\", 0.2)\n}\n";
    let sb = "DEF on_create() {\n    inv = RECALL(\"inv\")\n    PRINT(inv[1] + \" \" + inv.gold)\n}\n";
    let p = project("remember", &[("a.jblueprint", a), ("b.jblueprint", b), ("a.jlb", sa), ("b.jlb", sb)]);
    let text = p.read_text("res://a.jblueprint").unwrap();
    let tree = gex_core::blueprint::load(&text, &|r: &str| p.read_text(r)).unwrap();
    let mut h = crate::thread::GameHandle::start(p, tree);
    let mut seen = String::new();
    let mut faded_in = false;
    for _ in 0..200 {
        h.request_frame(crate::FrameInput { dt: 1.0 / 60.0, ..Default::default() });
        std::thread::sleep(std::time::Duration::from_millis(2));
        for m in h.poll() {
            if let crate::thread::FromGame::Frame { logs, list, .. } = m {
                for l in logs {
                    seen.push_str(&l.text);
                }
                if seen.contains("sword") && list.fade[3] > 0.0 && list.fade[3] < 1.0 {
                    faded_in = true;
                }
            }
        }
        if faded_in {
            break;
        }
    }
    assert!(seen.contains("sword 5"), "{}", seen);
    assert!(faded_in);
}

/// Every engine command is in the scripting guide.
#[test]
fn every_command_is_documented() {
    let doc = include_str!("../../../docs/SCRIPTING.md");
    let missing: Vec<&str> = crate::commands::COMMANDS.iter().map(|c| c.0).filter(|n| !doc.contains(&format!("`{}(", n))).collect();
    assert!(missing.is_empty(), "add these to docs/SCRIPTING.md: {:?}", missing);
}

#[test]
fn tweens_move_props_and_report_the_end() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), scripts: ["res://t.jlb"], children: [
        (name: "Box", kind: Some(Sprite2D)),
    ]))"#;
    let script = "DEF on_create() {\n    TWEEN(\"Box\", \"x\", 100, 1, \"linear\")\n    TWEEN(\"Box\", \"color\", \"#ff0000\", 0.5)\n    TWEEN(NULL, \"x\", 50, 0.25)\n}\nDEF on_tween_end(p) {\n    PRINT(\"done \" + p)\n}\n";
    let p = project("tween", &[("main.jblueprint", blueprint), ("t.jlb", script)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    for _ in 0..30 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let bx = g.tree().find_path(None, "Root/Box").unwrap();
    let x = g.tree().objects[bx].transform.position.x;
    assert!((x - 50.0).abs() < 2.5, "halfway along a linear tween: {}", x);
    for _ in 0..40 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let t = g.tree();
    assert_eq!(t.objects[bx].transform.position.x, 100.0);
    assert_eq!(t.objects[bx].color("color"), [1.0, 0.0, 0.0, 1.0]);
    drop(t);
    let logs: Vec<String> = g.take_logs().into_iter().map(|l| l.text).collect();
    assert!(logs.iter().any(|l| l == "done x"), "the root's own tween ended: {:?}", logs);
    assert_eq!(logs.iter().filter(|l| l.starts_with("done")).count(), 1, "on_tween_end runs on the tweened object only");
}

/// The scripts from docs/TUTORIAL-1-BASICS.md, as written there.
#[test]
fn tutorial_one_works() {
    let blueprint = r#"(root: (name: "Main", kind: Some(Object2D), scripts: ["res://scripts/main.jlb"], children: [
        (name: "Player", kind: Some(Sprite2D), transform: Some((position: (640.0, 360.0, 0.0))), props: {"width": Num(40.0), "height": Num(40.0)},
            scripts: ["res://scripts/player.jlb"], children: [(name: "Collider2D", kind: Some(Collider2D), props: {"width": Num(40.0), "height": Num(40.0)})]),
        (name: "Coin", kind: Some(Sprite2D), transform: Some((position: (640.0, 250.0, 0.0))), props: {"width": Num(20.0), "height": Num(20.0)},
            children: [(name: "Collider2D", kind: Some(Collider2D), props: {"width": Num(20.0), "height": Num(20.0)})]),
        (name: "Score", kind: Some(Label), transform: Some((position: (20.0, 16.0, 0.0))), props: {"text": Text("Score: 0")}),
    ]))"#;
    let coin = r#"(root: (name: "Coin", kind: Some(Sprite2D), children: [(name: "Collider2D", kind: Some(Collider2D))]))"#;
    let player = "speed = 300\n\nDEF on_step(dt) {\n\tIF KEY_DOWN(\"left\") OR KEY_DOWN(\"a\") { self.x -= speed * dt }\n\tIF KEY_DOWN(\"right\") OR KEY_DOWN(\"d\") { self.x += speed * dt }\n\tIF KEY_DOWN(\"up\") OR KEY_DOWN(\"w\") { self.y -= speed * dt }\n\tIF KEY_DOWN(\"down\") OR KEY_DOWN(\"s\") { self.y += speed * dt }\n}\n\nDEF on_collide_start(other) {\n\tDESTROY(other)\n\tEMIT(\"coin_taken\")\n}\n";
    let main = "score = 0\n\nDEF on_coin_taken() {\n\tGLOBAL score += 1\n\tSET(\"Score\", \"text\", \"Score: \" + score)\n}\n\nDEF on_create() {\n\tTIMER(\"spawn\", 2, TRUE)\n}\n\nDEF on_timer_spawn() {\n\tSPAWN(\"res://blueprints/coin.jblueprint\", RANDOM(40, 1240), RANDOM(80, 680))\n}\n";
    let p = project("tutorial1", &[("blueprints/main.jblueprint", blueprint), ("blueprints/coin.jblueprint", coin), ("scripts/player.jlb", player), ("scripts/main.jlb", main)]);
    let mut g = Game::from_blueprint(p, "res://blueprints/main.jblueprint", None).unwrap();
    for _ in 0..30 {
        g.frame(1.0 / 60.0, ["up".to_string()].into(), [0.0; 2], HashSet::new());
    }
    for _ in 0..100 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let t = g.tree();
    let score = t.find_path(None, "Main/Score").unwrap();
    assert_eq!(t.objects[score].text("text"), "Score: 1");
    let coins: Vec<_> = t.walk().into_iter().filter(|i| t.objects[*i].name == "Coin").collect();
    assert_eq!(coins.len(), 1, "the first coin is gone and the timer spawned one");
    assert_ne!(t.objects[coins[0]].transform.position.y, 250.0);
    drop(t);
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:?}", errs);
}

/// Scripts from docs/TUTORIAL-2-ADVANCED.md: a chasing enemy, sprite animation, a condition event.
#[test]
fn tutorial_two_snippets_work() {
    let row = |s: &str| s.to_string();
    let mut grid = vec![row("-1,-1,-1,-1,-1,-1,-1,-1,-1,-1"); 8];
    grid[3] = row("-1,-1,-1,-1,0,-1,-1,-1,-1,-1");
    grid[4] = row("-1,-1,-1,-1,0,-1,-1,-1,-1,-1");
    grid[5] = row("-1,-1,-1,-1,0,-1,-1,-1,-1,-1");
    let blueprint = format!(r#"(root: (name: "Level", kind: Some(Object2D), children: [
        (name: "Ground", kind: Some(TileMap2D), props: {{"tiles": Text("{}")}}),
        (name: "Player", kind: Some(Sprite2D), transform: Some((position: (272.0, 144.0, 0.0)))),
        (name: "Enemy", kind: Some(Sprite2D), transform: Some((position: (48.0, 144.0, 0.0))),
            props: {{"hframes": Num(9.0), "animations": Text("idle=0; run=1-6; jump=7,8"), "hp": Num(3.0)}},
            scripts: ["res://enemy.jlb", "res://anim.jlb", "res://dead.jlb"]),
    ]))"#, grid.join("\\n"));
    let enemy = "speed = 120\npath = NULL\nstep = 1\n\nDEF on_create() {\n\tTIMER(\"think\", 0.5, TRUE)\n}\n\nDEF on_timer_think() {\n\tGLOBAL path = FIND_PATH(\"../Ground\", self.x, self.y, GET(\"../Player\", \"x\"), GET(\"../Player\", \"y\"))\n\tGLOBAL step = 2\n}\n\nDEF on_step(dt) {\n\tIF path == NULL OR step > LEN(path) { RETURN }\n\ttarget = path[step]\n\tdx = target.x - self.x\n\tdy = target.y - self.y\n\td = (dx * dx + dy * dy) !^ 2\n\tIF d < 4 {\n\t\tGLOBAL step += 1\n\t} ELSE {\n\t\tself.x += dx / d * speed * dt\n\t\tself.y += dy / d * speed * dt\n\t}\n}\n";
    let anim = "last_x = self.x\n\nDEF on_step(dt) {\n\tdx = self.x - last_x\n\tGLOBAL last_x = self.x\n\tIF dx !== 0 {\n\t\tANIMATE(NULL, \"run\")\n\t\tself.flip_h = dx < 0\n\t} ELSE {\n\t\tANIMATE(NULL, \"idle\")\n\t}\n}\n";
    let dead = "DEF when_dead() {\n\tRETURN self.hp <= 0\n}\n\nDEF on_dead() {\n\tPRINT(\"down\")\n}\n";
    let p = project("tutorial2", &[("main.jblueprint", &blueprint), ("enemy.jlb", enemy), ("anim.jlb", anim), ("dead.jlb", dead)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    let mut frames_seen = HashSet::new();
    for _ in 0..240 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        let t = g.tree();
        let e = t.find_path(None, "Level/Enemy").unwrap();
        frames_seen.insert(t.objects[e].num("frame", 0.0) as i64);
    }
    let t = g.tree();
    let e = t.find_path(None, "Level/Enemy").unwrap();
    let pos = t.objects[e].transform.position;
    assert!((pos.x - 272.0).abs() < 20.0 && (pos.y - 144.0).abs() < 20.0, "went around the wall to the player: {}", pos);
    assert!(frames_seen.len() >= 4, "the run animation cycled: {:?}", frames_seen);
    drop(t);
    g.st.borrow_mut().tree.objects.iter_mut().find(|(_, n)| n.name == "Enemy").unwrap().1.props.insert("hp".into(), Variant::Num(0.0));
    g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    let logs: Vec<_> = g.take_logs();
    assert!(logs.iter().all(|l| l.kind != LogKind::Error), "{:?}", logs);
    assert!(logs.iter().any(|l| l.text == "down"));
}

#[test]
fn pause_and_time_scale() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), children: [
        (name: "Mover", kind: Some(Sprite2D), scripts: ["res://mover.jlb"]),
        (name: "Menu", kind: Some(Object), groups: ["while_paused"], scripts: ["res://menu.jlb"]),
    ]))"#;
    let mover = "ticks = 0\nDEF on_create() {\n\tTIMER(\"t\", 0.5, TRUE)\n\tTWEEN(NULL, \"y\", 100, 1, \"linear\")\n}\nDEF on_step(dt) {\n\tself.x += 60 * dt\n}\nDEF on_timer_t() {\n\tGLOBAL ticks += 1\n\tself.ticks = ticks\n}\n";
    let menu = "DEF on_press_p() {\n\tPAUSE(!PAUSED())\n}\nDEF on_press_s() {\n\tTIME_SCALE(0.5)\n}\nDEF on_step(dt) {\n\tself.alive = GAME_TIME()\n}\n";
    let p = project("pause", &[("main.jblueprint", blueprint), ("mover.jlb", mover), ("menu.jlb", menu)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    let f = |g: &mut Game, k: &[&str]| g.frame(0.1, k.iter().map(|x| x.to_string()).collect(), [0.0; 2], HashSet::new());
    for _ in 0..6 {
        f(&mut g, &[]);
    }
    let x_of = |g: &Game| {
        let t = g.tree();
        let m = t.find_path(None, "Root/Mover").unwrap();
        (t.objects[m].transform.position.x, t.objects[m].transform.position.y, t.objects[m].num("ticks", 0.0))
    };
    let (x0, y0, t0) = x_of(&g);
    assert!((x0 - 36.0).abs() < 0.01 && t0 == 1.0, "{} {}", x0, t0);
    f(&mut g, &["p"]);
    for _ in 0..10 {
        f(&mut g, &[]);
    }
    let (x1, y1, t1) = x_of(&g);
    assert_eq!((x1, y1, t1), (x0, y0, t0), "frozen while paused");
    let menu_object = g.tree().find_path(None, "Root/Menu").unwrap();
    assert!(g.tree().objects[menu_object].prop("alive").is_some(), "the menu still runs");
    f(&mut g, &["p"]);
    f(&mut g, &[]);
    let (x2, _, _) = x_of(&g);
    assert!((x2 - x0 - 12.0).abs() < 0.01, "moving again (the unpause frame and the next): {}", x2);
    f(&mut g, &["s"]);
    let (x3, _, _) = x_of(&g);
    f(&mut g, &[]);
    let (x4, _, _) = x_of(&g);
    assert!((x4 - x3 - 3.0).abs() < 0.01, "half speed: {}", x4 - x3);
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:?}", errs);
}

#[test]
fn tasks_wait_in_game_time() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), children: [
        (name: "Actor", kind: Some(Sprite2D), scripts: ["res://actor.jlb"]),
        (name: "Doomed", kind: Some(Object), scripts: ["res://doomed.jlb"]),
    ]))"#;
    let actor = "DEF on_create() {\n\tSTART intro()\n}\n\nDEF intro() {\n\tself.x = 10\n\tWAIT(1)\n\tself.x = 20\n\tSET(NULL, \"step\", 2)\n\tWAIT(1)\n\tTWEEN(NULL, \"x\", 100, 0.5)\n\tPRINT(\"intro done\")\n}\n\nDEF on_press_w() {\n\tWAIT(1)\n}\n";
    let doomed = "DEF on_create() {\n\tSTART forever()\n}\nDEF forever() {\n\tn = 0\n\tWHILE TRUE {\n\t\tn += 1\n\t\tWAIT(0.1)\n\t}\n}\n";
    let p = project("tasks", &[("main.jblueprint", blueprint), ("actor.jlb", actor), ("doomed.jlb", doomed)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    let x = |g: &Game| {
        let t = g.tree();
        let a = t.find_path(None, "Root/Actor").unwrap();
        (t.objects[a].transform.position.x, t.objects[a].num("step", 0.0))
    };
    g.frame(0.1, HashSet::new(), [0.0; 2], HashSet::new());
    assert_eq!(x(&g), (10.0, 0.0));
    for _ in 0..11 {
        g.frame(0.1, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert_eq!(x(&g), (20.0, 2.0), "a second of game time later: {:?}", g.take_logs());
    // A paused game holds the task.
    g.st.borrow_mut().paused = true;
    for _ in 0..20 {
        g.frame(0.1, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert_eq!(x(&g).0, 20.0);
    g.st.borrow_mut().paused = false;
    for _ in 0..20 {
        g.frame(0.1, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert_eq!(x(&g).0, 100.0);
    // Destroying an object stops its tasks.
    let d = g.tree().find_path(None, "Root/Doomed").unwrap();
    g.st.borrow_mut().queue.push_back(crate::world::Pending::Destroy(d));
    g.frame(0.1, HashSet::new(), [0.0; 2], HashSet::new());
    g.frame(0.1, HashSet::new(), [0.0; 2], HashSet::new());
    assert!(g.it.live_task_tags().is_empty(), "{:?}", g.it.live_task_tags());
    // WAIT straight in an event explains what to do.
    g.frame(0.1, ["w".to_string()].into(), [0.0; 2], HashSet::new());
    let logs = g.take_logs();
    assert!(logs.iter().any(|l| l.text == "intro done"));
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert_eq!(errs.len(), 1, "{:?}", errs);
    assert!(errs[0].contains("START"), "{}", errs[0]);
}

#[test]
fn collision_layers_and_3d_areas() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object3D), children: [
        (name: "Floor", kind: Some(Body3D), props: {"body": Text("static"), "width": Num(20.0), "height": Num(1.0), "depth": Num(20.0)}),
        (name: "Ghost", kind: Some(Body3D), transform: Some((position: (0.0, 3.0, 0.0))), props: {"layer": Text("ghosts"), "mask": Text("ghosts")}),
        (name: "Crate", kind: Some(Body3D), transform: Some((position: (3.0, 3.0, 0.0)))),
        (name: "Zone", kind: Some(Collider3D), transform: Some((position: (3.0, 0.9, 0.0))), props: {"width": Num(2.0), "height": Num(0.5), "depth": Num(2.0)}, scripts: ["res://zone.jlb"]),
        (name: "Picky", kind: Some(Collider3D), transform: Some((position: (3.0, 0.9, 0.0))), props: {"shape": Text("sphere"), "radius": Num(1.0), "mask": Text("ghosts")}, scripts: ["res://picky.jlb"]),
    ]))"#;
    let zone = "DEF on_collide_start(other) {\n\tPRINT(\"zone \" + NAME_OF(other))\n}\n";
    let picky = "DEF on_collide_start(other) {\n\tPRINT(\"picky \" + NAME_OF(other))\n}\n";
    let p = project("layers", &[("main.jblueprint", blueprint), ("zone.jlb", zone), ("picky.jlb", picky)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    for _ in 0..180 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let t = g.tree();
    let y = |n: &str| t.objects[t.find_path(None, &format!("Root/{}", n)).unwrap()].transform.position.y;
    assert!(y("Ghost") < -5.0, "the ghost falls through the floor: {}", y("Ghost"));
    assert!((y("Crate") - 1.0).abs() < 0.1, "the crate lands: {}", y("Crate"));
    drop(t);
    let logs: Vec<String> = g.take_logs().into_iter().map(|l| l.text).collect();
    assert!(logs.contains(&"zone Crate".to_string()), "{:?}", logs);
    assert!(!logs.iter().any(|l| l.starts_with("picky")), "Picky only notices ghosts: {:?}", logs);
    // Raycasts can be limited to layers.
    let st = g.st.borrow();
    let all = st.physics.raycast_3d(glam::Vec3::new(3.0, 10.0, 0.0), glam::Vec3::NEG_Y, 100.0, None, u32::MAX).unwrap();
    assert_eq!(st.tree.objects[all.object].name, "Crate");
    let ghosts = st.physics.layers.bits("ghosts", 0);
    assert!(st.physics.raycast_3d(glam::Vec3::new(3.0, 10.0, 0.0), glam::Vec3::NEG_Y, 100.0, None, ghosts).is_none());
}

/// With a real map: `GEX_TEST_BSP=/folder/with/map.bsp cargo test -p gex-script real_map -- --ignored`.
#[test]
#[ignore]
fn real_map_entities_and_props() {
    let Ok(file) = std::env::var("GEX_TEST_BSP") else { return };
    let path = std::path::Path::new(&file);
    let dir = path.parent().unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    let blueprint = format!(r#"(root: (name: "Root", kind: Some(Object3D), scripts: ["res://probe.jlb"], children: [
        (name: "Map", kind: Some(Mesh3D), props: {{"mesh": Text("res://{}"), "collision": Bool(true)}}),
    ]))"#, name);
    std::fs::write(dir.join("main.jblueprint"), blueprint).unwrap();
    std::fs::write(dir.join("probe.jlb"), "DEF on_create() {\n\tall = MAP_ENTITIES(\"Map\")\n\tspawns = MAP_ENTITIES(\"Map\", \"info_player*\")\n\tPRINT(LEN(all) + \" entities, \" + LEN(spawns) + \" spawns\")\n\ts = spawns[1]\n\tPRINT(s.classname + \" at \" + ROUND(s.x, 2) + \" \" + ROUND(s.y, 2) + \" \" + ROUND(s.z, 2) + \" facing \" + ROUND(s.rotation_y))\n}\n").unwrap();
    std::fs::write(dir.join("project.jproj"), "(name: \"maptest\")").unwrap();
    let p = gex_core::project::Project::open(dir).unwrap();
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    for _ in 0..3 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    for l in g.take_logs() {
        println!("{:?}: {}", l.kind, l.text);
    }
    let props = g.tree().in_group("map_props").len();
    println!("{} physics props", props);
    let doors = g.tree().in_group("map_doors");
    println!("{} doors", doors.len());
    if let Some(d) = doors.first().copied() {
        let before = g.tree().objects[d].transform;
        let root = g.tree().root.unwrap();
        std::fs::write(dir.join("probe.jlb"), "DEF on_press_o() {\n\tUSE_DOOR(OBJECTS(\"map_doors\")[1])\n}\n").unwrap();
        g.reload_script("res://probe.jlb");
        let _ = root;
        g.frame(1.0 / 60.0, ["o".to_string()].into(), [0.0; 2], HashSet::new());
        for _ in 0..240 {
            g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        }
        let after = g.tree().objects[d].transform;
        println!("door {} moved {:?} -> {:?}", g.tree().objects[d].name, before.position, after.position);
        for l in g.take_logs() {
            println!("{:?}: {}", l.kind, l.text);
        }
    }
    let map = g.tree().find_path(None, "Root/Map").unwrap();
    println!("map mesh while playing: {}", g.tree().objects[map].text("mesh"));
}

/// With a skinned model: `GEX_TEST_SKINNED=/folder/Fox.glb cargo test -p gex-script ragdoll -- --ignored`.
#[test]
#[ignore]
fn ragdoll_falls() {
    let Ok(file) = std::env::var("GEX_TEST_SKINNED") else { return };
    let path = std::path::Path::new(&file);
    let dir = path.parent().unwrap();
    let name = path.file_name().unwrap().to_string_lossy();
    let blueprint = format!(r#"(root: (name: "Root", kind: Some(Object3D), children: [
        (name: "Floor", kind: Some(Mesh3D), props: {{"mesh": Text("plane"), "collision": Bool(true)}}, transform: Some((scale: (40.0, 1.0, 40.0)))),
        (name: "Guy", kind: Some(Mesh3D), transform: Some((position: (0.0, 1.0, 0.0), scale: (0.02, 0.02, 0.02))), props: {{"mesh": Text("res://{}")}}, scripts: ["res://drop.jlb"]),
    ]))"#, name);
    std::fs::write(dir.join("main.jblueprint"), blueprint).unwrap();
    std::fs::write(dir.join("drop.jlb"), "DEF on_create() {\n\tRAGDOLL(NULL, 0, 0, 2)\n}\n").unwrap();
    std::fs::write(dir.join("project.jproj"), "(name: \"ragdolltest\")").unwrap();
    let p = gex_core::project::Project::open(dir).unwrap();
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    let first = g.render_list().meshes.iter().find_map(|m| m.pose.clone()).expect("posed by the ragdoll");
    for _ in 0..180 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let last = g.render_list().meshes.iter().find_map(|m| m.pose.clone()).unwrap();
    assert!(last.iter().all(|m| m.is_finite()));
    let moved = first.iter().zip(last.iter()).map(|(a, b)| a.w_axis.distance(b.w_axis)).fold(0.0f32, f32::max);
    println!("bones: {}, most moved: {}", last.len(), moved);
    assert!(moved > 0.1);
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:?}", errs);
}

#[test]
fn animator_clips_play_and_end() {
    use gex_core::anim::{AnimFile, Clip, Key, Track};
    let k = |t: f32, v: f64| Key { t, value: Variant::Num(v), ease: "linear".into() };
    let file = AnimFile {
        clips: vec![
            Clip { name: "slide".into(), length: 1.0, looped: false, tracks: vec![Track { target: "Box".into(), prop: "x".into(), keys: vec![k(0.0, 0.0), k(1.0, 100.0)] }] },
            Clip { name: "spin".into(), length: 0.5, looped: true, tracks: vec![Track { target: String::new(), prop: "rotation".into(), keys: vec![k(0.0, 0.0), k(0.5, 90.0)] }] },
        ],
    };
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), scripts: ["res://a.jlb"], children: [
        (name: "Box", kind: Some(Sprite2D)),
        (name: "Anim", kind: Some(Animator), props: {"clips": Res("res://a.anim"), "autoplay": Text("slide")}),
    ]))"#;
    let script = "DEF on_clip_end(c) {\n    PRINT(\"ended \" + c)\n    PLAY_CLIP(\"Anim\", \"spin\")\n}\n";
    let text = file.to_text();
    let p = project("animator", &[("main.jblueprint", blueprint), ("a.jlb", script), ("a.anim", &text)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    for _ in 0..30 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let bx = g.tree().find_path(None, "Root/Box").unwrap();
    let x = g.tree().objects[bx].transform.position.x;
    assert!((x - 50.0).abs() < 3.0, "autoplayed halfway: {}", x);
    for _ in 0..45 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    assert_eq!(g.tree().objects[bx].transform.position.x, 100.0);
    let logs: Vec<String> = g.take_logs().into_iter().map(|l| l.text).collect();
    assert!(logs.iter().any(|l| l == "ended slide"), "{:?}", logs);
    assert!(!logs.iter().any(|l| l == "ended spin"), "looping clips don't end");
    let root = g.tree().find_path(None, "Root").unwrap();
    let r = g.tree().objects[root].transform.rotation;
    assert!(r.z.abs() > 1.0, "the spin clip turns the Animator's parent");
}

#[test]
fn colors_from_text_and_lists() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), scripts: ["res://a.jlb"], children: [
        (name: "A", kind: Some(Sprite2D)), (name: "B", kind: Some(Sprite2D)), (name: "C", kind: Some(Sprite2D)),
    ]))"#;
    let script = "DEF on_create() {\n    SET(\"A\", \"color\", \"#ff0000\")\n    SET(\"B\", \"color\", [0, 1, 0, 0.5])\n    SET(\"C\", \"color\", \"blue\")\n}\n";
    let p = project("colors", &[("main.jblueprint", blueprint), ("a.jlb", script)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    let t = g.tree();
    let c = |n: &str| t.objects[t.find_path(None, &format!("Root/{}", n)).unwrap()].color("color");
    assert_eq!(c("A"), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(c("B"), [0.0, 1.0, 0.0, 0.5]);
    assert!(c("C")[2] > 0.9 && c("C")[0] < 0.3);
}

#[test]
fn settings_change_live_from_scripts() {
    // Five crates side by side, each with a different body setting; gravity turned up from a script.
    let blueprint = r#"(root: (name: "R", kind: Some(Object2D), scripts: ["res://main.jlb"], children: [
        (name: "Plain", kind: Some(Body2D), transform: Some((position: (0.0, 0.0, 0.0)))),
        (name: "Off", kind: Some(Body2D), transform: Some((position: (100.0, 0.0, 0.0))), props: {"enabled": Bool(false)}),
        (name: "Rail", kind: Some(Body2D), transform: Some((position: (200.0, 0.0, 0.0))), props: {"lock_y": Bool(true), "vx": Num(60.0)}),
        (name: "Thick", kind: Some(Body2D), transform: Some((position: (300.0, 0.0, 0.0))), props: {"linear_damping": Num(8.0)}),
        (name: "Spinner", kind: Some(Body2D), transform: Some((position: (400.0, 0.0, 0.0))), props: {"gravity_scale": Num(0.0), "spin": Num(90.0)}),
    ]))"#;
    let main = r#"
DEF on_create() {
    SET_PHYSICS("gravity_2d", [0, 2000])
    SET_PHYSICS("tick_rate", 120)
    SET_GRAPHICS("bloom", FALSE)
    SET_GRAPHICS("tonemap", "aces")
    self.gy = PHYSICS("gravity_2d").y
    self.shadows = GRAPHICS().shadows
}
DEF on_step(dt) {
    self.tm = GRAPHICS("tonemap")
}
"#;
    let p = project("settings", &[("main.jblueprint", blueprint), ("main.jlb", main)]);
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    for _ in 0..60 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    let logs = g.take_logs();
    let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.clone()).collect();
    assert!(errs.is_empty(), "{:#?}", errs);
    let list = g.render_list();
    assert!(!list.graphics.bloom);
    assert_eq!(list.graphics.tonemap, "aces");
    let t = g.tree();
    let y = |n: &str| t.objects[t.find_path(None, n).unwrap()].transform.position.y;
    let x = |n: &str| t.objects[t.find_path(None, n).unwrap()].transform.position.x;
    // One second at 2000 px/s²: about 1000 px down (much more than the default 980 gives).
    assert!(y("R/Plain") > 900.0, "plain fell {}", y("R/Plain"));
    assert_eq!(y("R/Off"), 0.0, "disabled bodies stay put");
    assert!(y("R/Rail").abs() < 0.5 && x("R/Rail") > 230.0, "rail body slides but doesn't fall: {}, {}", x("R/Rail"), y("R/Rail"));
    assert!(y("R/Thick") > 10.0 && y("R/Thick") < y("R/Plain") * 0.5, "damping slows the fall: {}", y("R/Thick"));
    let spun = t.objects[t.find_path(None, "R/Spinner").unwrap()].transform.rotation.z;
    assert!((spun - 90.0).abs() < 5.0, "spinner turned {}", spun);
    let root = t.find_path(None, "R").unwrap();
    assert_eq!(t.objects[root].props.get("tm"), Some(&Variant::Text("aces".into())));
    assert_eq!(t.objects[root].props.get("gy"), Some(&Variant::Num(2000.0)));
    assert_eq!(t.objects[root].props.get("shadows"), Some(&Variant::Bool(true)));
}

#[test]
fn dollar_variables_are_set_per_instance() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), children: [
        (name: "A", kind: Some(Sprite2D), scripts: ["res://mover.jlb"], props: {"speed": Num(60.0)}),
        (name: "B", kind: Some(Sprite2D), scripts: ["res://mover.jlb"]),
    ]))"#;
    let script = "$speed >> how fast\n$label\nDEF on_step(dt) {\n    self.x += $speed * dt\n    $label = \"moved\"\n}\n";
    assert_eq!(gex_core::object::script_vars(script), vec!["speed".to_string(), "label".to_string()]);
    let p = project("dollar", &[("blueprints/main.jblueprint", blueprint), ("mover.jlb", script)]);
    let mut g = Game::from_blueprint(p, "res://blueprints/main.jblueprint", None).unwrap();
    for _ in 0..60 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0, 0.0], HashSet::new());
    }
    let errors: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errors.is_empty(), "{:#?}", errors);
    let t = g.tree();
    let (a, b) = (t.find_path(None, "Root/A").unwrap(), t.find_path(None, "Root/B").unwrap());
    assert!((t.objects[a].transform.position.x - 60.0).abs() < 0.01, "A uses its own speed");
    assert_eq!(t.objects[b].transform.position.x, 0.0, "B was given none: 0");
    assert_eq!(t.objects[a].props.get("label"), Some(&Variant::Text("moved".into())));
}

#[test]
fn global_variables_are_shared_by_every_script() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), scripts: ["res://a.jlb"], children: [
        (name: "B", kind: Some(Sprite2D), scripts: ["res://b.jlb"]),
        (name: "C", kind: Some(Sprite2D), scripts: ["res://b.jlb"]),
    ]))"#;
    let a = "GLOBAL score = 10\nDEF on_step(dt) {\n    x = GLOBAL.score + GLOBAL.bonus\n}\n";
    let b = "DEF on_create() {\n    GLOBAL.score += 1\n    GLOBAL.bonus = 100\n}\n";
    let p = project("globals", &[("blueprints/main.jblueprint", blueprint), ("a.jlb", a), ("b.jlb", b)]);
    let mut g = Game::from_blueprint(p, "res://blueprints/main.jblueprint", None).unwrap();
    for _ in 0..3 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0, 0.0], HashSet::new());
    }
    let errors: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errors.is_empty(), "{:#?}", errors);
    let t = g.tree();
    assert_eq!(t.objects[t.root.unwrap()].transform.position.x, 112.0);
}

#[test]
fn properties_can_be_named_without_self() {
    let blueprint = r#"(root: (name: "Root", kind: Some(Object2D), children: [
        (name: "A", kind: Some(Sprite2D), scripts: ["res://mover.jlb"], props: {"speed": Num(60.0), "hits": Num(0.0)}),
    ]))"#;
    // x, speed and hits are the object's; count is the script's own and stays so; a LOCAL or
    // an input with a property's name is the function's own.
    let script = "count = 0\nDEF on_step(dt) {\n    x += speed * dt\n    hits = hits + 1\n    GLOBAL count += 1\n    LOCAL y = 500\n    nudge(3)\n}\nDEF nudge(rotation) {\n    rotation = 9\n}\n";
    let p = project("bare", &[("blueprints/main.jblueprint", blueprint), ("mover.jlb", script)]);
    let mut g = Game::from_blueprint(p, "res://blueprints/main.jblueprint", None).unwrap();
    for _ in 0..60 {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0, 0.0], HashSet::new());
    }
    let errors: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errors.is_empty(), "{:#?}", errors);
    let t = g.tree();
    let a = t.find_path(None, "Root/A").unwrap();
    assert!((t.objects[a].transform.position.x - 60.0).abs() < 0.01);
    assert_eq!(t.objects[a].transform.position.y, 0.0, "LOCAL y is the function's own");
    assert_eq!(t.objects[a].transform.rotation.z, 0.0, "an input is the function's own");
    assert_eq!(t.objects[a].props.get("hits"), Some(&Variant::Num(60.0)));
    assert_eq!(t.objects[a].props.get("count"), None);
}

/// Runs a blueprint for some frames at a frame rate and gives the game back.
fn run_blueprint(name: &str, kind: &str, kids: &str, files: &[(&str, &str)], fps: f64, frames: usize) -> Game {
    let blueprint = format!(r#"(root: (name: "Root", kind: Some({}), children: [{}]))"#, kind, kids);
    let mut all = vec![("blueprints/main.jblueprint", blueprint.as_str())];
    all.extend_from_slice(files);
    let mut g = Game::from_blueprint(project(name, &all), "res://blueprints/main.jblueprint", None).unwrap();
    for _ in 0..frames {
        g.frame(1.0 / fps, HashSet::new(), [0.0, 0.0], HashSet::new());
    }
    let errors: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errors.is_empty(), "{:#?}", errors);
    g
}

#[test]
fn physics_2d_rests_blocks_and_moves_smoothly() {
    let mut kids = String::from(r#"(name: "Floor", kind: Some(Body2D), transform: Some((position: (400.0, 500.0, 0.0))), props: {"body": Text("static"), "width": Num(800.0), "height": Num(32.0)}),"#);
    for i in 0..5 {
        kids.push_str(&format!(r#"(name: "B{}", kind: Some(Body2D), transform: Some((position: (400.0, {}, 0.0)))),"#, i, 468.0 - 33.0 * i as f32));
    }
    kids.push_str(r#"(name: "Wall", kind: Some(Body2D), transform: Some((position: (700.0, 300.0, 0.0))), props: {"body": Text("static"), "width": Num(8.0), "height": Num(300.0)}),"#);
    kids.push_str(r#"(name: "Ball", kind: Some(Body2D), transform: Some((position: (100.0, 300.0, 0.0))), props: {"shape": Text("circle"), "radius": Num(8.0), "vx": Num(6000.0), "gravity_scale": Num(0.0)}),"#);
    kids.push_str(r#"(name: "Pusher", kind: Some(Body2D), transform: Some((position: (600.0, 200.0, 0.0))), props: {"gravity_scale": Num(0.0)}, scripts: ["res://push.jlb"]),"#);
    kids.push_str(r#"(name: "Faller", kind: Some(Body2D), transform: Some((position: (100.0, -4000.0, 0.0), rotation: (0.0, 0.0, 30.0))), props: {"spin": Num(90.0)}),"#);
    let files = [("push.jlb", "DEF on_step(dt) {\n self.x += 200 * dt\n}\n")];
    // A frame rate that isn't the physics rate: some frames have no step, and still move.
    let mut g = run_blueprint("p2d", "Object2D", &kids, &files, 144.0, 144);
    let y = |g: &Game| {
        let t = g.tree();
        t.objects[t.find_path(None, "Root/Faller").unwrap()].transform.position.y
    };
    let mut last = y(&g);
    for _ in 0..40 {
        g.frame(1.0 / 144.0, HashSet::new(), [0.0, 0.0], HashSet::new());
        let now = y(&g);
        assert!(now - last > 0.5, "a falling, turning body moves every frame (it moved {})", now - last);
        last = now;
    }
    for _ in 0..(144 * 3) {
        g.frame(1.0 / 144.0, HashSet::new(), [0.0, 0.0], HashSet::new());
    }
    let t = g.tree();
    let at = |name: &str| t.objects[t.find_path(None, &format!("Root/{}", name)).unwrap()].clone();
    for i in 0..5 {
        let n = at(&format!("B{}", i));
        assert!((n.transform.position.y - (468.0 - 32.0 * i as f32)).abs() < 1.5 && n.transform.position.x == 400.0 && n.num("vy", 0.0).abs() < 0.5, "the stack rests: B{} at {:?}", i, n.transform.position);
    }
    assert!(at("Ball").transform.position.x < 700.0, "a fast ball doesn't pass through a thin wall");
    assert!(at("Pusher").transform.position.x < 700.0, "a body a script walks into a wall stays on its side");
}

#[test]
fn physics_3d_stack_rests_and_a_tumbling_body_lands() {
    let mut kids = String::from(r#"(name: "Floor", kind: Some(Body3D), transform: Some((position: (0.0, -0.5, 0.0))), props: {"body": Text("static"), "width": Num(40.0), "height": Num(1.0), "depth": Num(40.0)}),"#);
    for i in 0..5 {
        kids.push_str(&format!(r#"(name: "B{}", kind: Some(Body3D), transform: Some((position: (0.0, {}, 0.0)))),"#, i, 0.5 + 1.02 * i as f32));
    }
    kids.push_str(r#"(name: "Tumbler", kind: Some(Body3D), transform: Some((position: (-6.0, 3.0, 0.0), rotation: (30.0, 20.0, 40.0))), props: {"spin_x": Num(200.0), "spin_z": Num(140.0)}),"#);
    kids.push_str(r#"(name: "Crate", kind: Some(Body3D), transform: Some((position: (8.0, 0.5, 0.0)))),"#);
    kids.push_str(r#"(name: "Walker", kind: Some(Body3D), transform: Some((position: (6.0, 0.5, 0.0))), props: {"body": Text("kinematic")}, scripts: ["res://walk.jlb"]),"#);
    let files = [("walk.jlb", "DEF on_physics(dt) {\n MOVE(self.id, 1.5 * dt, 0, 0)\n}\n")];
    let g = run_blueprint("p3d", "Object3D", &kids, &files, 60.0, 300);
    let t = g.tree();
    let at = |name: &str| t.objects[t.find_path(None, &format!("Root/{}", name)).unwrap()].clone();
    for i in 0..5 {
        let n = at(&format!("B{}", i));
        let p = n.transform.position;
        let speed = (n.num("vx", 0.0).powi(2) + n.num("vy", 0.0).powi(2) + n.num("vz", 0.0).powi(2)).sqrt();
        assert!((p.y - (0.5 + i as f32)).abs() < 0.02 && p.x.abs() < 0.01 && p.z.abs() < 0.01 && speed < 0.01, "the stack rests without creeping: B{} at {:?}, speed {}", i, p, speed);
    }
    let tumbler = at("Tumbler").transform.position;
    assert!((tumbler.y - 0.5).abs() < 0.05, "a spinning body falls and lands (it's at {:?})", tumbler);
    assert!(at("Crate").transform.position.x > 9.0, "a body moved with MOVE pushes a crate along (it's at {:?})", at("Crate").transform.position);
}

#[test]
fn projects_from_before_the_renaming_still_run() {
    // Scenes are blueprints now and nodes objects: old files, kinds and commands carry over.
    let main = r#"(root: (name: "Root", kind: Some(Node2D), children: [(name: "A", kind: Some(Node), scripts: ["res://a.jlb"])]))"#;
    let script = "DEF on_create() {\n    PRINT(NAME_OF(NODE(\"..\")))\n    PRINT(\"res://scenes/other.jscene\")\n}\n";
    let p = project("old", &[("scenes/main.jscene", main), ("a.jlb", script)]);
    assert!(p.dir.join("scenes/main.jblueprint").is_file() && !p.dir.join("scenes/main.jscene").exists());
    let mut g = Game::from_blueprint(p, "res://scenes/main.jblueprint", None).unwrap();
    g.frame(1.0 / 60.0, HashSet::new(), [0.0, 0.0], HashSet::new());
    let logs: Vec<String> = g.take_logs().into_iter().map(|l| l.text).collect();
    assert_eq!(logs, vec!["Root".to_string(), "res://scenes/other.jblueprint".to_string()], "{:?}", logs);
}

/// Timing, not a check: `cargo test --release -p gex-script bench_many -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_many_objects() {
    let mut kids = String::new();
    for i in 0..std::env::var("BENCH_N").ok().and_then(|v| v.parse().ok()).unwrap_or(3000) {
        kids += &format!("(name: \"S{}\", kind: Some(Sprite2D), transform: Some((position: ({}.0, {}.0, 0.0))), scripts: [\"res://mover.jlb\"]),\n", i, i % 800, i / 4);
    }
    let blueprint = format!("(root: (name: \"R\", kind: Some(Object2D), children: [(name: \"G\", kind: Some(Object2D), children: [{}])]))", kids);
    let mover = "speed = 50\nDEF on_step(dt) {\n    x += speed * dt\n    IF x > 800 { x = 0 }\n}\n";
    let p = project("bench", &[("main.jblueprint", &blueprint), ("mover.jlb", mover)]);
    let t0 = std::time::Instant::now();
    let mut g = Game::from_blueprint(p, "res://main.jblueprint", None).unwrap();
    println!("load {:?}", t0.elapsed());
    let t0 = std::time::Instant::now();
    let frames: u32 = std::env::var("BENCH_F").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    for _ in 0..frames {
        g.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
    }
    println!("60 frames {:?} ({:?}/frame)", t0.elapsed(), t0.elapsed() / 60);
    let t0 = std::time::Instant::now();
    for _ in 0..60 {
        std::hint::black_box(gex_core::render::extract(&g.tree()));
    }
    println!("60 extracts {:?}", t0.elapsed());
    let errs: Vec<_> = g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
    assert!(errs.is_empty(), "{:?}", &errs[..errs.len().min(3)]);
}

/// Runs every blueprint of a project for a few seconds and lists the errors:
/// `GEX_SWEEP=/path/to/project cargo test --release -p gex-script sweep -- --ignored --nocapture`.
#[test]
#[ignore]
fn sweep_project_blueprints() {
    let Ok(dir) = std::env::var("GEX_SWEEP") else { return };
    let p = Project::open(std::path::Path::new(&dir)).unwrap();
    let mut stack = vec![p.dir.clone()];
    let mut files = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() && !e.file_name().to_string_lossy().starts_with('.') {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "jblueprint") {
                files.push(path);
            }
        }
    }
    files.sort();
    let keys: Vec<HashSet<String>> = vec![HashSet::new(), ["right".to_string(), "space".to_string(), "d".to_string()].into(), ["left".to_string(), "up".to_string(), "w".to_string(), "mouse_left".to_string()].into()];
    for f in files {
        let res = format!("res://{}", f.strip_prefix(&p.dir).unwrap().to_string_lossy().replace('\\', "/"));
        let t0 = std::time::Instant::now();
        let mut g = match Game::from_blueprint(p.clone(), &res, None) {
            Ok(g) => g,
            Err(e) => {
                println!("{}: LOAD {}", res, e);
                continue;
            }
        };
        let mut errs = Vec::new();
        for i in 0..240 {
            g.frame(1.0 / 60.0, keys[(i / 40) % 3].clone(), [400.0, 300.0], if i % 50 < 3 { ["mouse_left".to_string()].into() } else { HashSet::new() });
            errs.extend(g.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text.lines().next().unwrap_or("").to_string()));
        }
        errs.dedup();
        println!("{}: {:?} {}", res, t0.elapsed() / 240, if errs.is_empty() { "ok".to_string() } else { format!("{:?}", errs) });
    }
}
