//! jlb-engine: the editor, or (with --play) just the game.
//!
//!   jlb-engine                  the project library
//!   jlb-engine <project folder> open a project
//!   jlb-engine --play <folder>  run a project's main scene

// No console window behind the editor on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod code;
mod panels;
mod state;
mod templates;
mod theme;
mod viewport;

use std::path::PathBuf;

use eframe::egui;

fn options(title: &str, size: [f32; 2]) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(title).with_inner_size(size).with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        // Only redraw when something changes (or a game is running), so an idle editor
        // costs next to nothing.
        run_and_return: false,
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a.as_str()) == Some("--play") {
        let dir = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| ".".into()));
        let project = match jle_core::project::Project::open(&dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        };
        let [w, h] = project.config.window_size;
        let title = project.config.name.clone();
        return eframe::run_native(
            &title,
            options(&title, [w as f32, h as f32]),
            Box::new(move |_cc| app::PlayerApp::new(project).map(|a| Box::new(a) as Box<dyn eframe::App>).map_err(|e| e.into())),
        );
    }
    let project = args.first().map(PathBuf::from);
    eframe::run_native("JLB Engine", options("JLB Engine", [1500.0, 900.0]), Box::new(move |cc| Ok(Box::new(app::EditorApp::new(cc, project)))))
}
