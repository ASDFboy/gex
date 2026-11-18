//! Editor tool panels: the profiler and find-in-files.

use crate::docs::Tip as _;
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
        ui.label("Objects / sprites / meshes");
        ui.label(format!("{} / {} / {}", last.objects, last.sprites, last.meshes));
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
    /// Put the cursor in the search box (Ctrl+F).
    pub focus: bool,
    /// The find window is showing (over everything else).
    pub open: bool,
}

/// Find in files: a window over whatever is open (Ctrl+F; Escape closes).
pub fn search_window(ctx: &egui::Context, st: &mut EditorState) {
    if !st.search.open {
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        st.search.open = false;
        return;
    }
    let mut open = true;
    egui::Window::new("Find in files")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([680.0, 380.0])
        .anchor(egui::Align2::CENTER_TOP, [0.0, 54.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            crate::theme::fade_in(ui);
            crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:find");
            search(ui, st)
        });
    if !open {
        st.search.open = false;
    }
}

pub fn search(ui: &mut egui::Ui, st: &mut EditorState) {
    let mut go = false;
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut st.search.query).hint_text("find in every script and blueprint…").desired_width(ui.available_width() - 120.0));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            go = true;
        }
        if std::mem::take(&mut st.search.focus) {
            r.request_focus();
        }
        ui.checkbox(&mut st.search.case, "Aa").tip("match case");
        if ui.button("Find").tip("Searches now.").clicked() {
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
        st.search.open = false;
        if file.ends_with(".jblueprint") {
            if st.playing.is_none() {
                st.open_blueprint(&file);
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
            } else if matches!(p.extension().and_then(|x| x.to_str()), Some("jlb" | "jblueprint" | "jproj" | "txt" | "wgsl" | "json" | "md")) {
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
        let r = super::find_in_files(&dir, "change_blueprint", false);
        assert!(r.iter().any(|(f, _, _)| f == "res://scripts/menu.jlb"));
        assert!(r.iter().all(|(_, l, t)| *l > 0 && t.to_lowercase().contains("change_blueprint")));
        assert!(super::find_in_files(&dir, "change_blueprint", true).is_empty(), "case matters when asked");
    }
}

/// The Command reference (View → Commands, F1) as a window (the classic layout; the tabbed
/// one shows it in a tab): every engine command with how to use it and an example, the
/// events, and the basics. Searchable.
pub fn command_window(ctx: &egui::Context, st: &mut EditorState) {
    if !st.commands_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Commands")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([860.0, 600.0])
        .default_pos(ctx.content_rect().center() - egui::vec2(430.0, 300.0))
        .min_size([520.0, 300.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            crate::theme::fade_in(ui);
            command_page(ui, st);
        });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        st.commands_open = false;
    }
}

/// The Command reference's contents: the list of commands (searchable) on the left and the
/// picked one's page. In a tab of its own, or in a window in the classic layout.
pub fn command_page(ui: &mut egui::Ui, st: &mut EditorState) {
    use gex_script::command_guide::{BASICS, EVENTS, GROUPS, GUIDE};
    let id = egui::Id::new("command reference");
    let (mut filter, mut pick): (String, String) = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| (String::new(), "Basics".into()));
    if let Some(want) = st.commands_show.take() {
        pick = want;
        filter.clear();
    }
    egui::Panel::left("command list").resizable(true).default_size(230.0).min_size(160.0).show(ui, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search commands").desired_width(f32::INFINITY));
        if r.changed() {
            // The first match shows as you type.
            let f = filter.to_lowercase();
            if let Some(g) = GUIDE.iter().find(|g| !f.is_empty() && g.0.to_lowercase().contains(&f)) {
                pick = g.0.to_string();
            }
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let f = filter.to_lowercase();
            let hit = |g: &(&str, &str, &str, &str)| {
                let summary = gex_script::commands::COMMANDS.iter().find(|c| c.0 == g.0).map_or("", |c| c.2);
                f.is_empty() || g.0.to_lowercase().contains(&f) || summary.to_lowercase().contains(&f) || g.2.to_lowercase().contains(&f)
            };
            if f.is_empty() {
                for page in ["Basics", "Events"] {
                    if ui.selectable_label(pick == page, RichText::new(page).strong()).clicked() {
                        pick = page.into();
                    }
                }
            }
            for (group, _) in GROUPS {
                let cmds: Vec<_> = GUIDE.iter().filter(|g| g.1 == *group && hit(g)).collect();
                if cmds.is_empty() {
                    continue;
                }
                ui.add_space(6.0);
                ui.label(RichText::new(group.to_uppercase()).small().color(crate::theme::TEXT_DIM));
                for g in cmds {
                    if ui.selectable_label(pick == g.0, RichText::new(g.0).monospace()).clicked() {
                        pick = g.0.to_string();
                    }
                }
            }
        });
    });
    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(&pick).show(ui, |ui| {
            ui.add_space(4.0);
            match pick.as_str() {
                "Basics" => {
                    ui.heading("Using commands");
                    ui.add_space(6.0);
                    for para in BASICS.split("\n\n") {
                        ui.label(para);
                        ui.add_space(6.0);
                    }
                }
                "Events" => {
                    ui.heading("Events");
                    ui.label(RichText::new("Functions with these names in a script are called by the engine. Add them with Events → Add event.").color(crate::theme::TEXT_DIM));
                    ui.add_space(8.0);
                    egui::Grid::new("events").num_columns(2).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
                        for (name, when) in EVENTS {
                            ui.label(RichText::new(*name).monospace().color(crate::theme::ACCENT));
                            ui.add(egui::Label::new(*when).wrap());
                            ui.end_row();
                        }
                    });
                }
                name => {
                    let Some(g) = GUIDE.iter().find(|g| g.0 == name) else { return };
                    let (inputs, summary) = gex_script::commands::COMMANDS.iter().find(|c| c.0 == name).map_or(("", ""), |c| (c.1, c.2));
                    ui.label(RichText::new(format!("{}{}", g.0, inputs)).monospace().size(18.0).color(crate::theme::ACCENT));
                    ui.label(RichText::new(summary).color(crate::theme::TEXT_DIM));
                    ui.add_space(10.0);
                    for para in g.2.split("\n\n") {
                        ui.label(para);
                        ui.add_space(6.0);
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("EXAMPLE").small().color(crate::theme::TEXT_DIM));
                        if ui.small_button("Copy").on_hover_text("Copies the example, to paste into a script").clicked() {
                            ui.ctx().copy_text(g.3.to_string());
                            st.status = format!("copied the {} example", g.0);
                        }
                    });
                    egui::Frame::new().fill(Color32::from_rgb(0x12, 0x12, 0x12)).inner_margin(10.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let job = crate::code::highlight(&g.3.replace('\t', "    "), egui::FontId::monospace(14.0));
                        ui.label(job);
                    });
                    // The others in its group, to wander on to.
                    ui.add_space(12.0);
                    ui.label(RichText::new(format!("MORE IN {}", g.1.to_uppercase())).small().color(crate::theme::TEXT_DIM));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        for o in GUIDE.iter().filter(|o| o.1 == g.1 && o.0 != g.0) {
                            if ui.link(RichText::new(o.0).monospace()).clicked() {
                                pick = o.0.to_string();
                            }
                        }
                    });
                }
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(id, (filter, pick)));
}
