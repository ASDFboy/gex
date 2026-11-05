//! Files for new projects and the built-in demo project.

use std::path::Path;

use jle_core::project::{Project, ProjectConfig, PROJECT_FILE};

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
];

fn write_all(dir: &Path, files: &[(&str, &str)]) -> Result<(), String> {
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
    write_all(dir, &[("scenes/main.jscene", &jle_core::scene::new_scene_text("Main", jle_core::NodeKind::Node2D))])?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use jle_script::{Game, LogKind};

    /// The demo's scripts run without errors, and playing it actually works.
    #[test]
    fn demo_plays() {
        let dir = std::env::temp_dir().join(format!("jle-demo-{}", std::process::id()));
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
        let mut g3 = Game::from_scene(p, "res://scenes/world3d.jscene", None).unwrap();
        for _ in 0..30 {
            g3.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        }
        let errs: Vec<_> = g3.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
        assert!(errs.is_empty(), "{:#?}", errs);
        let t = g3.tree();
        let cube = t.find_path(None, "World/Cube").unwrap();
        assert!(t.nodes[cube].transform.rotation.y > 10.0);
    }
}
