//! The script editor: JLB syntax highlighting, line numbers, live syntax checks.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};

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

pub fn script_ui(ui: &mut egui::Ui, st: &mut EditorState, res: &str) {
    let Some(s) = st.scripts.get_mut(res) else {
        ui.label("This script isn't open.");
        return;
    };
    if s.checked_text != s.text {
        s.problems = check(&s.text);
        s.checked_text = s.text.clone();
    }
    let mut save = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(res).monospace().color(theme::TEXT_DIM));
        if s.dirty() {
            ui.label(egui::RichText::new("*").color(theme::WARN)).on_hover_text("unsaved changes");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Save").on_hover_text("Ctrl+S").clicked() {
                save = true;
            }
            match s.problems.first() {
                Some((l, m)) => {
                    ui.label(egui::RichText::new(format!("line {}: {}", l, m)).color(theme::BAD));
                }
                None => {
                    ui.label(egui::RichText::new("✔ no syntax problems").color(theme::GOOD));
                }
            }
        });
    });
    ui.separator();

    let font = FontId::monospace(14.0);
    let row_h = ui.fonts_mut(|f| f.row_height(&font));
    let lines = s.text.lines().count().max(1) + usize::from(s.text.ends_with('\n'));
    let bad_lines: Vec<usize> = s.problems.iter().map(|p| p.0).collect();
    let goto = s.goto_line.take();
    let id = egui::Id::new(("script", res));
    let mut scroll = egui::ScrollArea::both().auto_shrink([false, false]).id_salt(("scroll", res));
    if let Some(l) = goto {
        scroll = scroll.vertical_scroll_offset(((l as f32 - 6.0).max(0.0)) * row_h);
    }
    scroll.show(ui, |ui| {
        ui.horizontal_top(|ui| {
            // Line numbers.
            let mut nums = String::new();
            for n in 1..=lines {
                nums.push_str(&format!("{:>4}\n", n));
            }
            let mut job = LayoutJob::default();
            for (k, l) in nums.lines().enumerate() {
                let c = if bad_lines.contains(&(k + 1)) { theme::BAD } else { theme::C_COMMENT };
                job.append(&format!("{}\n", l), 0.0, TextFormat { font_id: font.clone(), color: c, ..Default::default() });
            }
            ui.add(egui::Label::new(job).selectable(false));
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                let job = highlight(text.as_str(), font.clone());
                ui.fonts_mut(|f| f.layout_job(job))
            };
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
            if let Some(l) = goto {
                // Put the cursor at the start of that line.
                let idx: usize = s.text.lines().take(l.saturating_sub(1)).map(|x| x.chars().count() + 1).sum();
                let mut state = out.state.clone();
                let c = egui::text::CCursor::new(idx);
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(c)));
                state.store(ui.ctx(), id);
                out.response.request_focus();
            }
        });
    });
    if save {
        st.save_script(res);
    }
}
