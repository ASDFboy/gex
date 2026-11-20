use super::*;

#[test]
fn text_edits_cross_over() {
    let base = "hello world".to_string();
    let a = Op { s: 5, e: 5, t: ",".into() };
    let b = Op { s: 11, e: 11, t: "!".into() };
    let (mut x, mut y) = (base.clone(), base.clone());
    apply_op(&mut x, &a);
    apply_op(&mut x, &xf(&b, &a, false));
    apply_op(&mut y, &b);
    apply_op(&mut y, &xf(&a, &b, true));
    assert_eq!(x, "hello, world!");
    assert_eq!(x, y);
    // Any two edits, wherever they are: both orders give the same text.
    let mut seed: u64 = 12345;
    let mut rnd = |n: u32| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) % n.max(1) as u64) as u32
    };
    for _ in 0..20000 {
        let base: String = (0..rnd(12)).map(|i| (b'a' + (i as u8 % 26)) as char).collect();
        let n = char_len(&base);
        let mut op = |tag: char| {
            let s = rnd(n + 1);
            let e = s + rnd(n - s + 1);
            let t: String = std::iter::repeat_n(tag, rnd(3) as usize).collect();
            Op { s, e, t }
        };
        let (a, b) = (op('A'), op('B'));
        let (mut x, mut y) = (base.clone(), base.clone());
        apply_op(&mut x, &a);
        apply_op(&mut x, &xf(&b, &a, false));
        apply_op(&mut y, &b);
        apply_op(&mut y, &xf(&a, &b, true));
        assert_eq!(x, y, "{:?} {:?} on {:?}", a, b, base);
    }
    assert_eq!(text_diff("abcdef", "abXYef"), Some(Op { s: 2, e: 4, t: "XY".into() }));
    let mut t = "héllo".to_string();
    apply_op(&mut t, &Op { s: 1, e: 2, t: "e".into() });
    assert_eq!(t, "hello");
}

#[test]
fn blueprints_follow_patches_and_keep_objects_through_renames() {
    let mut a = BlueprintTree::new();
    let root = a.add(None, Object::new("Root", ObjectKind::Object2D));
    let p = a.add(Some(root), Object::new("Player", ObjectKind::Sprite2D));
    a.add(Some(p), Object::new("Gun", ObjectKind::Sprite2D));
    let wall = a.add(Some(root), Object::new("Wall", ObjectKind::Sprite2D));
    let mut b = a.clone();
    let before = flat(&a);
    // Move one, rename one and move it under another, add one, remove one.
    a.objects[p].transform.position.x = 50.0;
    a.objects[wall].name = "Fence".into();
    a.reparent(wall, p, Some(0));
    a.add(Some(p), Object::new("Hat", ObjectKind::Sprite2D));
    let gun = a.find_path(None, "Root/Player/Gun").unwrap();
    a.remove(gun);
    let ch = diff(&before, &flat(&a));
    patch_tree(&mut b, &ch);
    assert_eq!(flat(&a), flat(&b));
    assert!(b.contains(wall) && b.objects[wall].name == "Fence", "the renamed object is the same object");
    // Copies get their own uids, the same way everywhere.
    let mut c = a.clone();
    let dup = c.copy_subtree(&a, p, Some(root));
    let mut d = c.clone();
    fix_uids(&mut c);
    fix_uids(&mut d);
    assert_ne!(c.objects[dup].uid, c.objects[p].uid);
    assert_eq!(flat(&c), flat(&d));
}

#[test]
fn invite_codes_and_encryption() {
    let c = new_code();
    assert_eq!(c.len(), 19);
    assert_ne!(c, new_code());
    assert_eq!(normal_code(&c.to_lowercase().replace('-', " ")), c.replace('-', ""));
    let (jn, hn) = (random::<16>(), random::<16>());
    let (a1, b1) = keys(&c, &jn, &hn);
    let (a2, _) = keys(&c, &jn, &hn);
    assert_eq!(a1, a2);
    assert_ne!(a1, b1);
    let (mut tx, mut rx) = (Cipher::new(&a1), Cipher::new(&a2));
    let s1 = tx.seal(0, b"hi");
    let s2 = tx.seal(1, b"there");
    assert_eq!(rx.open(&s1), Some((0, b"hi".to_vec())));
    assert_eq!(rx.open(&s2), Some((1, b"there".to_vec())));
    // Replayed, tampered with, or another code: doesn't open.
    assert_eq!(rx.open(&s1), None);
    let mut t = Cipher::new(&a1).seal(0, b"x");
    t[0] ^= 1;
    assert_eq!(Cipher::new(&a1).open(&t), None);
    let (w, _) = keys("AAAA-AAAA-AAAA-AAAA", &jn, &hn);
    assert_eq!(Cipher::new(&w).open(&Cipher::new(&a1).seal(0, b"x")), None);
}

#[test]
fn paths_from_others_stay_in_the_project() {
    let d = Path::new("/p");
    assert!(safe_path(d, "scripts/a.jlb").is_some());
    assert!(safe_path(d, "../x").is_none());
    assert!(safe_path(d, "/etc/passwd").is_none());
    assert!(safe_path(d, "C:/x").is_none());
    assert!(safe_path(d, "a/../../x").is_none());
    assert!(safe_path(d, ".gex/layout.ron").is_none());
    assert!(safe_res("res://scripts/a.jlb") && !safe_res("res://../a") && !safe_res("/etc/x"));
}

fn run(ctx: &egui::Context, a: &mut EditorState, b: &mut EditorState, ms: u64) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        // Both are working (as if typing and clicking): changes go out right away.
        for s in [&mut a.collab, &mut b.collab].into_iter().flatten() {
            s.last_input = Instant::now();
        }
        tick(ctx, a);
        tick(ctx, b);
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn wait_join(ctx: &egui::Context, a: &mut EditorState, j: &mut Session) {
    let t0 = Instant::now();
    while !j.ready {
        assert!(t0.elapsed() < Duration::from_secs(20), "joining took too long: {}", j.status);
        assert!(j.ended.is_none(), "{:?}", j.ended);
        tick(ctx, a);
        tick_joining(ctx, j);
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_end(ctx: &egui::Context, a: &mut EditorState, j: &mut Session) {
    let t0 = Instant::now();
    while j.ended.is_none() {
        assert!(t0.elapsed() < Duration::from_secs(10), "{}", j.status);
        tick(ctx, a);
        tick_joining(ctx, j);
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Two editors on this computer: one shares the demo project, the other joins with the invite
/// code and gets a copy; then objects, typing (both at once), files, roles, chat and pointers
/// cross over.
#[test]
fn two_editors_work_together() {
    let tmp = std::env::temp_dir().join(format!("gex-collab-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let a_dir = tmp.join("a");
    crate::templates::create_demo(&a_dir).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let ctx = egui::Context::default();
    let mut a = EditorState::new(gex_core::project::Project::open(&a_dir).unwrap());
    a.collab = Some(Session::host(&ctx, &a, "Ann", port).unwrap());
    let code = a.collab.as_ref().unwrap().code.clone();
    // A wrong code doesn't get in, and gets nothing.
    let mut wrong = Session::join(&ctx, "Eve", &format!("localhost:{}", port), "AAAA-AAAA-AAAA-AAAA", &tmp.join("e"));
    wait_end(&ctx, &mut a, &mut wrong);
    assert!(wrong.ended.as_deref().unwrap().contains("code"), "{:?}", wrong.ended);
    assert!(walk(&tmp.join("e")).is_empty(), "nothing was sent");
    let mut j = Session::join(&ctx, "Bob", &format!("localhost:{}", port), &code.to_lowercase(), &tmp.join("b"));
    wait_join(&ctx, &mut a, &mut j);
    for (rel, _, _) in walk(&a_dir) {
        assert_eq!(std::fs::read(a_dir.join(&rel)).unwrap(), std::fs::read(j.dir.join(&rel)).unwrap_or_default(), "{}", rel);
    }
    let mut b = EditorState::new(gex_core::project::Project::open(&j.dir).unwrap());
    j.opened = true;
    b.collab = Some(j);
    run(&ctx, &mut a, &mut b, 400);
    assert_eq!(a.blueprint_res, b.blueprint_res);
    assert!(b.collab.as_ref().unwrap().people.iter().any(|p| p.name == "Ann"));
    // Ann moves an object while Bob recolors the same one and adds another: all three stay.
    let root = a.tree.root.unwrap();
    let first = a.tree.objects[root].children[0];
    let uid = a.tree.objects[first].uid;
    let bf = b.tree.objects.iter().find(|(_, o)| o.uid == uid).unwrap().0;
    a.tree.objects[first].transform.position.x = 1234.0;
    b.tree.objects[bf].props.insert("tint".into(), Variant::Text("#ff0000".into()));
    let broot = b.tree.root.unwrap();
    b.tree.add(Some(broot), Object::new("FromBob", ObjectKind::Sprite2D));
    run(&ctx, &mut a, &mut b, 600);
    assert_eq!(flat(&a.tree), flat(&b.tree), "the blueprints match");
    assert_eq!(b.tree.objects[bf].transform.position.x, 1234.0);
    assert_eq!(a.tree.objects[first].props.get("tint"), Some(&Variant::Text("#ff0000".into())));
    // Ann renames it: Bob's object stays the same object (still selected).
    b.selected = Some(bf);
    a.tree.objects[first].name = "Renamed".into();
    run(&ctx, &mut a, &mut b, 300);
    assert!(b.tree.contains(bf) && b.tree.objects[bf].name == "Renamed" && b.selected == Some(bf));
    // Both type into the same script at once.
    let res = "res://scripts/shared_test.jlb";
    std::fs::write(a_dir.join("scripts/shared_test.jlb"), "hello world\n").unwrap();
    run(&ctx, &mut a, &mut b, 2200);
    assert!(b.project.dir.join("scripts/shared_test.jlb").is_file(), "a new file arrives");
    a.open_script(res, None);
    b.open_script(res, None);
    run(&ctx, &mut a, &mut b, 400);
    a.scripts.get_mut(res).unwrap().text = "hello, world\n".into();
    b.scripts.get_mut(res).unwrap().text = "hello world!\n".into();
    run(&ctx, &mut a, &mut b, 600);
    assert_eq!(a.scripts[res].text, "hello, world!\n");
    assert_eq!(b.scripts[res].text, a.scripts[res].text);
    for i in 0..20 {
        a.scripts.get_mut(res).unwrap().text.insert(0, 'a');
        let bt = &mut b.scripts.get_mut(res).unwrap().text;
        let at = bt.len() - 1;
        bt.insert(at, if i % 2 == 0 { 'x' } else { 'y' });
        tick(&ctx, &mut a);
        tick(&ctx, &mut b);
    }
    run(&ctx, &mut a, &mut b, 600);
    assert_eq!(a.scripts[res].text, b.scripts[res].text);
    assert!(a.scripts[res].text.starts_with(&"a".repeat(20)) && a.scripts[res].text.ends_with("xyxyxyxyxyxyxyxyxyxy\n"), "{:?}", a.scripts[res].text);
    // Chat.
    b.collab.as_mut().unwrap().say("hi Ann");
    run(&ctx, &mut a, &mut b, 200);
    assert!(a.collab.as_ref().unwrap().chat.iter().any(|l| l.text == "hi Ann" && l.name == "Bob"));
    // View-only: Bob's changes are put back, and Ann's still arrive.
    let bob = a.collab.as_ref().unwrap().people[0].id;
    a.collab.as_mut().unwrap().set_editor(bob, false);
    run(&ctx, &mut a, &mut b, 300);
    assert!(!b.collab.as_ref().unwrap().editor);
    b.tree.objects[bf].transform.position.y = -999.0;
    b.scripts.get_mut(res).unwrap().text.push_str("sneaky");
    a.tree.objects[first].transform.position.y = 77.0;
    run(&ctx, &mut a, &mut b, 500);
    assert_eq!(b.tree.objects[bf].transform.position.y, 77.0);
    assert!(!a.scripts[res].text.contains("sneaky") && !b.scripts[res].text.contains("sneaky"));
    a.collab.as_mut().unwrap().set_editor(bob, true);
    run(&ctx, &mut a, &mut b, 200);
    // Bob's mouse in the view, as Ann sees it.
    viewport(&egui::Painter::new(ctx.clone(), egui::LayerId::background(), egui::Rect::EVERYTHING), &mut b, &|_| None, Some(gex_core::glam::Vec3::new(5.0, 6.0, 0.0)), false);
    run(&ctx, &mut a, &mut b, 300);
    let p = a.collab.as_ref().unwrap().people.iter().find(|p| p.name == "Bob").unwrap().clone();
    assert_eq!(p.p.cursor, Some([5.0, 6.0, 0.0]));
    // Ann removes Bob: he's told, and his computer can't come back this session.
    a.collab.as_mut().unwrap().kick(bob);
    run(&ctx, &mut a, &mut b, 300);
    assert!(b.collab.as_ref().unwrap().ended.as_deref().is_some_and(|e| e.contains("removed")), "{:?}", b.collab.as_ref().unwrap().ended);
    let mut again = Session::join(&ctx, "Bob", &format!("localhost:{}", port), &code, &tmp.join("b2"));
    wait_end(&ctx, &mut a, &mut again);
    // Ann stops: the port is free again.
    a.collab = None;
    std::thread::sleep(Duration::from_millis(100));
    assert!(Session::host(&ctx, &a, "Ann", port).is_ok());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The connection drops: the joined editor gets back in by itself, keeping what it changed
/// meanwhile.
#[test]
fn joined_editor_reconnects() {
    let tmp = std::env::temp_dir().join(format!("gex-collab-re-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let a_dir = tmp.join("a");
    crate::templates::create_demo(&a_dir).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let ctx = egui::Context::default();
    let mut a = EditorState::new(gex_core::project::Project::open(&a_dir).unwrap());
    a.collab = Some(Session::host(&ctx, &a, "Ann", port).unwrap());
    let code = a.collab.as_ref().unwrap().code.clone();
    let mut j = Session::join(&ctx, "Bob", &format!("127.0.0.1:{}", port), &code, &tmp.join("b"));
    wait_join(&ctx, &mut a, &mut j);
    let mut b = EditorState::new(gex_core::project::Project::open(&j.dir).unwrap());
    j.opened = true;
    b.collab = Some(j);
    run(&ctx, &mut a, &mut b, 400);
    // Cut Bob's connection from Ann's side, and change things on both sides meanwhile.
    for p in &mut a.collab.as_mut().unwrap().peers {
        p.link.close();
    }
    run(&ctx, &mut a, &mut b, 100);
    let broot = b.tree.root.unwrap();
    b.tree.add(Some(broot), Object::new("WhileAway", ObjectKind::Sprite2D));
    let aroot = a.tree.root.unwrap();
    a.tree.add(Some(aroot), Object::new("AnnMeanwhile", ObjectKind::Sprite2D));
    let t0 = Instant::now();
    while b.collab.as_ref().unwrap().reconnecting() || a.tree.find_named("WhileAway").is_none() {
        assert!(t0.elapsed() < Duration::from_secs(15), "{}", b.collab.as_ref().unwrap().status);
        assert!(b.collab.as_ref().unwrap().ended.is_none());
        tick(&ctx, &mut a);
        tick(&ctx, &mut b);
        std::thread::sleep(Duration::from_millis(3));
    }
    run(&ctx, &mut a, &mut b, 300);
    assert!(b.tree.find_named("AnnMeanwhile").is_some());
    assert_eq!(flat(&a.tree), flat(&b.tree));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// The host has a folder of art mounted that the joiner doesn't: what the project names is
/// offered and arrives, and anything else read later is fetched when it's missed.
#[test]
fn mounted_assets_reach_others() {
    let tmp = std::env::temp_dir().join(format!("gex-collab-assets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let a_dir = tmp.join("a");
    crate::templates::create_demo(&a_dir).unwrap();
    let art = tmp.join("art");
    std::fs::create_dir_all(art.join("sub")).unwrap();
    std::fs::write(art.join("hero.png"), b"pretend png").unwrap();
    std::fs::write(art.join("sub/boom.wav"), b"pretend wav").unwrap();
    std::fs::write(art.join("unused.png"), b"never named").unwrap();
    std::fs::write(a_dir.join("scripts/uses_art.jlb"), "DEF on_create() {\n    SET(NULL, \"texture\", \"art://hero.png\")\n}\n").unwrap();
    let mut p = gex_core::project::Project::open(&a_dir).unwrap();
    p.config.mounts.push(gex_core::project::MountConfig { scheme: "art".into(), kind: gex_core::project::MountKind::Folder(art.clone()) });
    p.save().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let ctx = egui::Context::default();
    let mut a = EditorState::new(gex_core::project::Project::open(&a_dir).unwrap());
    a.collab = Some(Session::host(&ctx, &a, "Ann", port).unwrap());
    let code = a.collab.as_ref().unwrap().code.clone();
    let mut j = Session::join(&ctx, "Bob", &format!("127.0.0.1:{}", port), &code, &tmp.join("b"));
    wait_join(&ctx, &mut a, &mut j);
    let mut b = EditorState::new(gex_core::project::Project::open(&j.dir).unwrap());
    j.opened = true;
    b.collab = Some(j);
    // On another computer that folder isn't there.
    b.project.config.mounts.clear();
    b.remount();
    assert!(b.vfs.read("art://hero.png").is_err() || b.vfs.has_locally("art://hero.png") == false);
    run(&ctx, &mut a, &mut b, 3500);
    let cache = b.project.dir.join(gex_assets::vfs::REMOTE_DIR).join("art");
    assert_eq!(std::fs::read(cache.join("hero.png")).ok().as_deref(), Some(&b"pretend png"[..]), "what the project names arrives");
    assert!(!cache.join("unused.png").exists(), "what it doesn't name stays");
    assert_eq!(b.vfs.read("art://hero.png").unwrap(), b"pretend png");
    // Read something else (a model's texture, a sound a script builds the name of): fetched.
    assert!(b.vfs.read("art://sub/boom.wav").is_err());
    run(&ctx, &mut a, &mut b, 800);
    assert_eq!(b.vfs.read("art://sub/boom.wav").unwrap(), b"pretend wav");
    assert!(b.collab.as_ref().unwrap().fetching.is_empty(), "nothing left waiting");
    // Paths can't leave the cache.
    assert!(b.vfs.remote_path("art", "../../x").is_none() && b.vfs.remote_path("a:b", "x").is_none());
    a.collab = None;
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Joining puts the copy beside another project of the same name, never over it; joining again
/// later updates that copy: files the host removed go, files changed here meanwhile are kept
/// aside before the host's replace them, and files made here are shared.
#[test]
fn joining_again_updates_the_copy_safely() {
    let tmp = std::env::temp_dir().join(format!("gex-collab-copy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let a_dir = tmp.join("a");
    crate::templates::create_demo(&a_dir).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let ctx = egui::Context::default();
    let mut a = EditorState::new(gex_core::project::Project::open(&a_dir).unwrap());
    a.collab = Some(Session::host(&ctx, &a, "Ann", port).unwrap());
    let code = a.collab.as_ref().unwrap().code.clone();
    let name = sanitize(&a.project.config.name);
    // Bob already has a project of his own with that name.
    let base = tmp.join("b");
    std::fs::create_dir_all(base.join(&name)).unwrap();
    std::fs::write(base.join(&name).join("mine.txt"), "Bob's own").unwrap();
    let addr = format!("127.0.0.1:{}", port);
    let mut j = Session::join(&ctx, "Bob", &addr, &code, &base);
    wait_join(&ctx, &mut a, &mut j);
    assert_ne!(j.dir, base.join(&name), "not over Bob's own project");
    assert_eq!(std::fs::read_to_string(base.join(&name).join("mine.txt")).unwrap(), "Bob's own");
    assert!(!base.join(&name).join(gex_core::project::PROJECT_FILE).exists());
    let copy = j.dir.clone();
    drop(j);
    // Meanwhile: Ann removes one file and changes another; Bob changes that one too, and makes one.
    let files: Vec<String> = walk(&a_dir).into_iter().map(|f| f.0).filter(|f| f.ends_with(".jlb")).collect();
    let (gone, both, mine) = (&files[0], &files[1], &files[2]);
    std::fs::write(copy.join(mine), "// only Bob changed this\n").unwrap();
    std::fs::remove_file(a_dir.join(gone)).unwrap();
    std::fs::write(a_dir.join(both), "// Ann's\n").unwrap();
    std::fs::write(copy.join(both), "// Bob's\n").unwrap();
    std::fs::write(copy.join("scripts/new_by_bob.jlb"), "// new\n").unwrap();
    run(&ctx, &mut a, &mut EditorState::new(gex_core::project::Project::open(&a_dir).unwrap()), 1700);
    let mut j = Session::join(&ctx, "Bob", &addr, &code, &base);
    wait_join(&ctx, &mut a, &mut j);
    assert_eq!(j.dir, copy, "the same copy again");
    assert!(!copy.join(gone).exists(), "removed at the host: removed here");
    assert_eq!(std::fs::read_to_string(copy.join(both)).unwrap(), "// Ann's\n");
    let aside = PathBuf::from(format!("{} (your changes)", copy.display()));
    assert_eq!(std::fs::read_to_string(aside.join(both)).unwrap(), "// Bob's\n", "Bob's version is kept aside");
    let mut b = EditorState::new(gex_core::project::Project::open(&copy).unwrap());
    j.opened = true;
    b.collab = Some(j);
    run(&ctx, &mut a, &mut b, 2500);
    assert_eq!(std::fs::read_to_string(a_dir.join("scripts/new_by_bob.jlb")).unwrap(), "// new\n", "made by Bob: shared");
    assert_eq!(std::fs::read_to_string(a_dir.join(mine)).unwrap(), "// only Bob changed this\n", "changed only by Bob: shared");
    // A script open at Bob's (not at Ann's) is changed on Ann's computer by another program.
    let res = "res://scripts/new_by_bob.jlb";
    b.open_script(res, None);
    run(&ctx, &mut a, &mut b, 400);
    std::fs::write(a_dir.join("scripts/new_by_bob.jlb"), "// changed outside\n").unwrap();
    run(&ctx, &mut a, &mut b, 2200);
    assert_eq!(b.scripts[res].text, "// changed outside\n");
    // Ann stops sharing: Bob is told (rather than trying to get back in).
    a.collab = None;
    let t0 = Instant::now();
    while b.collab.as_ref().unwrap().ended.is_none() {
        assert!(t0.elapsed() < Duration::from_secs(5), "{}", b.collab.as_ref().unwrap().status);
        tick(&ctx, &mut b);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(b.collab.as_ref().unwrap().ended.as_deref().unwrap().contains("stopped sharing"));
    // Joining from the computer that shares it: a copy beside it, not onto itself.
    a.collab = Some(Session::host(&ctx, &a, "Ann", port).unwrap());
    let code = a.collab.as_ref().unwrap().code.clone();
    let mut me = Session::join(&ctx, "Ann again", &addr, &code, &tmp);
    wait_join(&ctx, &mut a, &mut me);
    assert_ne!(me.dir, a_dir);
    drop(me);
    a.collab = None;
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Mounted paths found in text, and paths that would leave a mount.
#[test]
fn mounted_paths_are_found_and_kept_inside() {
    let mut r = Vec::new();
    find_refs(r#"SET(NULL, "model", "hl2://models/a.mdl") x art://b/c.png, res://x.png https://e.com gex://y"#, &mut r);
    assert_eq!(r, vec!["hl2://models/a.mdl".to_string(), "art://b/c.png".to_string()]);
    let mut r = Vec::new();
    variant_refs(&Variant::List(vec![Variant::Res("hl2://m.mdl".into()), Variant::Num(1.0)]), &mut r);
    assert_eq!(r, vec!["hl2://m.mdl".to_string()]);
    let tmp = std::env::temp_dir().join(format!("gex-collab-mnt-{}", std::process::id()));
    std::fs::create_dir_all(tmp.join("hl2")).unwrap();
    let mut v = gex_assets::Vfs::new(&tmp, &Default::default());
    v.mounts.clear();
    let cfg = gex_core::project::ProjectConfig { mounts: vec![gex_core::project::MountConfig { scheme: "art".into(), kind: gex_core::project::MountKind::Folder(tmp.join("hl2")) }], ..Default::default() };
    let v2 = gex_assets::Vfs::new(&tmp, &cfg);
    for bad in ["art://../x", "art://C:/Windows/win.ini", "art://\\\\server\\x", "art://a\\..\\..\\x"] {
        assert!(v2.folder_file(bad).is_none() && !v2.has_locally(bad), "{}", bad);
    }
    // Others' folder mounts are never taken on (they'd let them read any folder named here).
    let incoming = ron::ser::to_string(&cfg).unwrap();
    let here = tmp.join("p.jproj");
    let merged: gex_core::project::ProjectConfig = ron::from_str(&String::from_utf8(merge_project_file(&here, incoming.as_bytes())).unwrap()).unwrap();
    assert!(merged.mounts.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
    drop(v);
}
