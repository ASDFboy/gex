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
