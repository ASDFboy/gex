//! A project: a folder with a `project.jproj` file. `res://` paths point inside it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const PROJECT_FILE: &str = "project.jproj";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
    /// The blueprint the game starts in (res:// path).
    #[serde(alias = "main_scene")]
    pub main_blueprint: String,
    /// The game window: resolution, full screen, scaling (see settings.rs).
    pub display: crate::settings::Display,
    /// Extra asset sources (see gex-assets): e.g. a Source-engine game install.
    pub mounts: Vec<MountConfig>,
    /// Older project files kept gravity here; it moves into `physics` when they're opened.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "bare_some")]
    pub gravity_2d: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "bare_some")]
    pub gravity_3d: Option<[f32; 3]>,
    /// How the game looks (see settings.rs).
    pub graphics: crate::settings::Graphics,
    /// How physics works.
    pub physics: crate::settings::PhysicsSettings,
    /// Input actions: a name scripts use ("jump") → the keys and buttons that trigger it
    /// (["space", "w", "pad_a"]). `KEY_DOWN("jump")` and `on_press_jump()` then work with any.
    pub actions: std::collections::BTreeMap<String, Vec<String>>,
    /// Older project files kept the window settings here; they move into `display` when opened.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "bare_some")]
    pub window_size: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "bare_some")]
    pub fullscreen: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "bare_some")]
    pub resizable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "bare_some")]
    pub vsync: Option<bool>,
    /// A .png for the window and the exported program's icon.
    pub icon: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig { name: "New Game".into(), main_blueprint: "res://blueprints/main.jblueprint".into(), display: Default::default(), mounts: Vec::new(), gravity_2d: None, gravity_3d: None, graphics: Default::default(), physics: Default::default(), actions: Default::default(), window_size: None, fullscreen: None, resizable: None, vsync: None, icon: String::new() }
    }
}

/// Older settings were written as plain values (`window_size: (960, 540)`), not `Some(...)`.
fn bare_some<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

impl ProjectConfig {
    /// Brings settings from older project files to where they live now.
    pub fn migrate(&mut self) {
        if let Some(g) = self.gravity_2d.take() {
            self.physics.gravity_2d = g;
        }
        if let Some(g) = self.gravity_3d.take() {
            self.physics.gravity_3d = g;
        }
        if let Some([w, h]) = self.window_size.take() {
            self.display.width = w;
            self.display.height = h;
        }
        if let Some(v) = self.fullscreen.take() {
            self.display.fullscreen = v;
        }
        if let Some(v) = self.resizable.take() {
            self.display.resizable = v;
        }
        if let Some(v) = self.vsync.take() {
            self.display.vsync = v;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MountConfig {
    /// The path prefix it answers to, like "hl2" for hl2://materials/...
    pub scheme: String,
    pub kind: MountKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MountKind {
    /// A plain folder.
    Folder(PathBuf),
    /// A Source-engine game install: the folder holding hl2.exe, plus the game folders
    /// to search in order (e.g. ["ep2", "episodic", "hl2"]). Files are read in place
    /// (loose files and .vpk archives); nothing is copied into the project.
    SourceGame { install: PathBuf, games: Vec<String> },
}

#[derive(Clone, Debug)]
pub struct Project {
    pub dir: PathBuf,
    pub config: ProjectConfig,
}

impl Project {
    pub fn open(dir: &Path) -> Result<Project, String> {
        rename_old_files(dir);
        let f = dir.join(PROJECT_FILE);
        let text = std::fs::read_to_string(&f).map_err(|e| format!("couldn't open {}: {}", f.display(), e))?;
        let mut config: ProjectConfig = ron::from_str(&text).map_err(|e| format!("{}: {}", f.display(), e))?;
        config.migrate();
        Ok(Project { dir: dir.to_path_buf(), config })
    }
    /// Reads a project's settings without opening it (nothing in the folder is touched).
    pub fn peek(dir: &Path) -> Result<ProjectConfig, String> {
        let f = dir.join(PROJECT_FILE);
        let text = std::fs::read_to_string(&f).map_err(|e| format!("couldn't open {}: {}", f.display(), e))?;
        let mut config: ProjectConfig = ron::from_str(&text).map_err(|e| format!("{}: {}", f.display(), e))?;
        config.migrate();
        Ok(config)
    }
    pub fn save(&self) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(&self.config, ron::ser::PrettyConfig::new().struct_names(false)).map_err(|e| e.to_string())?;
        std::fs::write(self.dir.join(PROJECT_FILE), text).map_err(|e| e.to_string())
    }
    /// A res:// path as a file on disk (None for other schemes or paths leaving the project).
    pub fn res_to_path(&self, res: &str) -> Option<PathBuf> {
        let rel = res.strip_prefix("res://")?;
        if rel.split(['/', '\\']).any(|p| p == "..") {
            return None;
        }
        Some(self.dir.join(rel))
    }
    /// A file inside the project as a res:// path.
    pub fn path_to_res(&self, p: &Path) -> Option<String> {
        let rel = p.strip_prefix(&self.dir).ok()?;
        Some(format!("res://{}", rel.to_string_lossy().replace('\\', "/")))
    }
    /// Reads a res:// file as text (what blueprint instancing uses).
    pub fn read_text(&self, res: &str) -> Result<String, String> {
        let p = self.res_to_path(res).ok_or_else(|| format!("{} isn't a project path", res))?;
        std::fs::read_to_string(&p).map_err(|e| e.to_string())
    }
}

/// Projects from before scenes were called blueprints: `.jscene` files become `.jblueprint`,
/// and scripts, blueprints and the project file that name them are fixed to match.
fn rename_old_files(dir: &Path) {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && name != ".gex" {
                continue;
            }
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    if !files.iter().any(|f| f.extension().is_some_and(|e| e == "jscene")) {
        return;
    }
    for f in &files {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "jscene" | "jlb" | "jproj" | "ron" | "janim" | "txt" | "json") {
            if let Ok(text) = std::fs::read_to_string(f) {
                if text.contains(".jscene") {
                    let _ = std::fs::write(f, text.replace(".jscene", ".jblueprint"));
                }
            }
        }
        if ext == "jscene" {
            let _ = std::fs::rename(f, f.with_extension("jblueprint"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_window_settings_move_into_display() {
        let old = r#"(name: "Old", main_blueprint: "res://main.jblueprint", window_size: (1920, 1080), fullscreen: true, resizable: false, vsync: false, gravity_2d: (0, 500))"#;
        let mut c: ProjectConfig = ron::from_str(old).unwrap();
        c.migrate();
        assert_eq!(c.display.size(), [1920, 1080]);
        assert!(c.display.fullscreen && !c.display.resizable && !c.display.vsync);
        assert_eq!(c.physics.gravity_2d, [0.0, 500.0]);
        let saved = ron::ser::to_string(&c).unwrap();
        assert!(!saved.contains("window_size") && saved.contains("display"), "{}", saved);
        let again: ProjectConfig = ron::from_str(&saved).unwrap();
        assert_eq!(again.display, c.display);
    }
}
