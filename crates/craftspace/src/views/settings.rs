//! The Settings window. Edits a draft copy that is saved on "Save". Settings a machine policy
//! sets are shown but can't be changed.

use craftspace_core::platform::Os;
use craftspace_core::settings::Theme;
use eframe::egui::{self, Align, Layout, RichText, Ui};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

pub fn window(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    let Some(mut draft) = app.settings_draft.take() else { return };
    let p = app.palette;
    let mut keep_open = true;
    let mut save = false;
    let os = app.manager.platform().os;
    let policy = app.manager.policy().clone();
    let free = |key: &str| !policy.locks(key);

    let modal = egui::Modal::new(egui::Id::new("settings")).show(ctx, |ui| {
        ui.set_width(580.0);
        ui.heading("Settings");
        if policy.is_active() {
            ui.add_space(4.0);
            theme::chip(ui, "Some settings are managed by your organization", p.warn);
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 220.0).show(ui, |ui| {
            section(ui, &p, "Updates");
            ui.add_enabled(free("auto_check_updates"), egui::Checkbox::new(&mut draft.auto_check_updates, "Check for updates automatically"));
            ui.add_enabled_ui(draft.auto_check_updates, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label("Every");
                    ui.add_enabled(free("check_interval_hours"), egui::DragValue::new(&mut draft.check_interval_hours).range(1..=168).suffix(" h"));
                });
                ui.add_enabled(free("auto_install_updates"), egui::Checkbox::new(&mut draft.auto_install_updates, "Install updates as soon as they're found (open apps update when you close them)"));
            });
            ui.add_enabled(free("include_prereleases"), egui::Checkbox::new(&mut draft.include_prereleases, "Offer pre-releases (release candidates)"));
            ui.label(RichText::new("Each app can follow its own channel or stay on a version: see its Overview page.").size(12.0).color(p.weak));
            ui.add_enabled(free("keep_previous_version"), egui::Checkbox::new(&mut draft.keep_previous_version, "Keep the previous version after updating, so it can be rolled back"));
            ui.horizontal(|ui| {
                ui.add_enabled(free("notifications"), egui::Checkbox::new(&mut draft.notifications, "Show notifications when updates are found or installed"));
                if ui.small_button("Send a test").clicked() {
                    app.actions.push(Action::TestNotification);
                }
            });

            section(ui, &p, "Downloads");
            ui.horizontal(|ui| {
                ui.label("Download and install");
                ui.add_enabled(free("max_parallel_downloads"), egui::DragValue::new(&mut draft.max_parallel_downloads).range(1..=6));
                ui.label("apps at a time");
            });
            ui.horizontal(|ui| {
                let mut limited = draft.download_limit_kbps.is_some();
                ui.add_enabled(free("download_limit_kbps"), egui::Checkbox::new(&mut limited, "Limit download speed to"));
                let mut kbps = draft.download_limit_kbps.unwrap_or(2048);
                ui.add_enabled(limited && free("download_limit_kbps"), egui::DragValue::new(&mut kbps).range(64..=1_000_000).suffix(" KB/s").speed(16));
                draft.download_limit_kbps = limited.then_some(kbps);
            });
            ui.label(RichText::new("Interrupted downloads continue where they stopped.").size(12.0).color(p.weak));
            ui.add_enabled(free("allow_unverified_downloads"), egui::Checkbox::new(&mut draft.allow_unverified_downloads, "Allow packages that publish no checksum (not recommended)"));

            section(ui, &p, "Installation");
            ui.label("Install apps in");
            ui.horizontal(|ui| {
                let current = draft.install_dir.clone().unwrap_or_else(|| app.manager.paths().apps.clone());
                ui.add(egui::Label::new(RichText::new(current.display().to_string()).monospace()).truncate());
                ui.add_enabled_ui(free("install_dir"), |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if draft.install_dir.is_some() && ui.button("Default").clicked() {
                            draft.install_dir = None;
                        }
                        if ui.button("Change…").clicked() {
                            if let Some(dir) = rfd::FileDialog::new().set_title("Install ArtCraft apps in").pick_folder() {
                                draft.install_dir = Some(dir);
                            }
                        }
                    });
                });
            });
            ui.label(
                RichText::new("Apps already installed stay where they are. To put one app somewhere else, use \"Install in a folder…\" in its ⋯ menu.")
                    .size(12.0)
                    .color(p.weak),
            );
            if os == Os::Linux && draft.prefer_system_installer && craftspace_core::platform::linux_package_format().is_some() {
                ui.label(
                    RichText::new("System packages go where the package manager puts them (/usr). Turn off \"Install apps as system packages\" below to install into this folder instead.")
                        .size(12.0)
                        .color(p.warn),
                );
            }
            if os != Os::Macos {
                ui.add_enabled(free("desktop_shortcuts"), egui::Checkbox::new(&mut draft.desktop_shortcuts, "Add desktop shortcuts"));
            }
            match os {
                Os::Windows => {
                    ui.add_enabled(free("prefer_system_installer"), egui::Checkbox::new(&mut draft.prefer_system_installer, "Use the Windows Installer (MSI) instead of the portable build"))
                        .on_hover_text("MSI installs go to Program Files and may ask for administrator rights. Portable installs are per-user and can be rolled back.");
                }
                Os::Linux => {
                    if let Some(format) = craftspace_core::platform::linux_package_format() {
                        let (ext, tool) = if format == craftspace_core::platform::AssetKind::Rpm { (".rpm", "dnf") } else { (".deb", "apt") };
                        ui.add_enabled(free("prefer_system_installer"), egui::Checkbox::new(&mut draft.prefer_system_installer, format!("Install apps as system packages ({ext} through {tool})")))
                            .on_hover_text("The package manager owns the files; CraftSpace still finds and installs updates. Installing asks for your password.");
                    }
                    ui.add_enabled(free("prefer_appimage"), egui::Checkbox::new(&mut draft.prefer_appimage, "Install AppImages, so updates only download what changed"))
                        .on_hover_text("Uses the .zsync files published with each release. How much is saved depends on how much changed between versions.");
                }
                _ => {}
            }

            section(ui, &p, "Files");
            let f = &app.settings.files;
            let mut on: Vec<&str> = Vec::new();
            if f.app_recents {
                on.push("apps' recent files");
            }
            if f.thumbnails {
                on.push("thumbnails");
            }
            if f.watch {
                on.push("live updates");
            }
            if f.find_portable {
                on.push("portable settings import");
            }
            let types = f.open_with_craftspace.len();
            if types > 0 {
                on.push("opening file types");
            }
            ui.label(if on.is_empty() { "All optional Files features are off.".to_string() } else { format!("On: {}.", on.join(", ")) });
            if types > 0 {
                ui.label(RichText::new(format!("CraftSpace opens {types} file type{}.", if types == 1 { "" } else { "s" })).size(12.0).color(p.weak));
            }
            if ui.button("Choose Files features…").clicked() {
                app.actions.push(Action::OpenFilesSetup);
            }

            section(ui, &p, "When you close the window");
            ui.add_enabled(free("keep_running_in_tray"), egui::Checkbox::new(&mut draft.keep_running_in_tray, "Keep CraftSpace running in the tray to check for updates"));
            ui.add_enabled(free("start_at_login"), egui::Checkbox::new(&mut draft.start_at_login, "Start CraftSpace in the background when I log in"));

            section(ui, &p, "Appearance");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut draft.theme, Theme::Dark, "Dark");
                ui.selectable_value(&mut draft.theme, Theme::Light, "Light");
                ui.selectable_value(&mut draft.theme, Theme::System, "Match system");
            });

            section(ui, &p, "Your apps");
            ui.label(RichText::new("Save the list of installed apps (with versions and channels) to set up another computer the same way.").color(p.weak));
            ui.horizontal(|ui| {
                if ui.button("Export app list…").clicked() {
                    app.actions.push(Action::ExportList);
                }
                if ui.button("Import app list…").clicked() {
                    app.actions.push(Action::ImportList);
                }
            });

            section(ui, &p, "GitHub");
            ui.label("Release information comes from GitHub. Without a token, GitHub allows 60 requests an hour; CraftSpace caches and falls back to direct downloads when the limit is hit. A token with no scopes raises the limit.");
            let mut token = draft.github_token.clone().unwrap_or_default();
            ui.add_enabled(
                free("github_token"),
                egui::TextEdit::singleline(&mut token).password(true).hint_text("Personal access token (optional)").desired_width(f32::INFINITY),
            );
            draft.github_token = (!token.trim().is_empty()).then(|| token.trim().to_string());
            ui.add_enabled(free("remote_catalog"), egui::Checkbox::new(&mut draft.remote_catalog, "Get new apps from the online catalog"));

            section(ui, &p, "About");
            ui.label(format!("CraftSpace {} on {}", crate::app::version_string(), app.manager.platform().display()));
            if let Some(src) = &policy.source {
                ui.label(RichText::new(format!("Policy: {}", src.display())).size(12.0).color(p.weak));
            }
            ui.label(RichText::new("An independent, open-source installer for the ArtCraft apps. Not affiliated with Adobe or Microsoft.").size(12.5).color(p.weak));
            ui.horizontal(|ui| {
                if ui.button("Open data folder").clicked() {
                    app.actions.push(Action::OpenPath(app.manager.paths().root.clone()));
                }
                if ui.button("Clear downloads").clicked() {
                    app.actions.push(Action::Cleanup);
                }
                if ui.button("Source code ↗").clicked() {
                    app.actions.push(Action::OpenUrl("https://github.com/emircesur/craftspace".into()));
                }
            });
        });
        ui.add_space(12.0);
        ui.separator();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::primary(ui, &p, "Save").clicked() {
                save = true;
                keep_open = false;
            }
            if theme::pill(ui, &p, "Cancel").clicked() {
                keep_open = false;
            }
        });
    });
    if modal.should_close() {
        keep_open = false;
    }
    if save {
        app.actions.push(Action::SaveSettings(Box::new(draft.clone())));
    }
    if keep_open {
        app.settings_draft = Some(draft);
    }
}

fn section(ui: &mut Ui, p: &crate::theme::Palette, title: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).size(15.0).strong().color(p.text));
    ui.add_space(2.0);
}
