//! Keeping GEX up to date. When the editor starts (and every 6 hours while it's open) it asks
//! GitHub for the newest release of ASDFboy/gex. A newer one is downloaded in the background
//! (or first offered, as chosen in Updates), checked against the release's SHA-256 list, and
//! swapped in: the running program is renamed out of the way (Windows allows that) and kept, so
//! Updates can go back to it. Then a restart is offered.
//!
//! Security: only https (curl is told to refuse anything else, redirects included), only
//! downloads from this repository's releases, versions and sizes checked, and a file whose
//! SHA-256 doesn't match the release's list is thrown away. Uses the system's curl (Windows 10
//! and later have it), so GEX carries no web library and nothing runs between checks.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::Digest;

pub const REPO: &str = "ASDFboy/gex";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RECHECK: Duration = Duration::from_secs(6 * 3600);
const CHECKSUMS: &str = "SHA256SUMS";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Mode {
    /// Download and install by itself, then offer a restart.
    #[default]
    Auto,
    /// Say there's a new version and wait for Install.
    Ask,
    /// Don't check.
    Off,
}

/// Update settings for this computer (every project).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub mode: Mode,
    /// A version not to offer again.
    pub skip: Option<String>,
}

fn settings_file() -> PathBuf {
    crate::state::config_dir().join("updates.ron")
}

impl Settings {
    pub fn load() -> Settings {
        if let Some(s) = std::fs::read_to_string(settings_file()).ok().and_then(|t| ron::from_str(&t).ok()) {
            return s;
        }
        // Before 0.3: a file that turned updating off.
        let old = crate::state::config_dir().join("no_auto_update");
        Settings { mode: if old.exists() { Mode::Off } else { Mode::Auto }, ..Default::default() }
    }
    pub fn save(&self) {
        let f = settings_file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(t) = ron::to_string(self) {
            let _ = std::fs::write(f, t);
        }
        let _ = std::fs::remove_file(crate::state::config_dir().join("no_auto_update"));
    }
}

/// A release newer than this copy.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// What's new, as written on the release.
    pub notes: String,
    url: String,
    size: u64,
    sums_url: String,
    linux_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Off,
    Checking,
    /// Asking first: there's a new version.
    Available(Release),
    Downloading(Release),
    /// Installed; takes effect after a restart.
    Ready(Release),
    UpToDate,
    Failed(String),
}

pub struct Updater {
    rx: Option<Receiver<State>>,
    pub state: State,
    pub settings: Settings,
    /// The notice was closed.
    pub dismissed: bool,
    /// The Updates window is open.
    pub window: bool,
    /// What's new is showing.
    pub notes: bool,
    /// Where this program was started from (after an update the running copy has moved aside,
    /// and on Linux asking the system where it is would give the moved one).
    exe: Option<PathBuf>,
    checked: Instant,
}

impl Updater {
    /// Starts the first check (unless turned off, or this is a development build).
    pub fn start() -> Updater {
        cleanup();
        let mut u = Updater { rx: None, state: State::Off, settings: Settings::load(), dismissed: false, window: false, notes: false, exe: std::env::current_exe().ok(), checked: Instant::now() };
        if u.settings.mode != Mode::Off && !cfg!(debug_assertions) && std::env::var_os("GEX_NO_UPDATE").is_none() {
            u.check();
        }
        u
    }

    /// Asks GitHub now.
    pub fn check(&mut self) {
        if matches!(self.state, State::Checking | State::Downloading(_) | State::Ready(_)) {
            return;
        }
        self.checked = Instant::now();
        let (tx, rx) = channel();
        let (auto, skip, exe) = (self.settings.mode == Mode::Auto, self.settings.skip.clone(), self.exe.clone());
        let started = std::thread::Builder::new().name("update".into()).spawn(move || {
            let tx2 = tx.clone();
            let r = latest().and_then(|rel| match rel {
                None => Ok(State::UpToDate),
                Some(rel) if skip.as_deref() == Some(rel.version.as_str()) => Ok(State::UpToDate),
                Some(rel) if !auto => Ok(State::Available(rel)),
                Some(rel) => {
                    let _ = tx2.send(State::Downloading(rel.clone()));
                    install(&rel, exe.as_deref()).map(|_| State::Ready(rel))
                }
            });
            let _ = tx.send(r.unwrap_or_else(State::Failed));
        });
        if started.is_ok() {
            self.state = State::Checking;
            self.rx = Some(rx);
            self.dismissed = false;
        }
    }

    /// Downloads and installs an offered release.
    pub fn install(&mut self) {
        let State::Available(rel) = self.state.clone() else { return };
        let (tx, rx) = channel();
        let exe = self.exe.clone();
        self.state = State::Downloading(rel.clone());
        self.rx = Some(rx);
        let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
            let r = install(&rel, exe.as_deref()).map(|_| State::Ready(rel));
            let _ = tx.send(r.unwrap_or_else(State::Failed));
        });
    }

    /// Don't offer this version again.
    pub fn skip(&mut self) {
        if let State::Available(r) = &self.state {
            self.settings.skip = Some(r.version.clone());
            self.settings.save();
            self.state = State::UpToDate;
        }
    }

    pub fn exe(&self) -> Option<&Path> {
        self.exe.as_deref()
    }

    /// Picks up results; looks again every 6 hours. Gives how far a download is (0 to 1).
    pub fn poll(&mut self) -> Option<f32> {
        if let Some(rx) = &self.rx {
            while let Ok(s) = rx.try_recv() {
                if let State::Ready(_) = &s {
                    self.settings = Settings::load();
                }
                self.state = s;
            }
        }
        if matches!(self.state, State::UpToDate | State::Failed(_)) && self.settings.mode != Mode::Off && self.checked.elapsed() > RECHECK && !cfg!(debug_assertions) {
            self.check();
        }
        match &self.state {
            State::Downloading(r) => {
                let part = self.exe.as_deref().map(|e| e.with_file_name("gex.update.part"));
                let got = part.and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
                Some((got as f32 / r.size.max(1) as f32).clamp(0.0, 1.0))
            }
            _ => None,
        }
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
    // https only, redirects included, and fail on HTTP errors.
    c.args(["-fsSL", "--proto", "=https", "--proto-redir", "=https", "--tlsv1.2", "-H", "User-Agent: GEX"]);
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

/// Where the running copy steps aside while an update takes its place (removed on the next start).
fn old_path(exe: &Path) -> PathBuf {
    exe.with_file_name(if cfg!(windows) { "gex.old.exe" } else { "gex.old" })
}

/// Removes the version an update replaced, and leftovers of an interrupted one. In the
/// background, trying for a little while: just after an update the old copy may still be closing
/// (Windows can't remove a program that's running).
fn cleanup() {
    let Ok(exe) = std::env::current_exe() else { return };
    let files: Vec<PathBuf> = ["gex.update.part", "gex.sums.part", "gex.rollback.tmp", "gex.old.exe", "gex.old", "gex.previous.exe", "gex.previous"].iter().map(|f| exe.with_file_name(f)).filter(|f| f.exists()).collect();
    if files.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..40 {
            if files.iter().all(|f| std::fs::remove_file(f).is_ok() || !f.exists()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
}

/// Only this repository's release downloads.
fn trusted(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{}/releases/download/", REPO)) && !url.contains("..") && url.len() < 512
}

fn download(url: &str, to: &Path, max: u64) -> Result<(), String> {
    if !trusted(url) {
        return Err("refused a download from outside GEX's releases".into());
    }
    let out = curl().args(["--max-time", "900", "--max-filesize"]).arg(max.to_string()).arg("-o").arg(to).arg(url).output().map_err(|e| format!("couldn't run curl: {}", e))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(to);
        return Err(format!("download failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

fn sha256_file(p: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

/// The newest release, when it's newer than this copy.
fn latest() -> Result<Option<Release>, String> {
    let out = curl()
        .args(["--max-time", "20", "--max-filesize", "4000000", "-H", "Accept: application/vnd.github+json"])
        .arg(format!("https://api.github.com/repos/{}/releases/latest", REPO))
        .output()
        .map_err(|e| format!("couldn't run curl: {}", e))?;
    if !out.status.success() {
        return Err("couldn't reach GitHub".into());
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let tag = j["tag_name"].as_str().ok_or("no release found")?;
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    if version.is_empty() || version.len() > 20 || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(format!("a release with an odd version ({:?})", tag));
    }
    if !newer(&version, VERSION) {
        return Ok(None);
    }
    let assets = j["assets"].as_array().cloned().unwrap_or_default();
    let find = |name: &str| assets.iter().find(|a| a["name"].as_str() == Some(name)).and_then(|a| Some((a["browser_download_url"].as_str()?.to_string(), a["size"].as_u64().unwrap_or(0))));
    let (url, size) = find(asset_name()).ok_or_else(|| format!("release {} has no {}", version, asset_name()))?;
    let (sums_url, _) = find(CHECKSUMS).ok_or_else(|| format!("release {} has no checksums, so it can't be checked", version))?;
    if !trusted(&url) || !trusted(&sums_url) {
        return Err("the release points outside GEX's releases".into());
    }
    if !(1 << 20..=1 << 30).contains(&size) {
        return Err("the release's program has an odd size".into());
    }
    let notes: String = j["body"].as_str().unwrap_or("").chars().filter(|c| !c.is_control() || *c == '\n').take(4000).collect();
    let linux_url = if cfg!(windows) { find("gex-linux").map(|a| a.0).filter(|u| trusted(u)) } else { None };
    Ok(Some(Release { version, notes, url, size, sums_url, linux_url }))
}

/// The SHA-256 the release lists for a file.
fn listed_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, n) = (it.next()?, it.next()?);
        (n.trim_start_matches('*') == name && h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
    })
}

/// Downloads, checks and swaps in a release; the old copy is removed on the next start.
fn install(rel: &Release, exe: Option<&Path>) -> Result<(), String> {
    let exe = exe.ok_or("can't tell where GEX is")?;
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let sums_file = exe.with_file_name("gex.sums.part");
    download(&rel.sums_url, &sums_file, 64 << 10)?;
    let sums = std::fs::read_to_string(&sums_file).unwrap_or_default();
    let _ = std::fs::remove_file(&sums_file);
    let want = listed_sum(&sums, asset_name()).ok_or("the release's checksum list doesn't include this system's program")?;
    let part = exe.with_file_name("gex.update.part");
    download(&rel.url, &part, rel.size + 1).map_err(|e| format!("{} (is the GEX folder writable?)", e))?;
    if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) != rel.size || sha256_file(&part)? != want {
        let _ = std::fs::remove_file(&part);
        return Err("the download didn't match the release's checksum, so it was thrown away".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
    }
    // Swap: the running copy steps aside (it can't be removed while running; the new one
    // removes it when it starts), the new one takes its name.
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("couldn't replace GEX: {}", e))?;
    if let Err(e) = std::fs::rename(&part, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("couldn't replace GEX: {}", e));
    }
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(&old);
    // The Linux player Windows exports with, when it's there.
    if let (Some(u), Some(want)) = (&rel.linux_url, listed_sum(&sums, "gex-linux")) {
        let player = exe.with_file_name("players").join("gex-linux");
        if player.exists() {
            let tmp = player.with_file_name("gex-linux.part");
            if download(u, &tmp, 1 << 30).is_ok() && sha256_file(&tmp).ok().as_deref() == Some(want.as_str()) {
                let _ = std::fs::remove_file(&player);
                let _ = std::fs::rename(&tmp, &player);
            } else {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    Ok(())
}

/// Starts the given program with the same project and closes this one.
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
    use super::*;

    #[test]
    fn settings_from_older_versions_still_load() {
        let s: Settings = ron::from_str(r#"(mode: Ask, skip: Some("0.3.0"), previous: Some("0.2.0"))"#).unwrap();
        assert_eq!(s.mode, Mode::Ask);
        assert_eq!(s.skip.as_deref(), Some("0.3.0"));
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
        assert!(newer("0.1.1", "0.1"));
    }

    #[test]
    fn only_trusted_downloads_and_listed_checksums() {
        assert!(trusted("https://github.com/ASDFboy/gex/releases/download/v0.3.0/gex-linux"));
        assert!(!trusted("http://github.com/ASDFboy/gex/releases/download/v0.3.0/gex-linux"));
        assert!(!trusted("https://github.com/someone/gex/releases/download/v1/gex-linux"));
        assert!(!trusted("https://github.com/ASDFboy/gex/releases/download/../../x"));
        assert!(!trusted("https://evil.example/https://github.com/ASDFboy/gex/releases/download/"));
        let h = "a".repeat(64);
        let sums = format!("{}  gex-linux\n{}  GEX.zip\n", h, "b".repeat(64));
        assert_eq!(listed_sum(&sums, "gex-linux"), Some(h));
        assert_eq!(listed_sum(&sums, "gex-windows.exe"), None);
        assert_eq!(listed_sum("zz  gex-linux", "gex-linux"), None);
        let dir = std::env::temp_dir().join(format!("gex-sha-{}", std::process::id()));
        std::fs::write(&dir, b"abc").unwrap();
        assert_eq!(sha256_file(&dir).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_file(&dir);
    }
}
