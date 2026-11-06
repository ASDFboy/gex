//! Plays a scene without a window and prints what its scripts logged: a quick check that a
//! game runs without script errors.
//!
//!   cargo run -p gex-script --example run_scene -- <project dir> <res://scene> <seconds> [keys]
//!
//! keys: comma-separated `name@from-to` (seconds), e.g. `space@0.5-0.6,right@1-3`.

use std::collections::HashSet;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let project = gex_core::project::Project::open(std::path::Path::new(&a[0])).expect("project");
    let secs: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let keys: Vec<(String, f64, f64)> = a
        .get(3)
        .map(|s| {
            s.split(',')
                .filter_map(|k| {
                    let (name, range) = k.split_once('@')?;
                    let (f, t) = range.split_once('-')?;
                    Some((name.to_string(), f.parse().ok()?, t.parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut g = gex_script::world::Game::from_scene(project, &a[1], None).expect("scene");
    let dt = 1.0 / 60.0;
    let mut t = 0.0;
    let mut errors = 0;
    while t < secs {
        let down: HashSet<String> = keys.iter().filter(|(_, f, to)| t >= *f && t < *to).map(|(k, _, _)| k.clone()).collect();
        g.frame(dt, down, [640.0, 360.0], HashSet::new());
        for l in g.take_logs() {
            if format!("{:?}", l.kind).contains("Error") {
                errors += 1;
            }
            println!("[{:.2}] {:?}: {}", t, l.kind, l.text);
        }
        t += dt;
    }
    let tree = g.tree();
    println!("--- {} nodes after {:.1}s, {} errors", tree.walk().len(), secs, errors);
}
