//! A project: a folder with a `project.jproj` file. `res://` paths point inside it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const PROJECT_FILE: &str = "project.jproj";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub name: String,
    /// The scene the game starts in (res:// path).
    pub main_scene: String,
    pub window_size: [u32; 2],
    /// Extra asset sources (see gex-assets): e.g. a Source-engine game install.
    pub mounts: Vec<MountConfig>,
    /// Gravity for 2D bodies, in pixels per second² (y grows downward).
    pub gravity_2d: [f32; 2],
    /// Gravity for 3D bodies, in metres per second².
    pub gravity_3d: [f32; 3],
    /// Input actions: a name scripts use ("jump") → the keys and buttons that trigger it
    /// (["space", "w", "pad_a"]). `KEY_DOWN("jump")` and `on_press_jump()` then work with any.
    pub actions: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        ProjectConfig { name: "New Game".into(), main_scene: "res://scenes/main.jscene".into(), window_size: [1280, 720], mounts: Vec::new(), gravity_2d: [0.0, 980.0], gravity_3d: [0.0, -9.81, 0.0], actions: Default::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MountConfig {
    /// The path prefix it answers to, like "hl2" for hl2://materials/...
    pub scheme: String,
    pub kind: MountKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
        let f = dir.join(PROJECT_FILE);
        let text = std::fs::read_to_string(&f).map_err(|e| format!("couldn't open {}: {}", f.display(), e))?;
        let config = ron::from_str(&text).map_err(|e| format!("{}: {}", f.display(), e))?;
        Ok(Project { dir: dir.to_path_buf(), config })
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
    /// Reads a res:// file as text (what scene instancing uses).
    pub fn read_text(&self, res: &str) -> Result<String, String> {
        let p = self.res_to_path(res).ok_or_else(|| format!("{} isn't a project path", res))?;
        std::fs::read_to_string(&p).map_err(|e| e.to_string())
    }
}
