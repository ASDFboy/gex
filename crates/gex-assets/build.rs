//! Embeds the built-in placeholder pack (builtin/, made by tools/make_placeholders.py) so every
//! GEX program can read it at gex://, including exported games.

use std::fmt::Write;
use std::path::Path;

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, root, out);
        } else {
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, p.canonicalize().unwrap().to_string_lossy().into_owned()));
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin");
    println!("cargo:rerun-if-changed=builtin");
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.sort();
    let mut src = String::from("/// (path under gex://, bytes), sorted by path.\npub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in &files {
        writeln!(src, "    ({:?}, include_bytes!({:?})),", rel, abs).unwrap();
    }
    src.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin_files.rs");
    std::fs::write(out, src).unwrap();
}
