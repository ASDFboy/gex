use std::collections::HashMap;

use crate::object::{Object, ObjectKind};
use crate::blueprint;
use crate::tree::BlueprintTree;
use crate::variant::Variant;

fn files() -> HashMap<&'static str, String> {
    let mut f = HashMap::new();
    f.insert(
        "res://enemy.jblueprint",
        r#"(format: 1, root: (name: "Enemy", kind: Some(Sprite2D), props: {"color": Color((1.0, 0.0, 0.0, 1.0))},
            scripts: ["res://enemy.jlb"], groups: ["enemies"],
            children: [(name: "Hitbox", kind: Some(Collider2D))]))"#
            .to_string(),
    );
    f.insert(
        "res://goblin.jblueprint",
        r#"(root: (name: "Goblin", instance: Some("res://enemy.jblueprint"), props: {"width": Num(16.0)}, scripts: ["res://goblin.jlb"]))"#.to_string(),
    );
    f.insert("res://loop.jblueprint", r#"(root: (name: "L", instance: Some("res://loop.jblueprint")))"#.to_string());
    f
}

#[test]
fn tree_basics() {
    let mut t = BlueprintTree::new();
    let root = t.add(None, Object::new("Root", ObjectKind::Object2D));
    let a = t.add(Some(root), Object::new("A", ObjectKind::Sprite2D));
    let a2 = t.add(Some(root), Object::new("A", ObjectKind::Sprite2D));
    assert_eq!(t.objects[a2].name, "A2", "sibling names stay unique");
    let b = t.add(Some(a), Object::new("B", ObjectKind::Object2D));
    assert_eq!(t.path_of(b), "Root/A/B");
    assert_eq!(t.find_path(None, "Root/A/B"), Some(b));
    assert_eq!(t.find_path(None, "/A/B"), Some(b));
    assert_eq!(t.find_path(Some(b), "../../A2"), Some(a2));
    assert!(!t.reparent(a, b, None), "can't move an object into itself");
    assert!(t.reparent(b, a2, None));
    assert_eq!(t.path_of(b), "Root/A2/B");
    t.objects[root].transform.position = glam::Vec3::new(10.0, 0.0, 0.0);
    t.objects[a2].transform.position = glam::Vec3::new(5.0, 2.0, 0.0);
    assert_eq!(t.global_matrix(b).w_axis.truncate(), glam::Vec3::new(15.0, 2.0, 0.0));
    assert_eq!(t.remove(a2).len(), 2);
    assert_eq!(t.walk(), vec![root, a]);
}

#[test]
fn instancing_inheritance_and_round_trip() {
    let f = files();
    let read = |p: &str| f.get(p).cloned().ok_or_else(|| "missing".to_string());
    let text = r#"(root: (name: "Level", kind: Some(Object2D), children: [
        (name: "G1", instance: Some("res://goblin.jblueprint"), transform: Some((position: (100.0, 50.0, 0.0)))),
        (name: "Plain", kind: Some(Sprite2D)),
    ]))"#;
    let tree = blueprint::load(text, &read).unwrap();
    let g = tree.find_path(None, "Level/G1").unwrap();
    let gn = &tree.objects[g];
    assert_eq!(gn.instance_of.as_deref(), Some("res://goblin.jblueprint"));
    // Inherited from enemy.jblueprint through goblin.jblueprint, with goblin's overrides.
    assert_eq!(gn.props["color"], Variant::Color([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(gn.props["width"], Variant::Num(16.0));
    assert_eq!(gn.scripts, vec!["res://enemy.jlb", "res://goblin.jlb"]);
    assert_eq!(gn.groups, vec!["enemies"]);
    let hit = tree.find_path(Some(g), "Hitbox").unwrap();
    assert!(tree.objects[hit].from_instance);
    assert_eq!(tree.in_group("enemies"), vec![g]);

    // Saving writes the instance by reference, with only the overrides.
    let saved = blueprint::save(&tree, &read).unwrap();
    assert!(saved.contains("goblin.jblueprint") && !saved.contains("Hitbox") && !saved.contains("enemy.jlb"), "{}", saved);
    let again = blueprint::load(&saved, &read).unwrap();
    assert_eq!(blueprint::save(&again, &read).unwrap(), saved, "save/load/save is stable");
    let g2 = again.find_path(None, "Level/G1").unwrap();
    assert_eq!(again.objects[g2].transform.position.x, 100.0);
}

#[test]
fn instance_cycles_are_errors() {
    let f = files();
    let read = |p: &str| f.get(p).cloned().ok_or_else(|| "missing".to_string());
    let e = blueprint::load(r#"(root: (instance: Some("res://loop.jblueprint")))"#, &read).err().unwrap();
    assert!(e.0.contains("containing itself"), "{}", e);
}

#[test]
fn render_extraction() {
    let mut t = BlueprintTree::new();
    let r = t.add(None, Object::new("R", ObjectKind::Object));
    let mut s = Object::new("S", ObjectKind::Sprite2D);
    s.props.insert("layer".into(), Variant::Num(2.0));
    t.add(Some(r), s);
    t.add(Some(r), Object::new("Under", ObjectKind::Sprite2D));
    let mut hidden = Object::new("H", ObjectKind::Mesh3D);
    hidden.visible = false;
    t.add(Some(r), hidden);
    t.add(Some(r), Object::new("Cam", ObjectKind::Camera3D));
    let l = crate::render::extract(&t);
    assert_eq!(l.sprites.len(), 2);
    assert_eq!(t.objects[l.sprites[0].object].name, "Under", "sorted by layer");
    assert!(l.meshes.is_empty());
    assert!(l.camera3d.is_some());
}

#[test]
fn editing_inside_an_instance_is_saved_as_overrides() {
    let f = files();
    let read = |p: &str| f.get(p).cloned().ok_or_else(|| "missing".to_string());
    let text = r#"(root: (name: "Level", kind: Some(Object2D), children: [(name: "E", instance: Some("res://enemy.jblueprint"))]))"#;
    let mut tree = blueprint::load(text, &read).unwrap();
    let hit = tree.find_path(None, "Level/E/Hitbox").unwrap();
    tree.objects[hit].props.insert("width".into(), Variant::Num(99.0));
    tree.objects[hit].transform.position.x = 5.0;
    tree.add(Some(hit), Object::new("Spark", ObjectKind::Sprite2D));
    let saved = blueprint::save(&tree, &read).unwrap();
    assert!(saved.contains("overrides") && saved.contains("Spark"), "{}", saved);
    let again = blueprint::load(&saved, &read).unwrap();
    let hit2 = again.find_path(None, "Level/E/Hitbox").unwrap();
    assert_eq!(again.objects[hit2].props["width"], Variant::Num(99.0));
    assert_eq!(again.objects[hit2].transform.position.x, 5.0);
    let spark = again.find_path(None, "Level/E/Hitbox/Spark").unwrap();
    assert!(!again.objects[spark].from_instance);
    assert_eq!(blueprint::save(&again, &read).unwrap(), saved);
}

/// The one-pass Sprite2D reading gives what reading each property by itself gives.
#[test]
fn sprites_read_in_one_pass() {
    use crate::render::{extract, pivot_of, Region, Sprite};
    let mut tree = BlueprintTree::new();
    let root = tree.add(None, Object::new("R", ObjectKind::Object2D));
    let mut a = Object::new("A", ObjectKind::Sprite2D);
    for (k, v) in [
        ("width", Variant::Num(40.0)),
        ("height", Variant::Num(20.0)),
        ("color", Variant::Color([0.5, 0.25, 1.0, 0.8])),
        ("pivot_x", Variant::Num(0.2)),
        ("texture", Variant::Res("res://a.png".into())),
        ("layer", Variant::Num(3.0)),
        ("hframes", Variant::Num(4.0)),
        ("frame", Variant::Num(2.0)),
        ("flip_h", Variant::Bool(true)),
        ("hue", Variant::Num(90.0)),
        ("saturation", Variant::Num(0.5)),
        ("invert", Variant::Bool(true)),
        ("value_2", Variant::Num(7.0)),
        ("opacity", Variant::Num(0.5)),
        ("shader", Variant::Res("res://s.wgsl".into())),
    ] {
        a.props.insert(k.to_string(), v);
    }
    let mut parent = Object::new("P", ObjectKind::Object2D);
    parent.props.insert("modulate".into(), Variant::Color([1.0, 0.5, 1.0, 1.0]));
    let p = tree.add(Some(root), parent);
    let id = tree.add(Some(p), a);
    let list = extract(&tree);
    let s = &list.sprites[0];
    let n = &tree.objects[id];
    assert_eq!(s.size, glam::Vec2::new(40.0, 20.0));
    assert_eq!(s.pivot, pivot_of(n));
    assert_eq!(s.color, crate::render::tinted(&tree, id, n.color("color")));
    assert_eq!(s.texture, "res://a.png");
    assert_eq!(s.layer, 3.0);
    assert_eq!(s.region, Region::Frame { h: 4, v: 1, frame: 2 });
    assert_eq!(s.flip, [true, false]);
    assert_eq!(s.effects, Sprite::effects_of(n));
    assert_eq!(s.values, Sprite::values_of(n));
    assert_eq!(s.shader, "res://s.wgsl");
}

#[test]
fn write_if_changed_leaves_the_same_contents_alone() {
    use crate::project::write_if_changed;
    let dir = std::env::temp_dir().join(format!("gex-wic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let f = dir.join("sub/a.txt");
    assert!(write_if_changed(&f, b"one").unwrap(), "new file (and its folder) made");
    let m = std::fs::metadata(&f).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(!write_if_changed(&f, b"one").unwrap());
    assert_eq!(std::fs::metadata(&f).unwrap().modified().unwrap(), m, "not touched");
    assert!(write_if_changed(&f, b"two").unwrap());
    assert_eq!(std::fs::read(&f).unwrap(), b"two");
    assert_eq!(std::fs::read_dir(dir.join("sub")).unwrap().count(), 1, "no .part file left behind");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn label_boxes_skip_tags_and_wrap() {
    use crate::render::label_box;
    let plain = label_box("hello", 20.0, 0.0);
    assert_eq!(label_box("[color=#ff0000]hello[/color]", 20.0, 0.0), plain);
    assert_eq!(label_box("a [x] c", 10.0, 0.0).x, 7.0 * 5.5, "brackets that aren't tags take room");
    let wrapped = label_box(&"word ".repeat(20), 10.0, 100.0);
    assert_eq!(wrapped.x, 100.0);
    assert!(wrapped.y > 10.0 * 1.2 * 4.0);
}
