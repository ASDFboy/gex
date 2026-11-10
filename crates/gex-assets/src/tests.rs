use gex_core::project::{MountConfig, MountKind, ProjectConfig};

use crate::{load_image, vpk, vtf, Vfs};

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gex-assets-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A fake Source install: loose files plus a VPK, laid out like Half-Life 2's.
#[test]
fn source_game_mount_reads_vpks_and_loose_files() {
    let root = tmp("src");
    let install = root.join("Half-Life 2");
    std::fs::create_dir_all(install.join("hl2/materials/loose")).unwrap();
    std::fs::create_dir_all(install.join("ep2")).unwrap();
    // 2x2 BGRA texture: red, green / blue, white.
    let bgra = [0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255];
    let tex = vtf::write_simple(2, 2, vtf::BGRA8888, &bgra);
    let vmt = b"\"LightmappedGeneric\"\n{\n  // a comment\n  \"$basetexture\" \"Brick\\Wall01\"\n  \"$surfaceprop\" \"brick\"\n}\n";
    vpk::write_simple(
        &install.join("hl2/hl2_misc_dir.vpk"),
        &[("materials/brick/wall01.vtf", &tex), ("materials/brick/wall01.vmt", vmt), ("sound/a.wav", b"RIFF")],
    )
    .unwrap();
    std::fs::write(install.join("hl2/materials/loose/thing.txt"), "hi").unwrap();
    std::fs::write(install.join("ep2/only_ep2.txt"), "ep2").unwrap();

    let proj = tmp("proj");
    let cfg = ProjectConfig {
        mounts: vec![MountConfig { scheme: "hl2".into(), kind: MountKind::SourceGame { install: install.clone(), games: vec![] } }],
        ..Default::default()
    };
    let vfs = Vfs::new(&proj, &cfg);
    assert!(vfs.warnings().is_empty(), "{:?}", vfs.warnings());
    assert_eq!(vfs.read_text("hl2://materials/loose/thing.txt").unwrap(), "hi");
    assert_eq!(vfs.read_text("hl2://only_ep2.txt").unwrap(), "ep2");
    assert_eq!(vfs.read("hl2://sound/a.wav").unwrap(), b"RIFF");
    assert!(vfs.read("hl2://../secret").is_err());

    let top: Vec<_> = vfs.list("hl2://").into_iter().map(|e| (e.name, e.is_dir)).collect();
    assert!(top.contains(&("materials".into(), true)) && top.contains(&("sound".into(), true)), "{:?}", top);
    let brick: Vec<_> = vfs.list("hl2://materials/brick").into_iter().map(|e| e.path).collect();
    assert_eq!(brick, ["hl2://materials/brick/wall01.vmt", "hl2://materials/brick/wall01.vtf"]);

    // A material resolves to its texture.
    let img = load_image(&vfs, "hl2://materials/brick/wall01.vmt").unwrap();
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(&img.pixels[0..4], &[255, 0, 0, 255]);
    assert_eq!(&img.pixels[8..12], &[0, 0, 255, 255]);
    assert!(!vfs.redistributable("hl2://materials/brick/wall01.vtf"));
}

#[test]
fn dxt1_decodes() {
    // One 4x4 block: color0 = pure red (0xF800), color1 = pure blue (0x001F), all pixels index 0
    // except the first row, which uses index 1.
    let mut block = Vec::new();
    block.extend_from_slice(&0xF800u16.to_le_bytes());
    block.extend_from_slice(&0x001Fu16.to_le_bytes());
    block.extend_from_slice(&0x0000_0055u32.to_le_bytes());
    let file = vtf::write_simple(4, 4, vtf::DXT1, &block);
    let img = vtf::decode(&file).unwrap();
    assert_eq!(&img.pixels[0..4], &[0, 0, 255, 255], "row 0 is color1");
    assert_eq!(&img.pixels[16..20], &[255, 0, 0, 255], "row 1 is color0");
}

#[test]
fn project_files_and_images() {
    let proj = tmp("res");
    std::fs::create_dir_all(proj.join("sprites")).unwrap();
    let img = image::RgbaImage::from_pixel(3, 1, image::Rgba([1, 2, 3, 4]));
    img.save(proj.join("sprites/a.png")).unwrap();
    let vfs = Vfs::new(&proj, &ProjectConfig::default());
    let i = load_image(&vfs, "res://sprites/a.png").unwrap();
    assert_eq!((i.width, i.height, &i.pixels[0..4]), (3, 1, &[1u8, 2, 3, 4][..]));
    assert_eq!(vfs.list("res://").len(), 1);
    assert!(vfs.read("res://../x").is_err());
}

#[test]
fn builtin_pack_is_readable() {
    let dir = std::env::temp_dir().join(format!("gex-builtin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let vfs = crate::Vfs::new(&dir, &gex_core::project::ProjectConfig::default());
    assert!(vfs.roots().iter().any(|r| r.path == "gex://"));
    let top: Vec<String> = vfs.list("gex://").into_iter().map(|e| e.name).collect();
    for f in ["shapes", "patterns", "colors", "ui", "icons", "characters", "particles", "skies"] {
        assert!(top.iter().any(|t| t == f), "{} missing from {:?}", f, top);
    }
    let img = crate::load_image(&vfs, "gex://shapes/white/star.png").unwrap();
    assert_eq!((img.width, img.height), (128, 128));
    assert!(vfs.list("gex://shapes").iter().any(|e| e.is_dir && e.name == "red"));
    assert!(vfs.list("gex://shapes/red").iter().any(|e| !e.is_dir && e.path == "gex://shapes/red/circle.png"));
    assert!(vfs.read("gex://nope.png").is_err());
    assert!(vfs.redistributable("gex://icons/coin.png"));
    // Every built-in picture loads.
    for (p, _) in crate::builtin::FILES.iter().filter(|(p, _)| p.ends_with(".png")) {
        crate::load_image(&vfs, &format!("gex://{}", p)).unwrap_or_else(|e| panic!("{}: {}", p, e));
    }
}
