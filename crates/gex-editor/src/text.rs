//! Game text: custom fonts (a .ttf / .otf from the project) and rich text tags.
//!
//! Tags: `[color=#ff8800]..[/color]` (or a name: red, green...), `[size=24]..[/size]`,
//! `[i]..[/i]`, `[u]..[/u]` (underline), `[s]..[/s]` (strikethrough), `[bg=#000000]..[/bg]`.
//! Anything else in square brackets is shown as written.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Stroke, TextFormat};

#[derive(Default)]
struct Loaded {
    /// Font file → its bytes (None: couldn't be read).
    data: HashMap<String, Option<Arc<Vec<u8>>>>,
}

fn store(ctx: &egui::Context) -> Arc<Mutex<Loaded>> {
    ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("gex fonts"), || Arc::new(Mutex::new(Loaded::default()))).clone())
}

/// The font family for a font file, loading it the first time. Until egui has it (the next
/// frame), and for an empty path or a file that won't load, the default font.
pub fn family(ctx: &egui::Context, vfs: &gex_assets::Vfs, path: &str) -> FontFamily {
    if path.is_empty() {
        return FontFamily::Proportional;
    }
    let name = FontFamily::Name(path.into());
    if ctx.fonts(|f| f.families().contains(&name)) {
        return name;
    }
    let st = store(ctx);
    let mut st = st.lock().unwrap();
    if st.data.contains_key(path) {
        return FontFamily::Proportional;
    }
    let bytes = vfs.read(path).ok().map(Arc::new);
    st.data.insert(path.to_string(), bytes);
    // Rebuild the font set: egui's own fonts, plus every game font as its own family (with
    // the default fonts behind it, for characters it doesn't have).
    let mut defs = base_fonts();
    let fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (p, b) in &st.data {
        let Some(b) = b else { continue };
        defs.font_data.insert(p.clone(), Arc::new(egui::FontData::from_owned(b.to_vec())));
        let mut list = vec![p.clone()];
        list.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(p.as_str().into()), list);
    }
    ctx.set_fonts(defs);
    FontFamily::Proportional
}

/// egui's own fonts, with the monospace one behind the others for the symbols they lack (arrows
/// like →, ▸, ■): text never shows empty boxes.
pub fn base_fonts() -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    let mono = defs.families.get(&FontFamily::Monospace).and_then(|m| m.first().cloned());
    if let (Some(m), Some(p)) = (mono, defs.families.get_mut(&FontFamily::Proportional)) {
        if !p.contains(&m) {
            p.push(m);
        }
    }
    defs
}

fn parse_color(s: &str) -> Option<Color32> {
    let t = s.trim().to_ascii_lowercase();
    let named = match t.as_str() {
        "white" => Some((255, 255, 255)),
        "black" => Some((0, 0, 0)),
        "red" => Some((230, 51, 51)),
        "green" => Some((77, 204, 77)),
        "blue" => Some((64, 115, 242)),
        "yellow" => Some((242, 217, 51)),
        "orange" => Some((242, 140, 38)),
        "purple" => Some((153, 77, 217)),
        "gray" | "grey" => Some((128, 128, 128)),
        _ => None,
    };
    if let Some((r, g, b)) = named {
        return Some(Color32::from_rgb(r, g, b));
    }
    let h = t.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(Color32::from_rgb(p(0)?, p(2)?, p(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(p(0)?, p(2)?, p(4)?, p(6)?)),
        _ => None,
    }
}

/// Text with tags as an egui layout. `scale` turns game pixels ([size=..]) into points.
pub fn job(text: &str, font: FontId, color: Color32, scale: f32, wrap: Option<f32>) -> LayoutJob {
    let mut job = LayoutJob::default();
    if let Some(w) = wrap {
        job.wrap.max_width = w;
    }
    let base = TextFormat { font_id: font.clone(), color, ..Default::default() };
    // A stack of (tag, format before it).
    let mut stack: Vec<(String, TextFormat)> = Vec::new();
    let mut fmt = base.clone();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(open) = rest.find('[') else {
            job.append(rest, 0.0, fmt.clone());
            break;
        };
        let Some(close) = rest[open..].find(']').map(|c| open + c) else {
            job.append(rest, 0.0, fmt.clone());
            break;
        };
        let tag = &rest[open + 1..close];
        let (name, arg) = tag.split_once('=').map(|(a, b)| (a.trim(), Some(b.trim()))).unwrap_or((tag.trim(), None));
        let lname = name.to_ascii_lowercase();
        let mut handled = true;
        let mut next = fmt.clone();
        if let Some(end) = lname.strip_prefix('/') {
            match stack.iter().rposition(|(t, _)| t == end) {
                Some(i) => {
                    next = stack[i].1.clone();
                    stack.truncate(i);
                }
                None => handled = false,
            }
        } else {
            match (lname.as_str(), arg) {
                ("color", Some(c)) => match parse_color(c) {
                    Some(c) => next.color = c,
                    None => handled = false,
                },
                ("bg", Some(c)) => match parse_color(c) {
                    Some(c) => next.background = c,
                    None => handled = false,
                },
                ("size", Some(n)) => match n.parse::<f32>() {
                    Ok(n) => next.font_id.size = (n * scale).max(4.0),
                    Err(_) => handled = false,
                },
                ("i", None) => next.italics = true,
                ("u", None) => next.underline = Stroke::new(1.0, fmt.color),
                ("s", None) => next.strikethrough = Stroke::new(1.0, fmt.color),
                _ => handled = false,
            }
            if handled {
                stack.push((lname.clone(), fmt.clone()));
            }
        }
        if handled {
            if open > 0 {
                job.append(&rest[..open], 0.0, fmt.clone());
            }
            fmt = next;
        } else {
            job.append(&rest[..=close], 0.0, fmt.clone());
        }
        rest = &rest[close + 1..];
    }
    job
}

/// Paints text (tags and all) anchored at `at`.
pub fn paint(painter: &egui::Painter, at: egui::Pos2, align: egui::Align2, text: &str, font: FontId, color: Color32, scale: f32) {
    paint_wrapped(painter, at, align, text, font, color, scale, None);
}

/// Paints text anchored at `at`, wrapping lines longer than `wrap` (screen points); each line
/// lines up the way `align` says.
#[allow(clippy::too_many_arguments)]
pub fn paint_wrapped(painter: &egui::Painter, at: egui::Pos2, align: egui::Align2, text: &str, font: FontId, color: Color32, scale: f32, wrap: Option<f32>) {
    let mut j = job(text, font, color, scale, wrap);
    j.halign = align.x();
    let galley = painter.layout_job(j);
    let rect = align.anchor_size(at, galley.size());
    painter.galley(rect.min - galley.rect.min.to_vec2(), galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        let j = job("a [color=red]b [i]c[/i][/color] [nope] d", FontId::proportional(10.0), Color32::WHITE, 1.0, None);
        assert_eq!(j.text, "a b c [nope] d");
        let fmt_of = |ch: char| {
            let i = j.text.find(ch).unwrap();
            j.sections.iter().find(|s| s.byte_range.start.0 <= i && i < s.byte_range.end.0).unwrap().format.clone()
        };
        assert_eq!(fmt_of('a').color, Color32::WHITE);
        assert_eq!(fmt_of('b').color, Color32::from_rgb(230, 51, 51));
        assert!(fmt_of('c').italics && fmt_of('c').color == Color32::from_rgb(230, 51, 51));
        assert!(!fmt_of('d').italics && fmt_of('d').color == Color32::WHITE);
        let j = job("[size=20]big[/size] [color=#00ff0080]x", FontId::proportional(10.0), Color32::WHITE, 2.0, None);
        assert_eq!(j.sections[0].format.font_id.size, 40.0);
        assert_eq!(j.text, "big x");
    }
}
