//! One place to read any asset path from.

use std::path::{Path, PathBuf};

use jle_core::project::{MountKind, ProjectConfig};

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

fn split(path: &str) -> Option<(&str, &str)> {
    path.split_once("://")
}

fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    if rel.split(['/', '\\']).any(|p| p == "..") {
        return None;
    }
    Some(base.join(rel.trim_start_matches('/')))
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
        let m = self.mounts.iter().find(|m| m.scheme == scheme).ok_or_else(|| format!("nothing is mounted at {}://", scheme))?;
        match &m.imp {
            MountImpl::Folder(base) => {
                let p = safe_join(base, rel).ok_or("paths can't leave the mount")?;
                std::fs::read(&p).map_err(|e| format!("{}: {}", path, e))
            }
            MountImpl::Source(g) => g.read(rel).unwrap_or_else(|| Err(format!("{} isn't in the game files", path))),
        }
    }

    pub fn read_text(&self, path: &str) -> Result<String, String> {
        self.read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Whether a file may go into an exported game. Files from mounted game installs
    /// never do: they stay on the machine they were installed on.
    pub fn redistributable(&self, path: &str) -> bool {
        path.starts_with("res://")
    }

    /// The top-level places to browse.
    pub fn roots(&self) -> Vec<Entry> {
        let mut r = vec![Entry { name: "res://".into(), path: "res://".into(), is_dir: true }];
        for m in &self.mounts {
            r.push(Entry { name: format!("{}://", m.scheme), path: format!("{}://", m.scheme), is_dir: true });
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
        let Some(m) = self.mounts.iter().find(|m| m.scheme == scheme) else { return Vec::new() };
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
