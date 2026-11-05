use std::collections::HashMap;

use crate::node::{Node, NodeKind};
use crate::scene;
use crate::tree::SceneTree;
use crate::variant::Variant;

fn files() -> HashMap<&'static str, String> {
    let mut f = HashMap::new();
    f.insert(
        "res://enemy.jscene",
        r#"(format: 1, root: (name: "Enemy", kind: Some(Sprite2D), props: {"color": Color((1.0, 0.0, 0.0, 1.0))},
            scripts: ["res://enemy.jlb"], groups: ["enemies"],
            children: [(name: "Hitbox", kind: Some(Collider2D))]))"#
            .to_string(),
    );
    f.insert(
        "res://goblin.jscene",
        r#"(root: (name: "Goblin", instance: Some("res://enemy.jscene"), props: {"width": Num(16.0)}, scripts: ["res://goblin.jlb"]))"#.to_string(),
    );
    f.insert("res://loop.jscene", r#"(root: (name: "L", instance: Some("res://loop.jscene")))"#.to_string());
    f
}

#[test]
fn tree_basics() {
    let mut t = SceneTree::new();
    let root = t.add(None, Node::new("Root", NodeKind::Node2D));
    let a = t.add(Some(root), Node::new("A", NodeKind::Sprite2D));
    let a2 = t.add(Some(root), Node::new("A", NodeKind::Sprite2D));
    assert_eq!(t.nodes[a2].name, "A2", "sibling names stay unique");
    let b = t.add(Some(a), Node::new("B", NodeKind::Node2D));
    assert_eq!(t.path_of(b), "Root/A/B");
    assert_eq!(t.find_path(None, "Root/A/B"), Some(b));
    assert_eq!(t.find_path(None, "/A/B"), Some(b));
    assert_eq!(t.find_path(Some(b), "../../A2"), Some(a2));
    assert!(!t.reparent(a, b, None), "can't move a node into itself");
    assert!(t.reparent(b, a2, None));
    assert_eq!(t.path_of(b), "Root/A2/B");
    t.nodes[root].transform.position = glam::Vec3::new(10.0, 0.0, 0.0);
    t.nodes[a2].transform.position = glam::Vec3::new(5.0, 2.0, 0.0);
    assert_eq!(t.global_matrix(b).w_axis.truncate(), glam::Vec3::new(15.0, 2.0, 0.0));
    assert_eq!(t.remove(a2).len(), 2);
    assert_eq!(t.walk(), vec![root, a]);
}

#[test]
fn instancing_inheritance_and_round_trip() {
    let f = files();
    let read = |p: &str| f.get(p).cloned().ok_or_else(|| "missing".to_string());
    let text = r#"(root: (name: "Level", kind: Some(Node2D), children: [
        (name: "G1", instance: Some("res://goblin.jscene"), transform: Some((position: (100.0, 50.0, 0.0)))),
        (name: "Plain", kind: Some(Sprite2D)),
    ]))"#;
    let tree = scene::load(text, &read).unwrap();
    let g = tree.find_path(None, "Level/G1").unwrap();
    let gn = &tree.nodes[g];
    assert_eq!(gn.instance_of.as_deref(), Some("res://goblin.jscene"));
    // Inherited from enemy.jscene through goblin.jscene, with goblin's overrides.
    assert_eq!(gn.props["color"], Variant::Color([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(gn.props["width"], Variant::Num(16.0));
    assert_eq!(gn.scripts, vec!["res://enemy.jlb", "res://goblin.jlb"]);
    assert_eq!(gn.groups, vec!["enemies"]);
    let hit = tree.find_path(Some(g), "Hitbox").unwrap();
    assert!(tree.nodes[hit].from_instance);
    assert_eq!(tree.in_group("enemies"), vec![g]);

    // Saving writes the instance by reference, with only the overrides.
    let saved = scene::save(&tree, &read).unwrap();
    assert!(saved.contains("goblin.jscene") && !saved.contains("Hitbox") && !saved.contains("enemy.jlb"), "{}", saved);
    let again = scene::load(&saved, &read).unwrap();
    assert_eq!(scene::save(&again, &read).unwrap(), saved, "save/load/save is stable");
    let g2 = again.find_path(None, "Level/G1").unwrap();
    assert_eq!(again.nodes[g2].transform.position.x, 100.0);
}

#[test]
fn instance_cycles_are_errors() {
    let f = files();
    let read = |p: &str| f.get(p).cloned().ok_or_else(|| "missing".to_string());
    let e = scene::load(r#"(root: (instance: Some("res://loop.jscene")))"#, &read).err().unwrap();
    assert!(e.0.contains("containing itself"), "{}", e);
}

#[test]
fn render_extraction() {
    let mut t = SceneTree::new();
    let r = t.add(None, Node::new("R", NodeKind::Node));
    let mut s = Node::new("S", NodeKind::Sprite2D);
    s.props.insert("layer".into(), Variant::Num(2.0));
    t.add(Some(r), s);
    t.add(Some(r), Node::new("Under", NodeKind::Sprite2D));
    let mut hidden = Node::new("H", NodeKind::Mesh3D);
    hidden.visible = false;
    t.add(Some(r), hidden);
    t.add(Some(r), Node::new("Cam", NodeKind::Camera3D));
    let l = crate::render::extract(&t);
    assert_eq!(l.sprites.len(), 2);
    assert_eq!(t.nodes[l.sprites[0].node].name, "Under", "sorted by layer");
    assert!(l.meshes.is_empty());
    assert!(l.camera3d.is_some());
}
