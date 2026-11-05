//! Any image path to RGBA8 pixels: common formats via the `image` crate, Valve
//! textures (.vtf) and materials (.vmt, through their $basetexture).

use crate::vfs::Vfs;

#[derive(Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn load_image(vfs: &Vfs, path: &str) -> Result<Rgba, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".vmt") {
        let m = crate::vmt::parse(&vfs.read_text(path)?);
        let scheme = path.split_once("://").map(|s| s.0).unwrap_or("res");
        let tex = m.base_texture().ok_or_else(|| format!("{} has no $basetexture", path))?;
        return load_image(vfs, &format!("{}://{}", scheme, tex));
    }
    let bytes = vfs.read(path)?;
    if lower.ends_with(".vtf") {
        return crate::vtf::decode(&bytes).map_err(|e| format!("{}: {}", path, e));
    }
    let img = image::load_from_memory(&bytes).map_err(|e| format!("{}: {}", path, e))?.to_rgba8();
    Ok(Rgba { width: img.width(), height: img.height(), pixels: img.into_raw() })
}

pub fn is_image(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".tga", ".bmp", ".vtf", ".vmt"].iter().any(|e| l.ends_with(e))
}
