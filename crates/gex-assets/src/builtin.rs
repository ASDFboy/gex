//! The built-in placeholder pack, readable at gex://: shapes, patterns, solid colors, UI
//! pieces, icons, characters, particles and skies. It's inside the program, so exported games
//! have it too.

include!(concat!(env!("OUT_DIR"), "/builtin_files.rs"));

/// A built-in file's bytes ("shapes/white/star.png").
pub fn get(rel: &str) -> Option<&'static [u8]> {
    let rel = rel.trim_start_matches('/');
    FILES.binary_search_by(|(p, _)| p.cmp(&rel)).ok().map(|i| FILES[i].1)
}

/// What's directly inside a built-in folder ("" is the top): (folders, files), sorted.
pub fn list(dir: &str) -> (Vec<String>, Vec<String>) {
    let dir = dir.trim_matches('/');
    let prefix = if dir.is_empty() { String::new() } else { format!("{}/", dir) };
    let mut folders: Vec<String> = Vec::new();
    let mut files = Vec::new();
    for (p, _) in FILES.iter() {
        let Some(rest) = p.strip_prefix(prefix.as_str()) else { continue };
        match rest.split_once('/') {
            Some((f, _)) => {
                if folders.last().map(|l| l != f).unwrap_or(true) && !folders.iter().any(|x| x == f) {
                    folders.push(f.to_string());
                }
            }
            None => files.push(rest.to_string()),
        }
    }
    folders.sort();
    (folders, files)
}
