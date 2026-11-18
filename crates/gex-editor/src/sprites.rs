//! The Sprite panel (a window over the view): a Sprite2D's picture cut into frames, its named
//! animations made by clicking frames, and a preview playing the one picked.

use crate::docs::Tip as _;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use gex_core::{ObjectId, ObjectKind, Variant};

use crate::state::EditorState;
use crate::theme;

/// `animations` ("idle=0; walk=1-5; jump=6,7,8") as names with their frames, in order.
pub fn parse(spec: &str) -> Vec<(String, Vec<u32>)> {
    let mut out = Vec::new();
    for part in spec.split([';', '\n']) {
        let Some((name, frames)) = part.split_once('=') else { continue };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let frames = gex_core::tiles::solid_set(frames).unwrap_or_default().into_iter().filter(|f| *f >= 0).map(|f| f as u32).collect();
        out.push((name.to_string(), frames));
    }
    out
}

/// The other way: runs of three or more frames in a row are written as ranges ("1-5").
pub fn write(anims: &[(String, Vec<u32>)]) -> String {
    let mut parts = Vec::new();
    for (name, frames) in anims {
        let mut bits: Vec<String> = Vec::new();
        let mut i = 0;
        while i < frames.len() {
            let mut j = i;
            while j + 1 < frames.len() && frames[j + 1] == frames[j] + 1 {
                j += 1;
            }
            if j - i >= 2 {
                bits.push(format!("{}-{}", frames[i], frames[j]));
            } else {
                bits.extend(frames[i..=j].iter().map(|f| f.to_string()));
            }
            i = j + 1;
        }
        parts.push(format!("{}={}", name, bits.join(",")));
    }
    parts.join("; ")
}

fn sprite(st: &EditorState) -> Option<ObjectId> {
    st.selected.filter(|s| st.tree.get(*s).is_some_and(|n| n.kind == ObjectKind::Sprite2D))
}

pub fn panel(ui: &mut egui::Ui, st: &mut EditorState) {
    let Some(id) = sprite(st) else {
        ui.label(RichText::new("Select a Sprite2D to cut its picture into frames and make its animations.").color(theme::TEXT_DIM));
        return;
    };
    let playing = st.playing.is_some();
    let n = &st.tree.objects[id];
    let texture = n.text("texture").to_string();
    let (mut cols, mut rows) = (n.num("hframes", 1.0).max(1.0) as u32, n.num("vframes", 1.0).max(1.0) as u32);
    let mut fps = n.num("anim_fps", 10.0);
    let mut looping = n.prop("anim_loop").is_none_or(|v| v.as_bool());
    let mut start = n.text("animation").to_string();
    let mut anims = parse(n.text("animations"));
    let current = n.num("frame", 0.0).max(0.0) as u32;
    let name = n.name.clone();

    // Which animation is being made (kept per sprite).
    let pick_id = egui::Id::new(("sprite panel pick", id));
    let mut pick: Option<String> = ui.data(|d| d.get_temp(pick_id)).filter(|p: &String| anims.iter().any(|a| a.0 == *p));
    if pick.is_none() {
        pick = anims.first().map(|a| a.0.clone());
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new(&name).strong());
        ui.label(RichText::new(if texture.is_empty() { "no picture".to_string() } else { texture.trim_start_matches("res://").to_string() }).small().color(theme::TEXT_DIM));
    });
    if texture.is_empty() {
        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::hover());
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, theme::GRAY), egui::StrokeKind::Inside);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Drop a picture from Assets here", egui::FontId::proportional(14.0), theme::TEXT_DIM);
        if let Some(p) = resp.dnd_release_payload::<crate::panels::AssetPath>() {
            if !playing {
                st.will_change("sprite picture");
                st.tree.objects[id].props.insert("texture".into(), Variant::Res(p.0.clone()));
            }
        }
        return;
    }
    let tex = st
        .ui_images
        .entry(texture.clone())
        .or_insert_with(|| {
            let img = gex_assets::load_image(&st.vfs, &texture).unwrap_or(gex_assets::Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] });
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
            ui.ctx().load_texture(texture.clone(), ci, egui::TextureOptions::NEAREST)
        })
        .clone();
    let size = tex.size_vec2();

    // Cutting the picture up.
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Columns");
        changed |= ui.add(egui::DragValue::new(&mut cols).range(1..=256)).tip("How many frames across the picture (hframes)").changed();
        ui.label("Rows");
        changed |= ui.add(egui::DragValue::new(&mut rows).range(1..=256)).tip("How many frames down the picture (vframes)").changed();
        if ui.button("Square frames").tip("Cuts the picture into square frames as tall (or as wide) as it is").clicked() {
            let (w, h) = (size.x as u32, size.y as u32);
            (cols, rows) = if w >= h { ((w / h.max(1)).max(1), 1) } else { (1, (h / w.max(1)).max(1)) };
            changed = true;
        }
        ui.label(RichText::new(format!("{}×{} px each", (size.x / cols as f32).floor(), (size.y / rows as f32).floor())).small().color(theme::TEXT_DIM));
    });
    let count = cols * rows;
    let cell = Vec2::new(size.x / cols as f32, size.y / rows as f32);
    let uv_of = |f: u32| {
        let (c, r) = ((f % cols) as f32, (f / cols) as f32);
        Rect::from_min_size(Pos2::new(c / cols as f32, r / rows as f32), Vec2::new(1.0 / cols as f32, 1.0 / rows as f32))
    };

    let mut new_frame = None;
    ui.columns(2, |c| {
        // Left: the sheet. Click a frame to add it to the animation being made (or, with none,
        // to show it); right-click to take it out.
        let ui = &mut c[0];
        let fit = (ui.available_width() / size.x).min(260.0 / size.y).min(6.0).max(0.1);
        let scale = if fit >= 1.0 { fit.floor() } else { fit };
        let (rect, resp) = ui.allocate_exact_size(size * scale, Sense::click());
        let p = ui.painter_at(rect.expand(1.0));
        checker(&p, rect);
        p.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        let frames_of_pick: Vec<u32> = pick.as_ref().and_then(|n| anims.iter().find(|a| a.0 == *n)).map(|a| a.1.clone()).unwrap_or_default();
        for f in 0..count {
            let r = Rect::from_min_size(rect.min + Vec2::new((f % cols) as f32, (f / cols) as f32) * cell * scale, cell * scale);
            let used: Vec<usize> = frames_of_pick.iter().enumerate().filter(|(_, x)| **x == f).map(|(i, _)| i + 1).collect();
            if !used.is_empty() {
                p.rect_filled(r, 0.0, theme::alpha(theme::ACCENT, 50));
                p.rect_stroke(r, 0.0, Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
                let order: Vec<String> = used.iter().map(|u| u.to_string()).collect();
                p.text(r.right_top() + Vec2::new(-3.0, 2.0), egui::Align2::RIGHT_TOP, order.join(","), egui::FontId::proportional(11.0), Color32::WHITE);
            } else {
                p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_black_alpha(140)), egui::StrokeKind::Inside);
            }
            if f == current && pick.is_none() {
                p.rect_stroke(r, 0.0, Stroke::new(2.0, theme::GOOD), egui::StrokeKind::Inside);
            }
            p.text(r.left_top() + Vec2::new(3.0, 2.0), egui::Align2::LEFT_TOP, f.to_string(), egui::FontId::proportional(10.0), Color32::from_white_alpha(170));
        }
        let at = |pos: Pos2| {
            let l = (pos - rect.min) / scale;
            let (c, r) = ((l.x / cell.x).floor() as i64, (l.y / cell.y).floor() as i64);
            (c >= 0 && r >= 0 && (c as u32) < cols && (r as u32) < rows).then(|| r as u32 * cols + c as u32)
        };
        if let Some(f) = resp.interact_pointer_pos().filter(|_| resp.clicked()).and_then(at) {
            match pick.as_ref().and_then(|n| anims.iter_mut().find(|a| a.0 == *n)) {
                Some(a) => {
                    a.1.push(f);
                    changed = true;
                }
                None => new_frame = Some(f),
            }
        }
        if let Some(f) = resp.interact_pointer_pos().filter(|_| resp.secondary_clicked()).and_then(at) {
            if let Some(a) = pick.as_ref().and_then(|n| anims.iter_mut().find(|a| a.0 == *n)) {
                if let Some(i) = a.1.iter().rposition(|x| *x == f) {
                    a.1.remove(i);
                    changed = true;
                }
            }
        }
        resp.on_hover_text("Click a frame to add it to the animation (again to add it twice). Right-click takes it out.");

        // Right: the animations, and the preview.
        let ui = &mut c[1];
        ui.horizontal(|ui| {
            theme::title(ui, "Animations");
            if ui.small_button("+ Animation").tip("A new animation; click frames on the left to fill it").clicked() {
                let name = (1..).map(|i| if i == 1 { "anim".to_string() } else { format!("anim{}", i) }).find(|n| !anims.iter().any(|a| a.0 == *n)).unwrap();
                anims.push((name.clone(), Vec::new()));
                pick = Some(name);
                changed = true;
            }
        });
        let mut remove = None;
        let mut rename = None;
        let mut clear = None;
        for (i, (name, frames)) in anims.iter().enumerate() {
            ui.horizontal(|ui| {
                let is_start = start == *name;
                if ui.add(egui::RadioButton::new(is_start, "")).tip("Plays when the game starts (animation)").clicked() {
                    start = if is_start { String::new() } else { name.clone() };
                    changed = true;
                }
                let mut text = name.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
                if r.gained_focus() || r.clicked() {
                    pick = Some(name.clone());
                }
                if r.changed() {
                    let t: String = text.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !t.is_empty() && !anims.iter().any(|a| a.0 == t) {
                        rename = Some((i, t));
                    }
                }
                let sel = pick.as_deref() == Some(name.as_str());
                if ui.selectable_label(sel, RichText::new(write(&[(String::new(), frames.clone())]).trim_start_matches('=').to_string()).monospace().small()).tip("Pick it to add frames to it").clicked() {
                    pick = Some(name.clone());
                }
                if ui.small_button("✖").tip("Delete this animation").clicked() {
                    remove = Some(i);
                }
                if sel && !frames.is_empty() && ui.small_button("Clear").tip("Takes all its frames out").clicked() {
                    clear = Some(i);
                }
            });
        }
        if let Some(i) = clear {
            anims[i].1.clear();
            changed = true;
        }
        if let Some((i, t)) = rename {
            if start == anims[i].0 {
                start = t.clone();
            }
            if pick.as_deref() == Some(anims[i].0.as_str()) {
                pick = Some(t.clone());
            }
            anims[i].0 = t;
            changed = true;
        }
        if let Some(i) = remove {
            let gone = anims.remove(i);
            if start == gone.0 {
                start.clear();
            }
            pick = anims.first().map(|a| a.0.clone());
            changed = true;
        }
        if anims.is_empty() {
            ui.label(RichText::new("None yet: + Animation, then click frames on the left.").small().color(theme::TEXT_DIM));
        }
        ui.horizontal(|ui| {
            ui.label("Speed");
            changed |= ui.add(egui::DragValue::new(&mut fps).range(0.1..=120.0).speed(0.2).suffix(" fps")).tip("Frames per second (anim_fps)").changed();
            changed |= ui.checkbox(&mut looping, "Loop").tip("Starts over at the end (anim_loop); off: stops on the last frame and runs on_animation_end").changed();
        });
        // The preview: the picked animation playing.
        let frames: Vec<u32> = pick.as_ref().and_then(|n| anims.iter().find(|a| a.0 == *n)).map(|a| a.1.clone()).filter(|f| !f.is_empty()).unwrap_or_else(|| vec![current.min(count.saturating_sub(1))]);
        let t = ui.input(|i| i.time);
        let step = (t * fps.max(0.1)) as usize;
        let k = if looping { step % frames.len() } else { step.min(frames.len() - 1) };
        let f = frames[k].min(count.saturating_sub(1));
        let room = ui.available_size().min(Vec2::new(ui.available_width(), 160.0));
        let s = (room.x / cell.x).min(room.y / cell.y).max(0.1);
        let s = if s >= 1.0 { s.floor() } else { s };
        let (r, _) = ui.allocate_exact_size(cell * s, Sense::hover());
        checker(ui.painter(), r);
        ui.painter().image(tex.id(), r, uv_of(f), Color32::WHITE);
        ui.label(RichText::new(format!("frame {}", f)).small().color(theme::TEXT_DIM));
        if frames.len() > 1 {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(1.0 / fps.max(0.1)));
        }
    });

    ui.data_mut(|d| match &pick {
        Some(p) => {
            d.insert_temp(pick_id, p.clone());
        }
        None => d.remove::<String>(pick_id),
    });
    if playing {
        return;
    }
    if let Some(f) = new_frame {
        st.will_change("sprite frame");
        st.tree.objects[id].props.insert("frame".into(), Variant::Num(f as f64));
    }
    if changed {
        st.will_change("sprite sheet");
        let p = &mut st.tree.objects[id].props;
        p.insert("hframes".into(), Variant::Num(cols as f64));
        p.insert("vframes".into(), Variant::Num(rows as f64));
        p.insert("anim_fps".into(), Variant::Num(fps));
        p.insert("anim_loop".into(), Variant::Bool(looping));
        p.insert("animations".into(), Variant::Text(write(&anims)));
        p.insert("animation".into(), Variant::Text(start));
    }
}

/// A gray checkerboard behind see-through pixels.
fn checker(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, 0.0, Color32::from_gray(0x2a));
    let k = 8.0;
    let mut y = r.top();
    let mut row = 0;
    while y < r.bottom() {
        let mut x = r.left() + if row % 2 == 0 { 0.0 } else { k };
        while x < r.right() {
            p.rect_filled(Rect::from_min_max(Pos2::new(x, y), Pos2::new((x + k).min(r.right()), (y + k).min(r.bottom()))), 0.0, Color32::from_gray(0x36));
            x += 2.0 * k;
        }
        y += k;
        row += 1;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn animations_read_and_write_back() {
        let a = super::parse("idle=0; walk=1-5; jump = 6,7,8,10; empty=");
        assert_eq!(a, vec![("idle".into(), vec![0]), ("walk".into(), vec![1, 2, 3, 4, 5]), ("jump".into(), vec![6, 7, 8, 10]), ("empty".into(), vec![])]);
        assert_eq!(super::write(&a), "idle=0; walk=1-5; jump=6-8,10; empty=");
        assert_eq!(super::write(&[("x".into(), vec![3, 2, 1, 1, 2])]), "x=3,2,1,1,2");
        assert_eq!(super::parse(&super::write(&a)), a);
        // The engine reads what's written the same way.
        assert_eq!(gex_core::tiles::animation_frames(&super::write(&a), "jump"), Some(vec![6, 7, 8, 10]));
    }
}
