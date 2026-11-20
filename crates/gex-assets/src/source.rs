//! Reading a Source-engine game install in place: each game folder's loose files and
//! its .vpk archives, searched in order (like the game's own search paths).

use std::path::{Path, PathBuf};

use crate::vpk::Vpk;

/// Half-Life 2's Steam app id. The episodes ship inside its install folder.
pub const HL2_APP_ID: u32 = 220;

/// Game folders to search, most specific first, when a mount doesn't list its own.
pub const HL2_GAMES: &[&str] = &["ep2", "episodic", "hl2", "hl2_complete"];

pub struct SourceGame {
    pub install: PathBuf,
    pub dirs: Vec<PathBuf>,
    pub vpks: Vec<Vpk>,
    /// Problems found while mounting (unreadable archives etc.), for the editor to show.
    pub warnings: Vec<String>,
}

impl SourceGame {
    pub fn mount(install: &Path, games: &[String]) -> SourceGame {
        let games: Vec<String> = if games.is_empty() { HL2_GAMES.iter().map(|s| s.to_string()).collect() } else { games.to_vec() };
        let mut g = SourceGame { install: install.to_path_buf(), dirs: Vec::new(), vpks: Vec::new(), warnings: Vec::new() };
        if !install.is_dir() {
            g.warnings.push(format!("{} doesn't exist", install.display()));
            return g;
        }
        for game in games {
            // A folder name inside the install, nothing that leads out of it.
            if game.is_empty() || game.contains(['/', '\\', ':']) || game == "." || game == ".." {
                continue;
            }
            let dir = install.join(&game);
            if !dir.is_dir() {
                continue;
            }
            let mut vpks: Vec<PathBuf> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().ends_with("_dir.vpk")))
                .collect();
            vpks.sort();
            for v in vpks {
                match Vpk::open(&v) {
                    Ok(v) => g.vpks.push(v),
                    Err(e) => g.warnings.push(e),
                }
            }
            g.dirs.push(dir);
        }
        if g.dirs.is_empty() {
            g.warnings.push(format!("no game folders found in {}", install.display()));
        }
        g
    }

    /// A path inside the game folders, or None when it would lead out of them.
    fn clean(path: &str) -> Option<&str> {
        let rel = path.trim_start_matches('/');
        (!rel.is_empty() && !rel.contains(':') && !rel.starts_with('\\') && !rel.split(['/', '\\']).any(|p| p == "..")).then_some(rel)
    }

    /// Whether a file is there (without reading it).
    pub fn has(&self, path: &str) -> bool {
        let Some(rel) = Self::clean(path) else { return false };
        let key = rel.to_ascii_lowercase().replace('\\', "/");
        self.dirs.iter().any(|d| d.join(rel).is_file()) || self.vpks.iter().any(|v| v.entries.contains_key(&key))
    }

    pub fn read(&self, path: &str) -> Option<Result<Vec<u8>, String>> {
        let rel = Self::clean(path)?;
        for d in &self.dirs {
            let p = d.join(rel);
            if p.is_file() {
                return Some(std::fs::read(&p).map_err(|e| e.to_string()));
            }
        }
        self.vpks.iter().find_map(|v| v.read(rel))
    }

    /// The folders and files directly inside `dir` ("" for the top).
    pub fn list(&self, dir: &str) -> (Vec<String>, Vec<String>) {
        let prefix = if dir.is_empty() { String::new() } else { format!("{}/", dir.trim_matches('/').to_ascii_lowercase()) };
        let mut folders = std::collections::BTreeSet::new();
        let mut files = std::collections::BTreeSet::new();
        for v in &self.vpks {
            for (k, _) in v.entries.range(prefix.clone()..) {
                let Some(rest) = k.strip_prefix(&prefix) else { break };
                match rest.split_once('/') {
                    Some((f, _)) => folders.insert(f.to_string()),
                    None => files.insert(rest.to_string()),
                };
            }
        }
        for d in &self.dirs {
            for e in std::fs::read_dir(d.join(&prefix)).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if e.path().is_dir() {
                    folders.insert(name);
                } else if !name.ends_with(".vpk") {
                    files.insert(name);
                }
            }
        }
        (folders.into_iter().collect(), files.into_iter().collect())
    }

    pub fn file_count(&self) -> usize {
        self.vpks.iter().map(|v| v.entries.len()).sum()
    }
}

/// Looks for a Steam game's install folder (Steam's own library list, then the usual places).
pub fn find_steam_game(app_id: u32, folder_name: &str) -> Option<PathBuf> {
    if let Ok(steam) = steamlocate::SteamDir::locate() {
        if let Ok(Some((app, lib))) = steam.find_app(app_id) {
            let p = lib.resolve_app_dir(&app);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let common = [
        r"C:\Program Files (x86)\Steam\steamapps\common",
        r"C:\Program Files\Steam\steamapps\common",
        r"D:\SteamLibrary\steamapps\common",
        r"E:\SteamLibrary\steamapps\common",
    ];
    common.iter().map(|c| Path::new(c).join(folder_name)).find(|p| p.is_dir())
}

pub fn find_hl2() -> Option<PathBuf> {
    find_steam_game(HL2_APP_ID, "Half-Life 2")
}
