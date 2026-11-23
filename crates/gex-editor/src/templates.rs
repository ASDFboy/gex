//! New projects, and (for tests only: it isn't part of GEX) a project with a few small games.

use std::path::Path;

use gex_core::project::{Project, ProjectConfig, PROJECT_FILE};

#[cfg(test)]
pub const DEMO: &[(&str, &str)] = &[
    ("project.jproj", include_str!("demo/project.jproj")),
    ("blueprints/main.jblueprint", include_str!("demo/blueprints/main.jblueprint")),
    ("blueprints/coin.jblueprint", include_str!("demo/blueprints/coin.jblueprint")),
    ("blueprints/world3d.jblueprint", include_str!("demo/blueprints/world3d.jblueprint")),
    ("scripts/player.jlb", include_str!("demo/scripts/player.jlb")),
    ("scripts/coin.jlb", include_str!("demo/scripts/coin.jlb")),
    ("scripts/game.jlb", include_str!("demo/scripts/game.jlb")),
    ("scripts/spinner.jlb", include_str!("demo/scripts/spinner.jlb")),
    ("scripts/bob.jlb", include_str!("demo/scripts/bob.jlb")),
    ("scripts/world.jlb", include_str!("demo/scripts/world.jlb")),
    ("blueprints/menu.jblueprint", include_str!("demo/blueprints/menu.jblueprint")),
    ("blueprints/platformer.jblueprint", include_str!("demo/blueprints/platformer.jblueprint")),
    ("blueprints/crate3d.jblueprint", include_str!("demo/blueprints/crate3d.jblueprint")),
    ("blueprints/fps.jblueprint", include_str!("demo/blueprints/fps.jblueprint")),
    ("scripts/fps.jlb", include_str!("demo/scripts/fps.jlb")),
    ("scripts/menu.jlb", include_str!("demo/scripts/menu.jlb")),
    ("scripts/hero.jlb", include_str!("demo/scripts/hero.jlb")),
    ("scripts/back_to_menu.jlb", include_str!("demo/scripts/back_to_menu.jlb")),
];

/// The demo's images and sounds (all made for GEX, free to reuse).
#[cfg(test)]
pub const DEMO_BIN: &[(&str, &[u8])] = &[
    ("assets/tiles.png", include_bytes!("demo/assets/tiles.png")),
    ("assets/ball.png", include_bytes!("demo/assets/ball.png")),
    ("assets/jump.wav", include_bytes!("demo/assets/jump.wav")),
];

#[cfg(test)]
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

#[cfg(test)]
pub fn create_demo(dir: &Path) -> Result<Project, String> {
    write_all(dir, DEMO)?;
    write_all(dir, DEMO_BIN)?;
    Project::open(dir)
}

/// An empty project: one 2D blueprint with a root object.
pub fn create_project(dir: &Path, name: &str) -> Result<Project, String> {
    if dir.join(PROJECT_FILE).exists() {
        return Project::open(dir);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let p = Project { dir: dir.to_path_buf(), config: ProjectConfig { name: name.to_string(), ..Default::default() } };
    p.save()?;
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
        let mut g = Game::from_blueprint(p.clone(), "res://blueprints/main.jblueprint", None).unwrap();
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
        assert_ne!(t.objects[score].text("text"), "Score: 0", "a coin was collected");
        assert!(!t.in_group("coins").is_empty(), "coins keep spawning");
        assert!(g.render_list().sprites.len() > 20, "the trail and the sparks draw");
        drop(t);
        // The 3D blueprint runs too.
        let mut g3 = Game::from_blueprint(p.clone(), "res://blueprints/world3d.jblueprint", None).unwrap();
        for _ in 0..30 {
            g3.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        }
        let errs: Vec<_> = g3.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
        assert!(errs.is_empty(), "{:#?}", errs);
        let t = g3.tree();
        let cube = t.find_path(None, "World/Cube").unwrap();
        assert!(t.objects[cube].transform.rotation.y > 10.0);
        drop(t);
        // Clicking drops a crate that falls and lands on the ground.
        g3.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], ["left".to_string()].into());
        for _ in 0..240 {
            g3.frame(1.0 / 60.0, HashSet::new(), [0.0; 2], HashSet::new());
        }
        let t = g3.tree();
        let crates = t.in_group("crates");
        assert_eq!(crates.len(), 1);
        let y = t.objects[crates[0]].transform.position.y;
        assert!((y - 0.5).abs() < 0.1, "resting on the ground: {}", y);
        drop(t);
        // The platformer: the hero lands on the tiles, runs right and jumps.
        let mut gp = Game::from_blueprint(p.clone(), "res://blueprints/platformer.jblueprint", None).unwrap();
        let hero_y = |g: &Game| {
            let t = g.tree();
            let h = t.find_path(None, "Level/Hero").unwrap();
            (t.objects[h].transform.position.x, t.objects[h].transform.position.y)
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
        // First person: the mouse turns the player, W walks the way it faces, things get shoved.
        let mut gf = Game::from_blueprint(p.clone(), "res://blueprints/fps.jblueprint", None).unwrap();
        let step = |g: &mut Game, keys: &[&str], d: [f32; 2], buttons: &[&str]| {
            g.step(gex_script::FrameInput { dt: 1.0 / 60.0, keys: keys.iter().map(|k| k.to_string()).collect(), mouse_delta: d, buttons: buttons.iter().map(|k| k.to_string()).collect(), ..Default::default() })
        };
        for _ in 0..30 {
            step(&mut gf, &[], [0.0; 2], &[]);
        }
        assert!(gf.render_list().mouse_locked);
        let pos = |g: &Game| {
            let t = g.tree();
            let id = t.find_path(None, "Arena/Player").unwrap();
            (t.objects[id].transform.position, t.objects[id].transform.rotation.y)
        };
        let (p0, _) = pos(&gf);
        assert!((p0.y - 0.9).abs() < 0.1, "standing on the floor: {}", p0);
        step(&mut gf, &[], [-600.0, 0.0], &[]);
        let (_, yaw) = pos(&gf);
        assert!((yaw - 90.0).abs() < 0.5, "600 px left = 90 degrees: {}", yaw);
        for _ in 0..60 {
            step(&mut gf, &["w"], [0.0; 2], &[]);
        }
        let (p1, _) = pos(&gf);
        assert!(p1.x < p0.x - 4.0 && (p1.z - p0.z).abs() < 0.5, "walked towards -x (facing left): {} -> {}", p0, p1);
        let errs: Vec<_> = gf.take_logs().into_iter().filter(|l| l.kind == LogKind::Error).map(|l| l.text).collect();
        assert!(errs.is_empty(), "{:#?}", errs);
        // The menu loads and its buttons' handlers exist.
        let mut gm = Game::from_blueprint(p, "res://blueprints/menu.jblueprint", None).unwrap();
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
        assert!(out.join("game/blueprints/main.jblueprint").is_file());
        assert!(std::fs::read_dir(&out).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("GEX Demo")));
    }

    /// Exports the demo for Windows and macOS with an icon. Needs a Windows GEX:
    /// GEX_TEST_WINEXE=/abs/path/gex.exe; GEX_TEST_EXPORT_TO keeps the result there.
    #[test]
    #[ignore]
    fn export_for_other_systems_with_an_icon() {
        use crate::export::Platform;
        let exe = std::path::PathBuf::from(std::env::var("GEX_TEST_WINEXE").expect("GEX_TEST_WINEXE"));
        let dir = std::env::temp_dir().join(format!("gex-xexport-src-{}", std::process::id()));
        let out = std::env::var("GEX_TEST_EXPORT_TO").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join(format!("gex-xexport-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&out);
        let mut p = super::create_demo(&dir).unwrap();
        // A 32×32 icon: an orange disc.
        let mut img = image::RgbaImage::new(32, 32);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let d = ((x as f32 - 15.5).powi(2) + (y as f32 - 15.5).powi(2)).sqrt();
            *px = if d < 15.0 { image::Rgba([240, 140, 40, 255]) } else { image::Rgba([0, 0, 0, 0]) };
        }
        img.save(dir.join("icon.png")).unwrap();
        p.config.icon = "res://icon.png".into();
        p.save().unwrap();
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        let w = st.export_game_for(&out.join("windows"), Platform::Windows, &exe, "Demo Game").unwrap();
        assert!(w.is_empty(), "{:?}", w);
        let made = std::fs::read(out.join("windows/Demo Game.exe")).unwrap();
        let pe = editpe::Image::parse(made.as_slice()).unwrap();
        assert!(pe.resource_directory().unwrap().get_main_icon().unwrap().is_some());
        assert!(out.join("windows/game/blueprints/menu.jblueprint").is_file());
        let me = std::env::current_exe().unwrap();
        st.export_game_for(&out.join("mac"), Platform::MacOs, &me, "Demo Game").unwrap();
        assert!(out.join("mac/Demo Game.app/Contents/MacOS/Demo Game").is_file());
        assert!(out.join("mac/Demo Game.app/Contents/Info.plist").is_file());
        assert!(out.join("mac/Demo Game.app/Contents/Resources/game/project.jproj").is_file());
    }

    #[test]
    fn events_get_their_own_scripts_and_new_things_start_at_zero() {
        let dir = std::env::temp_dir().join(format!("gex-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::create_demo(&dir).unwrap();
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        st.open_blueprint("res://blueprints/main.jblueprint");
        // A new object sits at 0,0 of its parent, wherever the view is looking.
        st.cam2.center = gex_core::glam::Vec2::new(500.0, 300.0);
        let id = st.add_object(gex_core::ObjectKind::Sprite2D);
        assert_eq!(st.tree.objects[id].transform.position, gex_core::glam::Vec3::ZERO);
        st.tree.objects[id].name = "Bullet".into();
        // Each event picked makes a script named after the object and the event.
        st.add_event_script(id, "on_step(dt)");
        st.add_event_script(id, "on_create()");
        st.add_event_script(id, "on_step(dt)");
        let names: Vec<&str> = st.tree.objects[id].scripts.iter().map(|s| s.rsplit('/').next().unwrap()).collect();
        assert_eq!(names, ["bullet_step.jlb", "bullet_create.jlb"]);
        assert!(dir.join("blueprints/bullet_step.jlb").is_file());
        assert_eq!(st.scripts["res://blueprints/bullet_step.jlb"].text.matches("DEF on_step").count(), 1);
        // An instance is named after its blueprint.
        st.selected = None;
        let a = st.instance_blueprint("res://blueprints/coin.jblueprint").unwrap();
        st.selected = None;
        let b = st.instance_blueprint("res://blueprints/coin.jblueprint").unwrap();
        assert_eq!((st.tree.objects[a].name.as_str(), st.tree.objects[b].name.as_str()), ("coin", "coin2"));
        // Highlight colors follow folders, and renames.
        st.set_asset_color("res://blueprints", Some("#a6e22e".into()));
        assert_eq!(st.asset_color("res://blueprints/coin.jblueprint"), Some("#a6e22e"));
        assert_eq!(st.asset_color("res://scripts/player.jlb"), None);
        st.rename_asset("res://blueprints", "levels").unwrap();
        assert_eq!(st.asset_color("res://levels/coin.jblueprint"), Some("#a6e22e"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_branch_as_blueprint_and_make_local() {
        let dir = std::env::temp_dir().join(format!("gex-branch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::create_demo(&dir).unwrap();
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        st.open_blueprint("res://blueprints/main.jblueprint");
        let player = st.tree.find_path(None, "Level/Player").unwrap();
        let pos = st.tree.objects[player].transform.position;
        let kids = st.tree.objects[player].children.len();
        let res = st.save_branch_as_blueprint(player).expect("saved");
        assert_eq!(res, "res://blueprints/player.jblueprint");
        assert!(dir.join("blueprints/player.jblueprint").is_file());
        let p2 = st.tree.find_path(None, "Level/Player").unwrap();
        let n = &st.tree.objects[p2];
        assert_eq!(n.instance_of.as_deref(), Some("res://blueprints/player.jblueprint"));
        assert_eq!(n.transform.position, pos, "stays where it was");
        assert_eq!(n.children.len(), kids);
        assert!(n.children.iter().all(|c| st.tree.objects[*c].from_instance));
        // The level saves it by reference and loads back the same.
        let text = st.blueprint_text().unwrap();
        assert!(text.contains("instance: Some(\"res://blueprints/player.jblueprint\")"), "{}", text);
        st.make_local(p2);
        assert!(st.tree.objects[p2].instance_of.is_none());
        st.undo();
        let p3 = st.tree.find_path(None, "Level/Player").unwrap();
        assert!(st.tree.objects[p3].instance_of.is_some(), "undo brings the instance back");
    }

    #[test]
    fn saving_a_blueprint_updates_its_copies_in_other_open_blueprints() {
        let dir = std::env::temp_dir().join(format!("gex-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::create_demo(&dir).unwrap();
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        st.open_blueprint("res://blueprints/main.jblueprint");
        // An override on one copy, unsaved.
        let c2 = st.tree.find_path(None, "Level/Coin2").unwrap();
        st.tree.objects[c2].props.insert("width".into(), gex_core::Variant::Num(99.0));
        st.open_blueprint("res://blueprints/coin.jblueprint");
        let root = st.tree.root.unwrap();
        st.tree.objects[root].props.insert("color".into(), gex_core::Variant::Color([1.0, 0.5, 0.0, 1.0]));
        st.save_blueprint();
        st.open_blueprint("res://blueprints/main.jblueprint");
        for name in ["Level/Coin", "Level/Coin2"] {
            let c = st.tree.find_path(None, name).unwrap();
            assert_eq!(st.tree.objects[c].color("color"), [1.0, 0.5, 0.0, 1.0], "{} follows the file", name);
        }
        let c2 = st.tree.find_path(None, "Level/Coin2").unwrap();
        assert_eq!(st.tree.objects[c2].num("width", 0.0), 99.0, "its own change is kept");
        assert!(!st.blueprint_text().unwrap().contains("Color((1.0, 0.5"), "no stale color saved as an override");
    }

    #[test]
    fn saving_everything_writes_only_what_changed() {
        let dir = std::env::temp_dir().join(format!("gex-only-changed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::create_demo(&dir).unwrap();
        // Laid out differently from how GEX writes it (by hand, another version, another computer).
        let main = dir.join("blueprints/main.jblueprint");
        let hand = format!("{}\n\n", std::fs::read_to_string(&main).unwrap().replace('\n', "\r\n"));
        std::fs::write(&main, &hand).unwrap();
        let snapshot = |d: &std::path::Path| {
            let mut out = std::collections::BTreeMap::new();
            let mut stack = vec![d.to_path_buf()];
            while let Some(x) = stack.pop() {
                for e in std::fs::read_dir(&x).unwrap().flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    if e.path().is_dir() {
                        stack.push(e.path());
                    } else {
                        let m = e.metadata().unwrap().modified().unwrap();
                        out.insert(e.path(), (std::fs::read(e.path()).unwrap(), m));
                    }
                }
            }
            out
        };
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        st.open_blueprint("res://blueprints/coin.jblueprint");
        st.open_blueprint("res://blueprints/main.jblueprint");
        st.open_script("res://scripts/player.jlb", None);
        let before = snapshot(&dir);
        st.save_all();
        st.save_blueprint();
        st.save_script("res://scripts/player.jlb");
        let _ = st.project.save();
        let now = snapshot(&dir);
        let touched: Vec<_> = now.iter().filter(|(p, v)| before.get(*p) != Some(v)).map(|(p, _)| p.clone()).collect();
        assert!(touched.is_empty() && now.len() == before.len(), "saving with nothing changed rewrote {:?}", touched);
        assert_eq!(std::fs::read_to_string(&main).unwrap(), hand);

        // One change: only that file is written.
        let root = st.tree.root.unwrap();
        st.tree.objects[root].props.insert("width".into(), gex_core::Variant::Num(123.0));
        st.scripts.get_mut("res://scripts/player.jlb").unwrap().text.push_str("\n>> changed\n");
        st.save_all();
        let after = snapshot(&dir);
        let changed: Vec<String> = after.iter().filter(|(p, v)| before.get(*p) != Some(v)).map(|(p, _)| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(changed, vec!["blueprints/main.jblueprint".to_string(), "scripts/player.jlb".to_string()]);
        assert_eq!(after.len(), before.len(), "no files added or left behind");
        // Saving again: nothing more.
        st.save_all();
        assert!(after == snapshot(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_changed_outside_are_noticed_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("gex-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::create_demo(&dir).unwrap();
        let mut st = crate::state::EditorState::new(gex_core::project::Project::open(&dir).unwrap());
        st.open_script("res://scripts/player.jlb", None);
        let f = dir.join("scripts/player.jlb");
        // Changed by another program, no unsaved edits here: read again, and saving keeps it.
        std::fs::write(&f, "DEF on_step(dt) { }\n>> from outside\n").unwrap();
        st.check_outside_changes();
        assert!(st.scripts["res://scripts/player.jlb"].text.contains("from outside"));
        st.save_all();
        assert!(std::fs::read_to_string(&f).unwrap().contains("from outside"));
        // Changed outside while edited here: the edits stay (and save), the other version is kept.
        st.scripts.get_mut("res://scripts/player.jlb").unwrap().text.push_str(">> mine\n");
        std::fs::write(&f, "DEF on_step(dt) { }\n>> second outside\n").unwrap();
        st.outside_checked = None;
        st.check_outside_changes();
        st.save_all();
        assert!(std::fs::read_to_string(&f).unwrap().contains(">> mine"));
        let kept = std::fs::read_dir(dir.join(".trash")).unwrap().flatten().any(|d| std::fs::read_to_string(d.path().join("scripts/player.jlb")).is_ok_and(|t| t.contains("second outside")));
        assert!(kept, "the other program's version is kept in .trash");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
