//! Keeping GEX up to date. When the editor starts it asks GitHub for the newest release of
//! ASDFboy/gex; when that's newer than this copy, it downloads it in the background and swaps
//! it in (the running program is renamed out of the way first, which Windows allows), then
//! offers a restart. Uses the system's curl (Windows 10 and later have it), so GEX carries no
//! web library, and nothing runs once the check is done.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};

pub const REPO: &str = "ASDFboy/gex";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Off,
    Checking,
    Downloading(String),
    /// Installed; takes effect after a restart.
    Ready(String),
    UpToDate,
    Failed(String),
}

pub struct Updater {
    rx: Option<Receiver<State>>,
    pub state: State,
    /// Where this program was started from (after an update, the running copy has moved aside,
    /// and on Linux asking the system where it is would give the moved one).
    exe: Option<PathBuf>,
    /// The notice was closed.
    pub dismissed: bool,
}

impl Updater {
    /// Starts the check (unless turned off, or this is a development build).
    pub fn start() -> Updater {
        cleanup();
        let exe = std::env::current_exe().ok();
        if !enabled() || cfg!(debug_assertions) || std::env::var_os("GEX_NO_UPDATE").is_some() {
            return Updater { rx: None, state: State::Off, dismissed: false, exe };
        }
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("update".into())
            .spawn(move || {
                let tx2 = tx.clone();
                let r = run(&move |s| {
                    let _ = tx2.send(s);
                });
                let _ = tx.send(r.unwrap_or_else(State::Failed));
            })
            .ok();
        Updater { rx: Some(rx), state: State::Checking, dismissed: false, exe }
    }

    pub fn exe(&self) -> Option<&Path> {
        self.exe.as_deref()
    }

    pub fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            while let Ok(s) = rx.try_recv() {
                self.state = s;
            }
        }
    }
}

fn off_file() -> PathBuf {
    crate::state::config_dir().join("no_auto_update")
}

/// Whether GEX updates itself when it starts (for every project on this computer).
pub fn enabled() -> bool {
    !off_file().exists()
}

pub fn set_enabled(on: bool) {
    let f = off_file();
    if on {
        let _ = std::fs::remove_file(f);
    } else {
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, "GEX doesn't update itself while this file is here.\n");
    }
}

/// "0.10.1" is newer than "0.9.3".
pub fn newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect() };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

fn curl() -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new("curl");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        c.creation_flags(0x0800_0000);
    }
    c.args(["-fsSL", "-H", "User-Agent: GEX"]);
    c
}

/// The release asset for this system: the program itself.
fn asset_name() -> &'static str {
    if cfg!(windows) {
        "gex-windows.exe"
    } else {
        "gex-linux"
    }
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_file_name(if cfg!(windows) { "gex.old.exe" } else { "gex.old" })
}

/// The copy replaced by the last update is no longer running: remove it.
fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
        let _ = std::fs::remove_file(exe.with_file_name("gex.update.part"));
    }
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let out = curl().args(["--max-time", "900", "-o"]).arg(to).arg(url).output().map_err(|e| format!("couldn't run curl: {}", e))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(to);
        return Err(format!("download failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

fn run(report: &dyn Fn(State)) -> Result<State, String> {
    let out = curl()
        .args(["--max-time", "15", "-H", "Accept: application/vnd.github+json"])
        .arg(format!("https://api.github.com/repos/{}/releases/latest", REPO))
        .output()
        .map_err(|e| format!("couldn't run curl: {}", e))?;
    if !out.status.success() {
        return Err("couldn't reach GitHub".into());
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let tag = j["tag_name"].as_str().ok_or("no release found")?;
    let latest = tag.trim_start_matches(['v', 'V']).to_string();
    if !newer(&latest, VERSION) {
        return Ok(State::UpToDate);
    }
    let assets = j["assets"].as_array().cloned().unwrap_or_default();
    let url = |name: &str| assets.iter().find(|a| a["name"].as_str() == Some(name)).and_then(|a| a["browser_download_url"].as_str()).map(str::to_string);
    let Some(main_url) = url(asset_name()) else { return Err(format!("release {} has no {}", latest, asset_name())) };
    report(State::Downloading(latest.clone()));
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let part = exe.with_file_name("gex.update.part");
    download(&main_url, &part).map_err(|e| format!("{} (is the GEX folder writable?)", e))?;
    let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if size < 1 << 20 {
        let _ = std::fs::remove_file(&part);
        return Err("the download was incomplete".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
    }
    // Swap: the running copy steps aside, the new one takes its name.
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("couldn't replace GEX: {}", e))?;
    if let Err(e) = std::fs::rename(&part, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("couldn't replace GEX: {}", e));
    }
    // The Linux player Windows exports with, when it's there.
    if cfg!(windows) {
        let player = exe.with_file_name("players").join("gex-linux");
        if let (true, Some(u)) = (player.exists(), url("gex-linux")) {
            let tmp = player.with_file_name("gex-linux.part");
            if download(&u, &tmp).is_ok() && std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0) > 1 << 20 {
                let _ = std::fs::remove_file(&player);
                let _ = std::fs::rename(&tmp, &player);
            } else {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    Ok(State::Ready(latest))
}

/// Starts the new copy with the same project and closes this one.
pub fn restart(ctx: &eframe::egui::Context, exe: Option<&Path>, project: Option<&Path>) {
    let Some(exe) = exe else { return };
    let mut c = Command::new(exe);
    if let Some(p) = project {
        c.arg(p);
    }
    if c.spawn().is_ok() {
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions_compare_by_number() {
        use super::newer;
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(newer("0.1.1", "0.1"));
    }
}
