//! Starting up safely: a log file, a message box when something goes wrong (release builds on
//! Windows have no console to print to), and trying other graphics APIs when the first one
//! won't start.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// The graphics APIs to try, in order, when GEX can't start with the default choice.
const FALLBACKS: [&str; 3] = ["vulkan", "dx12", "gl"];

/// Set once the window has drawn a frame: a window that closes before that didn't start.
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn mark_running() {
    if !RUNNING.swap(true, std::sync::atomic::Ordering::Relaxed) {
        note("window is up");
    }
}

pub fn log_path() -> PathBuf {
    crate::state::config_dir().join("gex.log")
}

struct FileLog(Mutex<Option<std::fs::File>>);

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn || (m.level() <= log::Level::Info && m.target().starts_with("gex"))
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!("[{}] {}: {}\n", r.level(), r.target(), r.args());
        if let Some(f) = self.0.lock().unwrap().as_mut() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
        eprint!("{}", line);
    }
    fn flush(&self) {}
}

/// Write a line to the log.
pub fn note(s: &str) {
    log::info!(target: "gex", "{}", s);
}

#[cfg(not(windows))]
fn message(title: &str, text: &str) {
    // Elsewhere GEX is usually started from a terminal, which shows this.
    eprintln!("{}: {}", title, text);
}

#[cfg(windows)]
fn message(title: &str, text: &str) {
    let _ = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title(title).set_description(text).set_buttons(rfd::MessageButtons::Ok).show();
}

pub fn init() {
    let path = log_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    // A retry appends to the log of the first attempt.
    let retry = std::env::var_os("GEX_BACKEND_TRY").is_some();
    let file = std::fs::OpenOptions::new().create(true).write(true).append(retry).truncate(!retry).open(&path).ok();
    if log::set_boxed_logger(Box::new(FileLog(Mutex::new(file)))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    note(&format!(
        "GEX {} on {} {}, graphics: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("WGPU_BACKEND").unwrap_or_else(|_| "default".into())
    ));
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
        let what = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown error".into());
        log::error!(target: "gex", "crashed: {}{}\n{}", what, at, std::backtrace::Backtrace::force_capture());
        default_hook(info);
        // Only the main thread's crash ends the program; game-thread crashes are reported in
        // the editor.
        if std::thread::current().name() == Some("main") {
            message("GEX crashed", &format!("{}{}\n\nDetails are in {}", what, at, log_path().display()));
        }
    }));
}

/// Runs the window. If it can't start (usually the graphics driver and the graphics API not
/// getting along), starts GEX again with the next graphics API, and if none works, says so.
pub fn run(r: eframe::Result) -> std::process::ExitCode {
    // eframe logs some startup failures (a lost graphics device) and returns Ok.
    let e = match r {
        Ok(()) if RUNNING.load(std::sync::atomic::Ordering::Relaxed) => return std::process::ExitCode::SUCCESS,
        Ok(()) => "the window closed before it could draw anything (the graphics device stopped)".to_string(),
        Err(e) => e.to_string(),
    };
    log::error!(target: "gex", "couldn't start: {}", e);
    let tried: usize = std::env::var("GEX_BACKEND_TRY").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let user_choice = std::env::var_os("WGPU_BACKEND").is_some() && tried == 0;
    if !user_choice && tried < FALLBACKS.len() {
        let next = FALLBACKS[tried];
        note(&format!("trying again with {}", next));
        if let Ok(exe) = std::env::current_exe() {
            let status = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env("WGPU_BACKEND", next).env("GEX_BACKEND_TRY", (tried + 1).to_string()).status();
            if let Ok(s) = status {
                return if s.success() { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE };
            }
        }
    }
    if tried == 0 || tried >= FALLBACKS.len() || user_choice {
        message(
            "GEX couldn't start",
            &format!("{}\n\nThis is usually the graphics driver. Updating it often helps.\nDetails are in {}", e, log_path().display()),
        );
    }
    std::process::ExitCode::FAILURE
}
