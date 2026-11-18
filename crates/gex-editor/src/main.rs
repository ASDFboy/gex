//! gex: the editor, or (with --play) just the game.
//!
//!   gex                  the project library
//!   gex <project folder> open a project
//!   gex --play <folder>  run a project's main blueprint

// No console window behind the editor on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod code;
mod export;
mod files;
mod gamepad;
mod panels;
mod preview;
mod startup;
mod state;
mod templates;
mod text;
mod theme;
mod timeline;
mod tools;
mod ui_layer;
mod vim;
mod gizmo;
mod rex;
mod launcher;
mod docs;
mod icons;
mod viewport;
mod workspace;
mod sprites;

use std::path::PathBuf;

use eframe::egui;

fn options(title: &str, size: [f32; 2]) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(title).with_inner_size(size).with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        // Come back from run_native on errors (instead of eframe quietly exiting), so
        // startup::run can try another graphics API or say what went wrong.
        run_and_return: true,
        ..Default::default()
    }
}

/// The project's icon picture, for the window.
fn window_icon(p: &gex_core::project::Project) -> Option<egui::IconData> {
    if p.config.icon.is_empty() {
        return None;
    }
    let vfs = gex_assets::Vfs::new(&p.dir, &p.config);
    let img = gex_assets::load_image(&vfs, &p.config.icon).ok()?;
    Some(egui::IconData { rgba: img.pixels, width: img.width, height: img.height })
}

fn main() -> std::process::ExitCode {
    startup::init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // An exported game: the program sits next to a "game" folder; play it.
    if args.is_empty() {
        if let Some(dir) = export::exported_game_dir() {
            args = vec!["--play".into(), dir.to_string_lossy().into_owned()];
        }
    }
    if args.first().map(|a| a.as_str()) == Some("--play") {
        let dir = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| ".".into()));
        let mut project = match gex_core::project::Project::open(&dir) {
            Ok(p) => p,
            Err(e) => {
                log::error!(target: "gex", "{}", e);
                return std::process::ExitCode::FAILURE;
            }
        };
        // Display settings the game kept with SAVE_DISPLAY win over the project's.
        if let Some(d) = gex_script::extras::saved_display(&project.config.name) {
            project.config.display = d;
        }
        let title = project.config.name.clone();
        let d = &project.config.display;
        let mut opts = options(&title, [d.width as f32, d.height as f32]);
        if !d.vsync {
            opts.wgpu_options.surface.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
        }
        // The real size (in screen pixels, inside the screen) is set on the first frame:
        // the screen's scaling isn't known yet. A full-screen window gets no size at all, or
        // the OS shrinks it back to that size.
        let mut vp = egui::ViewportBuilder::default().with_title(&title).with_min_inner_size([160.0, 90.0]).with_resizable(d.resizable).with_fullscreen(d.fullscreen);
        if !d.fullscreen {
            vp = vp.with_inner_size([d.width as f32 * d.window_scale, d.height as f32 * d.window_scale]).with_clamp_size_to_monitor_size(true);
        }
        opts.viewport = vp;
        if let Some(icon) = window_icon(&project) {
            opts.viewport = opts.viewport.with_icon(icon);
        }
        return startup::run(eframe::run_native(
            &title,
            opts,
            Box::new(move |cc| {
                cc.egui_ctx.set_theme(egui::Theme::Dark);
                app::PlayerApp::new(project).map(|a| Box::new(a) as Box<dyn eframe::App>).map_err(|e| e.into())
            }),
        ));
    }
    let project = args.first().map(PathBuf::from);
    // The editor draws its own title bar buttons in the menu bar (more room for everything else).
    let mut opts = options("GEX", [1500.0, 900.0]);
    opts.viewport = opts.viewport.with_decorations(false);
    startup::run(eframe::run_native("GEX", opts, Box::new(move |cc| Ok(Box::new(app::EditorApp::new(cc, project))))))
}
