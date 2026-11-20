//! Hierarchy, inspector, events, assets and console panels.

use crate::docs::Tip as _;
use eframe::egui::{self, Color32, RichText};
use gex_core::object::ALL_KINDS;
use gex_core::{ObjectId, ObjectKind, Variant};
use gex_script::LogKind;

use crate::state::EditorState;
use crate::theme;

pub fn add_object_menu(ui: &mut egui::Ui, st: &mut EditorState) {
    let groups: [(&str, fn(ObjectKind) -> bool); 4] = [
        ("General", |k| !k.is_2d() && !k.is_3d() && !k.is_ui()),
        ("2D", |k| k.is_2d()),
        ("UI (screen space)", |k| k.is_ui()),
        ("3D", |k| k.is_3d()),
    ];
    // A 2D blueprint offers no 3D objects, and the other way round.
    let root = st.tree.root.map(|r| st.tree.objects[r].kind);
    let skip = match root {
        Some(k) if k.is_2d() => "3D",
        Some(k) if k.is_3d() => "2D",
        _ => "",
    };
    theme::fade_in(ui);
    // One column per kind of object.
    ui.horizontal_top(|ui| {
        for (i, (title, f)) in groups.iter().filter(|g| g.0 != skip).enumerate() {
            if i > 0 {
                ui.separator();
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(*title).color(theme::TEXT_DIM));
                for k in ALL_KINDS.iter().filter(|k| f(**k)) {
                    if object_button(ui, st, *k).clicked() {
                        st.add_object(*k);
                        ui.close();
                    }
                }
            });
        }
    });
}

/// A menu row for a kind of object: its icon and name.
fn object_button(ui: &mut egui::Ui, st: &EditorState, k: ObjectKind) -> egui::Response {
    let r = crate::icons::row(ui, st, &crate::icons::Icon::Silk(crate::icons::kind(k).into()), RichText::new(k.name()), false);
    crate::docs::on(r, ui, format!("object:{}", k.name()))
}

/// The $variables a script declares (from the open editor, or the file read twice a second).
fn script_vars_of(ui: &egui::Ui, st: &EditorState, res: &str) -> Vec<String> {
    if let Some(s) = st.scripts.get(res) {
        return gex_core::object::script_vars(&s.text);
    }
    let id = egui::Id::new(("script vars", res));
    let now = ui.input(|i| i.time);
    if let Some((t, v)) = ui.data(|d| d.get_temp::<(f64, Vec<String>)>(id)) {
        if now - t < 0.5 {
            return v;
        }
    }
    let v = st.project.read_text(res).map(|t| gex_core::object::script_vars(&t)).unwrap_or_default();
    ui.data_mut(|d| d.insert_temp(id, (now, v.clone())));
    v
}

/// Picking a tile to paint with, from the TileMap2D's tileset image.
fn tile_palette(ui: &mut egui::Ui, st: &mut EditorState, id: ObjectId) {
    let n = &st.tree.objects[id];
    let path = n.text("tileset").to_string();
    let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
    ui.horizontal(|ui| {
        let painting = st.tile_brush.is_some();
        if ui.selectable_label(painting, "Paint tiles").tip("LMB paints in the blueprint view, Shift+LMB erases").clicked() {
            st.tile_brush = if painting { None } else { Some(0) };
        }
        if ui.selectable_label(st.tile_brush == Some(-1), "Eraser").clicked() {
            st.tile_brush = Some(-1);
        }
    });
    if path.is_empty() {
        ui.label(RichText::new("Set a tileset image to pick tiles from.").color(theme::TEXT_DIM));
        return;
    }
    let tex = st.ui_images.entry(path.clone()).or_insert_with(|| {
        let img = gex_assets::load_image(&st.vfs, &path).unwrap_or(gex_assets::Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] });
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
        ui.ctx().load_texture(path.clone(), ci, egui::TextureOptions::NEAREST)
    });
    let size = tex.size_vec2();
    let cols = (size.x / tw).floor().max(1.0) as i32;
    let rows = (size.y / th).floor().max(1.0) as i32;
    let scale = (ui.available_width() / size.x).min(3.0).max(0.25);
    let (rect, resp) = ui.allocate_exact_size(size * scale, egui::Sense::click());
    ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
    for c in 0..=cols {
        let x = rect.min.x + c as f32 * tw * scale;
        ui.painter().line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + rows as f32 * th * scale)], egui::Stroke::new(1.0, Color32::from_black_alpha(120)));
    }
    for r in 0..=rows {
        let y = rect.min.y + r as f32 * th * scale;
        ui.painter().line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.min.x + cols as f32 * tw * scale, y)], egui::Stroke::new(1.0, Color32::from_black_alpha(120)));
    }
    if let Some(b) = st.tile_brush.filter(|b| *b >= 0) {
        let (c, r) = ((b % cols) as f32, (b / cols) as f32);
        let r0 = egui::Rect::from_min_size(rect.min + egui::vec2(c * tw * scale, r * th * scale), egui::vec2(tw * scale, th * scale));
        ui.painter().rect_stroke(r0, 0.0, egui::Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Inside);
    }
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
        let local = (p - rect.min) / scale;
        let (c, r) = ((local.x / tw).floor() as i32, (local.y / th).floor() as i32);
        if c < cols && r < rows {
            st.tile_brush = Some(r * cols + c);
        }
    }
    ui.label(RichText::new(format!("tile {}", st.tile_brush.unwrap_or(-1))).small().color(theme::TEXT_DIM));
}

// ---------------------------------------------------------------- hierarchy

pub fn hierarchy(ui: &mut egui::Ui, st: &mut EditorState) {
    crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:instances");
    let playing = st.playing.is_some();
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!playing, |ui| {
            crate::docs::menu(ui, "+ Object", |ui| add_object_menu(ui, st));
            crate::docs::menu(ui, "+ Instance", |ui| { crate::theme::fade_in(ui);
                let blueprints = list_files(st, "res://", "jblueprint");
                if blueprints.is_empty() {
                    ui.label("no .jblueprint files yet");
                }
                for s in blueprints {
                    if ui.button(&s).clicked() {
                        st.instance_blueprint(&s);
                        ui.close();
                    }
                }
            });
        });
        if playing {
            ui.label(RichText::new("live").color(theme::GOOD));
        }
    });
    ui.separator();
    let tree = st.view_tree().clone();
    let Some(root) = tree.root else {
        ui.label("empty blueprint");
        return;
    };
    let mut action: Option<Action> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        object_row(ui, st, &tree, root, &mut action, playing);
    });
    match action {
        Some(Action::Select(id)) => st.select(Some(id)),
        Some(Action::Toggle(id)) => st.toggle_select(id),
        Some(Action::Reparent(id, to)) if !playing => {
            if st.tree.objects[id].from_instance {
                st.status = "objects inside an instanced blueprint can't be moved out of it".into();
            } else {
                st.will_change("move");
                // Keep where it is in the world.
                let world = st.tree.global_matrix(id);
                if st.tree.reparent(id, to, None) {
                    let local = st.tree.global_matrix(to).inverse() * world;
                    let (s2, r, t) = local.to_scale_rotation_translation();
                    let (ry, rx, rz) = r.to_euler(gex_core::glam::EulerRot::YXZ);
                    let tr = &mut st.tree.objects[id].transform;
                    tr.position = t;
                    tr.scale = s2;
                    tr.rotation = gex_core::glam::Vec3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees());
                }
            }
        }
        Some(Action::Delete(id)) if !playing => {
            if !st.is_selected(id) {
                st.select(Some(id));
            }
            st.delete_selected();
        }
        Some(Action::SaveBranch(id)) if !playing => {
            st.save_branch_as_blueprint(id);
        }
        Some(Action::MakeLocal(id)) if !playing => st.make_local(id),
        Some(Action::OpenBlueprint(res)) if !playing => st.open_blueprint(&res),
        Some(Action::Duplicate(id)) if !playing => {
            if !st.is_selected(id) {
                st.select(Some(id));
            }
            st.duplicate_selected();
        }
        Some(Action::Color(id, c)) if !playing => {
            st.will_change("color");
            match c {
                Some(c) => st.tree.objects[id].props.insert(COLOR_PROP.into(), Variant::Text(c)),
                None => st.tree.objects[id].props.remove(COLOR_PROP),
            };
        }
        Some(Action::Move(id, up)) if !playing => {
            if let Some(p) = st.tree.objects[id].parent {
                let kids = &st.tree.objects[p].children;
                let i = kids.iter().position(|c| *c == id).unwrap();
                let j = if up { i.checked_sub(1) } else { (i + 1 < kids.len()).then_some(i + 1) };
                if let Some(j) = j {
                    st.will_change("order");
                    st.tree.objects[p].children.swap(i, j);
                }
            }
        }
        Some(Action::AddChild(id, k)) if !playing => {
            st.add_child(Some(id), k);
        }
        _ => {}
    }
}

/// The property an object's highlight color in the Blueprint panel is kept in (only for looks).
pub const COLOR_PROP: &str = "editor_color";

enum Action {
    Select(ObjectId),
    Toggle(ObjectId),
    Reparent(ObjectId, ObjectId),
    Delete(ObjectId),
    Duplicate(ObjectId),
    Move(ObjectId, bool),
    AddChild(ObjectId, ObjectKind),
    SaveBranch(ObjectId),
    MakeLocal(ObjectId),
    OpenBlueprint(String),
    Color(ObjectId, Option<String>),
}

fn object_row(ui: &mut egui::Ui, st: &EditorState, tree: &gex_core::BlueprintTree, id: ObjectId, action: &mut Option<Action>, playing: bool) {
    let n = &tree.objects[id];
    let selected = st.is_selected(id);
    let label = n.name.clone();
    let icon = crate::icons::object(tree, id);
    let color = if n.from_instance { theme::TEXT_DIM } else if !n.visible { Color32::from_gray(110) } else { theme::TEXT };
    let draw_label = |ui: &mut egui::Ui, action: &mut Option<Action>| {
        let tint = std::iter::successors(Some(id), |i| tree.objects[*i].parent).find_map(|i| theme::hex(tree.objects[i].text(COLOR_PROP)));
        let r = theme::tinted(ui, tint, |ui| crate::icons::row(ui, st, &icon, RichText::new(&label).color(color), selected));
        crate::docs::mark(ui, r.rect, format!("object:{}", n.kind.name()));
        // Small marks after the name: it has scripts, it's an instance of a blueprint.
        for (on, mark, tip) in [(!n.scripts.is_empty(), "script", "Has a script"), (n.instance_of.is_some() && !n.from_instance, "link", "An instance of a blueprint")] {
            if on {
                let (mr, m) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
                crate::icons::paint(ui, st, &crate::icons::Icon::Silk(mark.into()), egui::Rect::from_center_size(mr.center(), egui::vec2(12.0, 12.0)));
                m.tip(tip);
            }
        }
        if r.clicked() {
            *action = Some(if ui.input(|i| i.modifiers.command) { Action::Toggle(id) } else { Action::Select(id) });
        }
        if !playing {
            r.dnd_set_drag_payload(id);
            if let Some(from) = r.dnd_release_payload::<ObjectId>() {
                if *from != id {
                    *action = Some(Action::Reparent(*from, id));
                }
            }
            if r.dnd_hover_payload::<ObjectId>().is_some() {
                ui.painter().rect_stroke(r.rect, 0.0, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
            }
        }
        let tip = match &n.instance_of {
            Some(p) => format!("{} — instance of {}", n.kind.name(), p),
            None => n.kind.name().to_string(),
        };
        let r = r.tip(tip);
        r.context_menu(|ui| { crate::theme::fade_in(ui);
            crate::docs::menu(ui, "Add child", |ui| { crate::theme::fade_in(ui);
                for k in ALL_KINDS {
                    if object_button(ui, st, *k).clicked() {
                        *action = Some(Action::AddChild(id, *k));
                        ui.close();
                    }
                }
            });
            if theme::menu_item(ui, "Duplicate", "Ctrl+D").tip("Makes a copy of the selected objects next to them.").clicked() {
                *action = Some(Action::Duplicate(id));
                ui.close();
            }
            ui.separator();
            if !n.from_instance && n.instance_of.is_none() && ui.button("Save branch as blueprint").tip("make this object and its children a reusable .jblueprint").clicked() {
                *action = Some(Action::SaveBranch(id));
                ui.close();
            }
            if let Some(p) = &n.instance_of {
                if ui.button("Open its blueprint").tip("Opens the blueprint file this instance comes from.").clicked() {
                    *action = Some(Action::OpenBlueprint(p.clone()));
                    ui.close();
                }
                if !n.from_instance && ui.button("Make local").tip("stop following the blueprint file: its objects become this blueprint's own").clicked() {
                    *action = Some(Action::MakeLocal(id));
                    ui.close();
                }
            }
            if !n.from_instance {
                if let Some(c) = theme::color_menu(ui) {
                    *action = Some(Action::Color(id, c));
                }
            }
            ui.separator();
            if ui.button("Move up").tip("Moves it earlier among its siblings (drawn earlier, so behind).").clicked() {
                *action = Some(Action::Move(id, true));
                ui.close();
            }
            if ui.button("Move down").tip("Moves it later among its siblings (drawn later, so in front).").clicked() {
                *action = Some(Action::Move(id, false));
                ui.close();
            }
            ui.separator();
            if ui.button(RichText::new("Delete  (Del)").color(theme::BAD)).clicked() {
                *action = Some(Action::Delete(id));
                ui.close();
            }
        });
    };
    if n.children.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            draw_label(ui, action);
        });
    } else {
        let cid = ui.make_persistent_id(("tree", id));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), cid, true)
            .show_header(ui, |ui| draw_label(ui, action))
            .body(|ui| {
                for c in &n.children {
                    object_row(ui, st, tree, *c, action, playing);
                }
            });
    }
}

// ---------------------------------------------------------------- inspector

fn transform_ui(ui: &mut egui::Ui, st: &mut EditorState, id: ObjectId) {
    let kind = st.tree.objects[id].kind;
    let mut t = st.tree.objects[id].transform;
    let before = t;
    let drag = |ui: &mut egui::Ui, v: &mut f32, speed: f64, label: &str| {
        ui.add(egui::DragValue::new(v).speed(speed).prefix(label).max_decimals(3));
    };
    egui::Grid::new("xform").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let flat = kind.is_2d();
        // Menus and buttons are placed by x and y only (their size is their width and height).
        if kind.is_ui() {
            ui.label("Position");
            ui.horizontal(|ui| {
                drag(ui, &mut t.position.x, 1.0, "x ");
                drag(ui, &mut t.position.y, 1.0, "y ");
            });
            ui.end_row();
            return;
        }
        let speed = if flat { 1.0 } else { 0.05 };
        ui.label("Position");
        ui.horizontal(|ui| {
            drag(ui, &mut t.position.x, speed, "x ");
            drag(ui, &mut t.position.y, speed, "y ");
            // In 2D, z is the drawing order: higher draws on top.
            drag(ui, &mut t.position.z, speed, "z ");
        });
        ui.end_row();
        ui.label("Rotation");
        ui.horizontal(|ui| {
            if flat {
                drag(ui, &mut t.rotation.z, 1.0, "° ");
            } else {
                drag(ui, &mut t.rotation.x, 1.0, "x ");
                drag(ui, &mut t.rotation.y, 1.0, "y ");
                drag(ui, &mut t.rotation.z, 1.0, "z ");
            }
        });
        ui.end_row();
        ui.label("Scale");
        ui.horizontal(|ui| {
            drag(ui, &mut t.scale.x, 0.01, "x ");
            drag(ui, &mut t.scale.y, 0.01, "y ");
            if !flat {
                drag(ui, &mut t.scale.z, 0.01, "z ");
            }
        });
        ui.end_row();
    });
    if t != before {
        st.will_change(&format!("xform {:?}", id));
        st.tree.objects[id].transform = t;
    }
}

/// Edits a property value; true when it changed.
/// A built-in property's editor: a list for choices, a slider for numbers with a range,
/// otherwise the plain editor for its type.
fn prop_ui(ui: &mut egui::Ui, v: &mut Variant, key: &str, info: &gex_core::props::PropInfo) -> bool {
    // A mesh is a built-in shape or a model file: a list of shapes, and a field that takes a
    // typed path or a model dropped from the assets (yours or a mounted game's).
    if let (true, Variant::Text(cur)) = (key == "mesh", &mut *v) {
        let mut changed = false;
        // Anything that isn't a shape's name is a model path (maybe still being typed).
        let model = !info.choices.contains(&cur.as_str());
        ui.horizontal(|ui| {
            let combo = egui::ComboBox::from_id_salt(("choice", key)).width(70.0).selected_text(if model { "model" } else { cur.as_str() }).show_ui(ui, |ui| {
                for c in info.choices {
                    if ui.selectable_label(cur == c, *c).clicked() && cur != c {
                        *cur = c.to_string();
                        changed = true;
                    }
                }
            });
            let mut path = if model { cur.clone() } else { String::new() };
            let field = ui.add(egui::TextEdit::singleline(&mut path).hint_text("or drop a model here").desired_width(f32::INFINITY));
            if field.changed() {
                *cur = if path.is_empty() { "cube".into() } else { path };
                changed = true;
            }
            for r in [&combo.response, &field] {
                if r.dnd_hover_payload::<AssetPath>().is_some_and(|p| gex_assets::model::is_model(&p.0)) {
                    ui.painter().rect_stroke(r.rect, 0.0, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
                }
                if let Some(p) = r.dnd_release_payload::<AssetPath>().filter(|p| gex_assets::model::is_model(&p.0)) {
                    *cur = p.0.clone();
                    changed = true;
                }
            }
        });
        return changed;
    }
    if !info.choices.is_empty() {
        if let Variant::Text(cur) = v {
            let mut changed = false;
            egui::ComboBox::from_id_salt(("choice", key)).selected_text(cur.as_str()).show_ui(ui, |ui| {
                for c in info.choices {
                    if ui.selectable_label(cur == c, *c).clicked() && cur != c {
                        *cur = c.to_string();
                        changed = true;
                    }
                }
            });
            return changed;
        }
    }
    if let (Some((lo, hi)), Variant::Num(n)) = (info.range, &mut *v) {
        return ui.add(egui::Slider::new(n, lo..=hi).clamping(egui::SliderClamping::Never).max_decimals(3)).changed();
    }
    variant_ui(ui, v, key)
}

fn variant_ui(ui: &mut egui::Ui, v: &mut Variant, key: &str) -> bool {
    match v {
        Variant::Null => {
            ui.label(RichText::new("NULL").color(theme::TEXT_DIM));
            false
        }
        Variant::Bool(b) => ui.checkbox(b, "").changed(),
        Variant::Num(n) => ui.add(egui::DragValue::new(n).speed(if key == "zoom" || key == "energy" { 0.01 } else { 0.5 }).max_decimals(3)).changed(),
        Variant::Text(s) if key == "tiles" || key == "animations" || key == "text" && s.contains('\n') => {
            ui.add(egui::TextEdit::multiline(s).code_editor().desired_rows(3).desired_width(f32::INFINITY)).changed()
        }
        Variant::Text(s) => ui.add(egui::TextEdit::singleline(s).desired_width(f32::INFINITY)).changed(),
        Variant::Res(s) => {
            let r = ui.add(egui::TextEdit::singleline(s).hint_text("drop an asset here").desired_width(f32::INFINITY));
            let mut changed = r.changed();
            if let Some(p) = r.dnd_release_payload::<AssetPath>() {
                *s = p.0.clone();
                changed = true;
            }
            changed
        }
        Variant::Color(c) => {
            let mut c32 = Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (c[3] * 255.0) as u8);
            let r = egui::color_picker::color_edit_button_srgba(ui, &mut c32, egui::color_picker::Alpha::OnlyBlend);
            if r.changed() {
                let [rr, g, b, a] = c32.to_srgba_unmultiplied();
                *c = [rr as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0];
            }
            r.changed()
        }
        Variant::Vec2(p) => {
            let mut ch = false;
            ui.horizontal(|ui| {
                ch |= ui.add(egui::DragValue::new(&mut p[0]).prefix("x ")).changed();
                ch |= ui.add(egui::DragValue::new(&mut p[1]).prefix("y ")).changed();
            });
            ch
        }
        Variant::Vec3(p) => {
            let mut ch = false;
            ui.horizontal(|ui| {
                for (k, l) in ["x ", "y ", "z "].iter().enumerate() {
                    ch |= ui.add(egui::DragValue::new(&mut p[k]).prefix(*l)).changed();
                }
            });
            ch
        }
        Variant::List(l) => {
            ui.label(format!("list of {}", l.len()));
            false
        }
    }
}

/// A value as plain text (for read-only views).
fn show_variant(v: &Variant) -> String {
    let n = |x: f64| format!("{}", (x * 1000.0).round() / 1000.0);
    match v {
        Variant::Null => "NULL".into(),
        Variant::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Variant::Num(x) => n(*x),
        Variant::Text(s) | Variant::Res(s) => s.clone(),
        Variant::Color(c) => format!("#{:02x}{:02x}{:02x}{}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, if c[3] < 0.999 { format!(" {}%", (c[3] * 100.0).round()) } else { String::new() }),
        Variant::Vec2(p) => format!("{}, {}", n(p[0]), n(p[1])),
        Variant::Vec3(p) => format!("{}, {}, {}", n(p[0]), n(p[1]), n(p[2])),
        Variant::List(l) => format!("list of {}", l.len()),
    }
}

/// An asset dragged from the asset browser.
#[derive(Clone)]
pub struct AssetPath(pub String);

pub fn inspector(ui: &mut egui::Ui, st: &mut EditorState) {
    crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:properties");
    let Some(id) = st.selected else {
        ui.label("Select an object to see its properties.");
        return;
    };
    if let Some(p) = &st.playing {
        // Live, read-only view of the running game.
        let Some(n) = p.tree.get(id) else {
            ui.label("This object isn't in the running game (it was destroyed).");
            return;
        };
        ui.label(RichText::new(&n.name).strong());
        ui.label(RichText::new("Live values from the running game").small().color(theme::GOOD));
        ui.separator();
        let t = n.transform;
        egui::Grid::new("live").num_columns(2).show(ui, |ui| {
            ui.label("position");
            ui.label(format!("{:.2}, {:.2}, {:.2}", t.position.x, t.position.y, t.position.z));
            ui.end_row();
            ui.label("rotation");
            ui.label(format!("{:.1}, {:.1}, {:.1}", t.rotation.x, t.rotation.y, t.rotation.z));
            ui.end_row();
            for (k, v) in &n.props {
                ui.label(k);
                ui.label(show_variant(v));
                ui.end_row();
            }
        });
        return;
    }
    if !st.tree.contains(id) {
        st.selected = st.tree.root;
        return;
    }
    let kind = st.tree.objects[id].kind;
    ui.horizontal(|ui| {
        // The object's icon; click to pick another.
        let icon = crate::icons::object(&st.tree, id);
        let (rect, r) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
        if r.hovered() {
            ui.painter().rect_filled(rect, 0.0, theme::GRAY_LIGHT);
        }
        crate::icons::paint(ui, st, &icon, rect.shrink(3.0));
        let r = crate::docs::on(r.tip("Icon: click to change"), ui, "ui:icon");
        let current = st.tree.objects[id].text(crate::icons::PROP).to_string();
        egui::Popup::menu(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            theme::fade_in(ui);
            if let Some(pick) = crate::icons::picker(ui, st, &current) {
                st.will_change("icon");
                if pick.is_empty() {
                    st.tree.objects[id].props.remove(crate::icons::PROP);
                } else {
                    st.tree.objects[id].props.insert(crate::icons::PROP.into(), Variant::Text(pick));
                }
                ui.close();
            }
        });
        let mut name = st.tree.objects[id].name.clone();
        let renamable = !st.tree.objects[id].from_instance;
        if ui.add_enabled(renamable, egui::TextEdit::singleline(&mut name).desired_width(160.0)).named("Name", "The object's name: what shows in Instances and what scripts find it by, as in OBJECT(\"../Player\"). Names inside an instance belong to its blueprint.").changed() && !name.is_empty() && !name.contains('/') {
            st.will_change(&format!("name {:?}", id));
            st.tree.objects[id].name = name;
        }
        let mut vis = st.tree.objects[id].visible;
        if ui.checkbox(&mut vis, "Visible").tip("Whether it's drawn. Hiding an object hides everything inside it too. Scripts: self.visible.").changed() {
            st.will_change("visible");
            st.tree.objects[id].visible = vis;
        }
    });
    ui.label(RichText::new(kind.name()).color(theme::TEXT_DIM));
    if let Some(p) = st.tree.objects[id].instance_of.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Instance of {}", p.trim_start_matches("res://"))).color(theme::ACCENT));
            if ui.small_button("Open").clicked() {
                st.open_blueprint(&p);
            }
        });
    }
    if st.tree.objects[id].from_instance {
        ui.label(RichText::new("Inside an instanced blueprint: changes here are saved as overrides for this instance only.").small().color(theme::WARN));
    }
    let n_sel = st.selected_roots().len();
    if n_sel > 1 {
        ui.label(RichText::new(format!("{} objects selected (editing this one; drag in the view moves them all)", n_sel)).small().color(theme::ACCENT));
    }
    ui.separator();
    if kind == ObjectKind::Sprite2D && ui.button("Sprite sheet & animations…").tip("Cut the picture into frames and make named animations by clicking them, with a preview").clicked() {
        st.open_sprite = true;
    }
    if kind == ObjectKind::Animator && ui.button("Edit clips in the Timeline").tip("Opens the Timeline for this Animator: clips that change properties over time.").clicked() {
        st.open_timeline = true;
    }
    // An instance is always placed by its own transform, whatever its blueprint's root is.
    if (kind != ObjectKind::Object && kind != ObjectKind::Animator) || st.tree.objects[id].instance_of.is_some() {
        let h = egui::CollapsingHeader::new("Transform").default_open(true).show(ui, |ui| transform_ui(ui, st, id));
        crate::docs::mark(ui, h.header_response.rect.union(h.body_response.map_or(h.header_response.rect, |b| b.rect)), "ui:transform");
    }
    // Built-in properties in their sections (Main open, the rest folded), then custom ones.
    let defaults: Vec<&str> = kind.default_props().iter().map(|p| p.0).collect();
    let keys: Vec<String> = st.tree.objects[id].props.keys().cloned().collect();
    let order: Vec<String> = defaults.iter().map(|d| d.to_string()).filter(|d| keys.contains(d)).collect();
    for group in gex_core::props::groups(kind) {
        let in_group: Vec<&String> = order.iter().filter(|k| gex_core::props::info(kind, k).group == group).collect();
        // Faces of their own are a cube's.
        if in_group.is_empty() || (group == "Advanced texture settings" && st.tree.objects[id].text("mesh") != "cube") {
            continue;
        }
        let title = if group == "Main" { "Properties".to_string() } else { group.to_string() };
        egui::CollapsingHeader::new(title).id_salt(("props", group)).default_open(group == "Main").show(ui, |ui| {
            egui::Grid::new(("props grid", group)).num_columns(2).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
                for k in in_group {
                    let info = gex_core::props::info(kind, k);
                    let label = ui.label(k.as_str());
                    let row = label.rect;
                    if !info.doc.is_empty() {
                        label.tip(info.doc);
                    }
                    let mut v = st.tree.objects[id].props[k].clone();
                    if prop_ui(ui, &mut v, k, &info) {
                        st.will_change(&format!("prop {:?} {}", id, k));
                        st.tree.objects[id].props.insert(k.clone(), v);
                    }
                    crate::docs::mark(ui, row.union(ui.min_rect().intersect(egui::Rect::everything_below(row.top())).intersect(egui::Rect::everything_above(row.bottom() + 4.0))), format!("prop:{}.{}", kind.name(), k));
                    ui.end_row();
                }
            });
        });
    }
    // $variables its scripts declare: set here, for this instance.
    let mut vars: Vec<String> = Vec::new();
    for res in st.tree.objects[id].scripts.clone() {
        for v in script_vars_of(ui, st, &res) {
            if !vars.contains(&v) && !defaults.contains(&v.as_str()) {
                vars.push(v);
            }
        }
    }
    if !vars.is_empty() {
        let h = egui::CollapsingHeader::new("Script variables").default_open(true).show(ui, |ui| {
            egui::Grid::new("script vars").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                for k in &vars {
                    ui.label(RichText::new(format!("${}", k)).color(theme::C_EVENT)).tip("Declared in this object's script; the value here is this instance's");
                    let mut v = st.tree.objects[id].props.get(k).cloned().unwrap_or(Variant::Num(0.0));
                    let mut changed = variant_ui(ui, &mut v, k);
                    let kind = match &v {
                        Variant::Num(_) => "Number",
                        Variant::Bool(_) => "Switch",
                        Variant::Color(_) => "Color",
                        _ => "Text",
                    };
                    egui::ComboBox::from_id_salt(("var type", k)).width(64.0).selected_text(kind).show_ui(ui, |ui| {
                        for (label, nv) in [("Number", Variant::Num(0.0)), ("Text", Variant::Text(String::new())), ("Switch", Variant::Bool(false)), ("Color", Variant::Color([1.0; 4]))] {
                            if ui.selectable_label(kind == label, label).clicked() && kind != label {
                                v = nv;
                                changed = true;
                            }
                        }
                    });
                    if changed {
                        st.will_change(&format!("prop {:?} {}", id, k));
                        st.tree.objects[id].props.insert(k.clone(), v);
                    }
                    ui.end_row();
                }
            });
        });
        crate::docs::mark(ui, h.header_response.rect.union(h.body_response.map_or(h.header_response.rect, |b| b.rect)), "ui:script_vars");
    }
    let custom = egui::CollapsingHeader::new("Custom properties").default_open(true).show(ui, |ui| {
        let keys: Vec<String> = st.tree.objects[id].props.keys().filter(|k| !defaults.contains(&k.as_str()) && k.as_str() != crate::icons::PROP && k.as_str() != COLOR_PROP && !vars.contains(k)).cloned().collect();
        let mut remove = None;
        egui::Grid::new("custom props").num_columns(3).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
            for k in keys {
                ui.label(RichText::new(&k).color(theme::C_EVENT));
                let mut v = st.tree.objects[id].props[&k].clone();
                if variant_ui(ui, &mut v, &k) {
                    st.will_change(&format!("prop {:?} {}", id, k));
                    st.tree.objects[id].props.insert(k.clone(), v);
                }
                if ui.small_button("✖").tip("remove this property").clicked() {
                    remove = Some(k.clone());
                }
                ui.end_row();
            }
        });
        if let Some(k) = remove {
            st.will_change("remove prop");
            st.tree.objects[id].props.remove(&k);
        }
        // Custom properties: what scripts see as self.<name>.
        let mem_id = egui::Id::new("new prop name");
        let mut name: String = ui.data_mut(|d| d.get_temp(mem_id).unwrap_or_default());
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("new_property").desired_width(120.0));
            let ok = jlb::value::is_ident(&name) && !st.tree.objects[id].props.contains_key(&name);
            for (label, v) in [("Number", Variant::Num(0.0)), ("Text", Variant::Text(String::new())), ("Switch", Variant::Bool(false)), ("Color", Variant::Color([1.0; 4]))] {
                if ui.add_enabled(ok, egui::Button::new(format!("+ {}", label))).tip("Name it first, then pick its type").clicked() {
                    st.will_change("add prop");
                    st.tree.objects[id].props.insert(name.clone(), v);
                    name.clear();
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(mem_id, name));
    });
    crate::docs::mark(ui, custom.header_response.rect.union(custom.body_response.map_or(custom.header_response.rect, |b| b.rect)), "ui:custom_props");
    if kind == ObjectKind::TileMap2D {
        egui::CollapsingHeader::new("Tiles").default_open(true).show(ui, |ui| tile_palette(ui, st, id));
    }
    egui::CollapsingHeader::new("Groups").default_open(false).show(ui, |ui| {
        crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:groups");
        let mut groups = st.tree.objects[id].groups.join(", ");
        if ui.add(egui::TextEdit::singleline(&mut groups).hint_text("enemies, pickups").desired_width(f32::INFINITY)).changed() {
            st.will_change("groups");
            st.tree.objects[id].groups = groups.split(',').map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect();
        }
    });
}

// ---------------------------------------------------------------- events

/// The events a script can handle: (function signature, when it runs).
pub const EVENTS: &[(&str, &str)] = &[
    ("on_create()", "once the blueprint is loaded (or after SPAWN)"),
    ("on_step(dt)", "every frame"),
    ("on_physics(dt)", "60 times a second, before physics moves things"),
    ("on_draw()", "every frame, after on_step: the place for DRAW_* commands"),
    ("on_destroy()", "before the object is removed"),
    ("on_key_pressed(key)", "any key goes down"),
    ("on_key_released(key)", "any key goes up"),
    ("on_press_space()", "one key goes down (rename: on_press_a, on_press_left, on_press_pad_a...)"),
    ("on_release_space()", "one key goes up"),
    ("on_mouse_pressed(button, x, y)", "a mouse click"),
    ("on_mouse_moved(x, y)", "the mouse moved"),
    ("on_mouse_wheel(amount)", "the wheel turned"),
    ("on_collide_start(other)", "this object's collider or body starts touching another"),
    ("on_collide(other)", "every physics step while touching"),
    ("on_collide_end(other)", "they stop touching"),
    ("on_timer(name)", "a TIMER(name, seconds) on this object finishes"),
    ("on_timer_spawn()", "the TIMER named spawn finishes (rename for yours)"),
    ("on_pressed()", "this Button was clicked"),
    ("on_pressed_play()", "any Button named Play was clicked (runs on every script)"),
    ("on_text_changed(text)", "this TextInput's text changed"),
    ("on_text_submitted(text)", "Enter was pressed in this TextInput"),
    ("on_animation_end(name)", "a non-looping animation on this object finished"),
    ("on_tween_end(property)", "a TWEEN of one of this object's properties finished"),
    ("when_ready()", "a condition: return TRUE to run on_ready()"),
    ("on_ready()", "runs when when_ready() turns TRUE"),
];

/// An event function's name as people say it: on_press_space → "Key pressed: space".
fn event_label(h: &str) -> String {
    let fixed = [
        ("on_create", "Create"),
        ("on_destroy", "Destroy"),
        ("on_step", "Step"),
        ("on_physics", "Physics step"),
        ("on_draw", "Draw"),
        ("on_key_pressed", "Any key pressed"),
        ("on_key_released", "Any key released"),
        ("on_mouse_pressed", "Mouse pressed"),
        ("on_mouse_moved", "Mouse moved"),
        ("on_mouse_wheel", "Mouse wheel"),
        ("on_collide_start", "Collision start"),
        ("on_collide", "While colliding"),
        ("on_collide_end", "Collision end"),
        ("on_timer", "Any timer"),
        ("on_pressed", "Clicked"),
        ("on_text_changed", "Text changed"),
        ("on_text_submitted", "Text submitted"),
        ("on_animation_end", "Animation end"),
        ("on_tween_end", "Tween end"),
        ("on_ready", "When ready"),
        ("when_ready", "Ready condition"),
    ];
    if let Some((_, l)) = fixed.iter().find(|(n, _)| *n == h) {
        return l.to_string();
    }
    for (prefix, what) in [("on_press_", "Key pressed"), ("on_release_", "Key released"), ("on_timer_", "Timer"), ("on_pressed_", "Clicked")] {
        if let Some(rest) = h.strip_prefix(prefix) {
            return format!("{}: {}", what, rest);
        }
    }
    format!("Custom: {}", h.trim_start_matches("on_"))
}

/// Names sent with EMIT / BROADCAST / SEND anywhere in the project's scripts.
fn broadcast_names(st: &EditorState) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in list_files(st, "res://", "jlb") {
        let text = st.script_text(&f);
        for call in ["EMIT(\"", "BROADCAST(\""] {
            for part in text.split(call).skip(1) {
                if let Some(name) = part.split('"').next().filter(|n| jlb::value::is_ident(n)) {
                    if !out.iter().any(|o| o == name) {
                        out.push(name.to_string());
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// The "Add event" menu. Gives back the function to add, like `on_press_space()`.
fn add_event_menu(ui: &mut egui::Ui, st: &EditorState) -> Option<String> {
    let pick = std::cell::RefCell::new(None::<String>);
    let item = |ui: &mut egui::Ui, label: &str, sig: &str| {
        let what = EVENTS.iter().find(|e| e.0 == sig).map(|e| e.1).unwrap_or("");
        let mut r = ui.button(label);
        if !what.is_empty() {
            r = r.tip(what);
        }
        if r.clicked() {
            *pick.borrow_mut() = Some(sig.to_string());
        }
    };
    item(ui, "Create", "on_create()");
    item(ui, "Destroy", "on_destroy()");
    item(ui, "Step", "on_step(dt)");
    item(ui, "Physics step", "on_physics(dt)");
    item(ui, "Draw", "on_draw()");
    ui.separator();
    for (title, any, prefix) in [("Key pressed", "on_key_pressed(key)", "on_press_"), ("Key released", "on_key_released(key)", "on_release_")] {
        crate::docs::menu(ui, title, |ui| { crate::theme::fade_in(ui);
            item(ui, "Any key", any);
            ui.separator();
            for k in ["left", "right", "up", "down", "space", "enter", "escape", "shift", "w", "a", "s", "d"] {
                item(ui, k, &format!("{}{}()", prefix, k));
            }
            ui.separator();
            let mem = egui::Id::new(("other key", prefix));
            let mut key: String = ui.data_mut(|d| d.get_temp(mem).unwrap_or_default());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut key).hint_text("other key or action").desired_width(120.0));
                if ui.add_enabled(jlb::value::is_ident(&key), egui::Button::new("Add")).tip("A key (f, tab, pad_a...) or an input action from Project settings").clicked() {
                    *pick.borrow_mut() = Some(format!("{}{}()", prefix, key.to_lowercase()));
                    key.clear();
                }
            });
            ui.data_mut(|d| d.insert_temp(mem, key));
        });
    }
    crate::docs::menu(ui, "Mouse", |ui| { crate::theme::fade_in(ui);
        item(ui, "Pressed", "on_mouse_pressed(button, x, y)");
        item(ui, "Moved", "on_mouse_moved(x, y)");
        item(ui, "Wheel", "on_mouse_wheel(amount)");
    });
    crate::docs::menu(ui, "Collision", |ui| { crate::theme::fade_in(ui);
        item(ui, "Start", "on_collide_start(other)");
        item(ui, "While colliding", "on_collide(other)");
        item(ui, "End", "on_collide_end(other)");
    });
    crate::docs::menu(ui, "Other", |ui| { crate::theme::fade_in(ui);
        item(ui, "Timer", "on_timer(name)");
        item(ui, "Animation end", "on_animation_end(name)");
        item(ui, "Tween end", "on_tween_end(property)");
        item(ui, "Clicked (a Button)", "on_pressed()");
        item(ui, "Text changed (a TextInput)", "on_text_changed(text)");
        item(ui, "Text submitted (a TextInput)", "on_text_submitted(text)");
    });
    ui.separator();
    crate::docs::menu(ui, "Custom", |ui| { crate::theme::fade_in(ui);
        ui.label(RichText::new("Runs when any script calls BROADCAST(\"name\"),\nor SEND(object, \"name\") for one object.").small().color(theme::TEXT_DIM));
        let known = broadcast_names(st);
        if !known.is_empty() {
            ui.separator();
        }
        for n in known {
            if ui.button(&n).tip("Already broadcast somewhere in this project").clicked() {
                *pick.borrow_mut() = Some(format!("on_{}()", n));
            }
        }
        ui.separator();
        let mem = egui::Id::new("custom event");
        let mut custom: String = ui.data_mut(|d| d.get_temp(mem).unwrap_or_default());
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut custom).hint_text("new name").desired_width(120.0));
            if ui.add_enabled(jlb::value::is_ident(&custom), egui::Button::new("Add")).clicked() {
                *pick.borrow_mut() = Some(format!("on_{}()", custom));
                custom.clear();
            }
        });
        ui.data_mut(|d| d.insert_temp(mem, custom));
    });
    if pick.borrow().is_some() {
        ui.close();
    }
    pick.into_inner()
}

/// The selected object's events (the on_... functions of its scripts) and the scripts themselves.
pub fn events(ui: &mut egui::Ui, st: &mut EditorState) {
    crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:events");
    let Some(id) = st.selected.filter(|i| st.tree.contains(*i)) else {
        ui.label(RichText::new("Select an object to see its events.").color(theme::TEXT_DIM));
        return;
    };
    let playing = st.playing.is_some();
    let scripts = st.tree.objects[id].scripts.clone();
    ui.horizontal(|ui| {
        theme::title(ui, &format!("Events of {}", st.tree.objects[id].name));
        if !playing {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut add = None;
                crate::docs::menu(ui, "Add event", |ui| add = add_event_menu(ui, st));
                if let Some(sig) = add {
                    st.add_event_script(id, &sig);
                }
            });
        }
    });
    let mut any = false;
    for res in &scripts {
        let text = st.script_text(res);
        // In the order they're written.
        let found = gex_script::world::scan_handlers_text(&text);
        let mut handlers: Vec<(usize, String)> = found.into_iter().filter_map(|h| text.lines().position(|l| l.trim_start().strip_prefix("DEF ").is_some_and(|r| r.trim_start().starts_with(&format!("{}(", h)))).map(|l| (l + 1, h))).collect();
        handlers.sort();
        for (line, h) in handlers {
            any = true;
            let r = ui.add(egui::Button::selectable(false, event_label(&h))).tip(format!("{} in {}", h, res.trim_start_matches("res://")));
            if crate::docs::on(r, ui, format!("event:{}", h)).clicked() {
                st.open_script(res, Some(line));
            }
        }
    }
    if !any {
        ui.label(RichText::new("None yet.").color(theme::TEXT_DIM));
    }
    ui.add_space(6.0);
    ui.separator();
    theme::title(ui, "Scripts");
    let mut detach = None;
    for (i, res) in scripts.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.link(res.trim_start_matches("res://")).clicked() {
                st.open_script(res, None);
            }
            if !playing && ui.small_button("Detach").tip("Remove from this object (the file stays)").clicked() {
                detach = Some(i);
            }
        });
    }
    if let Some(i) = detach {
        st.will_change("detach");
        st.tree.objects[id].scripts.remove(i);
    }
    if playing {
        return;
    }
    ui.horizontal(|ui| {
        if ui.small_button("New script").tip("Makes a new script file and opens it.").clicked() {
            st.new_script_for(id);
        }
        crate::docs::menu(ui, "Attach", |ui| { crate::theme::fade_in(ui);
            let all: Vec<String> = list_files(st, "res://", "jlb").into_iter().filter(|s| !scripts.contains(s)).collect();
            if all.is_empty() {
                ui.label(RichText::new("No other scripts in the project.").color(theme::TEXT_DIM));
            }
            for s in all {
                if ui.button(s.trim_start_matches("res://")).clicked() {
                    st.will_change("attach");
                    st.tree.objects[id].scripts.push(s);
                    ui.close();
                }
            }
        });
    });
}

/// Every file with this extension under a folder (res:// paths).
pub fn list_files(st: &EditorState, dir: &str, ext: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_string()];
    while let Some(d) = stack.pop() {
        for e in st.vfs.list(&d) {
            if e.is_dir {
                stack.push(e.path);
            } else if e.name.to_ascii_lowercase().ends_with(&format!(".{}", ext)) {
                out.push(e.path);
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------- assets

/// Right-click menu for a file or folder in the Assets panel.
fn asset_context_menu(r: &egui::Response, st: &mut EditorState, e: &gex_assets::vfs::Entry, in_project: bool) {
    if st.playing.is_some() {
        return;
    }
    if in_project {
        r.context_menu(|ui| { crate::theme::fade_in(ui);
            if e.name.ends_with(".jblueprint") {
                if ui.button("Open").tip("Opens the blueprint in a tab of its own").clicked() {
                    st.open_blueprint(&e.path);
                    ui.close();
                }
                if ui.button("Open in its own tab").tip("The blueprint view, to place and arrange things in it").clicked() {
                    st.open_blueprint(&e.path);
                    ui.close();
                }
                if st.blueprint_res.as_deref() != Some(e.path.as_str()) && ui.button("Place in the current blueprint").tip("Adds an instance of this blueprint to the one being edited.").clicked() {
                    st.instance_blueprint(&e.path);
                    ui.close();
                }
                ui.separator();
            }
            if e.is_dir {
                new_asset_menu(ui, st, &e.path);
                ui.separator();
            }
            if let Some(c) = theme::color_menu(ui) {
                st.set_asset_color(&e.path, c);
            }
            if ui.button("Rename…").tip("Gives it a new name. Blueprints and scripts that use it are updated.").clicked() {
                set_dialog(ui.ctx(), AssetDialog::Rename(e.path.clone(), e.name.clone()));
                ui.close();
            }
            if ui.button("Show in file manager").tip("Opens the folder it's in.").clicked() {
                if let Some(p) = st.project.res_to_path(&e.path) {
                    crate::files::reveal(&p);
                }
                ui.close();
            }
            ui.separator();
            if ui.button(RichText::new("Delete").color(theme::BAD)).tip("moves it to the project's hidden .trash folder").clicked() {
                set_dialog(ui.ctx(), AssetDialog::Delete(e.path.clone()));
                ui.close();
            }
        });
    } else if e.path.starts_with("gex://") && !e.is_dir {
        r.context_menu(|ui| { crate::theme::fade_in(ui);
            if ui.button("Copy into project").tip("makes a copy in res://assets/placeholders you can change").clicked() {
                st.copy_builtin(&e.path);
                ui.close();
            }
        });
    }
}

fn is_sound(n: &str) -> bool {
    gex_assets::formats::is_audio(n)
}

/// Everything in the project as (path, is a folder), without the project file and hidden
/// ones, looked up twice a second.
fn project_files(ui: &egui::Ui, st: &EditorState) -> std::sync::Arc<Vec<(String, bool)>> {
    // The flag: the last look found the same files as the one before. Until it has, the
    // window redraws when the list goes stale, so a new file shows up without a mouse move.
    type Cache = (f64, std::path::PathBuf, std::sync::Arc<Vec<(String, bool)>>, bool);
    let id = egui::Id::new("project files");
    let now = ui.input(|i| i.time);
    let old = ui.data(|d| d.get_temp::<Cache>(id)).filter(|c| c.1 == st.project.dir);
    if let Some((t, _, files, settled)) = &old {
        if now - t < 0.5 {
            if !settled {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(0.5 - (now - t) + 0.02));
            }
            return files.clone();
        }
    }
    let mut out = Vec::new();
    let mut todo = vec!["res://".to_string()];
    while let Some(dir) = todo.pop() {
        for e in st.vfs.list(&dir) {
            if e.name.starts_with('.') || e.name == gex_core::project::PROJECT_FILE || out.len() >= 20000 {
                continue;
            }
            if e.is_dir {
                todo.push(e.path.clone());
            }
            out.push((e.path, e.is_dir));
        }
    }
    // A script that belongs to just one object isn't listed: it's opened from that object
    // (Events). Counted over every blueprint file, with the open one as it is right now.
    {
        type Uses = (u64, std::sync::Arc<Vec<String>>);
        let uid = egui::Id::new("blueprint texts");
        let blueprints: Vec<&String> = out.iter().filter(|(p, d)| !d && p.ends_with(".jblueprint") && Some(p) != st.blueprint_res.as_ref()).map(|(p, _)| p).collect();
        let stamp = blueprints.iter().fold(blueprints.len() as u64, |h, p| {
            let m = st.project.res_to_path(p).and_then(|f| std::fs::metadata(f).ok());
            let t = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
            h.wrapping_mul(31).wrapping_add(t ^ m.map_or(0, |m| m.len()))
        });
        let texts = match ui.data(|d| d.get_temp::<Uses>(uid)).filter(|u| u.0 == stamp) {
            Some(u) => u.1,
            None => {
                let t = std::sync::Arc::new(blueprints.iter().filter_map(|p| st.project.read_text(p).ok()).collect::<Vec<_>>());
                ui.data_mut(|d| d.insert_temp::<Uses>(uid, (stamp, t.clone())));
                t
            }
        };
        let here = |script: &str| st.tree.objects.iter().filter(|(_, n)| !n.from_instance && n.scripts.iter().any(|x| x == script)).count();
        out.retain(|(p, d)| {
            if *d || !p.ends_with(".jlb") {
                return true;
            }
            let quoted = format!("\"{}\"", p);
            texts.iter().map(|t| t.matches(quoted.as_str()).count()).sum::<usize>() + here(p) != 1
        });
    }
    out.sort_by_key(|p| p.0.to_lowercase());
    let files = std::sync::Arc::new(out);
    let settled = old.is_some_and(|o| *o.2 == *files);
    if !settled {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(520));
    }
    ui.data_mut(|d| d.insert_temp::<Cache>(id, (now, st.project.dir.clone(), files.clone(), settled)));
    files
}

/// "New folder / blueprint / script" entries that make the thing in `dir` and ask for its name.
fn new_asset_menu(ui: &mut egui::Ui, st: &mut EditorState, dir: &str) {
    let mut made = None;
    if ui.button("New folder").tip("Makes a folder here and asks for its name.").clicked() {
        made = st.new_folder(dir);
    }
    crate::docs::menu(ui, "New blueprint", |ui| { crate::theme::fade_in(ui);
        for (label, k) in [("2D", ObjectKind::Object2D), ("3D", ObjectKind::Object3D), ("UI / menu", ObjectKind::Object)] {
            if ui.button(label).clicked() {
                made = st.new_blueprint_file(dir, k);
            }
        }
    });
    if ui.button("New script").tip("Makes a new script file and opens it.").clicked() {
        made = st.new_script_file(dir);
    }
    if let Some(r) = made {
        let name = r.rsplit('/').next().unwrap_or("").to_string();
        set_dialog(ui.ctx(), AssetDialog::Rename(r, name));
        ui.close();
    }
}

/// The folder a path is in.
fn parent_dir(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) if path[..i].ends_with(':') || path[..i].ends_with(":/") => &path[..i + 1],
        Some(i) => &path[..i],
        None => path,
    }
}

/// One file in the asset tree: its icon (pictures and blueprints show themselves), click to
/// select, drag to use or to move into a folder, double-click to open, right-click for more.
fn asset_row(ui: &mut egui::Ui, st: &mut EditorState, path: &str) {
    let name = path.rsplit('/').next().unwrap_or(path);
    let sel = st.asset_selected.as_deref() == Some(path);
    let icon = if gex_assets::image_load::is_image(name) {
        crate::icons::Icon::Image(path.to_string())
    } else if name.ends_with(".jblueprint") && path.starts_with("res://") {
        crate::icons::blueprint(st, path)
    } else {
        crate::icons::Icon::Silk(crate::icons::file(name).into())
    };
    let label = name.rsplit_once('.').map_or(name, |x| x.0);
    let tint = st.asset_color(path).and_then(theme::hex);
    let r = theme::tinted(ui, tint, |ui| crate::icons::row(ui, st, &icon, RichText::new(label), sel)).tip(name);
    asset_interact(ui, st, &r, path);
}

/// What every file in the tree does: select, drag, right-click menu, double-click to open.
fn asset_interact(ui: &egui::Ui, st: &mut EditorState, r: &egui::Response, path: &str) {
    let name = path.rsplit('/').next().unwrap_or(path);
    crate::docs::mark(ui, r.rect, format!("file:{}", path));
    r.dnd_set_drag_payload(AssetPath(path.to_string()));
    let in_project = path.starts_with("res://");
    if r.clicked() {
        st.asset_selected = Some(path.to_string());
        if in_project {
            st.asset_dir = parent_dir(path).to_string();
        }
    }
    asset_context_menu(r, st, &gex_assets::vfs::Entry { name: name.to_string(), path: path.to_string(), is_dir: false }, in_project);
    if r.double_clicked() && in_project && st.playing.is_none() {
        if name.ends_with(".jblueprint") {
            st.open_blueprint(path);
        } else if [".jlb", ".txt", ".vmt", ".wgsl", ".json", ".md"].iter().any(|x| name.ends_with(x)) {
            st.open_script(path, None);
        }
    }
}

/// The pictures of a folder, as a grid of thumbnails (their names show when pointed at).
fn asset_tiles(ui: &mut egui::Ui, st: &mut EditorState, paths: &[&str]) {
    if paths.is_empty() {
        return;
    }
    const SIZE: f32 = 44.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for path in paths {
            let name = path.rsplit('/').next().unwrap_or(path);
            let (rect, r) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::click_and_drag());
            if ui.is_rect_visible(rect) {
                let sel = st.asset_selected.as_deref() == Some(*path);
                ui.painter().rect_filled(rect, 0.0, if sel { theme::GRAY_LIGHT } else if r.hovered() { theme::GRAY } else { Color32::from_rgb(0x16, 0x16, 0x16) });
                if let Some(c) = st.asset_color(path).and_then(theme::hex) {
                    ui.painter().rect_filled(rect, 0.0, c.gamma_multiply(0.20));
                }
                crate::icons::paint(ui, st, &crate::icons::Icon::Image(path.to_string()), rect.shrink(3.0));
                if sel {
                    ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
                }
            }
            let r = r.tip(name);
            asset_interact(ui, st, &r, path);
        }
    });
    ui.add_space(2.0);
}

/// A folder in the asset tree and, when open, what's in it. Project folders take drops
/// (things dragged onto them move in) and can be dragged themselves.
fn asset_dir_row(ui: &mut egui::Ui, st: &mut EditorState, path: &str, name: &str, files: Option<&[(String, bool)]>) {
    let id = ui.make_persistent_id(("asset folder", path));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let in_project = path.starts_with("res://");
    let open = state.is_open();
    let tint = st.asset_color(path).and_then(theme::hex);
    let slot = ui.painter().add(egui::Shape::Noop);
    let r = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            state.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
            crate::icons::row(ui, st, &crate::icons::Icon::Silk(if open { "folder" } else { "folder" }.into()), RichText::new(name), in_project && st.asset_dir == path && st.asset_selected.is_none())
        })
        .inner;
    if let Some(c) = tint {
        let band = egui::Rect::from_min_max(egui::pos2(ui.max_rect().left(), r.rect.top()), egui::pos2(ui.clip_rect().right(), r.rect.bottom()));
        ui.painter().set(slot, egui::Shape::rect_filled(band, 0.0, c.gamma_multiply(0.20)));
    }
    crate::docs::mark(ui, r.rect, "ui:folder");
    if r.clicked() {
        state.toggle(ui);
        if in_project {
            st.asset_dir = path.to_string();
            st.asset_selected = None;
        }
    }
    if in_project {
        r.dnd_set_drag_payload(AssetPath(path.to_string()));
        take_asset_drop(ui, st, &r, path);
        asset_context_menu(&r, st, &gex_assets::vfs::Entry { name: name.to_string(), path: path.to_string(), is_dir: true }, true);
    }
    state.show_body_indented(&r, ui, |ui| match files {
        Some(f) => asset_level(ui, st, f, path),
        None => asset_folder(ui, st, path),
    });
}

/// Something dragged from the tree and let go on a folder moves into it.
fn take_asset_drop(ui: &egui::Ui, st: &mut EditorState, r: &egui::Response, dir: &str) {
    let ok = |p: &AssetPath| p.0.starts_with("res://") && p.0 != dir && parent_dir(&p.0) != dir && !dir.starts_with(&format!("{}/", p.0));
    if r.dnd_hover_payload::<AssetPath>().is_some_and(|p| ok(&p)) {
        ui.painter().rect_stroke(r.rect, 0.0, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
    }
    if let Some(p) = r.dnd_release_payload::<AssetPath>().filter(|p| ok(p)) {
        match st.move_asset(&p.0, dir) {
            Ok(new) => {
                st.asset_selected = Some(new);
                // Show where it went.
                let id = ui.make_persistent_id(("asset folder", dir));
                let mut s = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                s.set_open(true);
                s.store(ui.ctx());
            }
            Err(e) => st.status = e,
        }
    }
}

/// What's directly in a project folder: folders first, then files.
fn asset_level(ui: &mut egui::Ui, st: &mut EditorState, files: &[(String, bool)], dir: &str) {
    let here: Vec<&(String, bool)> = files.iter().filter(|(p, _)| parent_dir(p) == dir).collect();
    for (p, _) in here.iter().filter(|e| e.1) {
        asset_dir_row(ui, st, p, p.rsplit('/').next().unwrap_or(p), Some(files));
    }
    for (p, _) in here.iter().filter(|e| !e.1 && !gex_assets::image_load::is_image(&e.0)) {
        asset_row(ui, st, p);
    }
    let pictures: Vec<&str> = here.iter().filter(|e| !e.1 && gex_assets::image_load::is_image(&e.0)).map(|e| e.0.as_str()).collect();
    asset_tiles(ui, st, &pictures);
}

fn thumb<'a>(ui: &egui::Ui, st: &EditorState, tex: &'a mut std::collections::HashMap<String, egui::TextureHandle>, path: &str) -> &'a egui::TextureHandle {
    tex.entry(path.to_string()).or_insert_with(|| {
        let img = gex_assets::load_image(&st.vfs, path).unwrap_or(gex_assets::Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] });
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
        ui.ctx().load_texture(path.to_string(), ci, egui::TextureOptions::NEAREST)
    })
}

/// A folder outside the project (built-in art, a mounted game), listed as it's opened.
fn asset_folder(ui: &mut egui::Ui, st: &mut EditorState, dir: &str) {
    let entries = st.vfs.list(dir);
    for e in entries.iter().filter(|e| e.is_dir).take(2000) {
        asset_dir_row(ui, st, &e.path, &e.name, None);
    }
    for e in entries.iter().filter(|e| !e.is_dir && !gex_assets::image_load::is_image(&e.name)).take(2000) {
        asset_row(ui, st, &e.path);
    }
    let pictures: Vec<&str> = entries.iter().filter(|e| !e.is_dir && gex_assets::image_load::is_image(&e.name)).take(2000).map(|e| e.path.as_str()).collect();
    asset_tiles(ui, st, &pictures);
}

/// The asset tree: the project's folders and files as they are on disk (drag things onto
/// folders to move them), then the built-in art and mounted games; a preview of the selected
/// file underneath.
pub fn assets(ui: &mut egui::Ui, st: &mut EditorState, ctx_textures: &mut std::collections::HashMap<String, egui::TextureHandle>) {
    crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:assets");
    let filter_id = egui::Id::new("assets filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id).unwrap_or_default());
    if !st.asset_dir.starts_with("res://") || (st.asset_dir != "res://" && !st.project.res_to_path(&st.asset_dir).is_some_and(|p| p.is_dir())) {
        st.asset_dir = "res://".into();
    }
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search").desired_width(f32::INFINITY));
    if st.playing.is_none() {
        ui.horizontal(|ui| {
            let dir = st.asset_dir.clone();
            let place = if dir == "res://" { "the project's top folder".to_string() } else { dir.trim_start_matches("res://").to_string() };
            let mut made = None;
            crate::docs::menu(ui, "+Blueprint", |ui| { crate::theme::fade_in(ui);
                for (label, k) in [("2D", ObjectKind::Object2D), ("3D", ObjectKind::Object3D), ("UI / menu", ObjectKind::Object)] {
                    if ui.button(label).clicked() {
                        made = st.new_blueprint_file(&dir, k);
                        ui.close();
                    }
                }
            })
            .response
            .tip(format!("Makes a new blueprint in {}.", place));
            if ui.button("+Script").tip(format!("Makes a new script in {} and opens it.", place)).clicked() {
                made = st.new_script_file(&dir);
            }
            if ui.button("+Folder").tip(format!("Makes a new folder in {}.", place)).clicked() {
                made = st.new_folder(&dir);
            }
            if let Some(r) = made {
                let name = r.rsplit('/').next().unwrap_or("").to_string();
                set_dialog(ui.ctx(), AssetDialog::Rename(r, name));
            }
            crate::docs::menu(ui, "+", |ui| { crate::theme::fade_in(ui);
                #[cfg(windows)]
                if ui.button("Import files…").tip("Copies files from your computer into the selected folder.").clicked() {
                    if let Some(files) = rfd::FileDialog::new().pick_files() {
                        st.import_files(&files);
                    }
                    ui.close();
                }
                if ui.button("Open project folder").tip("In the file manager. Files dropped on the GEX window are imported too").clicked() {
                    crate::files::reveal(&st.project.dir);
                    ui.close();
                }
                if let Some(last) = st.file_undo.last() {
                    ui.separator();
                    if ui.button(format!("Undo: {}", last.describe())).clicked() {
                        st.undo_file_op();
                        ui.close();
                    }
                }
            })
            .response
            .tip("Import files, open the project folder, undo the last file change");
        });
    }
    ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    asset_dialogs(ui, st);
    let _ = &ctx_textures;
    let files = project_files(ui, st);
    let needle = filter.trim().to_lowercase();
    let preview = st.asset_selected.clone().filter(|s| gex_assets::image_load::is_image(s) || gex_assets::model::is_model(s) || is_sound(s));
    let preview_h = if preview.is_some() { 190.0 } else { 0.0 };
    egui::ScrollArea::vertical().id_salt("asset tree").max_height((ui.available_height() - preview_h).max(60.0)).auto_shrink([false, false]).show(ui, |ui| {
        if !needle.is_empty() {
            // Searching: the matching files, flat.
            let found: Vec<&(String, bool)> = files.iter().filter(|(p, d)| !d && p.rsplit('/').next().unwrap_or(p).to_lowercase().contains(&needle)).collect();
            if found.is_empty() {
                ui.label(RichText::new("Nothing matches.").color(theme::TEXT_DIM));
            }
            for (p, _) in found {
                asset_row(ui, st, p);
            }
            return;
        }
        if files.is_empty() {
            ui.label(RichText::new("Nothing here yet. The buttons above make blueprints, scripts and folders; files dropped on the window are imported.").color(theme::TEXT_DIM));
        }
        asset_level(ui, st, &files, "res://");
        ui.add_space(6.0);
        // What comes with the engine, and connected game installs (read in place).
        for r in st.vfs.roots().into_iter().filter(|r| !r.path.starts_with("res://")) {
            asset_dir_row(ui, st, &r.path, if r.path.starts_with("gex://") { "Built-in" } else { &r.name }, None);
        }
        // The empty space under the tree is the project's top folder.
        let rest = ui.available_rect_before_wrap();
        let bg = ui.interact(rest, ui.id().with("asset root"), egui::Sense::click());
        take_asset_drop(ui, st, &bg, "res://");
        if bg.clicked() {
            st.asset_dir = "res://".into();
            st.asset_selected = None;
        }
        if st.playing.is_none() {
            bg.context_menu(|ui| new_asset_menu(ui, st, "res://"));
        }
    });
    let Some(sel) = preview else { return };
    ui.separator();
    let w = ui.available_width().min(150.0);
    ui.label(RichText::new(sel.rsplit('/').next().unwrap_or("")).small().color(theme::TEXT_DIM));
    let editing = st.playing.is_none();
    if gex_assets::image_load::is_image(&sel) {
        let t = thumb(ui, st, ctx_textures, &sel);
        let size = t.size_vec2();
        let scale = (w / size.x).min(120.0 / size.y).min(4.0);
        ui.image((t.id(), size * scale));
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}×{}", size.x, size.y)).small().color(theme::TEXT_DIM));
            if let Some(id) = st.selected.filter(|i| editing && st.tree.objects.get(*i).is_some_and(|n| n.props.contains_key("texture"))) {
                if ui.small_button("Use on selected").tip("Set the selected object's texture to this").clicked() {
                    st.will_change("texture");
                    st.tree.objects[id].props.insert("texture".into(), Variant::Res(sel.clone()));
                }
            }
        });
    } else if gex_assets::model::is_model(&sel) {
        st.previews.model(ui, &st.project, &sel, w.min(130.0));
        if let Some(id) = st.selected.filter(|i| editing && st.tree.objects.get(*i).is_some_and(|n| n.kind == ObjectKind::Mesh3D)) {
            if ui.small_button("Use on selected").tip("Sets this file on the object selected in the blueprint.").clicked() {
                st.will_change("model");
                st.tree.objects[id].props.insert("mesh".into(), Variant::Text(sel.clone()));
            }
        }
    } else {
        st.previews.sound(ui, &st.vfs, &sel);
    }
    if !st.vfs.redistributable(&sel) {
        ui.label(RichText::new("From a mounted game: read in place, never copied or exported.").small().color(theme::WARN));
    }
}

// ---------------------------------------------------------------- console

/// The debugger, while the game is stopped at a breakpoint.
fn debugger(ui: &mut egui::Ui, st: &mut EditorState) {
    let Some(p) = st.playing.as_mut() else { return };
    let Some(at) = p.handle.paused.clone() else { return };
    egui::Frame::group(ui.style()).fill(theme::WARN.gamma_multiply(0.08)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("⏸ paused at {} line {}", at.file.rsplit(['/', '\\']).next().unwrap_or(""), at.line)).strong().color(theme::WARN));
            if ui.button("Continue (F8)").tip("Runs the game on from the breakpoint.").clicked() {
                p.handle.resume();
            }
            if ui.button("Step (F10)").tip("Runs one more line, then stops again.").clicked() {
                p.handle.step();
            }
        });
        ui.columns(3, |c| {
            c[0].label(RichText::new("Calls").small().color(theme::TEXT_DIM));
            for (f, l) in &at.stack {
                c[0].label(RichText::new(format!("{}  line {}", f, l)).monospace());
            }
            c[1].label(RichText::new("Local variables").small().color(theme::TEXT_DIM));
            for (k, v) in &at.locals {
                c[1].label(RichText::new(format!("{} = {}", k, v)).monospace());
            }
            c[2].label(RichText::new("Script variables").small().color(theme::TEXT_DIM));
            for (k, v) in &at.outside {
                c[2].label(RichText::new(format!("{} = {}", k, v)).monospace());
            }
        });
    });
    watches(ui, st, &at);
}

/// Watch expressions: worked out each time the game stops.
fn watches(ui: &mut egui::Ui, st: &mut EditorState, at: &gex_script::thread::Paused) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Watch").small().color(theme::TEXT_DIM));
        let mut remove = None;
        for (i, w) in st.watches.iter().enumerate() {
            let shown = at.watches.iter().find(|(k, _)| k == w).map(|(_, v)| v.clone()).unwrap_or_else(|| "(at the next stop)".into());
            let color = if shown.starts_with('⚠') { theme::BAD } else { theme::TEXT };
            ui.label(RichText::new(format!("{} = {}", w, shown)).monospace().color(color));
            if ui.small_button("✖").clicked() {
                remove = Some(i);
            }
        }
        if let Some((_, e)) = at.watches.iter().find(|(k, _)| k == "⚠") {
            ui.label(RichText::new(e).color(theme::BAD));
        }
        let r = ui.add(egui::TextEdit::singleline(&mut st.watch_input).hint_text("+ watch: hp * 2").desired_width(140.0).font(egui::TextStyle::Monospace));
        let mut changed = false;
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !st.watch_input.trim().is_empty() {
            st.watches.push(st.watch_input.trim().to_string());
            st.watch_input.clear();
            changed = true;
        }
        if let Some(i) = remove {
            st.watches.remove(i);
            changed = true;
        }
        if changed {
            st.sync_breakpoints();
        }
    });
}

pub fn console(ui: &mut egui::Ui, st: &mut EditorState) {
    crate::docs::mark(ui, ui.available_rect_before_wrap(), "ui:console");
    debugger(ui, st);
    ui.horizontal(|ui| {
        if !st.logs.is_empty() && ui.button("Clear").tip("Empties the console.").clicked() {
            st.logs.clear();
        }
        let errors = st.logs.iter().filter(|l| l.kind == LogKind::Error).count();
        if errors > 0 {
            ui.label(RichText::new(format!("{} error(s)", errors)).color(theme::BAD));
        }
    });
    ui.separator();
    let mut open = None;
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        let start = st.logs.len().saturating_sub(2000);
        for l in &st.logs[start..] {
            let (c, prefix) = match l.kind {
                LogKind::Print => (theme::TEXT, ""),
                LogKind::Info => (theme::TEXT_DIM, "· "),
                LogKind::Error => (theme::BAD, "✖ "),
            };
            let text = RichText::new(format!("{}{}", prefix, l.text)).monospace().color(c);
            if let Some(f) = &l.file {
                if ui.link(text).tip("open at this line").clicked() {
                    open = Some((f.clone(), l.line));
                }
            } else {
                ui.label(text);
            }
        }
    });
    if let Some((file, line)) = open {
        if let Some(res) = st.project.path_to_res(std::path::Path::new(&file)) {
            st.open_script(&res, Some(line));
        } else if let Ok(c) = std::path::Path::new(&file).canonicalize() {
            if let Some(res) = st.project.dir.canonicalize().ok().and_then(|d| c.strip_prefix(&d).ok().map(|r| format!("res://{}", r.to_string_lossy().replace('\\', "/")))) {
                st.open_script(&res, Some(line));
            }
        }
    }
}

/// A small window the Assets panel is asking something in.
#[derive(Clone)]
enum AssetDialog {
    Rename(String, String),
    Delete(String),
}

fn dialog_id() -> egui::Id {
    egui::Id::new("gex asset dialog")
}

fn set_dialog(ctx: &egui::Context, d: AssetDialog) {
    ctx.data_mut(|m| m.insert_temp(dialog_id(), Some(d)));
}

/// The rename / delete dialog, if one is open.
fn asset_dialogs(ui: &mut egui::Ui, st: &mut EditorState) {
    // The open dialog, if any.
    let ctx = ui.ctx().clone();
    let Some(mut d) = ctx.data(|m| m.get_temp::<Option<AssetDialog>>(dialog_id())).flatten() else { return };
    let mut open = true;
    let mut done = false;
    let title = match &d {
        AssetDialog::Rename(..) => "Rename",
        AssetDialog::Delete(_) => "Delete",
    };
    egui::Window::new(title).collapsible(false).resizable(false).open(&mut open).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(&ctx, |ui| {
        theme::fade_in(ui);
        match &mut d {
        AssetDialog::Rename(res, name) => {
            // The name is shown and typed without its ending (.jblueprint, .png...).
            let ext = res.rsplit('/').next().and_then(|n| n.rfind('.').filter(|i| *i > 0).map(|i| n[i..].to_string())).unwrap_or_default();
            if !ext.is_empty() && name.ends_with(&ext) {
                name.truncate(name.len() - ext.len());
            }
            let r = ui.text_edit_singleline(name);
            r.request_focus();
            ui.label(RichText::new("Blueprints and scripts that use it are updated too.").small().color(theme::TEXT_DIM));
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Rename").clicked() || enter {
                let full = format!("{}{}", name.trim(), ext);
                // The same name: nothing to do.
                if res.rsplit('/').next() == Some(full.as_str()) {
                    done = true;
                } else {
                    match st.rename_asset(res, &full) {
                        Ok(_) => done = true,
                        Err(e) => st.status = e,
                    }
                }
            }
        }
        AssetDialog::Delete(res) => {
            ui.label(format!("Move {} to the trash?", res));
            // Looked up once per dialog, not every frame.
            let uid = egui::Id::new(("asset users", res.as_str()));
            let users: Vec<String> = ui.data(|d| d.get_temp(uid)).unwrap_or_else(|| {
                let u = crate::files::users(&st.project, res);
                ui.data_mut(|d| d.insert_temp(uid, u.clone()));
                u
            });
            if !users.is_empty() {
                ui.label(RichText::new(format!("Used in {} file(s): its instances and attached scripts are taken out of those blueprints.", users.len())).color(theme::WARN));
            }
            ui.label(RichText::new("It goes to the project's hidden .trash folder, so you can get it back.").small().color(theme::TEXT_DIM));
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Delete").color(theme::BAD)).clicked() {
                    match st.delete_asset(res) {
                        Ok(()) => done = true,
                        Err(e) => st.status = e,
                    }
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
        }
    }});
    let keep = open && !done;
    ctx.data_mut(|m| m.insert_temp(dialog_id(), if keep { Some(d) } else { None }));
}
