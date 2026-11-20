//! Letting others reach GEX through the Windows Firewall. Windows asks the first time GEX
//! listens, but only for "Private" networks unless you tick more; ZeroTier, Tailscale and
//! Hamachi networks count as "Public", so friends on them get no answer. This adds one rule that
//! lets GEX in on every network type (after a Windows admin prompt), replacing any rules Windows
//! made for it, including a block rule left by pressing Cancel on its prompt.

use std::sync::mpsc::{channel, Receiver};

#[cfg(windows)]
fn netsh() -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("netsh");
    c.creation_flags(0x0800_0000);
    c
}

/// Whether GEX's rule is there (None: not Windows, or can't tell).
pub fn allowed() -> Option<bool> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().ok()?;
        let out = netsh().args(["advfirewall", "firewall", "show", "rule", "name=GEX", "verbose"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        Some(out.status.success() && text.contains(&exe.to_string_lossy().to_lowercase()))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Adds the rule (Windows shows its admin prompt); runs in the background.
pub fn allow() -> Receiver<Result<(), String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(run());
    });
    rx
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let p = exe.to_string_lossy().to_string();
    if p.contains('"') || p.contains('\'') {
        return Err("GEX's folder has a quote in its name; move it and try again".into());
    }
    // Rules for this program (Windows' own, a block one included) go; GEX's goes in.
    let cmd = format!("/c netsh advfirewall firewall delete rule name=all program=\"{p}\" & netsh advfirewall firewall add rule name=GEX dir=in action=allow program=\"{p}\" enable=yes profile=any");
    let ps = format!("Start-Process cmd -Verb RunAs -WindowStyle Hidden -Wait -ArgumentList '{}'", cmd);
    let st = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps])
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|e| e.to_string())?;
    if !st.success() {
        return Err("Windows didn't allow it (the admin prompt was declined?)".into());
    }
    match allowed() {
        Some(true) => Ok(()),
        _ => Err("the rule didn't go in (the admin prompt was declined?)".into()),
    }
}

#[cfg(not(windows))]
fn run() -> Result<(), String> {
    Err("only Windows needs this".into())
}
