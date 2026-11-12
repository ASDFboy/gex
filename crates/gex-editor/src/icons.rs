//! Icons: the Silk set by Mark James (famfamfam.com, CC BY 2.5), one for every kind of node
//! and file. A node or scene can pick any other, or show what it looks like.

use std::cell::RefCell;
use std::collections::HashMap;

use eframe::egui::{self, Color32};
use gex_core::{NodeId, NodeKind, SceneTree, Variant};

use crate::state::EditorState;

/// The property a node's chosen icon is kept in ("" = the usual one, "render" = its picture).
pub const PROP: &str = "editor_icon";

static SILK: &[u8] = include_bytes!("../assets/silk.bin");

thread_local! {
    static INDEX: Vec<(&'static str, &'static [u8])> = {
        let (mut out, mut i) = (Vec::new(), 0);
        while i < SILK.len() {
            let n = SILK[i] as usize;
            let name = std::str::from_utf8(&SILK[i + 1..i + 1 + n]).unwrap_or("");
            i += 1 + n;
            let len = u16::from_le_bytes([SILK[i], SILK[i + 1]]) as usize;
            out.push((name, &SILK[i + 2..i + 2 + len]));
            i += 2 + len;
        }
        out
    };
    static TEX: RefCell<HashMap<String, Option<egui::TextureHandle>>> = RefCell::new(HashMap::new());
}

/// Every Silk icon's name.
pub fn names() -> Vec<&'static str> {
    INDEX.with(|x| x.iter().map(|e| e.0).collect())
}

/// What stands for a node or file in lists.
#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    Silk(String),
    /// A picture from the project (a sprite's texture).
    Image(String),
    /// A plain colored square (a sprite or mesh without a texture).
    Color([f32; 4]),
}

pub fn kind(k: NodeKind) -> &'static str {
    use NodeKind::*;
    match k {
        Node => "shape_handles",
        Sound => "sound",
        Animator => "film",
        Node2D => "shape_group",
        Sprite2D => "picture",
        Label2D => "font",
        Camera2D => "camera",
        Collider2D => "shape_square",
        Body2D => "brick",
        TileMap2D => "map",
        Particles2D => "asterisk_yellow",
        Light2D => "lightbulb",
        Panel => "application",
        Button => "tab",
        Label => "style",
        TextInput => "textfield",
        ProgressBar => "chart_bar",
        Image => "photo",
        Node3D => "box",
        Mesh3D => "package",
        Camera3D => "webcam",
        Light3D => "weather_sun",
        Body3D => "bricks",
        Collider3D => "shading",
        Sprite3D => "image",
        Label3D => "text_uppercase",
        Particles3D => "asterisk_orange",
        Environment => "weather_cloudy",
    }
}

/// The icon for a file by its name (pictures show themselves instead).
pub fn file(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let ends = |l: &[&str]| l.iter().any(|e| n.ends_with(e));
    if ends(&[".jscene"]) {
        "layout_content"
    } else if ends(&[".jlb"]) {
        "script"
    } else if ends(&[".wav", ".mp3", ".ogg", ".flac"]) {
        "music"
    } else if gex_assets::model::is_model(&n) {
        "package"
    } else if gex_assets::image_load::is_image(&n) {
        "picture"
    } else if ends(&[".ttf", ".otf"]) {
        "font"
    } else if ends(&[".wgsl", ".json", ".vmt"]) {
        "page_white_code"
    } else if ends(&[".txt", ".md"]) {
        "page_white_text"
    } else {
        "page_white"
    }
}

/// What a node looks like: its texture or color, or (`deep`) the first one under it.
fn render(tree: &SceneTree, id: NodeId, deep: bool) -> Option<Icon> {
    let mut all = Vec::new();
    tree.walk_from(id, &mut all);
    for i in all.into_iter().take(if deep { 64 } else { 1 }) {
        let n = &tree.nodes[i];
        if !matches!(n.kind, NodeKind::Sprite2D | NodeKind::Sprite3D | NodeKind::Mesh3D | NodeKind::Image) {
            continue;
        }
        match n.props.get("texture") {
            Some(Variant::Res(t) | Variant::Text(t)) if !t.is_empty() => return Some(Icon::Image(t.clone())),
            _ => {}
        }
        if let Some(Variant::Color(c)) = n.props.get("color") {
            return Some(Icon::Color(*c));
        }
    }
    None
}

/// A node's icon: the one picked for it, or its picture (sprites, meshes and scenes holding
/// them), or its kind's.
pub fn node(tree: &SceneTree, id: NodeId) -> Icon {
    let n = &tree.nodes[id];
    let by_kind = || Icon::Silk(kind(n.kind).to_string());
    match n.text(PROP) {
        "" => {
            let pictured = matches!(n.kind, NodeKind::Sprite2D | NodeKind::Sprite3D | NodeKind::Mesh3D | NodeKind::Image) || n.instance_of.is_some();
            pictured.then(|| render(tree, id, false)).flatten().unwrap_or_else(by_kind)
        }
        "render" => render(tree, id, true).unwrap_or_else(by_kind),
        "kind" => by_kind(),
        name => Icon::Silk(name.to_string()),
    }
}

/// A scene file's icon (its root node's), read from disk and remembered until the file changes.
pub fn scene(st: &EditorState, res: &str) -> Icon {
    thread_local! {
        static SEEN: RefCell<HashMap<String, (Option<std::time::SystemTime>, Icon)>> = RefCell::new(HashMap::new());
    }
    let fallback = Icon::Silk(file(res).to_string());
    let Some(path) = st.project.res_to_path(res) else { return fallback };
    let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if let Some(i) = SEEN.with(|s| s.borrow().get(res).filter(|e| e.0 == stamp).map(|e| e.1.clone())) {
        return i;
    }
    let icon = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| gex_core::scene::load(&t, &st.reader()).ok())
        .and_then(|tree| {
            let root = tree.root?;
            // A scene is its file's icon unless it has a look (or a picked icon) of its own.
            match tree.nodes[root].text(PROP) {
                "" => render(&tree, root, false),
                "render" => render(&tree, root, true),
                "kind" => None,
                name => Some(Icon::Silk(name.to_string())),
            }
        })
        .unwrap_or(fallback);
    SEEN.with(|s| s.borrow_mut().insert(res.to_string(), (stamp, icon.clone())));
    icon
}

fn silk_tex(ctx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
    TEX.with(|t| {
        t.borrow_mut()
            .entry(format!("silk:{}", name))
            .or_insert_with(|| {
                let bytes = INDEX.with(|x| x.iter().find(|e| e.0 == name).map(|e| e.1))?;
                let img = image::load_from_memory(bytes).ok()?.to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], &img);
                Some(ctx.load_texture(format!("silk:{}", name), ci, egui::TextureOptions::NEAREST))
            })
            .clone()
    })
}

fn image_tex(ctx: &egui::Context, st: &EditorState, path: &str) -> Option<egui::TextureHandle> {
    TEX.with(|t| {
        t.borrow_mut()
            .entry(path.to_string())
            .or_insert_with(|| {
                let img = gex_assets::load_image(&st.vfs, path).ok()?;
                // Small: these are only ever shown as icons.
                let rgba = image::RgbaImage::from_raw(img.width, img.height, img.pixels)?;
                let k = 48.0 / img.width.max(img.height).max(1) as f32;
                let rgba = if k < 1.0 { image::imageops::thumbnail(&rgba, ((img.width as f32 * k) as u32).max(1), ((img.height as f32 * k) as u32).max(1)) } else { rgba };
                let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &rgba);
                Some(ctx.load_texture(format!("icon:{}", path), ci, egui::TextureOptions::LINEAR))
            })
            .clone()
    })
}

/// Pictures changed on disk are read again.
pub fn forget(path: &str) {
    TEX.with(|t| t.borrow_mut().remove(path));
}

/// Draws an icon into a square.
pub fn paint(ui: &egui::Ui, st: &EditorState, icon: &Icon, rect: egui::Rect) {
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let tex = match icon {
        Icon::Silk(name) => silk_tex(ui.ctx(), name).or_else(|| silk_tex(ui.ctx(), "page_white")),
        Icon::Image(path) => image_tex(ui.ctx(), st, path).or_else(|| silk_tex(ui.ctx(), "picture")),
        Icon::Color(c) => {
            let r = rect.shrink(2.0);
            ui.painter().rect_filled(r, 0.0, Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, 255));
            ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.0, Color32::from_black_alpha(120)), egui::StrokeKind::Inside);
            None
        }
    };
    if let Some(t) = tex {
        let size = t.size_vec2();
        let k = (rect.width() / size.x).min(rect.height() / size.y);
        // Whole pixels, so 16-pixel art stays sharp.
        let r = egui::Rect::from_center_size(rect.center(), size * k);
        let r = egui::Rect::from_min_size(egui::pos2(r.min.x.round(), r.min.y.round()), r.size());
        ui.painter().image(t.id(), r, uv, Color32::WHITE);
    }
}

/// A selectable row with an icon in front of its text (lists of nodes and files).
pub fn row(ui: &mut egui::Ui, st: &EditorState, icon: &Icon, text: egui::RichText, selected: bool) -> egui::Response {
    let pad = ui.spacing().button_padding;
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let size = egui::vec2(16.0 + 5.0 + galley.size().x + pad.x * 2.0, (galley.size().y).max(16.0) + pad.y * 2.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    if ui.is_rect_visible(rect) {
        let vis = ui.style().interact_selectable(&resp, selected);
        if selected || resp.hovered() || resp.is_pointer_button_down_on() {
            ui.painter().rect(rect, 0.0, vis.weak_bg_fill, vis.bg_stroke, egui::StrokeKind::Inside);
        }
        paint(ui, st, icon, egui::Rect::from_min_size(egui::pos2(rect.left() + pad.x, rect.center().y - 8.0), egui::vec2(16.0, 16.0)));
        ui.painter().galley(egui::pos2(rect.left() + pad.x + 21.0, rect.center().y - galley.size().y / 2.0), galley, vis.text_color());
    }
    resp
}

/// The window for picking a node's icon. Gives the pick ("" = the usual one).
pub fn picker(ui: &mut egui::Ui, st: &EditorState, current: &str) -> Option<String> {
    let mut pick = None;
    let id = egui::Id::new("icon picker filter");
    let mut filter: String = ui.data(|d| d.get_temp(id).unwrap_or_default());
    ui.horizontal(|ui| {
        for (value, label, tip) in [("", "Automatic", "Its picture for sprites, meshes and scenes; its kind's icon otherwise"), ("render", "Its picture", "What it looks like: its texture or color, or the first one inside it"), ("kind", "Its kind", "The icon of its kind of node")] {
            if ui.selectable_label(current == value, label).on_hover_text(tip).clicked() {
                pick = Some(value.to_string());
            }
        }
    });
    ui.add(egui::TextEdit::singleline(&mut filter).hint_text("Search 1000 icons").desired_width(f32::INFINITY));
    let needle = filter.trim().to_lowercase().replace(' ', "_");
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        ui.set_width(13.0 * 22.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for name in names().into_iter().filter(|n| needle.is_empty() || n.contains(&needle)) {
                let (rect, r) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                if r.hovered() || current == name {
                    ui.painter().rect_filled(rect, 0.0, crate::theme::GRAY_LIGHT);
                }
                paint(ui, st, &Icon::Silk(name.to_string()), rect.shrink(2.0));
                if r.on_hover_text(name.replace('_', " ")).clicked() {
                    pick = Some(name.to_string());
                }
            }
        });
    });
    ui.data_mut(|d| d.insert_temp(id, filter));
    pick
}
