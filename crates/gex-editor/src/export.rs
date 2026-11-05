//! Exporting a game for Windows, Linux or macOS: the GEX player (renamed after the game, with the
//! project's icon on Windows) next to a copy of the project.
//!
//! The player for this computer is the editor itself. Players for other systems are looked for
//! in a `players` folder next to the editor: `gex-windows.exe`, `gex-linux`, `gex-macos`.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    MacOs,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Linux => "Linux",
            Platform::MacOs => "macOS",
        }
    }
    fn player_file(self) -> &'static str {
        match self {
            Platform::Windows => "gex-windows.exe",
            Platform::Linux => "gex-linux",
            Platform::MacOs => "gex-macos",
        }
    }
}

/// The players this editor can export with, and where each one is.
pub fn players() -> Vec<(Platform, PathBuf)> {
    let Ok(exe) = std::env::current_exe() else { return Vec::new() };
    let mut out = vec![(Platform::current(), exe.clone())];
    if let Some(dir) = exe.parent() {
        for p in [Platform::Windows, Platform::Linux, Platform::MacOs] {
            let f = dir.join("players").join(p.player_file());
            if p != Platform::current() && f.is_file() {
                out.push((p, f));
            }
        }
    }
    out
}

/// A file name that works everywhere.
pub fn program_name(name: &str) -> String {
    let n: String = name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let n = n.trim().to_string();
    if n.is_empty() {
        "game".into()
    } else {
        n
    }
}

/// Puts an icon into a Windows program (as its main icon, the one Explorer shows).
pub fn set_exe_icon(exe: &[u8], rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("the icon's pixels don't match its size")?;
    // Square it (icons are square) by padding with transparency.
    let side = w.max(h);
    let mut sq = image::RgbaImage::new(side, side);
    image::imageops::overlay(&mut sq, &img, ((side - w) / 2) as i64, ((side - h) / 2) as i64);
    let dynimg = image::DynamicImage::ImageRgba8(sq);
    let mut pe = editpe::Image::parse(exe).map_err(|e| format!("reading the Windows player: {}", e))?;
    let mut res = pe.resource_directory().cloned().unwrap_or_default();
    res.set_main_icon(dynimg).map_err(|e| format!("making the icon: {}", e))?;
    pe.set_resource_directory(res).map_err(|e| format!("adding the icon: {}", e))?;
    let mut out = Vec::new();
    pe.write_writer(&mut out).map_err(|e| format!("writing the program: {}", e))?;
    Ok(out)
}

/// Where the exported program and the game's files go, for one system. Returns (program path,
/// game folder).
pub fn layout(dest: &Path, platform: Platform, name: &str) -> (PathBuf, PathBuf) {
    match platform {
        Platform::Windows => (dest.join(format!("{}.exe", name)), dest.join("game")),
        Platform::Linux => (dest.join(name), dest.join("game")),
        Platform::MacOs => {
            let c = dest.join(format!("{}.app", name)).join("Contents");
            (c.join("MacOS").join(name), c.join("Resources").join("game"))
        }
    }
}

/// The macOS app bundle's description.
pub fn info_plist(name: &str) -> String {
    let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key><string>{name}</string>
	<key>CFBundleDisplayName</key><string>{name}</string>
	<key>CFBundleExecutable</key><string>{name}</string>
	<key>CFBundleIdentifier</key><string>gex.game.{id}</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleVersion</key><string>1.0</string>
	<key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
        name = name.replace('&', "and").replace('<', "").replace('>', ""),
        id = if id.is_empty() { "game".into() } else { id }
    )
}

/// The game folder an exported player plays, if it is one: `game` next to it, or, inside a
/// macOS app, `Contents/Resources/game`.
pub fn exported_game_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("game"), dir.join("../Resources/game")].into_iter().find(|d| d.join(gex_core::project::PROJECT_FILE).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_layouts() {
        assert_eq!(program_name("My Game: Part 2!"), "My Game_ Part 2_");
        assert_eq!(program_name("  "), "game");
        let (exe, game) = layout(Path::new("/out"), Platform::MacOs, "Dash");
        assert_eq!(exe, Path::new("/out/Dash.app/Contents/MacOS/Dash"));
        assert_eq!(game, Path::new("/out/Dash.app/Contents/Resources/game"));
        assert!(info_plist("Dash").contains("<string>gex.game.dash</string>"));
        let (exe, _) = layout(Path::new("/out"), Platform::Windows, "Dash");
        assert_eq!(exe, Path::new("/out/Dash.exe"));
    }

    /// Needs a Windows build of GEX: GEX_TEST_WINEXE=path/to/gex.exe
    #[test]
    #[ignore]
    fn icon_goes_into_a_windows_program() {
        let path = std::env::var("GEX_TEST_WINEXE").expect("GEX_TEST_WINEXE");
        let exe = std::fs::read(path).unwrap();
        let mut px = Vec::new();
        for y in 0..40u32 {
            for x in 0..64u32 {
                px.extend_from_slice(&[(x * 4) as u8, (y * 6) as u8, 200, 255]);
            }
        }
        let out = set_exe_icon(&exe, &px, 64, 40).unwrap();
        let pe = editpe::Image::parse(out.as_slice()).unwrap();
        let icon = pe.resource_directory().unwrap().get_main_icon().unwrap();
        assert!(icon.is_some_and(|i| !i.is_empty()));
    }
}
