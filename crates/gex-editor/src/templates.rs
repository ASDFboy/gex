//! Files for new projects and the built-in demo project.

use std::path::Path;

use gex_core::project::{Project, ProjectConfig, PROJECT_FILE};

pub const DEMO: &[(&str, &str)] = &[
    ("project.jproj", include_str!("demo/project.jproj")),
    ("scenes/main.jscene", include_str!("demo/scenes/main.jscene")),
    ("scenes/coin.jscene", include_str!("demo/scenes/coin.jscene")),
    ("scenes/world3d.jscene", include_str!("demo/scenes/world3d.jscene")),
    ("scripts/player.jlb", include_str!("demo/scripts/player.jlb")),
    ("scripts/coin.jlb", include_str!("demo/scripts/coin.jlb")),
    ("scripts/game.jlb", include_str!("demo/scripts/game.jlb")),
    ("scripts/spinner.jlb", include_str!("demo/scripts/spinner.jlb")),
    ("scripts/bob.jlb", include_str!("demo/scripts/bob.jlb")),
    ("scripts/world.jlb", include_str!("demo/scripts/world.jlb")),
    ("scenes/menu.jscene", include_str!("demo/scenes/menu.jscene")),
    ("scenes/platformer.jscene", include_str!("demo/scenes/platformer.jscene")),
    ("scripts/menu.jlb", include_str!("demo/scripts/menu.jlb")),
    ("scripts/hero.jlb", include_str!("demo/scripts/hero.jlb")),
    ("scripts/back_to_menu.jlb", include_str!("demo/scripts/back_to_menu.jlb")),
];

/// The demo's images and sounds (all made for GEX, free to reuse).
pub const DEMO_BIN: &[(&str, &[u8])] = &[
    ("assets/tiles.png", include_bytes!("demo/assets/tiles.png")),
    ("assets/ball.png", include_bytes!("demo/assets/ball.png")),
    ("assets/jump.wav", include_bytes!("demo/assets/jump.wav")),
];

fn write_all<T: AsRef<[u8]>>(dir: &Path, files: &[(&str, T)]) -> Result<(), String> {
    for (p, text) in files {
        let path = dir.join(p);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {}", d.display(), e))?;
        }
        if !path.exists() {
            std::fs::write(&path, text).map_err(|e| format!("{}: {}", path.display(), e))?;
        }
    }
    Ok(())
}

pub fn create_demo(dir: &Path) -> Result<Project, String> {
    write_all(dir, DEMO)?;
    write_all(dir, DEMO_BIN)?;
    Project::open(dir)
}

/// An empty project: one 2D scene with a root node.
pub fn create_project(dir: &Path, name: &str) -> Result<Project, String> {
    if dir.join(PROJECT_FILE).exists() {
        return Project::open(dir);
    }
    std::fs::create_dir_all(dir.join("scripts")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| e.to_string())?;
    let p = Project { dir: dir.to_path_buf(), config: ProjectConfig { name: name.to_string(), ..Default::default() } };
    p.save()?;
    write_all(dir, &[("scenes/main.jscene", &gex_core::scene::new_scene_text("Main", gex_core::NodeKind::Node2D))])?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use gex_script::{Game, LogKind};

    /// The demo's scripts run without errors, and playing it actually works.
    #[test]
    fn demo_plays() {
        let dir = std::env::temp_dir().join(format!("gex-demo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = super::create_demo(&dir).unwrap();
        let mut g = Game::from_scene(p.clone(), "res://scenes/main.jscene", None).unwrap();
        // From (640, 360): up to y = 250, then left onto the first coin at (300, 250).
        for i in 0..240 {
            let k: HashSet<String> = if i < 22 { ["up".into()].into() } else if i < 90 { ["left".into()].into() } else { HashSet::new() };
            g.frame(1.0 / 60.0, k, [0.0; 2], HashSet::new());
        }
        let logs = g.take_logs();
        let errs: Vec<_> = logs.iter().filter(|l| l.kind == LogKind::Error).map(|l| &l.text).collect();
        assert!(errs.is_empty(), "{:#?}", errs);
        let t = g.tree();
        let score = t.find_path(None, "Level/Score").unwrap();
        assert_ne!(t.nodes[score].text("text"), "Score: 0", "a coin was collected");
        assert!(t.in_group("coins").len() >= 2, "coins keep spawning");
        drop(t);
        // The 3D scene runs too.
        let mut g3 = Game::from_scene(p.clone(), "res://scenes/world3d.jscene", None).unwrap();
        for _ in 0..30 {
            g3.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        }
        let errs: Vec<_> = g3.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
        assert!(errs.is_empty(), "{:#?}", errs);
        let t = g3.tree();
        let cube = t.find_path(None, "World/Cube").unwrap();
        assert!(t.nodes[cube].transform.rotation.y > 10.0);
        drop(t);
        // The platformer: the hero lands on the tiles, runs right and jumps.
        let mut gp = Game::from_scene(p.clone(), "res://scenes/platformer.jscene", None).unwrap();
        let hero_y = |g: &Game| {
            let t = g.tree();
            let h = t.find_path(None, "Level/Hero").unwrap();
            (t.nodes[h].transform.position.x, t.nodes[h].transform.position.y)
        };
        for _ in 0..60 {
            gp.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        }
        let (x0, y0) = hero_y(&gp);
        assert!((y0 - 586.0).abs() < 6.0, "standing on the ground row (y = 608 - 22): {}", y0);
        for i in 0..30 {
            let k: HashSet<String> = if i == 0 { ["right".into(), "space".into()].into() } else { ["right".into()].into() };
            gp.frame(1.0 / 60.0, k, [0.0; 2], HashSet::new());
        }
        let (x1, y1) = hero_y(&gp);
        assert!(x1 > x0 + 80.0, "ran right: {} -> {}", x0, x1);
        assert!(y1 < y0 - 40.0, "jumped: {} -> {}", y0, y1);
        let errs: Vec<_> = gp.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
        assert!(errs.is_empty(), "{:#?}", errs);
        // The menu loads and its buttons' handlers exist.
        let mut gm = Game::from_scene(p, "res://scenes/menu.jscene", None).unwrap();
        gm.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        let errs: Vec<_> = gm.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
        assert!(errs.is_empty(), "{:#?}", errs);
    }

    #[test]
    fn export_makes_a_runnable_folder_without_mounted_files() {
        let dir = std::env::temp_dir().join(format!("gex-export-src-{}", std::process::id()));
        let out = std::env::temp_dir().join(format!("gex-export-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
        let mut p = super::create_demo(&dir).unwrap();
        p.config.mounts.push(gex_core::project::MountConfig { scheme: "hl2".into(), kind: gex_core::project::MountKind::Folder(dir.join("nowhere")) });
        p.save().unwrap();
        std::fs::write(dir.join("scripts/uses_hl2.jlb"), "x = \"hl2://sound/a.wav\"\n").unwrap();
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        let warnings = st.export_game(&out).unwrap();
        assert!(warnings.iter().any(|w| w.contains("uses_hl2.jlb")), "{:?}", warnings);
        let cfg = gex_core::project::Project::open(&out.join("game")).unwrap().config;
        assert!(cfg.mounts.is_empty(), "mounts are stripped");
        assert!(out.join("game/scenes/main.jscene").is_file());
        assert!(std::fs::read_dir(&out).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("GEX Demo")));
    }

    #[test]
    fn save_branch_as_scene_and_make_local() {
        let dir = std::env::temp_dir().join(format!("gex-branch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::create_demo(&dir).unwrap();
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        st.open_scene("res://scenes/main.jscene");
        let player = st.tree.find_path(None, "Level/Player").unwrap();
        let pos = st.tree.nodes[player].transform.position;
        let kids = st.tree.nodes[player].children.len();
        let res = st.save_branch_as_scene(player).expect("saved");
        assert_eq!(res, "res://scenes/player.jscene");
        assert!(dir.join("scenes/player.jscene").is_file());
        let p2 = st.tree.find_path(None, "Level/Player").unwrap();
        let n = &st.tree.nodes[p2];
        assert_eq!(n.instance_of.as_deref(), Some("res://scenes/player.jscene"));
        assert_eq!(n.transform.position, pos, "stays where it was");
        assert_eq!(n.children.len(), kids);
        assert!(n.children.iter().all(|c| st.tree.nodes[*c].from_instance));
        // The level saves it by reference and loads back the same.
        let text = st.scene_text().unwrap();
        assert!(text.contains("instance: Some(\"res://scenes/player.jscene\")"), "{}", text);
        st.make_local(p2);
        assert!(st.tree.nodes[p2].instance_of.is_none());
        st.undo();
        let p3 = st.tree.find_path(None, "Level/Player").unwrap();
        assert!(st.tree.nodes[p3].instance_of.is_some(), "undo brings the instance back");
    }
}
