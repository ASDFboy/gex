//! Valve Material Type (.vmt) files: just enough KeyValues to find a material's
//! textures (`$basetexture`, and `include` for patch materials).

use std::collections::HashMap;

pub struct Material {
    /// The shader name ("LightmappedGeneric", "VertexLitGeneric", "patch", ...).
    pub shader: String,
    /// Keys lowercased, values as written. Nested blocks are flattened.
    pub keys: HashMap<String, String>,
}

impl Material {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.keys.get(&k.to_ascii_lowercase()).map(|s| s.as_str())
    }
    /// The base texture as a path under materials/, with .vtf.
    pub fn base_texture(&self) -> Option<String> {
        let t = self.get("$basetexture")?.replace('\\', "/").to_ascii_lowercase();
        let t = t.trim_start_matches("materials/").trim_end_matches(".vtf");
        Some(format!("materials/{}.vtf", t))
    }
}

fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let c: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            ch if ch.is_whitespace() => i += 1,
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
            }
            '{' | '}' => {
                out.push(c[i].to_string());
                i += 1;
            }
            '"' => {
                let s = i + 1;
                i += 1;
                while i < c.len() && c[i] != '"' {
                    i += 1;
                }
                out.push(c[s..i.min(c.len())].iter().collect());
                i += 1;
            }
            _ => {
                let s = i;
                while i < c.len() && !c[i].is_whitespace() && c[i] != '{' && c[i] != '}' && c[i] != '"' {
                    i += 1;
                }
                out.push(c[s..i].iter().collect());
            }
        }
    }
    out
}

pub fn parse(text: &str) -> Material {
    let t = tokens(text.trim_start_matches('\u{feff}'));
    let shader = t.first().cloned().unwrap_or_default();
    let mut keys = HashMap::new();
    let mut i = 1;
    while i < t.len() {
        if t[i] == "{" || t[i] == "}" {
            i += 1;
            continue;
        }
        match t.get(i + 1).map(|s| s.as_str()) {
            Some("{") => i += 2, // a nested block: flatten its contents
            Some(v) => {
                keys.entry(t[i].to_ascii_lowercase()).or_insert_with(|| v.to_string());
                i += 2;
            }
            None => break,
        }
    }
    Material { shader, keys }
}
