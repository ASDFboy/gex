//! Sync with GitHub: keeps a project the same on several computers. Each linked project is a
//! git repository whose copy lives in a private repository on the person's GitHub account: GEX
//! commits what's saved and pushes it, and brings in what was done on the other computers, on
//! its own (when a project opens, every so often while it's open, and when it closes).
//!
//! It uses Git (Git for Windows comes with Git Credential Manager, which signs in to GitHub once,
//! in the browser, and keeps the sign-in safe). GEX never stores the sign-in itself: it asks Git
//! for it when it needs GitHub's API (making the repository, listing them), and hands it to curl
//! through its standard input, never on a command line.
//!
//! When both computers changed the same part of a file without syncing in between, the newer
//! change is kept and the other is put beside the project in "<project> (sync conflicts)".

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};

use crate::docs::Tip as _;
use crate::state::EditorState;
use crate::theme;

/// Which repository a project is linked to (in its .gex folder: each computer's own).
pub const LINK_FILE: &str = "github.ron";
/// GitHub topic that marks repositories holding GEX projects (so they can be listed).
const TOPIC: &str = "gex-project";
/// The git remote GEX uses (not "origin", in case the project already has one of its own).
const REMOTE: &str = "gex";
const BRANCH: &str = "main";
/// GitHub refuses files bigger than 100 MB: those stay on this computer.
const MAX_FILE: u64 = 95 << 20;
/// How often to look for saved changes (on this computer only, cheap), and to sync anyway.
const LOOK_EVERY: Duration = Duration::from_secs(20);
const SYNC_EVERY: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Link {
    pub owner: String,
    pub repo: String,
    /// Commits are made as the GitHub account, with its private (noreply) address.
    pub email: String,
}

impl Link {
    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repo)
    }
}

fn link_path(dir: &Path) -> PathBuf {
    dir.join(".gex").join(LINK_FILE)
}

pub fn link_of(dir: &Path) -> Option<Link> {
    ron::from_str(&std::fs::read_to_string(link_path(dir)).ok()?).ok()
}

fn save_link(dir: &Path, l: &Link) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dir.join(".gex"));
    std::fs::write(link_path(dir), ron::ser::to_string_pretty(l, Default::default()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

// ---------- git and GitHub ----------

fn quiet(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        c.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = c;
}

/// Where git is, once found (and when it was last looked for, while it isn't).
static GIT: std::sync::Mutex<(Option<PathBuf>, Option<Instant>)> = std::sync::Mutex::new((None, None));

/// Where git is. Once found it's remembered; until then it's looked for again every few
/// seconds at most (it may have just been installed).
pub fn git_exe() -> Option<PathBuf> {
    let mut g = GIT.lock().unwrap_or_else(|e| e.into_inner());
    if g.0.is_none() && g.1.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
        g.0 = find_git();
        g.1 = Some(Instant::now());
    }
    g.0.clone()
}

/// Where git is: on the PATH, or where Git for Windows installs it (just installed, the PATH
/// GEX started with doesn't have it yet).
fn find_git() -> Option<PathBuf> {
    #[allow(unused_mut)]
    let mut list = vec![PathBuf::from("git")];
    #[cfg(windows)]
    {
        for base in [std::env::var_os("ProgramFiles"), std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs").into_os_string())].into_iter().flatten() {
            list.push(PathBuf::from(base).join("Git").join("cmd").join("git.exe"));
        }
    }
    list.into_iter().find(|g| {
        let mut c = Command::new(g);
        quiet(&mut c);
        c.arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    })
}

/// git for the background: never asks anything (no terminal prompt, no sign-in window: a sign-in
/// that's needed shows as a problem with a button instead), gives up on a stalled network, and
/// keeps files' line endings exactly as GEX writes them.
fn git_cmd(dir: &Path) -> Result<Command, String> {
    let mut c = git_signin_cmd(dir)?;
    c.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never");
    Ok(c)
}

/// git that may sign in (Git Credential Manager's window), for what the person asked for.
fn git_signin_cmd(dir: &Path) -> Result<Command, String> {
    let exe = git_exe().ok_or("Git isn't installed")?;
    let mut c = Command::new(exe);
    quiet(&mut c);
    c.current_dir(dir).args(["-c", "http.lowSpeedLimit=1000", "-c", "http.lowSpeedTime=30", "-c", "core.quotepath=off", "-c", "core.autocrlf=false", "-c", "core.safecrlf=false"]);
    Ok(c)
}

/// What a git failure means for syncing.
fn why(e: &str) -> String {
    let l = e.to_ascii_lowercase();
    if l.contains("authentication") || l.contains("could not read username") || l.contains("terminal prompts disabled") || l.contains("403") || l.contains("401") || l.contains("logon failed") {
        format!("{}{}", SIGN_IN, e.lines().last().unwrap_or(""))
    } else if l.contains("repository not found") || l.contains("not found") && l.contains("repository") {
        "the repository isn't on GitHub any more (deleted or renamed?)".into()
    } else if l.contains("could not resolve host") || l.contains("unable to access") || l.contains("timed out") || l.contains("connection") || l.contains("network") {
        format!("{}{}", OFFLINE, e.lines().last().unwrap_or(""))
    } else {
        e.lines().last().unwrap_or(e).to_string()
    }
}

const OFFLINE: &str = "offline: ";
const SIGN_IN: &str = "sign in to GitHub again: ";

/// Runs git; its output, or what went wrong.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_cmd(dir)?.args(args).stdin(Stdio::null()).output().map_err(|e| format!("couldn't run git: {}", e))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("git {} failed", args.first().unwrap_or(&"")) } else { err })
    }
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_ok()
}

/// The GitHub sign-in Git keeps (Git Credential Manager signs in, in the browser, the first time).
fn token() -> Result<String, String> {
    let mut c = git_signin_cmd(&std::env::temp_dir())?;
    let mut child = c.args(["credential", "fill"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("couldn't run git: {}", e))?;
    {
        use std::io::Write;
        let mut i = child.stdin.take().ok_or("couldn't talk to git")?;
        i.write_all(b"protocol=https\nhost=github.com\n\n").map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let user = text.lines().find_map(|l| l.strip_prefix("username=")).unwrap_or("").trim().to_string();
    let t = text
        .lines()
        .find_map(|l| l.strip_prefix("password="))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "couldn't sign in to GitHub (Git for Windows signs in through the browser; elsewhere, set up a Git credential helper, e.g. with `gh auth setup-git`)".to_string())?;
    *SIGNED_IN.lock().unwrap_or_else(|e| e.into_inner()) = user;
    Ok(t)
}

/// Who Git signed in as (to tell it the sign-in worked, so it's kept).
static SIGNED_IN: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Tells Git a sign-in worked (`approve`: keep it) or didn't (`reject`: forget it, ask next time).
fn tell(what: &str, token: &str) {
    let user = SIGNED_IN.lock().map(|u| u.clone()).unwrap_or_default();
    if let Ok(mut c) = git_cmd(&std::env::temp_dir()) {
        if let Ok(mut child) = c.args(["credential", what]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            if let Some(mut i) = child.stdin.take() {
                use std::io::Write;
                let user = if user.is_empty() { String::new() } else { format!("username={}\n", user) };
                let _ = i.write_all(format!("protocol=https\nhost=github.com\n{}password={}\n\n", user, token).as_bytes());
            }
            let _ = child.wait();
        }
    }
}

/// Calls GitHub's API: (status, body). The sign-in goes to curl on its standard input.
fn api(token: &str, method: &str, path: &str) -> Result<(u16, serde_json::Value), String> {
    let mut c = crate::update::curl_cmd();
    c.args(["-sS", "-X", method, "-K", "-", "-w", "\n%{http_code}", "-H", "Accept: application/vnd.github+json", "-H", "X-GitHub-Api-Version: 2022-11-28"]);
    c.arg(format!("https://api.github.com{}", path));
    answer(c, token)
}

/// Runs curl with the sign-in given on its standard input (as curl config): (status, body).
fn answer(mut c: Command, token: &str) -> Result<(u16, serde_json::Value), String> {
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("couldn't run curl: {}", e))?;
    {
        use std::io::Write;
        let mut i = child.stdin.take().ok_or("couldn't talk to curl")?;
        i.write_all(format!("header = \"Authorization: Bearer {}\"\n", token.replace(['"', '\\', '\n'], "")).as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (json, code) = text.rsplit_once('\n').unwrap_or(("", text.as_str()));
    let code: u16 = code.trim().parse().map_err(|_| format!("couldn't reach GitHub: {}", String::from_utf8_lossy(&out.stderr).trim()))?;
    Ok((code, serde_json::from_str(json).unwrap_or(serde_json::Value::Null)))
}

/// GitHub calls with a body: the body goes in a file (curl's config already uses stdin).
fn api_body(token: &str, method: &str, path: &str, body: &serde_json::Value) -> Result<(u16, serde_json::Value), String> {
    let f = std::env::temp_dir().join(format!("gex-github-{}-{}.json", std::process::id(), rand_tag()));
    std::fs::write(&f, body.to_string()).map_err(|e| e.to_string())?;
    let mut c = crate::update::curl_cmd();
    c.args(["-sS", "-X", method, "-K", "-", "-w", "\n%{http_code}", "-H", "Accept: application/vnd.github+json", "-H", "X-GitHub-Api-Version: 2022-11-28", "-H", "Content-Type: application/json", "--data-binary"])
        .arg(format!("@{}", f.display()))
        .arg(format!("https://api.github.com{}", path));
    let r = answer(c, token);
    let _ = std::fs::remove_file(&f);
    r
}

fn rand_tag() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

fn api_error(what: &str, code: u16, body: &serde_json::Value) -> String {
    let msg = body.get("message").and_then(|m| m.as_str()).unwrap_or("");
    match code {
        401 => "GitHub didn't accept the sign-in: try again, and Git asks you to sign in".into(),
        _ => format!("{} (GitHub said {}{}{})", what, code, if msg.is_empty() { "" } else { ": " }, msg),
    }
}

/// The signed-in account: (login, noreply email).
fn account(token: &str) -> Result<(String, String), String> {
    let (code, me) = api(token, "GET", "/user")?;
    if code == 401 {
        tell("reject", token);
    }
    if code != 200 {
        return Err(api_error("couldn't read your GitHub account", code, &me));
    }
    let login = me.get("login").and_then(|l| l.as_str()).ok_or("GitHub didn't say who you are")?.to_string();
    let id = me.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
    tell("approve", token);
    Ok((login.clone(), format!("{}+{}@users.noreply.github.com", id, login)))
}

/// A repository name GitHub takes: letters, digits, - _ and . (others become -).
pub fn repo_name(project: &str) -> String {
    let s: String = project.trim().chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect();
    let s = s.trim_matches(['-', '.']).to_string();
    if s.is_empty() {
        "gex-project".into()
    } else {
        s.chars().take(90).collect()
    }
}

fn computer() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "a computer".into())
}

fn ident(l: &Link) -> [String; 4] {
    ["-c".into(), format!("user.name={}", l.owner), "-c".into(), format!("user.email={}", l.email)]
}

fn git_as(dir: &Path, l: &Link, args: &[&str]) -> Result<String, String> {
    let id = ident(l);
    let mut all: Vec<&str> = id.iter().map(|s| s.as_str()).collect();
    all.extend_from_slice(args);
    git(dir, &all)
}

/// What a project keeps out of the repository: GEX's per-computer state, builds, exports.
fn ignore_file(dir: &Path) {
    let p = dir.join(".gitignore");
    let have = std::fs::read_to_string(&p).unwrap_or_default();
    let mut add = String::new();
    for l in [".gex/", "target/", "export/", "*.part"] {
        if !have.lines().any(|x| x.trim() == l) {
            add.push_str(l);
            add.push('\n');
        }
    }
    if !add.is_empty() {
        let sep = if have.is_empty() || have.ends_with('\n') { "" } else { "\n" };
        let _ = std::fs::write(&p, format!("{}{}# GEX: kept on each computer\n{}", have, sep, add));
    }
}

/// Files too big for GitHub stay on this computer (listed in git's local exclude file).
fn keep_big_files_here(dir: &Path) -> Vec<String> {
    let mut big = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "target" || name == "export" {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if e.metadata().is_ok_and(|m| m.len() > MAX_FILE) {
                if let Ok(rel) = e.path().strip_prefix(dir) {
                    big.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    let f = dir.join(".git").join("info").join("exclude");
    let old = std::fs::read_to_string(&f).unwrap_or_default();
    let mut keep: Vec<&str> = old.lines().take_while(|l| *l != "# GEX: too big for GitHub").collect();
    if !big.is_empty() {
        keep.push("# GEX: too big for GitHub");
        let lines: Vec<String> = big.iter().map(|b| format!("/{}", b)).collect();
        let text = format!("{}\n{}\n", keep.join("\n"), lines.join("\n"));
        if text != old {
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            let _ = std::fs::write(&f, text);
        }
    } else if keep.len() != old.lines().count() {
        let _ = std::fs::write(&f, format!("{}\n", keep.join("\n")));
    }
    big
}

// ---------- syncing ----------

/// What a sync did.
#[derive(Debug, Default)]
pub struct Synced {
    /// Files that changed here because of the other computers (to read again).
    pub pulled: Vec<String>,
    /// Files both computers changed: the other version is in "<project> (sync conflicts)".
    pub conflicts: Vec<String>,
    /// Changes from elsewhere wait for this computer's unsaved changes to be saved.
    pub waiting: bool,
    /// Files too big for GitHub (kept on this computer only).
    pub too_big: Vec<String>,
}

/// One sync: commit what's saved here, bring in what's new there (when `merge`), send it all.
pub fn sync(dir: &Path, l: &Link, merge: bool) -> Result<Synced, String> {
    // A merge cut short (GEX closed in the middle): finished first.
    if dir.join(".git").join("MERGE_HEAD").exists() {
        let _ = git(dir, &["add", "-A"]);
        let _ = git_as(dir, l, &["commit", "-q", "--no-edit"]);
    }
    ignore_file(dir);
    let mut out = Synced { too_big: keep_big_files_here(dir), ..Default::default() };
    git(dir, &["add", "-A"])?;
    if !git_ok(dir, &["diff", "--cached", "--quiet"]) {
        git_as(dir, l, &["commit", "-q", "-m", &format!("Saved on {}", computer())])?;
    }
    for attempt in 0..3 {
        bring_in(dir, l, merge, &mut out)?;
        match push(dir) {
            Ok(()) => break,
            // The other computer sent something meanwhile: bring that in too, and again.
            Err(e) if attempt < 2 && merge && e.contains("rejected") => continue,
            Err(e) if e.contains("rejected") => {
                out.waiting = true;
                break;
            }
            Err(e) => return Err(why(&e)),
        }
    }
    Ok(out)
}

/// Changes as a list of paths (NUL-separated output: any name comes through as it is).
fn paths(out: &str) -> Vec<String> {
    out.split('\0').filter(|p| !p.is_empty()).map(|p| p.to_string()).collect()
}

/// Brings in what's new on GitHub (when `merge`; otherwise notes that it's waiting).
fn bring_in(dir: &Path, l: &Link, merge: bool, out: &mut Synced) -> Result<(), String> {
    git(dir, &["fetch", "-q", REMOTE]).map_err(|e| why(&e))?;
    let theirs = format!("{}/{}", REMOTE, BRANCH);
    let has_theirs = git_ok(dir, &["rev-parse", "-q", "--verify", &theirs]);
    if has_theirs {
        let counts = git(dir, &["rev-list", "--left-right", "--count", &format!("HEAD...{}", theirs)])?;
        let behind: u64 = counts.split_whitespace().nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
        if behind > 0 {
            if !merge {
                out.waiting = true;
            } else {
                for p in paths(&git(dir, &["diff", "-z", "--name-only", "HEAD", &theirs])?) {
                    if !out.pulled.contains(&p) {
                        out.pulled.push(p);
                    }
                }
                if git_as(dir, l, &["merge", "-q", "--no-edit", &theirs]).is_err() {
                    out.conflicts.extend(settle(dir, l, &theirs)?);
                }
                out.waiting = false;
            }
        }
    }
    Ok(())
}

/// Sends what's new here (nothing to do when GitHub already has it).
fn push(dir: &Path) -> Result<(), String> {
    let theirs = format!("{}/{}", REMOTE, BRANCH);
    let ahead = if git_ok(dir, &["rev-parse", "-q", "--verify", &theirs]) {
        git(dir, &["rev-list", "--count", &format!("{}..HEAD", theirs)])?.trim().parse::<u64>().unwrap_or(0) > 0
    } else {
        git_ok(dir, &["rev-parse", "-q", "--verify", "HEAD"])
    };
    if ahead {
        git(dir, &["push", "-q", REMOTE, &format!("HEAD:{}", BRANCH)])?;
    }
    Ok(())
}

/// A merge where both sides changed the same lines: the side changed last wins each file; the
/// other side's version is kept in "<project> (sync conflicts)".
fn settle(dir: &Path, l: &Link, theirs: &str) -> Result<Vec<String>, String> {
    let files = paths(&git(dir, &["diff", "-z", "--name-only", "--diff-filter=U"])?);
    if files.is_empty() {
        let _ = git(dir, &["merge", "--abort"]);
        return Err("couldn't bring in the other computer's changes".into());
    }
    let when = |r: &str| git(dir, &["log", "-1", "--format=%ct", r]).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
    let mine_newer = when("HEAD") >= when(theirs);
    let (keep, aside, stage) = if mine_newer { ("--ours", "other computer", ":3:") } else { ("--theirs", "this computer", ":2:") };
    let aside_dir = PathBuf::from(format!("{} (sync conflicts)", dir.display()));
    for f in &files {
        // The version that loses, kept beside the project.
        if let Ok(b) = git_cmd(dir)?.args(["show", &format!("{}{}", stage, f)]).output() {
            if b.status.success() {
                // "scripts/player (other computer).jlb": the name says whose, the ending stays.
                let p = Path::new(f);
                let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let name = match p.extension() {
                    Some(e) => format!("{} ({}).{}", stem, aside, e.to_string_lossy()),
                    None => format!("{} ({})", stem, aside),
                };
                let to = aside_dir.join(p.with_file_name(name));
                let _ = std::fs::create_dir_all(to.parent().unwrap_or(&aside_dir));
                let _ = std::fs::write(to, &b.stdout);
            }
        }
        if git(dir, &["checkout", keep, "--", f]).is_ok() {
            git(dir, &["add", "--", f])?;
        } else {
            // Removed on the side that wins.
            git(dir, &["rm", "-q", "--", f])?;
        }
    }
    git_as(dir, l, &["commit", "-q", "--no-edit"])?;
    Ok(files)
}

/// Links a project to a new private repository (or an empty one with that name) and sends it.
fn link(dir: &Path, name: &str, say: &dyn Fn(&str)) -> Result<Link, String> {
    say("Signing in to GitHub (a browser window may open)…");
    let t = token()?;
    let (login, email) = account(&t)?;
    say("Making the private repository…");
    let (code, body) = api_body(&t, "POST", "/user/repos", &serde_json::json!({"name": name, "private": true, "description": "A GEX project", "has_issues": false, "has_wiki": false, "has_projects": false}))?;
    match code {
        201 => {}
        422 => {
            // Already there: fine if it's empty (made earlier and never filled).
            let (c, r) = api(&t, "GET", &format!("/repos/{}/{}", login, name))?;
            // Or this project's own (linked before, then syncing was stopped here).
            let ours = git(dir, &["remote", "get-url", REMOTE]).is_ok_and(|u| u.trim().trim_end_matches(".git").eq_ignore_ascii_case(&format!("https://github.com/{}/{}", login, name)));
            if !ours && (c != 200 || r.get("size").and_then(|s| s.as_u64()).unwrap_or(1) > 0) {
                return Err(format!("you already have a repository called {} with something in it: pick another name (or, to get that one, use From GitHub on the start screen)", name));
            }
        }
        _ => return Err(api_error("couldn't make the repository", code, &body)),
    }
    let _ = api_body(&t, "PUT", &format!("/repos/{}/{}/topics", login, name), &serde_json::json!({"names": [TOPIC]}));
    drop(t);
    let l = Link { owner: login, repo: name.to_string(), email };
    say("Sending the project…");
    if !dir.join(".git").is_dir() {
        git(dir, &["init", "-q", "-b", BRANCH])?;
    }
    let _ = git(dir, &["config", "core.autocrlf", "false"]);
    ignore_file(dir);
    let url = format!("{}.git", l.url());
    if git_ok(dir, &["remote", "get-url", REMOTE]) {
        git(dir, &["remote", "set-url", REMOTE, &url])?;
    } else {
        git(dir, &["remote", "add", REMOTE, &url])?;
    }
    keep_big_files_here(dir);
    git(dir, &["add", "-A"])?;
    if !git_ok(dir, &["diff", "--cached", "--quiet"]) {
        git_as(dir, &l, &["commit", "-q", "-m", &format!("Linked on {}", computer())])?;
    }
    // (Linked before: what's on GitHub comes in first.)
    if git_ok(dir, &["fetch", "-q", REMOTE]) && git_ok(dir, &["rev-parse", "-q", "--verify", &format!("{}/{}", REMOTE, BRANCH)]) {
        let mut out = Synced::default();
        bring_in(dir, &l, true, &mut out)?;
    }
    push(dir).map_err(|e| why(&e))?;
    save_link(dir, &l)?;
    Ok(l)
}

/// A GEX project on GitHub (for From GitHub).
#[derive(Clone, Debug)]
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub pushed: String,
}

fn list_repos(say: &dyn Fn(&str)) -> Result<(Vec<Repo>, String), String> {
    say("Signing in to GitHub (a browser window may open)…");
    let t = token()?;
    let (_, email) = account(&t)?;
    say("Looking for your GEX projects…");
    let mut out = Vec::new();
    for page in 1..=10 {
        let (code, list) = api(&t, "GET", &format!("/user/repos?per_page=100&page={}&affiliation=owner&sort=pushed", page))?;
        if code != 200 {
            return Err(api_error("couldn't list your repositories", code, &list));
        }
        let Some(arr) = list.as_array() else { break };
        for r in arr {
            let topics: Vec<&str> = r.get("topics").and_then(|t| t.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            if topics.contains(&TOPIC) {
                out.push(Repo {
                    owner: r.pointer("/owner/login").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    name: r.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    pushed: r.get("pushed_at").and_then(|x| x.as_str()).unwrap_or("").chars().take(10).collect(),
                });
            }
        }
        if arr.len() < 100 {
            break;
        }
    }
    Ok((out, email))
}

/// Gets a linked project onto this computer, in `base`/<name> (or "<name> (2)"... if taken).
fn get(r: &Repo, email: &str, base: &Path, say: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let name = repo_name(&r.name);
    let same = |d: &Path| link_of(d).is_some_and(|l| l.owner.eq_ignore_ascii_case(&r.owner) && l.repo.eq_ignore_ascii_case(&r.name));
    // Already on this computer (got before): that copy, brought up to date when it opens.
    if let Some(d) = crate::state::recent_projects().into_iter().find(|d| same(d)).or_else(|| Some(base.join(&name)).filter(|d| same(d))) {
        return Ok(d);
    }
    let mut dir = base.join(&name);
    let mut n = 2;
    while dir.exists() && std::fs::read_dir(&dir).map(|mut d| d.next().is_some()).unwrap_or(true) {
        dir = base.join(format!("{} ({})", name, n));
        n += 1;
    }
    std::fs::create_dir_all(base).map_err(|e| format!("couldn't make {}: {}", base.display(), e))?;
    say("Getting the project…");
    let url = format!("https://github.com/{}/{}.git", r.owner, r.name);
    let cloned = git_signin_cmd(base)
        .and_then(|mut c| c.args(["clone", "-q", "-o", REMOTE, &url]).arg(&dir).stdin(Stdio::null()).output().map_err(|e| e.to_string()))
        .and_then(|o| if o.status.success() { Ok(()) } else { Err(why(&String::from_utf8_lossy(&o.stderr))) });
    if let Err(e) = cloned {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("couldn't get it: {}", e));
    }
    let _ = git(&dir, &["config", "core.autocrlf", "false"]);
    if !dir.join(gex_core::project::PROJECT_FILE).is_file() {
        return Err(format!("{} doesn't have a GEX project in it", r.name));
    }
    save_link(&dir, &Link { owner: r.owner.clone(), repo: r.name.clone(), email: email.to_string() })?;
    Ok(dir)
}

// ---------- the open project's syncing ----------

enum Job {
    Sync { merge: bool, all: bool },
}

/// Syncing the open project, in the background.
pub struct Syncer {
    pub link: Link,
    dir: PathBuf,
    tx: Sender<Job>,
    /// A sync's result (None: looked, nothing saved here to send, not time to bring in yet).
    rx: Receiver<Result<Option<Synced>, String>>,
    pub busy: bool,
    /// Busy with a whole sync (not just a look for saved changes): shown as "Syncing…".
    busy_full: bool,
    pub last_ok: Option<Instant>,
    pub problem: Option<String>,
    pub waiting: bool,
    pub conflicts: Vec<String>,
    pub too_big: Vec<String>,
    looked: Instant,
    synced: Instant,
}

impl Syncer {
    pub fn start(ctx: &egui::Context, dir: &Path, link: Link) -> Syncer {
        let (tx, jobs) = channel::<Job>();
        let (done, rx) = channel();
        let (d, l, ctx2) = (dir.to_path_buf(), link.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("github sync".into())
            .spawn(move || {
                while let Ok(job) = jobs.recv() {
                    let Job::Sync { merge, all } = job;
                    // Nothing saved here since last time and no time for a full sync: done.
                    let r = if !all && git(&d, &["status", "--porcelain"]).is_ok_and(|s| s.trim().is_empty()) { Ok(None) } else { sync(&d, &l, merge).map(Some) };
                    if done.send(r).is_err() {
                        break;
                    }
                    ctx2.request_repaint();
                }
            })
            .ok();
        let now = Instant::now();
        let mut s = Syncer { link, dir: dir.to_path_buf(), tx, rx, busy: false, busy_full: false, last_ok: None, problem: None, waiting: false, conflicts: Vec::new(), too_big: Vec::new(), looked: now, synced: now };
        s.now(true);
        s
    }

    /// Syncs now (`merge`: bring in the other computers' changes too).
    pub fn now(&mut self, merge: bool) {
        if !self.busy && self.tx.send(Job::Sync { merge, all: true }).is_ok() {
            self.busy = true;
            self.busy_full = true;
            self.looked = Instant::now();
            self.synced = Instant::now();
        }
    }

    /// Every frame: results in, and the next sync when it's time.
    pub fn tick(&mut self, ctx: &egui::Context, st: &mut EditorState) {
        while let Ok(r) = self.rx.try_recv() {
            self.busy = false;
            self.busy_full = false;
            match r {
                Ok(None) => {}
                Ok(Some(s)) => {
                    self.last_ok = Some(Instant::now());
                    self.problem = None;
                    self.waiting = s.waiting;
                    self.too_big = s.too_big;
                    if !s.conflicts.is_empty() {
                        st.log(gex_script::LogKind::Info, format!("Sync: {} file(s) were changed on both computers; the newer version was kept and the other put in \"{} (sync conflicts)\": {}", s.conflicts.len(), self.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), s.conflicts.join(", ")));
                        self.conflicts = s.conflicts;
                    }
                    if !s.pulled.is_empty() {
                        st.reload_changed(&s.pulled);
                    }
                }
                Err(e) => self.problem = Some(e),
            }
        }
        if self.busy || st.playing.is_some() {
            ctx.request_repaint_after(LOOK_EVERY);
            return;
        }
        // After a problem, tried again soon (back online); a sign-in that's needed waits for the
        // button, or the usual time.
        let full = self.synced.elapsed() > SYNC_EVERY || self.problem.as_deref().is_some_and(|p| !p.starts_with(SIGN_IN));
        if full || self.looked.elapsed() > LOOK_EVERY {
            // Others' changes come in only when nothing here is unsaved (they'd clash).
            let merge = can_merge(st);
            let all = full || self.waiting && merge;
            if self.tx.send(Job::Sync { merge, all }).is_ok() {
                self.busy = true;
                self.busy_full = all;
                self.looked = Instant::now();
                if full {
                    self.synced = Instant::now();
                }
            }
        }
        ctx.request_repaint_after(LOOK_EVERY);
    }

    /// The project closing: what was saved here and isn't on GitHub yet goes up (waiting up to
    /// `wait`). Nothing to send: nothing to wait for.
    pub fn finish(self, wait: Duration) {
        let t = Instant::now();
        let mut busy = self.busy;
        while busy && t.elapsed() < wait {
            if self.rx.recv_timeout(Duration::from_millis(100)).is_ok() {
                busy = false;
            }
        }
        let theirs = format!("{}/{}", REMOTE, BRANCH);
        let to_send = git(&self.dir, &["status", "--porcelain"]).is_ok_and(|s| !s.trim().is_empty()) || git(&self.dir, &["rev-list", "--count", &format!("{}..HEAD", theirs)]).map_or(true, |c| c.trim() != "0");
        if !busy && to_send && self.tx.send(Job::Sync { merge: true, all: true }).is_ok() {
            let _ = self.rx.recv_timeout(wait.saturating_sub(t.elapsed()).max(Duration::from_secs(1)));
        }
    }

    /// A whole sync is going on (not just a look for saved changes).
    pub fn syncing(&self) -> bool {
        self.busy_full
    }

    /// A word for the top bar.
    pub fn status(&self) -> (String, egui::Color32) {
        if let Some(p) = &self.problem {
            let w = if p.starts_with(OFFLINE) {
                "Sync: offline"
            } else if p.starts_with(SIGN_IN) {
                "Sync: sign in"
            } else {
                "Sync problem"
            };
            return (w.into(), theme::WARN);
        }
        if self.busy_full {
            return ("Syncing…".into(), theme::TEXT_DIM);
        }
        if self.waiting {
            return ("Save to sync".into(), theme::WARN);
        }
        match self.last_ok {
            Some(_) => ("Synced".into(), theme::TEXT_DIM),
            None => ("Syncing…".into(), theme::TEXT_DIM),
        }
    }
}

fn unsaved(st: &EditorState) -> bool {
    st.scripts.values().any(|s| s.dirty()) || (st.blueprint_res.is_some() && st.blueprint_dirty())
}

/// Whether changes from elsewhere can come in now: nothing unsaved here, nor (working together)
/// anything others are changing in scripts and blueprints not open here.
fn can_merge(st: &EditorState) -> bool {
    !unsaved(st) && !st.collab.as_ref().is_some_and(|c| c.unsaved_shared(st))
}

// ---------- windows ----------

/// The Sync with GitHub window (and From GitHub on the start screen).
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    name: String,
    job: Option<Receiver<Msg>>,
    pub status: String,
    pub error: String,
    pub repos: Option<(Vec<Repo>, String)>,
    installing: bool,
    /// Got from GitHub: the project to open.
    pub got: Option<PathBuf>,
}

enum Msg {
    Say(String),
    Linked(Result<Link, String>),
    Repos(Result<(Vec<Repo>, String), String>),
    Got(Result<PathBuf, String>),
    #[cfg_attr(not(windows), allow(dead_code))]
    Installed(Result<(), String>),
    SignedIn(Result<(), String>),
}

fn run(ctx: &egui::Context, f: impl FnOnce(&dyn Fn(&str)) -> Msg + Send + 'static) -> Receiver<Msg> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let say = |s: &str| {
            let _ = tx.send(Msg::Say(s.to_string()));
            ctx.request_repaint();
        };
        let m = f(&say);
        let _ = tx.send(m);
        ctx.request_repaint();
    });
    rx
}

impl Panel {
    fn poll(&mut self) -> Vec<Msg> {
        let mut out = Vec::new();
        if let Some(rx) = &self.job {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Say(s) => self.status = s,
                    m => out.push(m),
                }
            }
        }
        if !out.is_empty() {
            self.job = None;
            self.status.clear();
        }
        out
    }

    fn git_missing(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("This uses Git (free), which isn't installed on this computer.").color(theme::WARN));
        ui.horizontal(|ui| {
            #[cfg(windows)]
            if ui.add_enabled(!self.installing, egui::Button::new("Install Git")).tip("Installs Git for Windows with winget (Windows' own installer); it signs in to GitHub for you, in the browser").clicked() {
                self.installing = true;
                self.error.clear();
                self.job = Some(run(ui.ctx(), |say| {
                    say("Installing Git (Windows may ask for permission)…");
                    let mut c = Command::new("winget");
                    quiet(&mut c);
                    let r = c
                        .args(["install", "--id", "Git.Git", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"])
                        .output()
                        .map_err(|e| format!("couldn't run winget: {}", e))
                        .and_then(|o| if git_exe().is_some() { Ok(()) } else { Err(format!("Git didn't install{}", if o.status.success() { "" } else { " (winget failed)" })) });
                    Msg::Installed(r)
                }));
            }
            if ui.button("Get Git yourself").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://git-scm.com/downloads"));
            }
        });
    }

    fn progress(&self, ui: &mut egui::Ui) {
        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(if self.status.is_empty() { "Working…" } else { &self.status });
            });
        }
        if !self.error.is_empty() {
            ui.label(RichText::new(&self.error).color(theme::BAD));
        }
    }

    /// File → Sync with GitHub: link the open project, or see how its syncing goes.
    pub fn show_project(&mut self, ctx: &egui::Context, st: &mut EditorState, syncer: &mut Option<Syncer>) {
        for m in self.poll() {
            match m {
                Msg::Linked(Ok(l)) => {
                    *syncer = Some(Syncer::start(ctx, &st.project.dir, l));
                }
                Msg::Linked(Err(e)) | Msg::SignedIn(Err(e)) => self.error = e,
                Msg::SignedIn(Ok(())) => {
                    if let Some(s) = syncer.as_mut() {
                        s.problem = None;
                        s.now(can_merge(st));
                    }
                }
                Msg::Installed(r) => {
                    self.installing = false;
                    if let Err(e) = r {
                        self.error = e;
                    }
                }
                _ => {}
            }
        }
        if !self.open {
            return;
        }
        let mut open = true;
        let mut stop = false;
        egui::Window::new("Sync with GitHub").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.set_width(440.0);
            match syncer {
                Some(s) => {
                    ui.horizontal(|ui| {
                        ui.label("Linked to");
                        ui.hyperlink_to(format!("{}/{}", s.link.owner, s.link.repo), s.link.url());
                    });
                    let (word, color) = s.status();
                    ui.label(RichText::new(match (&s.problem, s.last_ok) {
                        (Some(p), _) => format!("{}: {}", word, p),
                        (None, Some(t)) if !s.syncing() => format!("Synced {} ago.", ago(t.elapsed())),
                        _ => word,
                    }).color(color));
                    if s.waiting {
                        ui.label(RichText::new("Changes from your other computer are waiting: save what's open here and they come in.").small().color(theme::TEXT_DIM));
                    }
                    if !s.too_big.is_empty() {
                        ui.label(RichText::new(format!("Too big for GitHub (over 95 MB), kept on this computer only: {}", s.too_big.join(", "))).small().color(theme::WARN));
                    }
                    if !s.conflicts.is_empty() {
                        ui.label(RichText::new(format!("Changed on both computers (the other versions are in \"(sync conflicts)\" beside the project): {}", s.conflicts.join(", "))).small().color(theme::WARN));
                    }
                    ui.label(RichText::new("What you save goes up on its own, and what you did on your other computers comes in when the project opens and every couple of minutes. On another computer: From GitHub on the start screen.").small().color(theme::TEXT_DIM));
                    ui.horizontal(|ui| {
                        if s.problem.as_deref().is_some_and(|p| p.starts_with(SIGN_IN)) {
                            if ui.add_enabled(self.job.is_none(), egui::Button::new("Sign in to GitHub")).tip("Git opens GitHub's sign-in in your browser").clicked() {
                                self.error.clear();
                                self.job = Some(run(ctx, |say| {
                                    say("Signing in to GitHub (a browser window may open)…");
                                    Msg::SignedIn(token().and_then(|t| account(&t).map(|_| ())))
                                }));
                            }
                        } else if ui.add_enabled(!s.busy, egui::Button::new("Sync now")).clicked() {
                            s.now(can_merge(st));
                        }
                        if ui.button("Stop syncing").tip("This computer stops syncing this project (the repository and the other computers' copies stay as they are)").clicked() {
                            stop = true;
                        }
                    });
                }
                None => {
                    ui.label("Keep this project the same on all your computers: GEX keeps it in a private repository on your GitHub account, sends what you save, and brings in what you did elsewhere.");
                    ui.add_space(4.0);
                    if git_exe().is_none() {
                        self.git_missing(ui);
                    } else {
                        if self.name.is_empty() {
                            self.name = repo_name(&st.project.config.name);
                        }
                        ui.horizontal(|ui| {
                            ui.label("Repository");
                            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(220.0));
                            ui.label(RichText::new("(private)").small().color(theme::TEXT_DIM));
                        });
                        let busy = self.job.is_some();
                        if ui.add_enabled(!busy && !self.name.trim().is_empty(), egui::Button::new("Link to GitHub")).clicked() {
                            st.save_all();
                            self.error.clear();
                            let (dir, name) = (st.project.dir.clone(), repo_name(&self.name));
                            self.job = Some(run(ctx, move |say| Msg::Linked(link(&dir, &name, say))));
                        }
                        ui.label(RichText::new("The first time, Git asks you to sign in to GitHub in your browser. Then, on your other computer: From GitHub on the start screen.").small().color(theme::TEXT_DIM));
                    }
                }
            }
            self.progress(ui);
        });
        if stop {
            let _ = std::fs::remove_file(link_path(&st.project.dir));
            *syncer = None;
        }
        if !open {
            self.open = false;
        }
    }

    /// From GitHub, on the start screen: the GEX projects on your GitHub, to get one.
    pub fn show_start(&mut self, ctx: &egui::Context, base: &Path) {
        for m in self.poll() {
            match m {
                Msg::Repos(Ok(r)) => self.repos = Some(r),
                Msg::Repos(Err(e)) | Msg::Got(Err(e)) | Msg::Linked(Err(e)) => self.error = e,
                Msg::Got(Ok(d)) => {
                    self.got = Some(d);
                    self.open = false;
                }
                Msg::Installed(r) => {
                    self.installing = false;
                    if let Err(e) = r {
                        self.error = e;
                    }
                }
                _ => {}
            }
        }
        if !self.open {
            return;
        }
        let mut open = true;
        egui::Window::new("From GitHub").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.set_width(440.0);
            if base.as_os_str().is_empty() {
                ui.label(RichText::new("Choose where projects go first (\"Made in\", under New project).").color(theme::WARN));
                return;
            }
            ui.label("Your projects synced with GitHub (File → Sync with GitHub on the computer that has them). Getting one here keeps it in sync on this computer too.");
            ui.add_space(4.0);
            if git_exe().is_none() {
                self.git_missing(ui);
            } else if self.repos.is_none() && self.job.is_none() && self.error.is_empty() {
                self.job = Some(run(ctx, |say| Msg::Repos(list_repos(say))));
            }
            let mut pick = None;
            if let Some((list, _)) = &self.repos {
                if list.is_empty() {
                    ui.label(RichText::new("None yet.").color(theme::TEXT_DIM));
                }
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for r in list {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&r.name).strong());
                            ui.label(RichText::new(format!("changed {}", r.pushed)).small().color(theme::TEXT_DIM));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(self.job.is_none(), egui::Button::new("Get")).tip(format!("Into {}", base.display())).clicked() {
                                    pick = Some(r.clone());
                                }
                            });
                        });
                    }
                });
            }
            if let (Some(r), Some((_, email))) = (pick, &self.repos) {
                self.error.clear();
                let (email, base) = (email.clone(), base.to_path_buf());
                self.job = Some(run(ctx, move |say| Msg::Got(get(&r, &email, &base, say))));
            }
            if !self.error.is_empty() && self.job.is_none() && ui.button("Try again").clicked() {
                self.error.clear();
                self.repos = None;
            }
            self.progress(ui);
        });
        if !open {
            self.open = false;
        }
    }
}

fn ago(d: Duration) -> String {
    match d.as_secs() {
        s if s < 60 => "moments".into(),
        s if s < 3600 => format!("{} min", s / 60),
        s => format!("{} h", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        assert!(git(dir, args).is_ok(), "git {:?}: {:?}", args, git(dir, args));
    }

    /// Two computers' copies of a project, synced through a (local, bare) repository: changes
    /// go both ways, a file changed on both keeps the newer and puts the other aside, big files
    /// stay home, and GEX's own folder never goes up.
    #[test]
    fn two_computers_stay_in_step() {
        if git_exe().is_none() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("gex-github-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let (hub, a, b) = (tmp.join("hub.git"), tmp.join("a"), tmp.join("b"));
        std::fs::create_dir_all(&hub).unwrap();
        sh(&hub, &["init", "-q", "--bare", "-b", BRANCH]);
        crate::templates::create_demo(&a).unwrap();
        std::fs::create_dir_all(a.join(".gex")).unwrap();
        std::fs::write(a.join(".gex/workspace.ron"), "this computer's").unwrap();
        let l = Link { owner: "me".into(), repo: "game".into(), email: "me@example.com".into() };
        // What linking does, with the hub in place of GitHub.
        sh(&a, &["init", "-q", "-b", BRANCH]);
        ignore_file(&a);
        sh(&a, &["remote", "add", REMOTE, &hub.to_string_lossy()]);
        sync(&a, &l, true).unwrap();
        // The other computer gets it.
        sh(&tmp, &["clone", "-q", "-o", REMOTE, &hub.to_string_lossy(), "b"]);
        assert!(b.join(gex_core::project::PROJECT_FILE).is_file());
        assert!(!b.join(".gex/workspace.ron").exists(), "GEX's own folder stays on each computer");
        // b changes a script; a gets it.
        let script = walk_first(&b, ".jlb");
        std::fs::write(b.join(&script), "// from b\n").unwrap();
        sync(&b, &l, true).unwrap();
        let s = sync(&a, &l, true).unwrap();
        assert_eq!(std::fs::read_to_string(a.join(&script)).unwrap(), "// from b\n");
        assert!(s.pulled.contains(&script));
        // Unsaved work here: their changes wait.
        std::fs::write(b.join(&script), "// b again\n").unwrap();
        sync(&b, &l, true).unwrap();
        let s = sync(&a, &l, false).unwrap();
        assert!(s.waiting && std::fs::read_to_string(a.join(&script)).unwrap() == "// from b\n");
        sync(&a, &l, true).unwrap();
        // Both change it: the newer (b's, made a second later) wins; a's is put aside.
        std::fs::write(a.join(&script), "// a's\n").unwrap();
        sync(&a, &l, true).unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(b.join(&script), "// b's\n").unwrap();
        let s = sync(&b, &l, true).unwrap();
        assert_eq!(s.conflicts, vec![script.clone()]);
        assert_eq!(std::fs::read_to_string(b.join(&script)).unwrap(), "// b's\n");
        let aside = PathBuf::from(format!("{} (sync conflicts)", b.display())).join(script.replace(".jlb", " (other computer).jlb"));
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "// a's\n");
        sync(&a, &l, true).unwrap();
        assert_eq!(std::fs::read_to_string(a.join(&script)).unwrap(), "// b's\n", "and the other computer ends up the same");
        // A file too big for GitHub stays on its computer.
        let big = std::fs::File::create(a.join("huge.bin")).unwrap();
        big.set_len(MAX_FILE + 1).unwrap();
        let s = sync(&a, &l, true).unwrap();
        assert_eq!(s.too_big, vec!["huge.bin".to_string()]);
        sync(&b, &l, true).unwrap();
        assert!(!b.join("huge.bin").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn walk_first(dir: &Path, ext: &str) -> String {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let mut es: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().collect();
            es.sort_by_key(|e| e.file_name());
            for e in es {
                let p = e.path();
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else if p.to_string_lossy().ends_with(ext) {
                    return p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                }
            }
        }
        panic!("no {} file", ext)
    }

    #[test]
    fn repository_names() {
        assert_eq!(repo_name("GEX Sample Games"), "GEX-Sample-Games");
        assert_eq!(repo_name("  "), "gex-project");
        assert_eq!(repo_name("..x!y"), "x-y");
    }
}

#[cfg(test)]
mod live {
    /// Against the real GitHub (read only), with a token in GEX_TEST_GITHUB_TOKEN.
    #[test]
    #[ignore]
    fn reads_the_account_and_repositories() {
        let Ok(t) = std::env::var("GEX_TEST_GITHUB_TOKEN") else { return };
        let (login, email) = super::account(&t).unwrap();
        assert!(email.ends_with(&format!("+{}@users.noreply.github.com", login)), "{}", email);
        let (code, list) = super::api(&t, "GET", "/user/repos?per_page=5&affiliation=owner&sort=pushed").unwrap();
        // (A token limited to some repositories may not list them: 403.)
        assert!(code == 200 && list.as_array().is_some() || code == 403, "{}", code);
        let (code, _) = super::api_body(&t, "PUT", "/repos/nobody-at-all-x/none/topics", &serde_json::json!({"names": []})).unwrap();
        assert!(code == 404 || code == 403, "{}", code);
        eprintln!("OK as {}", login);
    }
}
