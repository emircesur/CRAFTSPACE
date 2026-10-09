//! The Settings window. Edits a draft copy that is saved on "Save". Settings a machine policy
//! sets are shown but can't be changed.

use craftspace_core::platform::Os;
use craftspace_core::policy::Policy;
use craftspace_core::settings::{Settings, Theme};
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
            let org = policy.organization.as_deref().unwrap_or("your organization");
            if policy.lock_settings {
                theme::chip(ui, &format!("Settings are managed by {org} and can't be changed here"), p.warn);
            } else {
                theme::chip(ui, &format!("Some settings are managed by {org}"), p.warn);
            }
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
            ui.add_enabled(free("auto_update_self"), egui::Checkbox::new(&mut draft.auto_update_self, "Update CraftSpace itself automatically (the new version runs from the next start)"));
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
            ui.add_enabled(free("detect_installed"), egui::Checkbox::new(&mut draft.detect_installed, "Find ArtCraft apps installed without CraftSpace, and update them where they are"))
                .on_hover_text("Apps from their own installers, disk images, packages or Flatpak, and portable copies in your usual folders.");
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
                RichText::new("Apps already installed stay where they are. To put one app somewhere else, use \"Install in a folder…\" in its ••• menu.")
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
                    if ui
                        .add_enabled(free("prefer_system_installer"), egui::Checkbox::new(&mut draft.prefer_system_installer, "Use each app's installer (MSI or Setup) instead of the portable build"))
                        .on_hover_text("Installers put apps in Program Files and may ask for administrator rights. Portable installs are per-user and can be rolled back.")
                        .changed()
                    {
                        draft.install_mode_chosen = true;
                    }
                }
                Os::Linux => {
                    if let Some(format) = craftspace_core::platform::linux_package_format() {
                        let (ext, tool) = if format == craftspace_core::platform::AssetKind::Rpm { (".rpm", "dnf") } else { (".deb", "apt") };
                        if ui
                            .add_enabled(free("prefer_system_installer"), egui::Checkbox::new(&mut draft.prefer_system_installer, format!("Install apps as system packages ({ext} through {tool})")))
                            .on_hover_text("The package manager owns the files; CraftSpace still finds and installs updates. Installing asks for your password.")
                            .changed()
                        {
                            draft.install_mode_chosen = true;
                        }
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
            if os == Os::Macos {
                use craftspace_core::settings::DockIcon;
                ui.add_space(4.0);
                ui.label("Dock icon");
                ui.radio_value(&mut draft.dock_icon, DockIcon::ColorWhenOpen, "Full colour while the window is open, macOS's icon style otherwise");
                ui.radio_value(&mut draft.dock_icon, DockIcon::Color, "Always full colour");
                ui.radio_value(&mut draft.dock_icon, DockIcon::System, "Always macOS's icon style (dark, clear or tinted, like the ArtCraft apps)");
                ui.label(RichText::new("The icon style is chosen in System Settings › Appearance.").size(12.0).color(p.weak));
            }

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

            section(ui, &p, "IT & Classroom");
            it_section(app, ui, &p, &policy, &mut draft);

            section(ui, &p, "Add-on repositories");
            ui.label(RichText::new("Besides the CraftSpace registry, add-ons can come from repositories such as the ArtCraft Store. CraftSpace hasn't checked their add-ons for security; it asks before installing one.").size(12.0).color(p.weak));
            for (store, _) in app.manager.addon_stores() {
                let mut enabled = !draft.addon_stores.disabled.contains(&store.id);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut enabled, RichText::new(&store.name).strong()).changed() {
                        if enabled {
                            draft.addon_stores.disabled.retain(|s| *s != store.id);
                        } else if !draft.addon_stores.disabled.contains(&store.id) {
                            draft.addon_stores.disabled.push(store.id.clone());
                        }
                    }
                    if let Some(repo) = &store.repo {
                        ui.label(RichText::new(format!("github.com/{repo}")).size(12.0).color(p.weak));
                    }
                    if draft.addon_stores.custom.iter().any(|s| s.id == store.id) && ui.small_button("Remove").clicked() {
                        draft.addon_stores.custom.retain(|s| s.id != store.id);
                    }
                });
            }
            if ui.button("Add a repository…").on_hover_text("Saves these settings, then opens Fonts & add-ons › Repositories").clicked() {
                app.apps_view = crate::app::AppsView::Addons;
                app.addon_repos_tab = true;
                app.tab = crate::app::Tab::Apps;
                save = true;
                keep_open = false;
            }

            section(ui, &p, "Other sources (advanced)");
            ui.add_enabled(
                free("other_sources"),
                egui::Checkbox::new(&mut draft.other_sources.enabled, "Let CraftSpace install and update apps from other GitHub repositories"),
            );
            ui.label(
                RichText::new("Off, CraftSpace manages the ArtCraft apps only. On, you can add any repository's app, or update an ArtCraft app or CraftSpace itself from a fork, a mirror or a backup.")
                    .size(12.0)
                    .color(p.weak),
            );
            if app.settings.other_sources.enabled && draft.other_sources.enabled {
                let other = app.settings.other_sources.clone();
                for custom in &other.apps {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&custom.name).strong());
                        ui.label(RichText::new(format!("github.com/{}", custom.repo)).size(12.0).color(p.weak));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let installed = app.manager.installed_app(&custom.id).is_some();
                            if ui
                                .add_enabled(!installed, egui::Button::new("Remove").small())
                                .on_disabled_hover_text("Uninstall it first")
                                .clicked()
                            {
                                app.actions.push(Action::Source(crate::worker::SourceOp::Remove { id: custom.id.clone() }));
                            }
                        });
                    });
                }
                for (id, repo) in &other.overrides {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} updates from github.com/{repo}", app.app_name(id)));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(egui::Button::new("Change…").small()).clicked() {
                                app.actions.push(Action::OpenSource(crate::views::sources::Target::App(id.clone())));
                            }
                        });
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(format!("CraftSpace updates from github.com/{}", craftspace_core::selfupdate::update_repo(&app.manager)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new("Change…").small()).clicked() {
                            app.actions.push(Action::OpenSource(crate::views::sources::Target::App(
                                craftspace_core::selfupdate::NAME.into(),
                            )));
                        }
                    });
                });
                if ui.button("Add an app from GitHub…").clicked() {
                    app.actions.push(Action::OpenSource(crate::views::sources::Target::NewApp));
                }
                ui.label(RichText::new("To update an ArtCraft app from a fork, use \"Update source…\" in its ••• menu.").size(12.0).color(p.weak));
            } else if draft.other_sources.enabled {
                ui.label(RichText::new("Save to turn them on, then add apps here.").size(12.0).color(p.warn));
            }

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

/// What the organization's policy sets, and the tools for computer labs: a shared package
/// cache, status reports.
fn it_section(app: &mut CraftSpaceApp, ui: &mut Ui, p: &theme::Palette, policy: &Policy, draft: &mut Settings) {
    let weak = |t: String| RichText::new(t).size(12.0).color(p.weak);
    match &policy.source {
        Some(src) => {
            let org = policy.organization.as_deref().unwrap_or("An organization");
            ui.label(format!("{org} manages CraftSpace on this computer."));
            ui.label(weak(format!("Policy file: {}", src.display())));
            if let Some(url) = &policy.policy_url {
                ui.label(weak(format!("Updated from: {url}")));
            }
            let mut facts = Vec::new();
            if let Some(w) = &policy.update_window {
                facts.push(format!("Updates install {}.", w.describe()));
            }
            if !policy.required_apps.is_empty() {
                facts.push(format!("Always installed: {}.", names(app, &policy.required_apps)));
            }
            if !policy.pinned_versions.is_empty() {
                let pins: Vec<String> =
                    policy.pinned_versions.iter().map(|(id, v)| format!("{} {v}", app.app_name(id))).collect();
                facts.push(format!("Kept on: {}.", pins.join(", ")));
            }
            if !policy.blocked_apps.is_empty() {
                facts.push(format!("Not available: {}.", names(app, &policy.blocked_apps)));
            }
            if policy.prevent_uninstall {
                facts.push("Apps can't be uninstalled or rolled back (except by an administrator).".into());
            }
            for (id, profile) in &policy.profiles {
                let when = match profile.apply {
                    craftspace_core::policy::ProfileApply::Once => "when it changes",
                    craftspace_core::policy::ProfileApply::EveryStart => "at every start",
                };
                facts.push(format!("{} gets the organization's setup {when}.", app.app_name(id)));
            }
            if let Some(dir) = &policy.report_dir {
                facts.push(format!("A status report is saved to {} after each check.", dir.display()));
            }
            for f in facts {
                ui.label(f);
            }
            if let Some(support) = &policy.support {
                ui.label(format!("Help: {support}"));
            }
        }
        None => {
            ui.label(weak(
                "Not managed. For labs and classrooms, a policy file can install apps on every computer, keep them on a version, \
                 limit updates to after-school hours and lock settings: run `craftspace-cli policy show` for where it goes, \
                 and see \"Managing many computers\" in the README."
                    .into(),
            ));
        }
    }
    ui.add_space(4.0);
    ui.label("Package cache (a shared folder, so a room of computers downloads each app once)");
    let locked = policy.locks("package_cache");
    ui.horizontal(|ui| {
        let text = draft.package_cache.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "Not used".into());
        ui.add(egui::Label::new(RichText::new(text).monospace()).truncate());
        ui.add_enabled_ui(!locked, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if draft.package_cache.is_some() && ui.button("Don't use").clicked() {
                    draft.package_cache = None;
                }
                if ui.button("Choose…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().set_title("Package cache folder").pick_folder() {
                        draft.package_cache = Some(dir);
                    }
                }
            });
        });
    });
    if draft.package_cache.is_some() {
        ui.add_enabled(
            !policy.locks("package_cache_write"),
            egui::Checkbox::new(&mut draft.package_cache_write, "Also put what this computer downloads into the cache"),
        );
        ui.label(weak("Packages are checked against their published checksums before they're used.".into()));
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Save a status report…")
            .on_hover_text("This computer, CraftSpace and every app's version, as JSON")
            .clicked()
        {
            app.actions.push(Action::SaveReport);
        }
        let cache_set = app.settings.package_cache.is_some();
        if ui
            .add_enabled(cache_set, egui::Button::new("Fill the package cache now"))
            .on_hover_text("Download the current version of the installed apps into the cache")
            .on_disabled_hover_text("Choose a package cache folder and save first")
            .clicked()
        {
            app.actions.push(Action::FillCache);
        }
    });
    ui.label(weak("To give an app fresh settings, use \"Reset settings…\" in its ••• menu. To set up every computer like this one, use \"Save its setup…\" there and hand the file out with the policy's \"profiles\".".into()));
}

fn names(app: &CraftSpaceApp, ids: &[String]) -> String {
    ids.iter().map(|id| app.app_name(id)).collect::<Vec<_>>().join(", ")
}

fn section(ui: &mut Ui, p: &crate::theme::Palette, title: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).size(15.0).strong().color(p.text));
    ui.add_space(2.0);
}
