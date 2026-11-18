//! The script editor: JLB syntax highlighting, line numbers, live syntax checks.

use crate::docs::Tip as _;
use eframe::egui::{self, text::LayoutJob, Color32, FontId, RichText, TextFormat};

use crate::vim::{Mode, Outcome};

use crate::state::EditorState;
use crate::theme;

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn highlight(text: &str, font: FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let fmt = |c: Color32| TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    let engine: Vec<&str> = gex_script::commands::COMMANDS.iter().map(|c| c.0).collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let at = |i: usize| chars.get(i).map(|c| c.1).unwrap_or('\0');
    let byte = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = at(i);
        let start = i;
        let color;
        if c == '>' && at(i + 1) == '>' && at(i + 2) == '>' {
            // Block comment to <<<
            i += 3;
            while i < chars.len() && !(at(i) == '<' && at(i + 1) == '<' && at(i + 2) == '<') {
                i += 1;
            }
            i = (i + 3).min(chars.len());
            color = theme::C_COMMENT;
        } else if c == '>' && at(i + 1) == '>' {
            while i < chars.len() && at(i) != '\n' {
                i += 1;
            }
            color = theme::C_COMMENT;
        } else if c == '"' {
            i += 1;
            while i < chars.len() && at(i) != '"' && at(i) != '\n' {
                if at(i) == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            color = theme::C_STRING;
        } else if c.is_ascii_digit() {
            while i < chars.len() && (at(i).is_ascii_alphanumeric() || at(i) == '.' || at(i) == '_') {
                i += 1;
            }
            color = theme::C_NUMBER;
        } else if c == '$' {
            // A variable set per instance in Properties.
            i += 1;
            while i < chars.len() && is_word(at(i)) {
                i += 1;
            }
            color = theme::C_EVENT;
        } else if is_word(c) {
            while i < chars.len() && is_word(at(i)) {
                i += 1;
            }
            let w = &text[byte(start)..byte(i)];
            color = if w.chars().all(|c| !c.is_ascii_lowercase()) && w.chars().any(|c| c.is_ascii_uppercase()) {
                if jlb::lexer::KEYWORDS.contains(&w) || matches!(w, "AND" | "OR" | "NOT" | "XOR" | "NAND" | "NOR" | "XNOR" | "FUNC" | "RTRN" | "TEXT" | "STRING" | "STR") {
                    theme::C_KEYWORD
                } else if engine.contains(&w) {
                    theme::C_ENGINE
                } else {
                    theme::C_BUILTIN
                }
            } else if w.starts_with("on_") || w.starts_with("when_") {
                theme::C_EVENT
            } else if w == "self" {
                theme::C_KEYWORD
            } else {
                theme::TEXT
            };
        } else {
            i += 1;
            color = theme::TEXT_DIM;
        }
        job.append(&text[byte(start)..byte(i)], 0.0, fmt(color));
    }
    job
}

/// Syntax problems as (line, message), from the interpreter's own checker.
pub fn check(text: &str) -> Vec<(usize, String)> {
    let json = jlb::ide::check(text);
    let mut out = Vec::new();
    let Some(list) = json.find("\"errors\":[").map(|i| &json[i + 10..]) else { return out };
    for obj in list.split("},{") {
        let msg = obj.find("\"message\":\"").map(|i| {
            let rest = &obj[i + 11..];
            let mut s = String::new();
            let mut esc = false;
            for c in rest.chars() {
                if esc {
                    s.push(match c {
                        'n' => '\n',
                        't' => '\t',
                        o => o,
                    });
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    break;
                } else {
                    s.push(c);
                }
            }
            s
        });
        let line = obj.find("\"line\":").and_then(|i| obj[i + 7..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok());
        if let (Some(m), Some(l)) = (msg, line) {
            out.push((l, m));
        }
    }
    out
}

/// Words worth completing: engine commands, built-ins, keywords, events, and the file's own names.
fn completions(text: &str, prefix: &str) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut push = |w: &str, kind: &'static str| {
        if w.len() > prefix.len() && w.starts_with(prefix) && !out.iter().any(|(x, _)| x == w) {
            out.push((w.to_string(), kind));
        }
    };
    if prefix.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        for (c, _, _) in gex_script::commands::COMMANDS {
            push(c, "engine");
        }
        for b in jlb::lexer::BUILTINS {
            push(b, "built-in");
        }
        for k in jlb::lexer::KEYWORDS {
            push(k, "keyword");
        }
    } else {
        for (sig, _) in crate::panels::EVENTS {
            push(sig.split('(').next().unwrap_or(""), "event");
        }
        push("self", "keyword");
        let mut w = String::new();
        for ch in text.chars().chain(std::iter::once(' ')) {
            if is_word(ch) {
                w.push(ch);
            } else {
                if !w.is_empty() && w.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    push(&w, "name");
                }
                w.clear();
            }
        }
    }
    out.sort_by_key(|(w, _)| w.len());
    out.truncate(12);
    out
}

/// The word around a char index: (start, end).
fn word_at(c: &[char], at: usize) -> (usize, usize) {
    let mut a = at.min(c.len());
    while a > 0 && is_word(c[a - 1]) {
        a -= 1;
    }
    let mut b = at.min(c.len());
    while b < c.len() && is_word(c[b]) {
        b += 1;
    }
    (a, b)
}

/// Where `DEF name(` is: in this script first, then any script in the project.
fn find_definition(st: &EditorState, res: &str, name: &str) -> Option<(String, usize)> {
    let find = |text: &str| {
        text.lines().position(|l| {
            let l = l.trim_start();
            let l = l.strip_prefix("GLOBAL ").unwrap_or(l);
            l.strip_prefix("DEF ").or_else(|| l.strip_prefix("FUNC ")).is_some_and(|r| r.trim_start().starts_with(&format!("{}(", name)))
        })
    };
    if let Some(l) = find(&st.script_text(res)) {
        return Some((res.to_string(), l + 1));
    }
    for f in crate::panels::list_files(st, "res://", "jlb") {
        if let Some(l) = find(&st.script_text(&f)) {
            return Some((f, l + 1));
        }
    }
    None
}

pub fn script_ui(ui: &mut egui::Ui, st: &mut EditorState, res: &str) {
    crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:script_editor");
    let vim_on = st.vim_enabled;
    let paused_here: Option<usize> = st.playing.as_ref().and_then(|p| p.handle.paused.as_ref()).and_then(|p| {
        let path = st.project.res_to_path(res)?.canonicalize().ok()?;
        (std::path::Path::new(&p.file) == path).then_some(p.line)
    });
    let bps: Vec<usize> = st.breakpoints.get(res).map(|b| b.iter().copied().collect()).unwrap_or_default();
    let conditional: Vec<usize> = st.bp_conditions.keys().filter(|(r, _)| r == res).map(|(_, l)| *l).collect();
    let Some(s) = st.scripts.get_mut(res) else {
        ui.label("This script isn't open.");
        return;
    };
    if s.checked_text != s.text {
        s.problems = if res.ends_with(".jlb") { check(&s.text) } else { Vec::new() };
        s.checked_text = s.text.clone();
    }
    let mut save = false;
    let mut toggle_vim = false;
    let mut f1_command: Option<String> = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(s.dirty(), egui::Button::new("Save")).tip("Saves this script (Ctrl+S).").clicked() {
            save = true;
        }
        if ui.selectable_label(s.find.is_some(), "Find").tip("Find and replace in this script (Ctrl+H)").clicked() {
            s.find = if s.find.is_some() { None } else { Some((String::new(), String::new())) };
        }
        if ui.selectable_label(vim_on, "Vim").tip("Vim keys: Esc for normal mode, i to type").clicked() {
            toggle_vim = true;
        }
        if ui.button("Commands").tip("Every engine command, explained with examples (F1; F1 on a command opens its page)").clicked() {
            f1_command = Some(String::new());
        }
        if vim_on {
            let m = if s.vim.mode == Mode::Insert { "INSERT" } else { "NORMAL" };
            ui.label(RichText::new(m).small().strong().color(if s.vim.mode == Mode::Insert { theme::GOOD } else { theme::ACCENT }));
        }
        // The first problem, if there is one (the file's name is on its tab).
        if let Some((l, m)) = s.problems.first() {
            ui.separator();
            ui.add(egui::Label::new(RichText::new(format!("Line {}: {}", l, m)).color(theme::BAD)).truncate());
        }
    });
    let id = egui::Id::new(("script", res));
    let focused = ui.memory(|m| m.has_focus(id));
    if focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::H)) {
        s.find = Some((s.find.take().map(|f| f.0).unwrap_or_default(), String::new()));
    }

    // Find / replace.
    let mut select: Option<(usize, usize)> = None;
    if let Some((q, r)) = &mut s.find {
        let mut next = false;
        let mut replace_one = false;
        let mut replace_all = false;
        let mut close = false;
        ui.horizontal(|ui| {
            let fr = ui.add(egui::TextEdit::singleline(q).hint_text("find").desired_width(160.0));
            if fr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                next = true;
                fr.request_focus();
            }
            ui.add(egui::TextEdit::singleline(r).hint_text("replace with").desired_width(160.0));
            next |= ui.button("Next").tip("Jumps to the next match.").clicked();
            replace_one = ui.button("Replace").tip("Replaces this match and moves to the next.").clicked();
            replace_all = ui.button("All").tip("Replaces every match in this script.").clicked();
            close = ui.small_button("✖").clicked();
            let n = if q.is_empty() { 0 } else { s.text.matches(q.as_str()).count() };
            ui.label(RichText::new(format!("{} found", n)).small().color(theme::TEXT_DIM));
        });
        let state = egui::TextEdit::load_state(ui.ctx(), id);
        let cur = state.as_ref().and_then(|st| st.cursor.char_range()).map(|r| r.primary.index.0).unwrap_or(0);
        if !q.is_empty() {
            let c: Vec<char> = s.text.chars().collect();
            let qc: Vec<char> = q.chars().collect();
            let find_from = |from: usize| -> Option<usize> {
                (from..c.len()).chain(0..from).find(|i| c[*i..].starts_with(&qc))
            };
            if replace_all {
                s.text = s.text.replace(q.as_str(), r.as_str());
            } else if replace_one {
                let sel = state.as_ref().and_then(|st| st.cursor.char_range()).map(|r| (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0)));
                if let Some((a, b)) = sel.filter(|(a, b)| c[*a..*b] == qc[..]) {
                    let mut nc = c.clone();
                    nc.splice(a..b, r.chars());
                    s.text = nc.iter().collect();
                    let nc: Vec<char> = s.text.chars().collect();
                    if let Some(i) = (a + r.chars().count()..nc.len()).find(|i| nc[*i..].starts_with(&qc)) {
                        select = Some((i, i + qc.len()));
                    }
                } else if let Some(i) = find_from(cur) {
                    select = Some((i, i + qc.len()));
                }
            } else if next {
                if let Some(i) = find_from(cur + 1) {
                    select = Some((i, i + qc.len()));
                }
            }
        }
        if close {
            s.find = None;
        }
    }
    ui.separator();

    // Keys handled before the text box sees them: Vim normal mode, completion, F12.
    let completion_id = id.with("completion");
    let mut popup: Option<(Vec<(String, &'static str)>, usize)> = ui.data(|d| d.get_temp(completion_id));
    let mut set_cursor: Option<usize> = None;
    let state = egui::TextEdit::load_state(ui.ctx(), id);
    let cursor = state.as_ref().and_then(|st| st.cursor.char_range()).map(|r| r.primary.index.0).unwrap_or(0);
    if focused {
        if let Some((items, sel)) = &mut popup {
            let (up, down, accept, esc) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                )
            });
            if up {
                *sel = sel.saturating_sub(1);
            }
            if down {
                *sel = (*sel + 1).min(items.len().saturating_sub(1));
            }
            if accept && !items.is_empty() {
                let c: Vec<char> = s.text.chars().collect();
                let (a, _) = word_at(&c, cursor);
                let (w, kind) = &items[*sel];
                let ins = if *kind == "engine" || *kind == "built-in" { format!("{}(", w) } else { w.clone() };
                let mut nc = c.clone();
                nc.splice(a..cursor, ins.chars());
                s.text = nc.iter().collect();
                set_cursor = Some(a + ins.chars().count());
                popup = None;
            } else if esc {
                popup = None;
            }
        }
        // F1 on a command: its page in the Command reference.
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1)) {
            let c: Vec<char> = s.text.chars().collect();
            let (a, b) = word_at(&c, cursor);
            let w: String = c[a..b].iter().collect();
            f1_command = Some(if gex_script::commands::COMMANDS.iter().any(|x| x.0 == w) { w } else { String::new() });
        }
        if ui.input(|i| i.key_pressed(egui::Key::F12)) {
            let c: Vec<char> = s.text.chars().collect();
            let (a, b) = word_at(&c, cursor);
            let w: String = c[a..b].iter().collect();
            if !w.is_empty() {
                let found = find_definition_text(&s.text, &w).map(|l| (res.to_string(), l));
                ui.data_mut(|d| d.insert_temp(id.with("goto"), (w, found)));
            }
        }
        if vim_on {
            let mut keys: Vec<(char, bool)> = Vec::new();
            let mut escape = false;
            ui.input_mut(|i| {
                i.events.retain(|e| match e {
                    egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                        escape = true;
                        false
                    }
                    _ if s.vim.mode == Mode::Insert => true,
                    egui::Event::Text(t) => {
                        keys.extend(t.chars().map(|c| (c, false)));
                        false
                    }
                    egui::Event::Key { key, pressed: true, modifiers, .. } if modifiers.ctrl && *key == egui::Key::R => {
                        keys.push(('r', true));
                        false
                    }
                    egui::Event::Key { key, .. } => !matches!(key, egui::Key::Backspace | egui::Key::Delete | egui::Key::Enter | egui::Key::Tab),
                    _ => true,
                })
            });
            if escape && s.vim.mode == Mode::Insert {
                s.vim.mode = Mode::Normal;
                set_cursor = Some(cursor.saturating_sub(1));
                popup = None;
            }
            if s.vim.mode == Mode::Normal {
                let mut cur = set_cursor.unwrap_or(cursor);
                for (k, ctrl) in keys {
                    if s.vim.key(k, ctrl, &mut s.text, &mut cur) == Outcome::Command {
                        st_open_command(ui);
                    }
                    if s.vim.mode == Mode::Insert {
                        break;
                    }
                }
                if cur != cursor || set_cursor.is_some() {
                    set_cursor = Some(cur);
                }
            }
        }
    }

    let font = FontId::monospace(14.0);
    let row_h = ui.fonts_mut(|f| f.row_height(&font));
    let lines = s.text.lines().count().max(1) + usize::from(s.text.ends_with('\n'));
    let bad_lines: Vec<usize> = s.problems.iter().map(|p| p.0).collect();
    let goto = s.goto_line.take().or(paused_here.filter(|_| ui.data(|d| d.get_temp::<usize>(id.with("shown pause"))) != paused_here));
    if paused_here.is_some() {
        ui.data_mut(|d| d.insert_temp(id.with("shown pause"), paused_here.unwrap()));
    }
    let mut scroll = egui::ScrollArea::both().auto_shrink([false, false]).id_salt(("scroll", res));
    if let Some(l) = goto {
        scroll = scroll.vertical_scroll_offset(((l as f32 - 6.0).max(0.0)) * row_h);
    }
    let mut toggle_bp: Option<usize> = None;
    let mut edit_condition: Option<usize> = None;
    let mut cursor_rect: Option<egui::Rect> = None;
    let mut typed = false;
    scroll.show(ui, |ui| {
        ui.horizontal_top(|ui| {
            // Gutter: line numbers; click one for a breakpoint.
            let gutter_w = ui.fonts_mut(|f| f.glyph_width(&font, '0')) * 6.0;
            let (g_rect, g_resp) = ui.allocate_exact_size(egui::vec2(gutter_w, row_h * lines.max(30) as f32), egui::Sense::click());
            let painter = ui.painter_at(g_rect);
            for n in 1..=lines {
                let y = g_rect.top() + (n - 1) as f32 * row_h;
                let r = egui::Rect::from_min_size(egui::pos2(g_rect.left(), y), egui::vec2(gutter_w, row_h));
                if paused_here == Some(n) {
                    painter.rect_filled(r, 0.0, theme::WARN.gamma_multiply(0.35));
                }
                if bps.contains(&n) {
                    let c = egui::pos2(r.left() + row_h * 0.45, r.center().y);
                    painter.circle_filled(c, row_h * 0.3, theme::BAD);
                    if conditional.contains(&n) {
                        painter.text(c, egui::Align2::CENTER_CENTER, "?", egui::FontId::monospace(row_h * 0.55), Color32::WHITE);
                    }
                }
                let c = if bad_lines.contains(&n) { theme::BAD } else { theme::C_COMMENT };
                painter.text(egui::pos2(r.right() - 6.0, r.center().y), egui::Align2::RIGHT_CENTER, n.to_string(), font.clone(), c);
            }
            if let Some(p) = g_resp.interact_pointer_pos().filter(|_| g_resp.clicked()) {
                toggle_bp = Some(((p.y - g_rect.top()) / row_h).floor() as usize + 1);
            }
            if let Some(p) = g_resp.interact_pointer_pos().filter(|_| g_resp.secondary_clicked()) {
                let l = ((p.y - g_rect.top()) / row_h).floor() as usize + 1;
                if l >= 1 && l <= lines {
                    edit_condition = Some(l);
                }
            }
            let g_resp = g_resp.tip("click a line number for a breakpoint (the game stops there); right-click for one that stops only when something is true");
            let _ = g_resp;
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                let job = highlight(text.as_str(), font.clone());
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let before = s.text.clone();
            let out = egui::TextEdit::multiline(&mut s.text)
                .id(id)
                .font(font.clone())
                .code_editor()
                .lock_focus(true)
                .desired_width(f32::INFINITY)
                .desired_rows(lines.max(30))
                .frame(egui::Frame::NONE)
                .layouter(&mut layouter)
                .show(ui);
            typed = s.text != before;
            // Middle-click a word: what it is.
            if let (true, Some(p)) = (crate::docs::clicked(ui.ctx()) && out.response.contains_pointer(), ui.ctx().pointer_latest_pos()) {
                let at: usize = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                let chars: Vec<char> = s.text.chars().collect();
                let word_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
                let (mut a, mut b) = (at.min(chars.len()), at.min(chars.len()));
                while a > 0 && word_char(chars[a - 1]) {
                    a -= 1;
                }
                while b < chars.len() && word_char(chars[b]) {
                    b += 1;
                }
                let w: String = chars[a..b].iter().collect();
                let topic = if w.starts_with('$') {
                    "ui:script_vars".to_string()
                } else if w.starts_with("on_") || w.starts_with("when_") {
                    format!("event:{}", w)
                } else if w.len() > 1 && w.chars().all(|c| !c.is_ascii_lowercase()) && w.chars().any(|c| c.is_ascii_uppercase()) {
                    format!("cmd:{}", w)
                } else {
                    "ui:script_editor".to_string()
                };
                crate::docs::open(ui.ctx(), topic);
            }
            if let Some(l) = paused_here {
                let r = egui::Rect::from_min_size(egui::pos2(out.response.rect.left(), out.response.rect.top() + (l - 1) as f32 * row_h), egui::vec2(out.response.rect.width(), row_h));
                ui.painter().rect_filled(r, 0.0, theme::WARN.gamma_multiply(0.12));
            }
            let target = set_cursor.map(|c| (c, c)).or(select).or(goto.map(|l| {
                // After the line's indent, ready to type.
                let indent = s.text.lines().nth(l.saturating_sub(1)).map(|x| x.chars().take_while(|c| c.is_whitespace()).count()).unwrap_or(0);
                let idx: usize = s.text.lines().take(l.saturating_sub(1)).map(|x| x.chars().count() + 1).sum::<usize>() + indent;
                (idx, idx)
            }));
            if let Some((a, b)) = target {
                let mut state = out.state.clone();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
                state.store(ui.ctx(), id);
                out.response.request_focus();
            }
            if let Some(r) = out.cursor_range {
                let p = out.galley.pos_from_cursor(r.primary);
                cursor_rect = Some(p.translate(out.galley_pos.to_vec2()));
            }
            // Ctrl+click: go to a function's definition.
            if out.response.clicked() && ui.input(|i| i.modifiers.command) {
                if let Some(r) = out.cursor_range {
                    let c: Vec<char> = s.text.chars().collect();
                    let (a, b) = word_at(&c, r.primary.index.0);
                    let w: String = c[a..b].iter().collect();
                    if !w.is_empty() {
                        ui.data_mut(|d| d.insert_temp(id.with("goto"), (w, Option::<(String, usize)>::None)));
                    }
                }
            }
        });
    });

    // Completion popup, from what's just been typed.
    let mut now_cursor = egui::TextEdit::load_state(ui.ctx(), id).and_then(|st| st.cursor.char_range()).map(|r| r.primary.index.0).unwrap_or(0);
    // Enter keeps the indentation (one more level after a "{").
    if typed && now_cursor > 0 {
        let c: Vec<char> = s.text.chars().collect();
        if c.get(now_cursor - 1) == Some(&'\n') {
            let ls = c[..now_cursor - 1].iter().rposition(|x| *x == '\n').map(|p| p + 1).unwrap_or(0);
            let prev: String = c[ls..now_cursor - 1].iter().collect();
            let mut indent: String = prev.chars().take_while(|x| *x == ' ' || *x == '\t').collect();
            if prev.trim_end().ends_with('{') {
                indent.push('\t');
            }
            if !indent.is_empty() {
                let mut nc = c.clone();
                nc.splice(now_cursor..now_cursor, indent.chars());
                s.text = nc.iter().collect();
                now_cursor += indent.chars().count();
                let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(now_cursor))));
                state.store(ui.ctx(), id);
            }
        }
    }
    if typed && (!vim_on || s.vim.mode == Mode::Insert) {
        let c: Vec<char> = s.text.chars().collect();
        let (a, _) = word_at(&c, now_cursor);
        let prefix: String = c[a..now_cursor.min(c.len())].iter().collect();
        let in_comment = c[..a].iter().rposition(|x| *x == '\n').map(|p| p + 1).map(|ls| c[ls..a].iter().collect::<String>().contains(">>")).unwrap_or(false);
        popup = if prefix.chars().count() >= 2 && !in_comment {
            let items = completions(&s.text, &prefix);
            (!items.is_empty()).then_some((items, 0))
        } else {
            None
        };
    } else if !focused {
        popup = None;
    }
    if let (Some((items, sel)), Some(r)) = (&popup, cursor_rect) {
        egui::Area::new(completion_id).fixed_pos(r.left_bottom()).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for (i, (w, kind)) in items.iter().enumerate() {
                    let help = gex_script::commands::COMMANDS.iter().find(|c| c.0 == w.as_str()).map(|c| format!("{}{}  {}", c.0, c.1, c.2));
                    let r = ui.add(egui::Button::selectable(i == *sel, RichText::new(format!("{}  ", w)).monospace()).right_text(RichText::new(*kind).small().color(theme::TEXT_DIM)));
                    if let Some(h) = help {
                        r.tip(h);
                    }
                }
                ui.label(RichText::new("Tab to accept, Up/Down to choose").small().color(theme::TEXT_DIM));
            });
        });
    }
    ui.data_mut(|d| match popup {
        Some(p) => {
            d.insert_temp(completion_id, p);
        }
        None => d.remove::<(Vec<(String, &'static str)>, usize)>(completion_id),
    });

    if let Some(l) = toggle_bp {
        if l >= 1 && l <= lines {
            st.toggle_breakpoint(res, l);
        }
    }
    if let Some(l) = edit_condition {
        let now = st.bp_conditions.get(&(res.to_string(), l)).cloned().unwrap_or_default();
        st.bp_edit = Some((res.to_string(), l, now));
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("bp condition focus"), true));
    }
    condition_window(ui.ctx(), st);
    if save {
        st.save_script(res);
    }
    if toggle_vim {
        st.vim_enabled = !st.vim_enabled;
        let _ = std::fs::create_dir_all(crate::state::config_dir());
        let _ = std::fs::write(crate::state::config_dir().join("vim"), if st.vim_enabled { "on" } else { "off" });
    }
    if ui.data(|d| d.get_temp::<bool>(egui::Id::new("vim wants command"))).unwrap_or(false) {
        ui.data_mut(|d| d.remove::<bool>(egui::Id::new("vim wants command")));
        st.open_command = true;
    }
    // Go to a definition (F12 or Ctrl+click).
    if let Some((w, _)) = ui.data_mut(|d| d.remove_temp::<(String, Option<(String, usize)>)>(id.with("goto"))) {
        match find_definition(st, res, &w) {
            Some((file, line)) => st.open_script(&file, Some(line)),
            None => st.status = format!("no DEF {}( found in the project", w),
        }
    }
    if let Some(w) = f1_command {
        st.commands_open = true;
        st.commands_show = (!w.is_empty()).then_some(w);
    }
}

fn find_definition_text(text: &str, name: &str) -> Option<usize> {
    text.lines().position(|l| l.trim_start().starts_with(&format!("DEF {}(", name))).map(|l| l + 1)
}

fn st_open_command(ui: &egui::Ui) {
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("vim wants command"), true));
}

/// The little window for a breakpoint's condition (right-click a line number).
fn condition_window(ctx: &egui::Context, st: &mut EditorState) {
    let Some((res, line, mut text)) = st.bp_edit.take() else { return };
    let mut open = true;
    let mut done = None;
    let first = ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("bp condition focus"))).unwrap_or(false);
    let mut w = egui::Window::new(format!("Breakpoint on line {}", line)).id(egui::Id::new("bp condition")).collapsible(false).resizable(false).open(&mut open);
    if first {
        if let Some(p) = ctx.pointer_latest_pos() {
            w = w.current_pos(p + egui::vec2(12.0, -10.0));
        }
    }
    w.show(ctx, |ui| {
        ui.label("Stop only when this is TRUE (empty: always stop):");
        let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text("hp < 10 AND self.x > 300").font(egui::TextStyle::Monospace).desired_width(280.0));
        if first {
            r.request_focus();
        }
        let enter = (r.has_focus() || r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            if ui.button("Set").tip("Keeps this condition: the game stops here only when it's true.").clicked() || enter {
                done = Some(true);
            }
            if ui.button("Remove breakpoint").tip("The game no longer stops at this line.").clicked() {
                done = Some(false);
            }
        });
    });
    match done {
        Some(true) => st.set_breakpoint_condition(&res, line, &text),
        Some(false) => {
            st.set_breakpoint_condition(&res, line, "");
            if st.breakpoints.get(&res).is_some_and(|b| b.contains(&line)) {
                st.toggle_breakpoint(&res, line);
            }
        }
        None if open => st.bp_edit = Some((res, line, text)),
        None => {}
    }
}
