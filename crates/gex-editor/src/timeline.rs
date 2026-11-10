//! The Timeline: making an Animator's clips. Pick a clip, add tracks (a node and one of its
//! properties), move the playhead, and press +key to record the property's current value there.

use eframe::egui::{self, Color32, RichText, Sense};
use gex_core::anim::{self, AnimFile, Clip, Key, Track};
use gex_core::{NodeId, NodeKind, Variant};

use crate::state::EditorState;
use crate::theme;

#[derive(Default)]
pub struct TimelineState {
    pub clip: String,
    pub time: f32,
    pub playing: bool,
    /// (track, key) being dragged or selected.
    pub selected: Option<(usize, usize)>,
    pub new_target: String,
    pub new_prop: String,
}

/// The Animator to edit: the selected node, or its first Animator child.
fn animator(st: &EditorState) -> Option<NodeId> {
    let id = st.selected.filter(|s| st.tree.contains(*s))?;
    let n = &st.tree.nodes[id];
    if n.kind == NodeKind::Animator {
        return Some(id);
    }
    n.children.iter().copied().find(|c| st.tree.nodes[*c].kind == NodeKind::Animator)
}

fn load(st: &EditorState, id: NodeId) -> AnimFile {
    let res = st.tree.nodes[id].text("clips");
    if res.is_empty() {
        return AnimFile::default();
    }
    st.project.read_text(res).ok().and_then(|t| AnimFile::parse(&t).ok()).unwrap_or_default()
}

fn save(st: &mut EditorState, id: NodeId, file: &AnimFile) {
    let mut res = st.tree.nodes[id].text("clips").to_string();
    if res.is_empty() {
        let base: String = st.tree.nodes[id].parent.map(|p| st.tree.nodes[p].name.clone()).unwrap_or_else(|| "anim".into()).chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        res = crate::files::free_name(&st.project, "res://animations", &format!("{}.anim", base)).unwrap_or_else(|| "res://animations/anim.anim".into());
        st.will_change("clips file");
        st.tree.nodes[id].props.insert("clips".into(), Variant::Res(res.clone()));
    }
    if let Some(p) = st.project.res_to_path(&res) {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        if let Err(e) = std::fs::write(&p, file.to_text()) {
            st.status = format!("couldn't save {}: {}", res, e);
        }
    }
}

/// Every property a track could animate on a node.
fn props_of(st: &EditorState, id: NodeId) -> Vec<String> {
    let n = &st.tree.nodes[id];
    let mut v: Vec<String> = if n.kind.is_3d() {
        ["x", "y", "z", "rotation_x", "rotation_y", "rotation_z", "scale_x", "scale_y", "scale_z", "visible"].iter().map(|s| s.to_string()).collect()
    } else {
        ["x", "y", "rotation", "scale_x", "scale_y", "visible"].iter().map(|s| s.to_string()).collect()
    };
    v.extend(n.props.iter().filter(|(_, val)| matches!(val, Variant::Num(_) | Variant::Color(_) | Variant::Bool(_) | Variant::Text(_) | Variant::Res(_))).map(|(k, _)| k.clone()));
    v
}

pub fn timeline(ui: &mut egui::Ui, st: &mut EditorState) {
    if st.playing.is_some() {
        ui.label(RichText::new("Stop the game to edit clips.").color(theme::TEXT_DIM));
        return;
    }
    let Some(anim_id) = animator(st) else {
        ui.label(RichText::new("Select an Animator node (or its parent) to make clips. Add one with + Node → General → Animator, as a child of what it animates.").color(theme::TEXT_DIM));
        return;
    };
    let mut file = load(st, anim_id);
    let mut changed = false;
    let mut tl = std::mem::take(&mut st.timeline);
    if file.clip(&tl.clip).is_none() {
        tl.clip = file.clips.first().map(|c| c.name.clone()).unwrap_or_default();
    }
    // ---- clip bar ----
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("🎞 {}", st.tree.nodes[anim_id].name)).strong());
        egui::ComboBox::from_id_salt("clip").selected_text(if tl.clip.is_empty() { "no clips" } else { tl.clip.as_str() }).show_ui(ui, |ui| {
            for c in &file.clips {
                ui.selectable_value(&mut tl.clip, c.name.clone(), &c.name);
            }
        });
        if ui.button("+ Clip").clicked() {
            let mut k = file.clips.len() + 1;
            while file.clip(&format!("clip{}", k)).is_some() {
                k += 1;
            }
            let name = format!("clip{}", k);
            file.clips.push(Clip { name: name.clone(), length: 1.0, looped: false, tracks: Vec::new() });
            tl.clip = name;
            changed = true;
        }
    });
    let Some(ci) = file.clips.iter().position(|c| c.name == tl.clip) else {
        st.timeline = tl;
        if changed {
            save(st, anim_id, &file);
        }
        return;
    };
    ui.horizontal(|ui| {
        let clip = &mut file.clips[ci];
        let mut name = clip.name.clone();
        ui.label("name");
        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(90.0)).changed() && !name.trim().is_empty() {
            clip.name = name.trim().to_string();
            tl.clip = clip.name.clone();
            changed = true;
        }
        ui.label("length");
        changed |= ui.add(egui::DragValue::new(&mut clip.length).range(0.05..=600.0).speed(0.05).suffix(" s")).changed();
        changed |= ui.checkbox(&mut clip.looped, "Loop").changed();
        let autoplay = st.tree.nodes[anim_id].text("autoplay") == clip.name;
        let mut ap = autoplay;
        if ui.checkbox(&mut ap, "Autoplay").changed() {
            st.will_change("autoplay");
            st.tree.nodes[anim_id].props.insert("autoplay".into(), Variant::Text(if ap { clip.name.clone() } else { String::new() }));
        }
        if ui.small_button(RichText::new("Delete clip").color(theme::BAD)).clicked() {
            file.clips.remove(ci);
            changed = true;
        }
    });
    if ci >= file.clips.len() {
        st.timeline = tl;
        save(st, anim_id, &file);
        return;
    }
    // ---- transport ----
    let length = file.clips[ci].length.max(0.05);
    let mut scrubbed = false;
    ui.horizontal(|ui| {
        if ui.button(if tl.playing { "⏸" } else { "▶" }).on_hover_text("preview the clip in the scene").clicked() {
            tl.playing = !tl.playing;
            if tl.playing {
                st.will_change("timeline preview");
            }
        }
        if ui.button("⏮").clicked() {
            tl.time = 0.0;
            scrubbed = true;
        }
        ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(80.0);
        let r = ui.add(egui::Slider::new(&mut tl.time, 0.0..=length).show_value(true).suffix(" s").step_by(0.01));
        if r.drag_started() {
            st.will_change("timeline scrub");
        }
        scrubbed |= r.changed();
    });
    if tl.playing {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        tl.time += dt;
        if tl.time > length {
            if file.clips[ci].looped {
                tl.time %= length;
            } else {
                tl.time = length;
                tl.playing = false;
            }
        }
        scrubbed = true;
        ui.ctx().request_repaint();
    }
    if scrubbed {
        anim::apply(&mut st.tree, anim_id, &file.clips[ci], tl.time);
    }
    // ---- tracks ----
    let base = st.tree.nodes[anim_id].parent.unwrap_or(anim_id);
    let row_h = 22.0;
    let label_w = 190.0;
    let mut remove_track = None;
    let mut key_track: Option<usize> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 34.0).show(ui, |ui| {
        let tracks_len = file.clips[ci].tracks.len();
        if tracks_len == 0 {
            ui.label(RichText::new("No tracks yet: add one below, then press +key to record values.").color(theme::TEXT_DIM));
        }
        for ti in 0..tracks_len {
            ui.horizontal(|ui| {
                let tr = &file.clips[ci].tracks[ti];
                let who = match anim::target(&st.tree, anim_id, &tr.target) {
                    Some(t) => st.tree.nodes[t].name.clone(),
                    None => format!("{} (missing)", tr.target),
                };
                let label = format!("{} · {}", who, tr.prop);
                ui.add_sized([label_w - 50.0, row_h], egui::Label::new(RichText::new(label).monospace()).truncate());
                if ui.small_button("+ Key").on_hover_text("key the property's current value at the playhead").clicked() {
                    key_track = Some(ti);
                }
                if ui.small_button("✖").on_hover_text("remove this track").clicked() {
                    remove_track = Some(ti);
                }
                // The strip: keys as diamonds; drag to move, right-click to delete.
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(60.0), row_h), Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, Color32::from_gray(24));
                let to_x = |t: f32| rect.left() + (t / length).clamp(0.0, 1.0) * rect.width();
                let to_t = |x: f32| ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * length;
                for k in 0..=10 {
                    let x = rect.left() + rect.width() * k as f32 / 10.0;
                    painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, Color32::from_gray(36)));
                }
                let keys = file.clips[ci].tracks[ti].keys.clone();
                let mut hit = None;
                for (ki, k) in keys.iter().enumerate() {
                    let c = egui::pos2(to_x(k.t), rect.center().y);
                    let sel = tl.selected == Some((ti, ki));
                    let s = 6.0;
                    let pts = vec![c + egui::vec2(0.0, -s), c + egui::vec2(s, 0.0), c + egui::vec2(0.0, s), c + egui::vec2(-s, 0.0)];
                    painter.add(egui::Shape::convex_polygon(pts, if sel { theme::ACCENT } else { Color32::from_rgb(220, 190, 90) }, egui::Stroke::NONE));
                    if let Some(p) = resp.interact_pointer_pos() {
                        if (p.x - c.x).abs() < 7.0 {
                            hit = Some(ki);
                        }
                    }
                }
                painter.vline(to_x(tl.time), rect.y_range(), egui::Stroke::new(1.5, theme::ACCENT));
                if resp.drag_started() || resp.clicked() {
                    tl.selected = hit.map(|k| (ti, k));
                    if hit.is_none() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            st.will_change("timeline scrub");
                            tl.time = to_t(p.x);
                            anim::apply(&mut st.tree, anim_id, &file.clips[ci], tl.time);
                        }
                    }
                }
                if resp.dragged() {
                    if let (Some((sti, ski)), Some(p)) = (tl.selected, resp.interact_pointer_pos()) {
                        if sti == ti {
                            if let Some(k) = file.clips[ci].tracks[ti].keys.get_mut(ski) {
                                k.t = (to_t(p.x) * 100.0).round() / 100.0;
                                changed = true;
                            }
                        }
                    }
                }
                if resp.drag_stopped() {
                    file.clips[ci].tracks[ti].sort();
                    tl.selected = None;
                    changed = true;
                }
                resp.context_menu(|ui| {
                    if let Some(k) = hit.or(tl.selected.filter(|s| s.0 == ti).map(|s| s.1)) {
                        let mut e = file.clips[ci].tracks[ti].keys[k].ease.clone();
                        ui.label(format!("key at {:.2} s", file.clips[ci].tracks[ti].keys[k].t));
                        for name in ["linear", "in", "out", "in_out", "step"] {
                            if ui.radio_value(&mut e, name.to_string(), name).clicked() {
                                file.clips[ci].tracks[ti].keys[k].ease = e.clone();
                                changed = true;
                                ui.close();
                            }
                        }
                        if ui.button(RichText::new("Delete key").color(theme::BAD)).clicked() {
                            file.clips[ci].tracks[ti].keys.remove(k);
                            tl.selected = None;
                            changed = true;
                            ui.close();
                        }
                    } else {
                        ui.label("right-click a key to change or delete it");
                    }
                });
            });
        }
    });
    if let Some(ti) = key_track {
        let tr = &file.clips[ci].tracks[ti];
        if let Some(target) = anim::target(&st.tree, anim_id, &tr.target) {
            if let Some(mut v) = anim::read(&st.tree, target, &tr.prop) {
                if let Variant::Num(n) = &mut v {
                    *n = (*n * 10000.0).round() / 10000.0;
                }
                let t = (tl.time * 100.0).round() / 100.0;
                let tr = &mut file.clips[ci].tracks[ti];
                match tr.keys.iter_mut().find(|k| (k.t - t).abs() < 0.005) {
                    Some(k) => k.value = v,
                    None => tr.keys.push(Key { t, value: v, ease: "linear".into() }),
                }
                tr.sort();
                changed = true;
            }
        }
    }
    if let Some(ti) = remove_track {
        file.clips[ci].tracks.remove(ti);
        changed = true;
    }
    // ---- add a track ----
    ui.horizontal(|ui| {
        ui.label("+ Track");
        let mut all = Vec::new();
        st.tree.walk_from(base, &mut all);
        let path_of = |id: NodeId| -> String {
            if id == base {
                return String::new();
            }
            let full = st.tree.path_of(id);
            let b = st.tree.path_of(base);
            full.strip_prefix(&format!("{}/", b)).unwrap_or(&full).to_string()
        };
        let shown = if tl.new_target.is_empty() { st.tree.nodes[base].name.clone() } else { tl.new_target.clone() };
        egui::ComboBox::from_id_salt("track node").selected_text(shown).width(140.0).show_ui(ui, |ui| {
            for id in &all {
                if *id == anim_id {
                    continue;
                }
                let p = path_of(*id);
                let label = if p.is_empty() { st.tree.nodes[*id].name.clone() } else { p.clone() };
                ui.selectable_value(&mut tl.new_target, p, label);
            }
        });
        let target = anim::target(&st.tree, anim_id, &tl.new_target);
        let props = target.map(|t| props_of(st, t)).unwrap_or_default();
        egui::ComboBox::from_id_salt("track prop").selected_text(if tl.new_prop.is_empty() { "property" } else { tl.new_prop.as_str() }).width(110.0).show_ui(ui, |ui| {
            for p in props {
                ui.selectable_value(&mut tl.new_prop, p.clone(), p);
            }
        });
        if ui.add_enabled(target.is_some() && !tl.new_prop.is_empty(), egui::Button::new("Add")).clicked() {
            file.clips[ci].tracks.push(Track { target: tl.new_target.clone(), prop: tl.new_prop.clone(), keys: Vec::new() });
            changed = true;
        }
    });
    st.timeline = tl;
    if changed {
        save(st, anim_id, &file);
    }
}
