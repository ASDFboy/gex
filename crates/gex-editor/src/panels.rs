//! Hierarchy, inspector, events, assets and console panels.

use eframe::egui::{self, Color32, RichText};
use gex_core::node::ALL_KINDS;
use gex_core::{NodeId, NodeKind, Variant};
use gex_script::LogKind;

use crate::state::EditorState;
use crate::theme;

pub fn kind_icon(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Node => "○",
        NodeKind::Sound => "🔊",
        NodeKind::Node2D => "⊞",
        NodeKind::Sprite2D => "▣",
        NodeKind::Label2D => "🔤",
        NodeKind::Camera2D => "⌖",
        NodeKind::Collider2D => "☐",
        NodeKind::Node3D => "🌍",
        NodeKind::Mesh3D => "■",
        NodeKind::Camera3D => "🎥",
        NodeKind::Light3D => "💡",
        NodeKind::Body2D => "✚",
        NodeKind::TileMap2D => "🗺",
        NodeKind::Particles2D => "✨",
        NodeKind::Panel => "🗖",
        NodeKind::Button => "🔘",
        NodeKind::Label => "🅰",
        NodeKind::TextInput => "⌨",
        NodeKind::ProgressBar => "📊",
        NodeKind::Image => "🖼",
        NodeKind::Body3D => "🔲",
        NodeKind::Environment => "⛅",
    }
}

pub fn add_node_menu(ui: &mut egui::Ui, st: &mut EditorState) {
    let groups: [(&str, fn(NodeKind) -> bool); 4] = [
        ("General", |k| !k.is_2d() && !k.is_3d() && !k.is_ui()),
        ("2D", |k| k.is_2d()),
        ("UI (screen space)", |k| k.is_ui()),
        ("3D", |k| k.is_3d()),
    ];
    for (i, (title, f)) in groups.iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        ui.label(RichText::new(*title).color(theme::TEXT_DIM));
        for k in ALL_KINDS.iter().filter(|k| f(**k)) {
            if ui.button(format!("{} {}", kind_icon(*k), k.name())).clicked() {
                st.add_node(*k);
                ui.close();
            }
        }
    }
}

/// Picking a tile to paint with, from the TileMap2D's tileset image.
fn tile_palette(ui: &mut egui::Ui, st: &mut EditorState, id: NodeId) {
    let n = &st.tree.nodes[id];
    let path = n.text("tileset").to_string();
    let (tw, th) = (n.num("tile_width", 32.0) as f32, n.num("tile_height", 32.0) as f32);
    ui.horizontal(|ui| {
        let painting = st.tile_brush.is_some();
        if ui.selectable_label(painting, "🖌 Paint tiles").on_hover_text("LMB paints in the scene view, Shift+LMB erases").clicked() {
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
    let playing = st.playing.is_some();
    ui.horizontal(|ui| {
        ui.add_enabled_ui(!playing, |ui| {
            ui.menu_button("+ Node", |ui| add_node_menu(ui, st));
            ui.menu_button("+ Scene", |ui| {
                let scenes = list_files(st, "res://", "jscene");
                if scenes.is_empty() {
                    ui.label("no .jscene files yet");
                }
                for s in scenes {
                    if ui.button(&s).clicked() {
                        st.instance_scene(&s);
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
        ui.label("empty scene");
        return;
    };
    let mut action: Option<Action> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        node_row(ui, st, &tree, root, &mut action, playing);
    });
    match action {
        Some(Action::Select(id)) => st.select(Some(id)),
        Some(Action::Toggle(id)) => st.toggle_select(id),
        Some(Action::Reparent(id, to)) if !playing => {
            if st.tree.nodes[id].from_instance {
                st.status = "nodes inside an instanced scene can't be moved out of it".into();
            } else {
                st.will_change("move");
                // Keep where it is in the world.
                let world = st.tree.global_matrix(id);
                if st.tree.reparent(id, to, None) {
                    let local = st.tree.global_matrix(to).inverse() * world;
                    let (s2, r, t) = local.to_scale_rotation_translation();
                    let (ry, rx, rz) = r.to_euler(gex_core::glam::EulerRot::YXZ);
                    let tr = &mut st.tree.nodes[id].transform;
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
            st.save_branch_as_scene(id);
        }
        Some(Action::MakeLocal(id)) if !playing => st.make_local(id),
        Some(Action::OpenScene(res)) if !playing => st.open_scene(&res),
        Some(Action::Duplicate(id)) if !playing => {
            if !st.is_selected(id) {
                st.select(Some(id));
            }
            st.duplicate_selected();
        }
        Some(Action::Move(id, up)) if !playing => {
            if let Some(p) = st.tree.nodes[id].parent {
                let kids = &st.tree.nodes[p].children;
                let i = kids.iter().position(|c| *c == id).unwrap();
                let j = if up { i.checked_sub(1) } else { (i + 1 < kids.len()).then_some(i + 1) };
                if let Some(j) = j {
                    st.will_change("order");
                    st.tree.nodes[p].children.swap(i, j);
                }
            }
        }
        Some(Action::AddChild(id, k)) if !playing => {
            st.select(Some(id));
            st.add_node(k);
        }
        _ => {}
    }
}

enum Action {
    Select(NodeId),
    Toggle(NodeId),
    Reparent(NodeId, NodeId),
    Delete(NodeId),
    Duplicate(NodeId),
    Move(NodeId, bool),
    AddChild(NodeId, NodeKind),
    SaveBranch(NodeId),
    MakeLocal(NodeId),
    OpenScene(String),
}

fn node_row(ui: &mut egui::Ui, st: &EditorState, tree: &gex_core::SceneTree, id: NodeId, action: &mut Option<Action>, playing: bool) {
    let n = &tree.nodes[id];
    let selected = st.is_selected(id);
    let mut label = format!("{} {}", kind_icon(n.kind), n.name);
    if n.instance_of.is_some() {
        label.push_str("  🎬");
    }
    if !n.scripts.is_empty() {
        label.push_str("  📜");
    }
    let color = if n.from_instance { theme::TEXT_DIM } else if !n.visible { Color32::from_gray(110) } else { theme::TEXT };
    let draw_label = |ui: &mut egui::Ui, action: &mut Option<Action>| {
        let r = ui.add(egui::Button::selectable(selected, RichText::new(&label).color(color))).interact(egui::Sense::click_and_drag());
        if r.clicked() {
            *action = Some(if ui.input(|i| i.modifiers.command) { Action::Toggle(id) } else { Action::Select(id) });
        }
        if !playing {
            r.dnd_set_drag_payload(id);
            if let Some(from) = r.dnd_release_payload::<NodeId>() {
                if *from != id {
                    *action = Some(Action::Reparent(*from, id));
                }
            }
            if r.dnd_hover_payload::<NodeId>().is_some() {
                ui.painter().rect_stroke(r.rect, 2.0, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
            }
        }
        let tip = match &n.instance_of {
            Some(p) => format!("{} — instance of {}", n.kind.name(), p),
            None => n.kind.name().to_string(),
        };
        let r = r.on_hover_text(tip);
        r.context_menu(|ui| {
            ui.menu_button("Add child", |ui| {
                for k in ALL_KINDS {
                    if ui.button(format!("{} {}", kind_icon(*k), k.name())).clicked() {
                        *action = Some(Action::AddChild(id, *k));
                        ui.close();
                    }
                }
            });
            if ui.button("Duplicate  (Ctrl+D)").clicked() {
                *action = Some(Action::Duplicate(id));
                ui.close();
            }
            ui.separator();
            if !n.from_instance && n.instance_of.is_none() && ui.button("Save branch as scene").on_hover_text("make this node and its children a reusable .jscene").clicked() {
                *action = Some(Action::SaveBranch(id));
                ui.close();
            }
            if let Some(p) = &n.instance_of {
                if ui.button("Open its scene").clicked() {
                    *action = Some(Action::OpenScene(p.clone()));
                    ui.close();
                }
                if !n.from_instance && ui.button("Make local").on_hover_text("stop following the scene file: its nodes become this scene's own").clicked() {
                    *action = Some(Action::MakeLocal(id));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Move up").clicked() {
                *action = Some(Action::Move(id, true));
                ui.close();
            }
            if ui.button("Move down").clicked() {
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
                    node_row(ui, st, tree, *c, action, playing);
                }
            });
    }
}

// ---------------------------------------------------------------- inspector

fn transform_ui(ui: &mut egui::Ui, st: &mut EditorState, id: NodeId) {
    let kind = st.tree.nodes[id].kind;
    let mut t = st.tree.nodes[id].transform;
    let before = t;
    let drag = |ui: &mut egui::Ui, v: &mut f32, speed: f64, label: &str| {
        ui.add(egui::DragValue::new(v).speed(speed).prefix(label).max_decimals(3));
    };
    egui::Grid::new("xform").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let flat = kind.is_2d();
        let speed = if flat { 1.0 } else { 0.05 };
        ui.label("Position");
        ui.horizontal(|ui| {
            drag(ui, &mut t.position.x, speed, "x ");
            drag(ui, &mut t.position.y, speed, "y ");
            if !flat {
                drag(ui, &mut t.position.z, speed, "z ");
            }
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
        st.tree.nodes[id].transform = t;
    }
}

/// Edits a property value; true when it changed.
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

/// An asset dragged from the asset browser.
#[derive(Clone)]
pub struct AssetPath(pub String);

pub fn inspector(ui: &mut egui::Ui, st: &mut EditorState) {
    let Some(id) = st.selected else {
        ui.label("Select a node to see its properties.");
        return;
    };
    if let Some(p) = &st.playing {
        // Live, read-only view of the running game.
        let Some(n) = p.tree.get(id) else {
            ui.label("This node isn't in the running game (it was destroyed).");
            return;
        };
        ui.heading(format!("{} {}", kind_icon(n.kind), n.name));
        ui.label(RichText::new("live values from the running game").color(theme::GOOD));
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
                ui.label(format!("{:?}", v));
                ui.end_row();
            }
        });
        return;
    }
    if !st.tree.contains(id) {
        st.selected = st.tree.root;
        return;
    }
    let kind = st.tree.nodes[id].kind;
    ui.horizontal(|ui| {
        ui.label(RichText::new(kind_icon(kind)).size(18.0));
        let mut name = st.tree.nodes[id].name.clone();
        let renamable = !st.tree.nodes[id].from_instance;
        if ui.add_enabled(renamable, egui::TextEdit::singleline(&mut name).desired_width(160.0)).changed() && !name.is_empty() && !name.contains('/') {
            st.will_change(&format!("name {:?}", id));
            st.tree.nodes[id].name = name;
        }
        let mut vis = st.tree.nodes[id].visible;
        if ui.checkbox(&mut vis, "visible").changed() {
            st.will_change("visible");
            st.tree.nodes[id].visible = vis;
        }
    });
    ui.label(RichText::new(kind.name()).color(theme::TEXT_DIM));
    if let Some(p) = st.tree.nodes[id].instance_of.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("🎬 instance of {}", p)).color(theme::ACCENT));
            if ui.small_button("open").clicked() {
                st.open_scene(&p);
            }
        });
    }
    if st.tree.nodes[id].from_instance {
        ui.label(RichText::new("Inside an instanced scene: changes here are saved as overrides for this instance only.").small().color(theme::WARN));
    }
    let n_sel = st.selected_roots().len();
    if n_sel > 1 {
        ui.label(RichText::new(format!("{} nodes selected (editing this one; drag in the view moves them all)", n_sel)).small().color(theme::ACCENT));
    }
    ui.separator();
    if kind != NodeKind::Node {
        egui::CollapsingHeader::new("Transform").default_open(true).show(ui, |ui| transform_ui(ui, st, id));
    }
    egui::CollapsingHeader::new("Properties").default_open(true).show(ui, |ui| {
        let defaults: Vec<&str> = kind.default_props().iter().map(|p| p.0).collect();
        let keys: Vec<String> = st.tree.nodes[id].props.keys().cloned().collect();
        let mut remove = None;
        egui::Grid::new("props").num_columns(3).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
            for k in keys {
                let custom = !defaults.contains(&k.as_str());
                ui.label(if custom { RichText::new(&k).color(theme::C_EVENT) } else { RichText::new(&k) });
                let mut v = st.tree.nodes[id].props[&k].clone();
                if variant_ui(ui, &mut v, &k) {
                    st.will_change(&format!("prop {:?} {}", id, k));
                    st.tree.nodes[id].props.insert(k.clone(), v);
                }
                if custom && ui.small_button("✖").on_hover_text("remove this property").clicked() {
                    remove = Some(k.clone());
                } else if !custom {
                    ui.label("");
                }
                ui.end_row();
            }
        });
        if let Some(k) = remove {
            st.will_change("remove prop");
            st.tree.nodes[id].props.remove(&k);
        }
        // Custom properties: what scripts see as self.<name>.
        let mem_id = egui::Id::new("new prop name");
        let mut name: String = ui.data_mut(|d| d.get_temp(mem_id).unwrap_or_default());
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("new_property").desired_width(120.0));
            let ok = jlb::value::is_ident(&name) && !st.tree.nodes[id].props.contains_key(&name);
            for (label, v) in [("number", Variant::Num(0.0)), ("text", Variant::Text(String::new())), ("bool", Variant::Bool(false)), ("color", Variant::Color([1.0; 4]))] {
                if ui.add_enabled(ok, egui::Button::new(format!("+{}", label))).clicked() {
                    st.will_change("add prop");
                    st.tree.nodes[id].props.insert(name.clone(), v);
                    name.clear();
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(mem_id, name));
    });
    if kind == NodeKind::TileMap2D {
        egui::CollapsingHeader::new("Tiles").default_open(true).show(ui, |ui| tile_palette(ui, st, id));
    }
    egui::CollapsingHeader::new("Groups").default_open(false).show(ui, |ui| {
        let mut groups = st.tree.nodes[id].groups.join(", ");
        if ui.add(egui::TextEdit::singleline(&mut groups).hint_text("enemies, pickups").desired_width(f32::INFINITY)).changed() {
            st.will_change("groups");
            st.tree.nodes[id].groups = groups.split(',').map(|g| g.trim().to_string()).filter(|g| !g.is_empty()).collect();
        }
    });
}

// ---------------------------------------------------------------- events

/// The events a script can handle: (function signature, when it runs).
pub const EVENTS: &[(&str, &str)] = &[
    ("on_create()", "once the scene is loaded (or after SPAWN)"),
    ("on_step(dt)", "every frame"),
    ("on_physics(dt)", "60 times a second, before physics moves things"),
    ("on_draw()", "every frame, after on_step: the place for DRAW_* commands"),
    ("on_destroy()", "before the node is removed"),
    ("on_key_pressed(key)", "any key goes down"),
    ("on_key_released(key)", "any key goes up"),
    ("on_press_space()", "one key goes down (rename: on_press_a, on_press_left, on_press_pad_a...)"),
    ("on_release_space()", "one key goes up"),
    ("on_mouse_pressed(button, x, y)", "a mouse click"),
    ("on_mouse_moved(x, y)", "the mouse moved"),
    ("on_mouse_wheel(amount)", "the wheel turned"),
    ("on_collide_start(other)", "this node's collider or body starts touching another"),
    ("on_collide(other)", "every physics step while touching"),
    ("on_collide_end(other)", "they stop touching"),
    ("on_timer(name)", "a TIMER(name, seconds) on this node finishes"),
    ("on_timer_spawn()", "the TIMER named spawn finishes (rename for yours)"),
    ("on_pressed()", "this Button was clicked"),
    ("on_pressed_play()", "any Button named Play was clicked (runs on every script)"),
    ("on_text_changed(text)", "this TextInput's text changed"),
    ("on_text_submitted(text)", "Enter was pressed in this TextInput"),
    ("on_animation_end(name)", "a non-looping animation on this node finished"),
    ("when_ready()", "a condition: return TRUE to run on_ready()"),
    ("on_ready()", "runs when when_ready() turns TRUE"),
];

pub fn events(ui: &mut egui::Ui, st: &mut EditorState) {
    let Some(id) = st.selected.filter(|i| st.tree.contains(*i)) else {
        ui.label("Select a node to see its scripts and events.");
        return;
    };
    let playing = st.playing.is_some();
    let name = st.tree.nodes[id].name.clone();
    ui.label(RichText::new(format!("Scripts on {}", name)).strong());
    ui.label(RichText::new("Each script is a .jlb file; its events are functions named on_…").color(theme::TEXT_DIM).small());
    ui.separator();
    let scripts = st.tree.nodes[id].scripts.clone();
    let mut detach = None;
    for (i, res) in scripts.iter().enumerate() {
        let text = st.script_text(res);
        let mut handlers: Vec<String> = gex_script::world::scan_handlers_text(&text).into_iter().collect();
        handlers.sort();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.link(RichText::new(format!("📜 {}", res.trim_start_matches("res://"))).monospace()).clicked() {
                    st.open_script(res, None);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !playing && ui.small_button("detach").on_hover_text("remove from this node (the file stays)").clicked() {
                        detach = Some(i);
                    }
                });
            });
            for h in &handlers {
                let line = text.lines().position(|l| l.contains(&format!("DEF {}(", h)) || l.contains(&format!("DEF {} (", h))).map(|l| l + 1);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    if ui.link(RichText::new(format!("⚡ {}", h)).color(theme::C_EVENT)).clicked() {
                        st.open_script(res, line);
                    }
                });
            }
            ui.menu_button("+ Event", |ui| {
                for (sig, what) in EVENTS {
                    if ui.button(*sig).on_hover_text(*what).clicked() {
                        st.add_event(res, sig);
                        ui.close();
                    }
                }
                ui.separator();
                let mem = egui::Id::new("custom event");
                let mut custom: String = ui.data_mut(|d| d.get_temp(mem).unwrap_or_default());
                ui.horizontal(|ui| {
                    ui.label("on_");
                    ui.add(egui::TextEdit::singleline(&mut custom).hint_text("my_event").desired_width(100.0));
                    if ui.add_enabled(jlb::value::is_ident(&custom), egui::Button::new("add")).on_hover_text("runs for SEND(node, \"my_event\") and EMIT(\"my_event\")").clicked() {
                        st.add_event(res, &format!("on_{}()", custom));
                        custom.clear();
                        ui.close();
                    }
                });
                ui.data_mut(|d| d.insert_temp(mem, custom));
            });
        });
    }
    if let Some(i) = detach {
        st.will_change("detach");
        st.tree.nodes[id].scripts.remove(i);
    }
    if playing {
        return;
    }
    ui.horizontal(|ui| {
        if ui.button("+ New script").clicked() {
            st.new_script_for(id);
        }
        ui.menu_button("Attach existing", |ui| {
            let all = list_files(st, "res://", "jlb");
            if all.is_empty() {
                ui.label("no .jlb files yet");
            }
            for s in all {
                if ui.button(&s).clicked() {
                    st.will_change("attach");
                    if !st.tree.nodes[id].scripts.contains(&s) {
                        st.tree.nodes[id].scripts.push(s);
                    }
                    ui.close();
                }
            }
        });
    });
    ui.separator();
    egui::CollapsingHeader::new("Engine commands").default_open(false).show(ui, |ui| {
        for (name, args, what) in gex_script::commands::COMMANDS {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{}{}", name, args)).monospace().color(theme::C_ENGINE));
                ui.label(RichText::new(*what).color(theme::TEXT_DIM).small());
            });
        }
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

pub fn assets(ui: &mut egui::Ui, st: &mut EditorState, ctx_textures: &mut std::collections::HashMap<String, egui::TextureHandle>) {
    ui.horizontal_wrapped(|ui| {
        for r in st.vfs.roots() {
            if ui.selectable_label(st.asset_dir.starts_with(&r.path), &r.name).clicked() {
                st.asset_dir = r.path.clone();
            }
        }
        if ui.button("⚙ Mounts").on_hover_text("connect game installs (Half-Life 2...)").clicked() {
            st.show_settings = true;
        }
    });
    // Breadcrumbs.
    ui.horizontal_wrapped(|ui| {
        let (scheme, rel) = st.asset_dir.split_once("://").map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
        if ui.link(format!("{}://", scheme)).clicked() {
            st.asset_dir = format!("{}://", scheme);
        }
        let mut acc = String::new();
        for part in rel.split('/').filter(|p| !p.is_empty()) {
            acc = if acc.is_empty() { part.to_string() } else { format!("{}/{}", acc, part) };
            ui.label("/");
            if ui.link(part).clicked() {
                st.asset_dir = format!("{}://{}", scheme, acc);
            }
        }
    });
    ui.separator();
    let entries = st.vfs.list(&st.asset_dir);
    let preview_w = 150.0;
    ui.horizontal_top(|ui| {
        let list_w = (ui.available_width() - preview_w - 12.0).max(120.0);
        egui::ScrollArea::vertical().id_salt("asset list").max_width(list_w).auto_shrink([false, false]).show(ui, |ui| ui.vertical(|ui| {
            ui.set_width(list_w);
            if entries.is_empty() {
                ui.label(RichText::new("empty").color(theme::TEXT_DIM));
            }
            for e in entries.iter().take(3000) {
                let icon = if e.is_dir {
                    "📁"
                } else if e.name.ends_with(".jscene") {
                    "🎬"
                } else if e.name.ends_with(".jlb") {
                    "📜"
                } else if gex_assets::image_load::is_image(&e.name) {
                    "🖼"
                } else if e.name.ends_with(".wav") || e.name.ends_with(".mp3") || e.name.ends_with(".ogg") {
                    "🔊"
                } else if e.name.ends_with(".mdl") || e.name.ends_with(".bsp") {
                    "🎮"
                } else {
                    "📄"
                };
                let sel = st.asset_selected.as_deref() == Some(&e.path);
                let r = ui.add(egui::Button::selectable(sel, format!("{} {}", icon, e.name))).interact(egui::Sense::click_and_drag());
                if !e.is_dir {
                    r.dnd_set_drag_payload(AssetPath(e.path.clone()));
                }
                if r.clicked() {
                    st.asset_selected = Some(e.path.clone());
                }
                if r.double_clicked() {
                    if e.is_dir {
                        st.asset_dir = e.path.clone();
                    } else if e.name.ends_with(".jscene") {
                        if st.playing.is_none() {
                            st.open_scene(&e.path);
                        }
                    } else if e.name.ends_with(".jlb") || e.name.ends_with(".txt") || e.name.ends_with(".vmt") {
                        if e.path.starts_with("res://") {
                            st.open_script(&e.path, None);
                        }
                    }
                }
            }
            if entries.len() > 3000 {
                ui.label(format!("… and {} more", entries.len() - 3000));
            }
        }));
        ui.separator();
        ui.vertical(|ui| {
            ui.set_width(preview_w);
            let Some(sel) = st.asset_selected.clone() else {
                ui.label(RichText::new("Select a file to preview it. Drag files onto the scene or onto texture fields.").color(theme::TEXT_DIM).small());
                return;
            };
            ui.label(RichText::new(sel.rsplit('/').next().unwrap_or("")).strong());
            if gex_assets::image_load::is_image(&sel) {
                let tex = ctx_textures.entry(sel.clone()).or_insert_with(|| {
                    let img = gex_assets::load_image(&st.vfs, &sel).unwrap_or(gex_assets::Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] });
                    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
                    ui.ctx().load_texture(sel.clone(), ci, egui::TextureOptions::NEAREST)
                });
                let size = tex.size_vec2();
                let scale = (preview_w / size.x).min(preview_w / size.y).min(4.0);
                ui.image((tex.id(), size * scale));
                ui.label(RichText::new(format!("{}×{}", size.x, size.y)).small().color(theme::TEXT_DIM));
                if let Some(id) = st.selected.filter(|_| st.playing.is_none()) {
                    let kind = st.tree.nodes.get(id).map(|n| n.kind);
                    if matches!(kind, Some(NodeKind::Sprite2D) | Some(NodeKind::Mesh3D)) && ui.button("Use on selected").clicked() {
                        st.will_change("texture");
                        st.tree.nodes[id].props.insert("texture".into(), Variant::Res(sel.clone()));
                    }
                }
            } else if gex_assets::model::is_model(&sel) {
                if let Some(id) = st.selected.filter(|_| st.playing.is_none()) {
                    if st.tree.nodes.get(id).is_some_and(|n| n.kind == NodeKind::Mesh3D) && ui.button("Use on selected").clicked() {
                        st.will_change("model");
                        st.tree.nodes[id].props.insert("mesh".into(), Variant::Text(sel.clone()));
                    }
                }
                ui.label(RichText::new("Drag into the 3D view to place it.").small().color(theme::TEXT_DIM));
            } else if sel.ends_with(".jscene") {
                if st.playing.is_none() && ui.button("Instance into scene").clicked() {
                    st.instance_scene(&sel);
                }
                if st.playing.is_none() && ui.button("Open").clicked() {
                    st.open_scene(&sel);
                }
            }
            if !st.vfs.redistributable(&sel) {
                ui.label(RichText::new("From a mounted game install: read in place, never copied or exported.").small().color(theme::WARN));
            }
        });
    });
}

// ---------------------------------------------------------------- console

/// The debugger, while the game is stopped at a breakpoint.
fn debugger(ui: &mut egui::Ui, st: &mut EditorState) {
    let Some(p) = st.playing.as_mut() else { return };
    let Some(at) = p.handle.paused.clone() else { return };
    egui::Frame::group(ui.style()).fill(theme::WARN.gamma_multiply(0.08)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("⏸ paused at {} line {}", at.file.rsplit(['/', '\\']).next().unwrap_or(""), at.line)).strong().color(theme::WARN));
            if ui.button("▶ Continue (F8)").clicked() {
                p.handle.resume();
            }
            if ui.button("⏭ Step (F10)").clicked() {
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
}

pub fn console(ui: &mut egui::Ui, st: &mut EditorState) {
    debugger(ui, st);
    ui.horizontal(|ui| {
        if ui.button("Clear").clicked() {
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
                if ui.link(text).on_hover_text("open at this line").clicked() {
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
