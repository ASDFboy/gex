//! Project file operations for the Assets panel: new folders, importing, renaming (fixing every
//! reference in scenes and scripts) and deleting (into a hidden .trash folder, so it can be undone
//! by hand).

use std::path::{Path, PathBuf};

use gex_core::project::Project;

/// Files a rename looks inside to fix references.
fn text_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if p.is_dir() {
            text_files(&p, out);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("jscene" | "jlb" | "jproj" | "txt" | "json" | "vmt")) {
            out.push(p);
        }
    }
}

/// A name that's safe as one path part.
pub fn check_name(name: &str) -> Result<&str, String> {
    let n = name.trim();
    if n.is_empty() || n == "." || n == ".." || n.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) || n.starts_with('.') {
        return Err(format!("{:?} can't be used as a file name", name));
    }
    Ok(n)
}

fn join_res(dir: &str, name: &str) -> String {
    if dir.ends_with("://") {
        format!("{}{}", dir, name)
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

pub fn new_folder(p: &Project, dir: &str, name: &str) -> Result<String, String> {
    let res = join_res(dir, check_name(name)?);
    let path = p.res_to_path(&res).ok_or("not a project folder")?;
    if path.exists() {
        return Err(format!("{} already exists", res));
    }
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(res)
}

/// A file in `dir` called `name`, or `name 2`, `name 3`... if that's taken.
pub fn free_name(p: &Project, dir: &str, name: &str) -> Option<String> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{}", e)),
        _ => (name.to_string(), String::new()),
    };
    (1..10000).map(|i| if i == 1 { join_res(dir, name) } else { join_res(dir, &format!("{}_{}{}", stem, i, ext)) }).find(|r| p.res_to_path(r).is_some_and(|x| !x.exists()))
}

/// Copies files (and folders) from anywhere on disk into a project folder. Gives the new paths.
pub fn import(p: &Project, dir: &str, files: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut made = Vec::new();
    for f in files {
        let name = f.file_name().ok_or("no file name")?.to_string_lossy().into_owned();
        let res = free_name(p, dir, &name).ok_or("no free name")?;
        let to = p.res_to_path(&res).ok_or("not a project folder")?;
        if to.starts_with(f) {
            return Err("can't copy a folder into itself".into());
        }
        copy_all(f, &to).map_err(|e| format!("{}: {}", f.display(), e))?;
        made.push(res);
    }
    Ok(made)
}

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            copy_all(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Swaps `old` for `new` where it's a whole path (or the start of one, for folders) inside quotes.
pub fn replace_ref(text: &str, old: &str, new: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut changed = false;
    while let Some(i) = rest.find(old) {
        let after = &rest[i + old.len()..];
        let ends = after.starts_with('"') || after.starts_with('/');
        let starts = rest[..i].ends_with('"');
        out.push_str(&rest[..i]);
        if ends && starts {
            out.push_str(new);
            changed = true;
        } else {
            out.push_str(old);
        }
        rest = after;
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// Renames a file or folder and fixes every `"res://..."` that pointed at it in the project's
/// scenes, scripts and settings. Gives the new path and the files it fixed.
pub fn rename(p: &Project, old: &str, new_name: &str) -> Result<(String, Vec<String>), String> {
    let from = p.res_to_path(old).ok_or("not a project file")?;
    if !from.exists() {
        return Err(format!("{} doesn't exist", old));
    }
    let parent = old.rsplit_once('/').map(|(d, _)| d.to_string()).filter(|d| d.contains("://") && !d.ends_with(':')).unwrap_or_else(|| "res://".into());
    let new = join_res(&parent, check_name(new_name)?);
    let to = p.res_to_path(&new).ok_or("not a project path")?;
    if to.exists() {
        return Err(format!("{} already exists", new));
    }
    std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    text_files(&p.dir, &mut files);
    let mut fixed = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        if let Some(t) = replace_ref(&text, old, &new) {
            if std::fs::write(&f, t).is_ok() {
                fixed.push(p.path_to_res(&f).unwrap_or_default());
            }
        }
    }
    Ok((new, fixed))
}

/// Moves a file or folder into the project's hidden `.trash` folder (not shown in the editor,
/// never exported). Gives where it went on disk.
pub fn trash(p: &Project, res: &str) -> Result<PathBuf, String> {
    let from = p.res_to_path(res).ok_or("not a project file")?;
    if from == p.dir {
        return Err("that's the whole project".into());
    }
    if !from.exists() {
        return Err(format!("{} doesn't exist", res));
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let rel = res.trim_start_matches("res://");
    let to = p.dir.join(".trash").join(stamp.to_string()).join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
    Ok(to)
}

/// Opens a folder in the system's file manager.
pub fn reveal(path: &Path) {
    let dir = if path.is_dir() { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_only_whole_paths() {
        let t = r#"scripts: ["res://scripts/a.jlb", "res://scripts/ab.jlb"], x: "res://scripts/a.jlbx""#;
        assert_eq!(replace_ref(t, "res://scripts/a.jlb", "res://scripts/b.jlb").unwrap(), r#"scripts: ["res://scripts/b.jlb", "res://scripts/ab.jlb"], x: "res://scripts/a.jlbx""#);
        // Folders: anything under them.
        let t = r#"Res("res://art/hero.png") Res("res://artwork/x.png")"#;
        assert_eq!(replace_ref(t, "res://art", "res://gfx").unwrap(), r#"Res("res://gfx/hero.png") Res("res://artwork/x.png")"#);
        assert!(replace_ref("nothing here", "res://a", "res://b").is_none());
    }

    #[test]
    fn rename_import_trash() {
        let dir = std::env::temp_dir().join(format!("gex-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = crate::templates::create_demo(&dir).unwrap();
        let (new, fixed) = rename(&p, "res://scripts/player.jlb", "hero_moves.jlb").unwrap();
        assert_eq!(new, "res://scripts/hero_moves.jlb");
        assert!(fixed.contains(&"res://scenes/main.jscene".to_string()), "{:?}", fixed);
        let main = std::fs::read_to_string(dir.join("scenes/main.jscene")).unwrap();
        assert!(main.contains("res://scripts/hero_moves.jlb") && !main.contains("res://scripts/player.jlb"));
        // A folder rename fixes everything under it, and the game still loads.
        rename(&p, "res://assets", "art").unwrap();
        assert!(std::fs::read_to_string(dir.join("scenes/platformer.jscene")).unwrap().contains("res://art/tiles.png"));
        let tree = gex_core::scene::load(&std::fs::read_to_string(dir.join("scenes/main.jscene")).unwrap(), &|r: &str| p.read_text(r)).unwrap();
        assert!(tree.find_path(None, "Level/Player").is_some());
        // Import copies, picking a free name.
        let ext = dir.join("outside.txt");
        std::fs::write(&ext, "hi").unwrap();
        let made = import(&p, "res://art", &[ext.clone(), ext.clone()]).unwrap();
        assert_eq!(made, vec!["res://art/outside.txt".to_string(), "res://art/outside_2.txt".to_string()]);
        assert!(new_folder(&p, "res://", "levels").is_ok());
        assert!(new_folder(&p, "res://", "../escape").is_err());
        // Trash moves it out of sight but keeps it.
        let gone = trash(&p, "res://art/outside.txt").unwrap();
        assert!(gone.is_file() && !dir.join("art/outside.txt").exists());
        assert!(trash(&p, "res://").is_err());
    }

    #[test]
    fn editor_follows_a_rename() {
        let dir = std::env::temp_dir().join(format!("gex-files-st-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::templates::create_demo(&dir).unwrap();
        let mut st = crate::state::EditorState::new(Project::open(&dir).unwrap());
        st.open_scene("res://scenes/main.jscene");
        st.open_script("res://scripts/player.jlb", None);
        assert!(!st.scene_dirty());
        st.rename_asset("res://scripts", "code").unwrap();
        let player = st.tree.find_path(None, "Level/Player").unwrap();
        assert_eq!(st.tree.nodes[player].scripts, vec!["res://code/player.jlb".to_string()]);
        assert!(st.scripts.contains_key("res://code/player.jlb"));
        assert_eq!(st.renamed_scripts, vec![("res://scripts/player.jlb".to_string(), "res://code/player.jlb".to_string())]);
        assert!(!st.scene_dirty(), "the open scene matches the fixed file");
        st.delete_asset("res://code/player.jlb").unwrap();
        assert!(!st.scripts.contains_key("res://code/player.jlb"));
        assert!(st.delete_asset("res://scenes/main.jscene").is_err(), "open scenes can't be deleted");
    }
}
