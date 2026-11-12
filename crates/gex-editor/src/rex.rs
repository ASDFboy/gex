//! REX: an assistant in the editor. It runs Claude Code (the `claude` program the user has
//! installed and signed in to) in the project's folder, the way editor plugins for it do,
//! and shows the conversation in an orange panel on the right.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

use eframe::egui::{self, Color32, RichText};

use crate::docs::Tip as _;
use crate::state::EditorState;

// REX's own colors: orange where the rest of the editor is gray.
const BG_TOP: Color32 = Color32::from_rgb(0xe0, 0x7a, 0x1c);
const BG_BOTTOM: Color32 = Color32::from_rgb(0xb8, 0x56, 0x0e);
const INK: Color32 = Color32::from_rgb(0x1c, 0x0f, 0x02);
const INK_DIM: Color32 = Color32::from_rgb(0x5c, 0x30, 0x06);
const PAPER: Color32 = Color32::from_rgb(0xf6, 0xb2, 0x62);
const MINE: Color32 = Color32::from_rgb(0x8a, 0x3e, 0x06);

enum Line {
    You(String),
    Rex(String),
    /// Something it did: read a file, changed one.
    Did(String),
    Problem(String),
}

enum FromClaude {
    Json(String),
    Stderr(String),
    Done,
}

pub struct Rex {
    pub open: bool,
    lines: Vec<Line>,
    input: String,
    /// It may change the project's files (otherwise it only reads and answers).
    can_edit: bool,
    /// The conversation so far, to continue it.
    session: Option<String>,
    running: Option<(Child, Receiver<FromClaude>)>,
    /// The project the conversation is about.
    project: std::path::PathBuf,
    /// Files may have changed: the editor should read them again.
    pub reload: bool,
    /// Where Claude Code is, once found.
    program: Option<std::path::PathBuf>,
    /// It wasn't found: the panel asks where it is.
    ask_path: bool,
    path_input: String,
}

impl Default for Rex {
    fn default() -> Self {
        Rex { open: false, lines: Vec::new(), input: String::new(), can_edit: false, session: None, running: None, project: Default::default(), reload: false, program: None, ask_path: false, path_input: String::new() }
    }
}

/// Where a path override for the `claude` program is kept, for installs REX can't find.
fn override_file() -> std::path::PathBuf {
    crate::state::config_dir().join("claude_path.txt")
}

/// Finds the `claude` program. The editor's own PATH can be out of date (programs started
/// before Claude Code was installed keep the old one) or lack what a terminal's profile adds,
/// so this also looks where the installers put it and at the PATH Windows has on record.
fn find_claude() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(p) = std::fs::read_to_string(override_file()).ok().map(|s| PathBuf::from(s.trim().trim_matches('"'))).filter(|p| p.is_file()) {
        return Some(p);
    }
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd", "claude.bat"] } else { &["claude"] };
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.push(home.join(".local").join("bin"));
    dirs.push(home.join(".claude").join("local"));
    dirs.push(home.join(".claude").join("bin"));
    #[cfg(windows)]
    {
        for v in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(d) = std::env::var_os(v).map(PathBuf::from) {
                dirs.push(d.join("npm"));
                dirs.push(d.join("Programs").join("claude"));
                dirs.push(d.join("Programs").join("Claude Code"));
                dirs.push(d.join("Microsoft").join("WinGet").join("Links"));
                dirs.push(d.join("pnpm"));
            }
        }
        dirs.push(home.join(".bun").join("bin"));
        dirs.push(home.join("scoop").join("shims"));
        // The PATH on record for this user and the machine, which may be newer than ours.
        use std::os::windows::process::CommandExt;
        for key in [r"HKCU\Environment", r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment"] {
            let out = Command::new("reg").args(["query", key, "/v", "Path"]).creation_flags(0x0800_0000).output();
            if let Ok(o) = out {
                let text = String::from_utf8_lossy(&o.stdout).to_string();
                if let Some(value) = text.lines().find_map(|l| l.split_once("REG_EXPAND_SZ").or_else(|| l.split_once("REG_SZ")).map(|x| x.1.trim().to_string())) {
                    for d in value.split(';').filter(|d| !d.trim().is_empty()) {
                        let d = expand_percent(d.trim());
                        dirs.push(PathBuf::from(d));
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(home.join(".npm-global").join("bin"));
    }
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// Fills in %NAME% parts of a Windows path from the environment.
#[cfg_attr(not(windows), allow(dead_code))]
fn expand_percent(s: &str) -> String {
    let mut out = String::new();
    let mut parts = s.split('%');
    out.push_str(parts.next().unwrap_or(""));
    // Odd pieces are names between two percent signs; a last unpaired one is kept as text.
    let rest: Vec<&str> = parts.collect();
    let mut i = 0;
    while i < rest.len() {
        if i + 1 < rest.len() {
            out.push_str(&std::env::var(rest[i]).unwrap_or_default());
            out.push_str(rest[i + 1]);
            i += 2;
        } else {
            out.push('%');
            out.push_str(rest[i]);
            i += 1;
        }
    }
    out
}

/// The command that runs Claude Code: the program itself, or through `cmd` when it's a
/// .cmd / .bat on Windows; never with a console window flashing up.
fn claude(at: &std::path::Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let script = at.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
        let mut c = if script {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(at);
            c
        } else {
            Command::new(at)
        };
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    Command::new(at)
}

/// What REX is told about where it is, ahead of the first question.
fn briefing() -> String {
    let docs = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("docs"))).filter(|d| d.is_dir());
    let mut s = String::from(
        "You are REX, the assistant built into the GEX game engine's editor. The folder you are in is a GEX project. \
         Blueprints (.jblueprint, RON text) are saved trees of objects; an object can be an instance of another blueprint. \
         Scripts (.jlb) are written in JLB: ALL-CAPS words are commands, functions named on_... are events the engine calls, \
         `self` is the object the script is on, `$name` is a variable set per instance. project.jproj holds the project's settings. \
         Answer briefly and plainly. Don't guess at commands or properties: ",
    );
    match docs {
        Some(d) => s.push_str(&format!("the engine's documentation is in {} (SCRIPTING.md lists every command and event, OBJECTS.md every kind of object and its properties); read what you need there.", d.display())),
        None => s.push_str("look at how the project's own scripts and blueprints do things."),
    }
    s.push_str(" Never touch the .gex or .trash folders.\n\n");
    s
}

impl Rex {
    fn send(&mut self, st: &mut EditorState, ctx: &egui::Context) {
        let ask = self.input.trim().to_string();
        if ask.is_empty() || self.running.is_some() {
            return;
        }
        self.input.clear();
        if self.project != st.project.dir {
            // Another project: a new conversation.
            self.project = st.project.dir.clone();
            self.session = None;
        }
        // What it reads should be what you see.
        st.save_all();
        let mut prompt = if self.session.is_none() { briefing() } else { String::new() };
        if let Some(b) = &st.blueprint_res {
            prompt.push_str(&format!("(Open in the editor: {}", b.trim_start_matches("res://")));
            if let Some(id) = st.selected.filter(|i| st.tree.contains(*i)) {
                prompt.push_str(&format!(", selected object: {}", st.tree.path_of(id)));
            }
            prompt.push_str(")\n\n");
        }
        prompt.push_str(&ask);
        let ask_again = ask.clone();
        self.lines.push(Line::You(ask));
        let Some(at) = self.program.clone().filter(|p| p.is_file()).or_else(find_claude) else {
            self.lines.push(Line::Problem("REX works through Claude Code, and can't find it on this computer.\n\nIf it's installed, paste the full path of its program below (in a terminal, `where claude` on Windows or `which claude` elsewhere shows it). Otherwise install Claude Code and run `claude` once to sign in.".into()));
            self.ask_path = true;
            self.input = ask_again;
            return;
        };
        self.program = Some(at.clone());
        self.ask_path = false;
        let mut cmd = claude(&at);
        cmd.current_dir(&st.project.dir).args(["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", if self.can_edit { "acceptEdits" } else { "plan" }]);
        if let Some(s) = &self.session {
            cmd.args(["--resume", s]);
        }
        if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("docs"))).filter(|d| d.is_dir()) {
            cmd.arg("--add-dir").arg(d);
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.lines.push(Line::Problem(format!(
                    "REX works through Claude Code, and couldn't start it ({}).\n\nInstall Claude Code, run `claude` once in a terminal to sign in, then ask again. REX uses that sign-in; GEX never sees or stores a key.",
                    e
                )));
                return;
            }
        };
        // The question goes in through its input (no quoting to get wrong), then the pipe closes.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }
        let (tx, rx) = channel();
        let (out, err) = (child.stdout.take(), child.stderr.take());
        let (tx2, ctx2) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(err) = err {
                for l in BufReader::new(err).lines().map_while(Result::ok) {
                    let _ = tx2.send(FromClaude::Stderr(l));
                }
            }
            ctx2.request_repaint();
        });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(out) = out {
                for l in BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = tx.send(FromClaude::Json(l));
                    ctx.request_repaint();
                }
            }
            let _ = tx.send(FromClaude::Done);
            ctx.request_repaint();
        });
        self.running = Some((child, rx));
    }

    fn stop(&mut self) {
        if let Some((mut child, _)) = self.running.take() {
            let _ = child.kill();
            let _ = child.wait();
            self.lines.push(Line::Did("stopped".into()));
            self.reload = true;
        }
    }

    /// Takes in what Claude Code has printed since the last frame.
    fn pump(&mut self) {
        let Some((_, rx)) = &self.running else { return };
        let got: Vec<FromClaude> = rx.try_iter().collect();
        let mut stderr = Vec::new();
        for m in got {
            match m {
                FromClaude::Json(l) => self.take(&l),
                FromClaude::Stderr(l) => stderr.push(l),
                FromClaude::Done => {
                    if let Some((mut child, _)) = self.running.take() {
                        let _ = child.wait();
                    }
                    self.reload = true;
                }
            }
        }
        let said: Vec<String> = stderr.into_iter().filter(|l| !l.trim().is_empty()).collect();
        if !said.is_empty() {
            let text = said.join("\n");
            let hint = if text.contains("not recognized") || text.contains("not found") { "\n\nClaude Code doesn't seem to be installed. Install it, run `claude` once in a terminal to sign in, then ask again." } else { "" };
            self.lines.push(Line::Problem(format!("{}{}", text, hint)));
        }
    }

    /// One line of Claude Code's stream.
    fn take(&mut self, line: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return };
        if let Some(s) = v["session_id"].as_str() {
            self.session = Some(s.to_string());
        }
        match v["type"].as_str() {
            Some("assistant") => {
                for part in v["message"]["content"].as_array().into_iter().flatten() {
                    match part["type"].as_str() {
                        Some("text") => {
                            let t = part["text"].as_str().unwrap_or("").trim();
                            if !t.is_empty() {
                                self.lines.push(Line::Rex(t.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            let input = &part["input"];
                            let what = ["file_path", "path", "pattern", "command", "description"].iter().find_map(|k| input[*k].as_str()).unwrap_or("");
                            let short = what.replace('\\', "/");
                            let short = short.rsplit('/').next().unwrap_or(&short);
                            let verb = match part["name"].as_str().unwrap_or("") {
                                "Read" => "read",
                                "Edit" | "MultiEdit" => "changed",
                                "Write" => "wrote",
                                "Glob" | "Grep" => "searched for",
                                "Bash" => "ran",
                                other => other,
                            };
                            self.lines.push(Line::Did(format!("{} {}", verb, short).trim().to_string()));
                        }
                        _ => {}
                    }
                }
            }
            Some("result") => {
                if v["is_error"].as_bool() == Some(true) {
                    self.lines.push(Line::Problem(v["result"].as_str().unwrap_or("something went wrong").to_string()));
                }
            }
            _ => {}
        }
    }

    /// The panel. Call before the rest of the editor's panels so it's outermost on the right.
    pub fn show(&mut self, ui: &mut egui::Ui, st: &mut EditorState) {
        self.pump();
        if !self.open {
            return;
        }
        let ctx = ui.ctx().clone();
        egui::Panel::right("rex").resizable(true).default_size(340.0).size_range(260.0..=700.0).frame(egui::Frame::NONE).show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().add(crate::theme::vgradient(full, BG_TOP, BG_BOTTOM));
            crate::docs::mark(ui, full, "ui:rex");
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(full.shrink(8.0)));
            let ui = &mut ui;
            // REX's own look, whatever the editor's is.
            let v = ui.visuals_mut();
            v.override_text_color = Some(INK);
            v.extreme_bg_color = Color32::from_rgb(0x2a, 0x16, 0x04);
            v.selection.bg_fill = Color32::from_rgb(0xff, 0xd0, 0x90);
            for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                w.weak_bg_fill = MINE;
                w.bg_fill = MINE;
                w.fg_stroke.color = Color32::WHITE;
                w.bg_stroke = egui::Stroke::NONE;
            }
            v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x6e, 0x30, 0x04);
            ui.horizontal(|ui| {
                ui.label(RichText::new("REX").size(20.0).strong().color(INK));
                ui.label(RichText::new("assistant").small().color(INK_DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(RichText::new("New chat").color(Color32::WHITE)).tip("Forgets this conversation and starts another").clicked() {
                        self.stop();
                        self.lines.clear();
                        self.session = None;
                    }
                });
            });
            ui.add_space(4.0);
            let input_h = if self.ask_path { 124.0 } else { 96.0 };
            egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).max_height((ui.available_height() - input_h).max(40.0)).show(ui, |ui| {
                if self.lines.is_empty() {
                    ui.label(RichText::new("Ask about GEX or your project, or tell me what to make.").color(INK));
                    ui.add_space(4.0);
                    for example in ["How do I make the player jump?", "Why doesn't my coin disappear when touched?", "Add a score label that counts coins."] {
                        if ui.add(egui::Button::new(RichText::new(example).color(Color32::WHITE)).wrap()).clicked() {
                            self.input = example.to_string();
                        }
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("REX runs Claude Code in this project's folder, with your Claude sign-in and plan.").small().color(INK_DIM));
                }
                for l in &self.lines {
                    match l {
                        Line::You(t) => {
                            egui::Frame::new().fill(MINE).inner_margin(6.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(t).color(Color32::WHITE));
                            });
                        }
                        Line::Rex(t) => {
                            egui::Frame::new().fill(PAPER).inner_margin(6.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(egui::Label::new(RichText::new(t).color(INK)).selectable(true));
                            });
                        }
                        Line::Did(t) => {
                            ui.label(RichText::new(format!("· {}", t)).small().color(INK_DIM));
                        }
                        Line::Problem(t) => {
                            egui::Frame::new().fill(Color32::from_rgb(0x5a, 0x10, 0x10)).inner_margin(6.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(egui::Label::new(RichText::new(t).color(Color32::WHITE)).selectable(true));
                            });
                        }
                    }
                    ui.add_space(4.0);
                }
                if self.running.is_some() {
                    let dots = ".".repeat(1 + (ui.input(|i| i.time) * 2.0) as usize % 3);
                    ui.label(RichText::new(format!("working{}", dots)).color(INK_DIM));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
                }
            });
            ui.add_space(4.0);
            if self.ask_path {
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.path_input).hint_text(RichText::new("path to claude").color(Color32::from_rgb(0xc8, 0x9a, 0x6a))).desired_width(ui.available_width() - 50.0).text_color(Color32::WHITE));
                    if ui.button(RichText::new("Use").color(Color32::WHITE)).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        let p = std::path::PathBuf::from(self.path_input.trim().trim_matches('"'));
                        if p.is_file() {
                            let _ = std::fs::create_dir_all(crate::state::config_dir());
                            let _ = std::fs::write(override_file(), p.to_string_lossy().as_bytes());
                            self.program = Some(p);
                            self.ask_path = false;
                            self.lines.push(Line::Did("found Claude Code: ask again".into()));
                        } else {
                            self.lines.push(Line::Problem(format!("There's no file at {}", p.display())));
                        }
                    }
                });
            }
            let r = ui.add(egui::TextEdit::multiline(&mut self.input).hint_text(RichText::new("Ask REX… (Enter sends, Shift+Enter: new line)").color(Color32::from_rgb(0xc8, 0x9a, 0x6a))).desired_rows(2).desired_width(f32::INFINITY).text_color(Color32::WHITE));
            let enter = r.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui.horizontal(|ui| {
                if self.running.is_some() {
                    if ui.button(RichText::new("Stop").color(Color32::WHITE)).clicked() {
                        self.stop();
                    }
                } else if ui.button(RichText::new("Send").color(Color32::WHITE)).clicked() || enter {
                    self.send(st, &ctx);
                }
                ui.checkbox(&mut self.can_edit, RichText::new("May change files").color(INK)).tip("Off: REX only reads the project and answers.\nOn: it can also create and change blueprints and scripts in this project. Everything is saved before it starts, and the editor shows its changes when it's done.");
            });
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_names_are_filled_in() {
        std::env::set_var("GEX_TEST_HOME", "C:\\Users\\me");
        assert_eq!(super::expand_percent("%GEX_TEST_HOME%\\.local\\bin"), "C:\\Users\\me\\.local\\bin");
        assert_eq!(super::expand_percent("C:\\plain"), "C:\\plain");
        assert_eq!(super::expand_percent("50% off"), "50% off");
    }
}
