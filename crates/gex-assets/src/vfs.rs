//! One place to read any asset path from.

use std::path::{Path, PathBuf};

use gex_core::project::{MountKind, ProjectConfig};

use crate::source::SourceGame;

pub enum MountImpl {
    Folder(PathBuf),
    Source(SourceGame),
}

pub struct Mount {
    pub scheme: String,
    pub imp: MountImpl,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    /// Full asset path ("res://a/b.png", "hl2://materials/x").
    pub path: String,
    pub is_dir: bool,
}

pub struct Vfs {
    pub project_dir: PathBuf,
    pub mounts: Vec<Mount>,
}

/// Where assets fetched from others go (working on a project together): mounted files from
/// another computer (its Half-Life 2, its asset folders) that this one doesn't have. Hidden, so
/// it isn't shared or exported.
pub const REMOTE_DIR: &str = ".gex/remote";

/// Paths under mounts that couldn't be read, by project, for working together to fetch (the
/// editor and a game it plays both read through their own Vfs).
type Misses = std::collections::HashMap<PathBuf, std::collections::HashSet<String>>;
static MISSES: std::sync::Mutex<Option<Misses>> = std::sync::Mutex::new(None);

/// What couldn't be read in a project since the last call (mounted paths only), and forgets them.
pub fn take_misses(project_dir: &Path) -> Vec<String> {
    MISSES.lock().ok().and_then(|mut m| m.as_mut()?.remove(project_dir)).map(|s| s.into_iter().collect()).unwrap_or_default()
}

fn missed(project_dir: &Path, path: &str) {
    if let Ok(mut m) = MISSES.lock() {
        let set = m.get_or_insert_with(Default::default).entry(project_dir.to_path_buf()).or_default();
        if set.len() < 10_000 {
            set.insert(path.to_string());
        }
    }
}

fn split(path: &str) -> Option<(&str, &str)> {
    path.split_once("://")
}

/// A path under `base` (never outside it: no "..", drive letters or absolute paths).
fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim_start_matches('/');
    if rel.contains(':') || rel.starts_with('\\') || rel.split(['/', '\\']).any(|p| p == "..") {
        return None;
    }
    Some(base.join(rel))
}

impl Vfs {
    pub fn new(project_dir: &Path, config: &ProjectConfig) -> Vfs {
        let mut v = Vfs { project_dir: project_dir.to_path_buf(), mounts: Vec::new() };
        for m in &config.mounts {
            let imp = match &m.kind {
                MountKind::Folder(p) => MountImpl::Folder(p.clone()),
                MountKind::SourceGame { install, games } => MountImpl::Source(SourceGame::mount(install, games)),
            };
            v.mounts.push(Mount { scheme: m.scheme.clone(), imp });
        }
        v
    }

    pub fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        for m in &self.mounts {
            if let MountImpl::Source(s) = &m.imp {
                w.extend(s.warnings.iter().map(|x| format!("{}://: {}", m.scheme, x)));
            }
        }
        w
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let (scheme, rel) = split(path).ok_or_else(|| format!("{} needs a scheme like res://", path))?;
        if scheme == "res" {
            let p = safe_join(&self.project_dir, rel).ok_or("paths can't leave the project")?;
            return std::fs::read(&p).map_err(|e| format!("{}: {}", path, e));
        }
        if scheme == "gex" {
            return crate::builtin::get(rel).map(|b| b.to_vec()).ok_or_else(|| format!("{} isn't one of the built-in assets", path));
        }
        let r = match self.mounts.iter().find(|m| m.scheme == scheme) {
            None => Err(format!("nothing is mounted at {}://", scheme)),
            Some(m) => match &m.imp {
                MountImpl::Folder(base) => {
                    let p = safe_join(base, rel).ok_or("paths can't leave the mount")?;
                    std::fs::read(&p).map_err(|e| format!("{}: {}", path, e))
                }
                MountImpl::Source(g) => g.read(rel).unwrap_or_else(|| Err(format!("{} isn't in the game files", path))),
            },
        };
        if r.is_ok() {
            return r;
        }
        // Not here: maybe fetched from someone working on the project with this computer.
        if let Some(b) = self.remote_path(scheme, rel).and_then(|p| std::fs::read(p).ok()) {
            return Ok(b);
        }
        missed(&self.project_dir, path);
        r
    }

    /// The plain file on disk behind a project or mounted-folder path, so another thread can
    /// read it (None for built-in assets and games' archives).
    pub fn disk_file(&self, path: &str) -> Option<PathBuf> {
        let (scheme, rel) = split(path)?;
        if scheme == "res" {
            return safe_join(&self.project_dir, rel).filter(|p| p.is_file());
        }
        self.folder_file(path).filter(|p| p.is_file())
    }

    /// Where a fetched copy of a mounted file goes.
    pub fn remote_path(&self, scheme: &str, rel: &str) -> Option<PathBuf> {
        if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return None;
        }
        let rel = rel.trim_start_matches('/');
        if rel.is_empty() || rel.contains(':') || rel.split(['/', '\\']).any(|p| p.is_empty() || p == "." || p == "..") {
            return None;
        }
        Some(self.project_dir.join(REMOTE_DIR).join(scheme).join(rel))
    }

    /// The file on disk behind a path in a mounted folder (not a game's archives).
    pub fn folder_file(&self, path: &str) -> Option<PathBuf> {
        let (scheme, rel) = split(path)?;
        match &self.mounts.iter().find(|m| m.scheme == scheme)?.imp {
            MountImpl::Folder(base) => safe_join(base, rel),
            MountImpl::Source(_) => None,
        }
    }

    /// Whether this computer can read a path itself (its own mounts, not fetched copies).
    pub fn has_locally(&self, path: &str) -> bool {
        let Some((scheme, rel)) = split(path) else { return false };
        let Some(m) = self.mounts.iter().find(|m| m.scheme == scheme) else { return false };
        match &m.imp {
            MountImpl::Folder(base) => safe_join(base, rel).is_some_and(|p| p.is_file()),
            MountImpl::Source(g) => g.has(rel),
        }
    }

    pub fn read_text(&self, path: &str) -> Result<String, String> {
        self.read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Whether a file may go into an exported game. Files from mounted game installs
    /// never do: they stay on the machine they were installed on.
    pub fn redistributable(&self, path: &str) -> bool {
        path.starts_with("res://") || path.starts_with("gex://")
    }

    /// The top-level places to browse.
    pub fn roots(&self) -> Vec<Entry> {
        let mut r = vec![
            Entry { name: "res://".into(), path: "res://".into(), is_dir: true },
            // The built-in placeholder pack.
            Entry { name: "gex:// (built-in)".into(), path: "gex://".into(), is_dir: true },
        ];
        for m in &self.mounts {
            r.push(Entry { name: format!("{}://", m.scheme), path: format!("{}://", m.scheme), is_dir: true });
        }
        // Mounts only another computer has: what was fetched from them.
        for e in std::fs::read_dir(self.project_dir.join(REMOTE_DIR)).into_iter().flatten().flatten() {
            let scheme = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !self.mounts.iter().any(|m| m.scheme == scheme) {
                r.push(Entry { name: format!("{}:// (from others)", scheme), path: format!("{}://", scheme), is_dir: true });
            }
        }
        r
    }

    /// What's directly inside a folder path (folders first, then files, sorted).
    pub fn list(&self, dir: &str) -> Vec<Entry> {
        let Some((scheme, rel)) = split(dir) else { return Vec::new() };
        let rel = rel.trim_matches('/');
        let base = |n: &str| if rel.is_empty() { format!("{}://{}", scheme, n) } else { format!("{}://{}/{}", scheme, rel, n) };
        let from_fs = |root: &Path| -> Vec<Entry> {
            let Some(d) = safe_join(root, rel) else { return Vec::new() };
            let mut v: Vec<Entry> = std::fs::read_dir(d)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    Entry { path: base(&name), is_dir: e.path().is_dir(), name }
                })
                .collect();
            v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            v
        };
        if scheme == "res" {
            return from_fs(&self.project_dir);
        }
        if scheme == "gex" {
            let (folders, files) = crate::builtin::list(rel);
            return folders
                .into_iter()
                .map(|n| Entry { path: base(&n), name: n, is_dir: true })
                .chain(files.into_iter().map(|n| Entry { path: base(&n), name: n, is_dir: false }))
                .collect();
        }
        let Some(m) = self.mounts.iter().find(|m| m.scheme == scheme) else {
            return from_fs(&self.project_dir.join(REMOTE_DIR).join(scheme));
        };
        match &m.imp {
            MountImpl::Folder(p) => from_fs(p),
            MountImpl::Source(g) => {
                let (folders, files) = g.list(rel);
                folders
                    .into_iter()
                    .map(|n| Entry { path: base(&n), name: n, is_dir: true })
                    .chain(files.into_iter().map(|n| Entry { path: base(&n), name: n, is_dir: false }))
                    .collect()
            }
        }
    }
}
