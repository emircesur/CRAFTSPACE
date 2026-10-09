//! CraftSpace: an installer and update manager for the open-source ArtCraft creative apps.

// No console window behind the app in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod dock;
mod renderer;
mod theme;
mod tray;
mod views;
mod worker;

use craftspace_core::Manager;
use eframe::egui;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let manager = match Manager::open() {
        Ok(m) => m,
        Err(err) => {
            rfd::MessageDialog::new()
                .set_title("CraftSpace")
                .set_description(format!("CraftSpace couldn't start: {err:#}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::exit(1);
        }
    };

    // `craftspace uninstall <app> [--yes]`: what Windows' Settings › Apps runs.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("uninstall") {
        std::process::exit(headless_uninstall(&manager, &args[1..]));
    }

    // `craftspace open <file>`: what double-clicking a file runs when CraftSpace opens its type.
    let mut open_request = None;
    if args.first().map(String::as_str) == Some(craftspace_core::file_types::OPEN_COMMAND) {
        let Some(file) = args.get(1).map(std::path::PathBuf::from) else { std::process::exit(2) };
        match manager.open_file(&file) {
            Ok(None) => std::process::exit(0),
            Ok(Some(app_id)) => open_request = Some((app_id, file)),
            Err(err) => {
                rfd::MessageDialog::new()
                    .set_title("CraftSpace")
                    .set_description(format!("Couldn't open {}: {err:#}", file.display()))
                    .set_level(rfd::MessageLevel::Error)
                    .show();
                std::process::exit(1);
            }
        }
    }

    // `--background`: started at login; stay in the tray until opened.
    let background = args.iter().any(|a| a == craftspace_core::autostart::BACKGROUND_FLAG);
    let renderer = renderer::choose(manager.paths());
    let viewport = egui::ViewportBuilder::default()
        .with_visible(!background)
        .with_title("CraftSpace")
        .with_app_id("craftspace")
        .with_inner_size([1240.0, 800.0])
        .with_min_inner_size([760.0, 520.0]);
    // On macOS the Dock icon is the app bundle's, or the full-colour one while the window is open
    // (see `dock`).
    #[cfg(not(target_os = "macos"))]
    let viewport = viewport.with_icon(
        eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/craftspace-256.png"))
            .expect("bundled icon is a valid PNG"),
    );
    let options = eframe::NativeOptions { viewport, renderer, ..Default::default() };
    let paths = manager.paths().clone();
    let result = eframe::run_native(
        "CraftSpace",
        options,
        Box::new(move |cc| Ok(Box::new(app::CraftSpaceApp::new(cc, manager, background, open_request)))),
    );
    if let Err(err) = &result {
        renderer::fall_back(&paths, renderer, err);
    }
    result
}

fn headless_uninstall(manager: &Manager, args: &[String]) -> i32 {
    let Some(id) = args.iter().find(|a| !a.starts_with('-')) else { return 2 };
    let quiet = args.iter().any(|a| a == "--yes" || a == "-y");
    let name = manager.app(id).map(|a| a.name).unwrap_or_else(|| id.clone());
    if !quiet {
        let answer = rfd::MessageDialog::new()
            .set_title("CraftSpace")
            .set_description(format!("Uninstall {name}? Your documents and preferences are kept."))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        if answer != rfd::MessageDialogResult::Yes {
            return 1;
        }
    }
    match manager.uninstall(id) {
        Ok(()) => 0,
        Err(err) => {
            if !quiet {
                rfd::MessageDialog::new()
                    .set_title("CraftSpace")
                    .set_description(format!("Couldn't uninstall {name}: {err:#}"))
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }
            1
        }
    }
}
