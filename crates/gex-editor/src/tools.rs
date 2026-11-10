//! Editor tool panels: the profiler and find-in-files.

use eframe::egui::{self, Color32, RichText};

use crate::state::EditorState;
use crate::theme;

/// How the running game spends its time.
pub fn profiler(ui: &mut egui::Ui, st: &mut EditorState) {
    let Some(p) = &st.playing else {
        ui.label(RichText::new("Press Play: frame times, script and physics costs, and the slowest event handlers show here.").color(theme::TEXT_DIM));
        return;
    };
    if p.stats.is_empty() {
        ui.label("waiting for frames…");
        return;
    }
    let n = p.stats.len() as f32;
    let avg = |f: &dyn Fn(&(f32, gex_script::world::FrameStats)) -> f32| p.stats.iter().map(f).sum::<f32>() / n;
    let frame = avg(&|s| s.0);
    let game = avg(&|s| s.1.total_ms);
    let scripts = avg(&|s| s.1.scripts_ms);
    let physics = avg(&|s| s.1.physics_ms);
    let last = &p.stats.back().unwrap().1;
    egui::Grid::new("prof").num_columns(2).show(ui, |ui| {
        ui.label("Frames per second");
        ui.label(RichText::new(format!("{:.0}", 1000.0 / frame.max(0.01))).strong());
        ui.end_row();
        ui.label("Game step");
        ui.label(format!("{:.2} ms", game));
        ui.end_row();
        ui.label("  scripts");
        ui.label(format!("{:.2} ms", scripts));
        ui.end_row();
        ui.label("  physics");
        ui.label(format!("{:.2} ms", physics));
        ui.end_row();
        ui.label("Nodes / sprites / meshes");
        ui.label(format!("{} / {} / {}", last.nodes, last.sprites, last.meshes));
        ui.end_row();
    });
    // The last few seconds of frame times, as bars (green under 16.7 ms, yellow under 33, red above).
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(100.0), 80.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(18));
    let max_ms = 50.0;
    let w = rect.width() / 240.0;
    for (i, (ft, s)) in p.stats.iter().enumerate() {
        let x = rect.left() + i as f32 * w;
        let h = (ft / max_ms).min(1.0) * rect.height();
        let c = if *ft < 17.0 { Color32::from_rgb(80, 180, 100) } else if *ft < 34.0 { Color32::from_rgb(220, 190, 60) } else { Color32::from_rgb(220, 80, 70) };
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.bottom() - h), egui::pos2(x + w.max(1.0), rect.bottom())), 0.0, c);
        let gh = (s.total_ms / max_ms).min(1.0) * rect.height();
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.bottom() - gh), egui::pos2(x + w.max(1.0), rect.bottom())), 0.0, Color32::from_rgb(90, 150, 230));
    }
    let y60 = rect.bottom() - 16.7 / max_ms * rect.height();
    painter.hline(rect.x_range(), y60, egui::Stroke::new(1.0, Color32::from_gray(90)));
    ui.label(RichText::new("bars: time between frames (line = 60 fps); blue: the game's own step").small().color(theme::TEXT_DIM));
    ui.separator();
    ui.label(RichText::new("Slowest events last frame").strong());
    if last.slowest.is_empty() {
        ui.label(RichText::new("nothing over 0.05 ms").color(theme::TEXT_DIM));
    }
    for (res, h, ms) in last.slowest.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{:.2} ms", ms)).monospace());
            if ui.link(format!("{}  {}()", res.trim_start_matches("res://"), h)).clicked() {
                st.open_script(&res, None);
            }
        });
    }
}

/// Searching every text file in the project.
#[derive(Default)]
pub struct Search {
    pub query: String,
    pub case: bool,
    /// (file, line number, the line)
    pub results: Vec<(String, usize, String)>,
    pub searched: String,
    /// Put the cursor in the search box (Ctrl+Shift+F).
    pub focus: bool,
}

pub fn search(ui: &mut egui::Ui, st: &mut EditorState) {
    let mut go = false;
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut st.search.query).hint_text("find in every script and scene…").desired_width(ui.available_width() - 120.0));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            go = true;
        }
        if std::mem::take(&mut st.search.focus) || (ui.memory(|m| m.focused().is_none()) && st.search.results.is_empty() && st.search.query.is_empty()) {
            r.request_focus();
        }
        ui.checkbox(&mut st.search.case, "Aa").on_hover_text("match case");
        if ui.button("Find").clicked() {
            go = true;
        }
    });
    if go && !st.search.query.is_empty() {
        st.search.results = find_in_files(&st.project.dir, &st.search.query, st.search.case);
        st.search.searched = st.search.query.clone();
    }
    ui.separator();
    if !st.search.searched.is_empty() {
        ui.label(RichText::new(format!("{} match(es) for {:?}", st.search.results.len(), st.search.searched)).color(theme::TEXT_DIM).small());
    }
    let mut open = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (file, line, text) in &st.search.results {
            ui.horizontal(|ui| {
                if ui.link(RichText::new(format!("{}:{}", file.trim_start_matches("res://"), line)).monospace()).clicked() {
                    open = Some((file.clone(), *line));
                }
                ui.label(RichText::new(text.trim()).monospace().color(theme::TEXT_DIM));
            });
        }
    });
    if let Some((file, line)) = open {
        if file.ends_with(".jscene") {
            if st.playing.is_none() {
                st.open_scene(&file);
            }
        } else {
            st.open_script(&file, Some(line));
        }
    }
}

pub fn find_in_files(dir: &std::path::Path, query: &str, case: bool) -> Vec<(String, usize, String)> {
    fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with('.') || e.file_name() == "target" {
                continue;
            }
            if p.is_dir() {
                walk(&p, out);
            } else if matches!(p.extension().and_then(|x| x.to_str()), Some("jlb" | "jscene" | "jproj" | "txt" | "wgsl" | "json" | "md")) {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    files.sort();
    let q = if case { query.to_string() } else { query.to_lowercase() };
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let res = format!("res://{}", f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().replace('\\', "/"));
        for (i, l) in text.lines().enumerate() {
            let hay = if case { l.to_string() } else { l.to_lowercase() };
            if hay.contains(&q) {
                out.push((res.clone(), i + 1, l.chars().take(200).collect()));
                if out.len() >= 2000 {
                    return out;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_across_files() {
        let dir = std::env::temp_dir().join(format!("gex-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::templates::create_demo(&dir).unwrap();
        let r = super::find_in_files(&dir, "change_scene", false);
        assert!(r.iter().any(|(f, _, _)| f == "res://scripts/menu.jlb"));
        assert!(r.iter().all(|(_, l, t)| *l > 0 && t.to_lowercase().contains("change_scene")));
        assert!(super::find_in_files(&dir, "change_scene", true).is_empty(), "case matters when asked");
    }
}
