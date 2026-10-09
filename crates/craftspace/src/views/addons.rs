//! Fonts & add-ons: font families from craft-fonts, and add-ons (presets, palettes, LUTs,
//! plug-ins) from the CraftSpace registry and add-on stores.

use std::collections::BTreeMap;

use craftspace_core::catalog::AddonKind;
use craftspace_core::fonts::{self, FontFile};
use eframe::egui::{self, Align, Layout, RichText, Ui};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

pub fn content(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    ui.heading("Fonts & add-ons");
    ui.label(RichText::new("Extras for the ArtCraft apps, installed for your user account.").color(p.weak));
    ui.add_space(12.0);

    let catalog = app.manager.catalog();
    for addon in catalog.addons.iter().filter(|a| a.kind == AddonKind::Fonts) {
        app.ensure_fonts(&addon.id);
        theme::card_frame(&p, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&addon.name).size(17.0).strong());
                    ui.add(egui::Label::new(RichText::new(&addon.description).color(p.weak)).wrap());
                });
            });
            ui.add_space(6.0);
            let fonts = app.fonts.clone();
            match fonts {
                None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(RichText::new("Loading the font list…").color(p.weak));
                    });
                }
                Some(Err(err)) => {
                    ui.label(RichText::new(format!("Couldn't load the font list: {err}")).color(p.bad));
                }
                Some(Ok(list)) => families(app, ui, &list),
            }
            if let Some(home) = &addon.homepage {
                ui.add_space(4.0);
                if ui
                    .add(egui::Button::new(RichText::new("Licences and sources ↗").color(p.accent)).frame(false))
                    .clicked()
                {
                    app.actions.push(Action::OpenUrl(home.clone()));
                }
            }
        });
        ui.add_space(12.0);
    }

    addon_list(app, ui);
    ui.add_space(30.0);
}

/// The add-ons from the CraftSpace registry and the add-on stores.
fn addon_list(app: &mut CraftSpaceApp, ui: &mut Ui) {
    use craftspace_core::addons::{self, Kind};
    let p = app.palette;
    if app.addons.is_empty() && !app.addons_requested {
        app.addons_requested = true;
        crate::worker::load_addons(&app.bus, &app.manager);
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Presets, plug-ins and more").size(18.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Submit an add-on ↗").on_hover_text("Anyone can add one to the CraftSpace registry").clicked()
            {
                app.actions.push(Action::OpenUrl(addons::SUBMIT_URL.into()));
            }
        });
    });
    ui.label(
        RichText::new("Each add-on goes where its app finds it. CraftSpace checks every download against its checksum when one is published.")
            .color(p.weak),
    );
    ui.add_space(6.0);

    // Filters: app, checked only.
    let mut apps: Vec<String> = Vec::new();
    for a in &app.addons {
        for id in &a.apps {
            if !apps.contains(id) {
                apps.push(id.clone());
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(app.addon_app.is_none(), "All apps").clicked() {
            app.addon_app = None;
        }
        for id in &apps {
            let name = app.app_name(id);
            if ui.selectable_label(app.addon_app.as_deref() == Some(id), name).clicked() {
                app.addon_app = Some(id.clone());
            }
        }
        ui.separator();
        ui.checkbox(&mut app.addon_checked_only, "Checked by CraftSpace only");
    });
    ui.add_space(8.0);

    if app.addons.is_empty() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(RichText::new("Loading add-ons…").color(p.weak));
        });
        return;
    }
    let installed = app.manager.installed_addons();
    let blocked = app.manager.policy().block_unchecked_addons;
    let platform = app.manager.platform();
    let list: Vec<addons::Addon> = app
        .addons
        .iter()
        .filter(|a| app.addon_app.as_ref().is_none_or(|id| a.apps.contains(id)))
        .filter(|a| !app.addon_checked_only || a.checked())
        .cloned()
        .collect();
    let mut sources: Vec<String> = Vec::new();
    for a in &list {
        if !sources.contains(&a.source) {
            sources.push(a.source.clone());
        }
    }
    for source in sources {
        let from_registry = source == "CraftSpace";
        ui.add_space(6.0);
        ui.label(
            RichText::new(if from_registry {
                "From the CraftSpace registry".to_string()
            } else {
                format!("From {source}")
            })
            .size(15.0)
            .strong(),
        );
        if !from_registry {
            ui.label(
                RichText::new("A community store. CraftSpace hasn't checked its add-ons for security.")
                    .size(12.0)
                    .color(p.warn),
            );
        }
        for a in list.iter().filter(|a| a.source == source) {
            let key = format!("addons:{}", a.id);
            let busy = app.jobs.contains_key(&key);
            let is_installed = installed.contains_key(&a.id);
            let file = a.file_for(platform);
            theme::card_frame(&p, false).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let text_width = (ui.available_width() - 150.0).max(200.0);
                    ui.vertical(|ui| {
                        ui.set_max_width(text_width);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&a.name).size(15.0).strong());
                            if let Some(v) = &a.version {
                                ui.label(RichText::new(v).size(12.0).color(p.weak));
                            }
                            if a.checked() {
                                theme::chip(ui, "✔ Checked by CraftSpace", p.good);
                            } else {
                                theme::chip(ui, "⚠ Not checked for security", p.warn);
                            }
                            theme::chip(ui, a.kind.label(), p.accent);
                            if is_installed {
                                theme::chip(ui, "Installed", p.good);
                            }
                        });
                        ui.add(egui::Label::new(RichText::new(&a.description).color(p.weak)).wrap());
                        let mut meta: Vec<String> = Vec::new();
                        meta.push(format!(
                            "For {}",
                            a.apps.iter().map(|id| app.app_name(id)).collect::<Vec<_>>().join(", ")
                        ));
                        if let Some(author) = &a.author {
                            meta.push(format!("by {author}"));
                        }
                        if let Some(license) = &a.license {
                            meta.push(license.clone());
                        }
                        if file.is_some_and(|f| f.sha256.is_none()) {
                            meta.push("no checksum published".into());
                        }
                        ui.label(RichText::new(meta.join(" · ")).size(12.0).color(p.weak));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if busy {
                            ui.add(egui::Spinner::new());
                        } else if is_installed {
                            if ui.button("Remove").clicked() {
                                app.actions.push(Action::RemoveAddon(a.id.clone()));
                            }
                        } else if file.is_none() {
                            ui.label(RichText::new(format!("Not for {}", platform.display())).size(12.0).color(p.weak));
                        } else if blocked && !a.checked() {
                            ui.label(RichText::new("Not allowed here").size(12.0).color(p.weak))
                                .on_hover_text("Your organization allows only add-ons checked by CraftSpace");
                        } else if theme::primary(ui, &p, "Install").clicked() {
                            app.actions.push(Action::InstallAddon(a.id.clone()));
                        }
                        if let Some(home) = &a.homepage {
                            if ui.small_button("↗").on_hover_text(home.as_str()).clicked() {
                                app.actions.push(Action::OpenUrl(home.clone()));
                            }
                        }
                    });
                });
                if is_installed && a.kind == Kind::Pack {
                    let library = app.manager.addon_library().join(addons::safe_name(&a.name));
                    if library.exists() && ui.small_button("Show files").clicked() {
                        app.actions.push(Action::OpenPath(library));
                    }
                }
            });
        }
    }

    // Plug-in sources CraftSpace lists but doesn't install from.
    let repos = app.manager.addon_registry().repositories;
    if !repos.is_empty() {
        ui.add_space(14.0);
        ui.label(RichText::new("More plug-in sources").size(15.0).strong());
        ui.label(
            RichText::new("Not checked for security by CraftSpace. Download from them at your own risk.")
                .size(12.0)
                .color(p.warn),
        );
        for r in repos.iter().filter(|r| app.addon_app.as_ref().is_none_or(|id| r.apps.contains(id))) {
            ui.horizontal(|ui| {
                if ui.link(&r.name).clicked() {
                    app.actions.push(Action::OpenUrl(r.url.clone()));
                }
                ui.label(RichText::new(&r.description).size(12.5).color(p.weak));
            });
        }
    }
}

/// "Install anyway?" for an add-on CraftSpace hasn't checked.
pub fn confirm(app: &mut CraftSpaceApp, ctx: &egui::Context, id: &str) {
    let p = app.palette;
    let Some(a) = app.addons.iter().find(|a| a.id == id).cloned() else {
        app.confirm_addon = None;
        return;
    };
    let checksum = a.file_for(app.manager.platform()).and_then(|f| f.sha256.clone());
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("confirm-addon")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(format!("Install {}?", a.name));
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(format!(
                    "{} comes from {}{} and isn't checked by CraftSpace for security.",
                    a.name,
                    a.source,
                    a.author.as_ref().map(|x| format!(" (by {x})")).unwrap_or_default()
                ))
                .color(p.text),
            )
            .wrap(),
        );
        ui.add_space(4.0);
        let detail = match (a.kind, checksum.is_some()) {
            (craftspace_core::addons::Kind::AudioPlugin, true) => "Audio plug-ins are programs that run inside SoundCraft with your permissions. CraftSpace checks that the download is exactly the one listed, but hasn't reviewed its code.",
            (craftspace_core::addons::Kind::AudioPlugin, false) => "Audio plug-ins are programs that run inside SoundCraft with your permissions, and this download has no checksum to check it against.",
            (_, true) => "CraftSpace checks that the download is exactly the one listed, but hasn't reviewed what it does.",
            (_, false) => "Its store publishes no checksum, so CraftSpace can't tell whether the file was changed. PhotoCraft runs plug-ins in a sandbox, without access to your files.",
        };
        ui.add(egui::Label::new(RichText::new(detail).color(p.weak)).wrap());
        if let Some(home) = &a.homepage {
            ui.add_space(4.0);
            if ui.link("See its source first ↗").clicked() {
                app.actions.push(Action::OpenUrl(home.clone()));
            }
        }
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::primary(ui, &p, "Install anyway").clicked() {
                app.actions.push(Action::InstallAddonAnyway(a.id.clone()));
                close = true;
            }
            if theme::pill(ui, &p, "Cancel").clicked() {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        app.confirm_addon = None;
    }
}

fn families(app: &mut CraftSpaceApp, ui: &mut Ui, list: &[FontFile]) {
    let p = app.palette;
    let installed = app.manager.installed_fonts();
    let mut by_family: BTreeMap<&str, Vec<&FontFile>> = BTreeMap::new();
    for f in list {
        by_family.entry(&f.family).or_default().push(f);
    }
    let all_installed = list.iter().all(|f| installed.contains_key(f.file_name()));
    let busy_all = app.jobs.keys().any(|k| k.starts_with("addon:"));
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} families", by_family.len())).color(p.weak));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !all_installed && !busy_all && theme::primary(ui, &p, "Install all").clicked() {
                app.actions.push(Action::InstallFonts(None));
            }
        });
    });
    ui.separator();
    for (family, files) in by_family {
        let have = files.iter().filter(|f| installed.contains_key(f.file_name())).count();
        let styles: Vec<&str> = files.iter().map(|f| f.style.as_str()).collect();
        let mut scripts: Vec<&str> = Vec::new();
        for name in files.iter().flat_map(|f| f.scripts.iter().map(|s| fonts::script_name(s))) {
            if !scripts.contains(&name) {
                scripts.push(name);
            }
        }
        let key_prefix = format!(":{family}");
        let busy = app.jobs.keys().any(|k| k.starts_with("addon:") && (k.ends_with(&key_prefix) || k.ends_with(":*")));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(family).size(15.0).strong());
                ui.label(
                    RichText::new(format!("{} · {} · {}", styles.join(", "), scripts.join(", "), files[0].license))
                        .size(12.0)
                        .color(p.weak),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if busy {
                    ui.add(egui::Spinner::new());
                } else if have == files.len() {
                    if ui.button("Remove").clicked() {
                        app.actions.push(Action::RemoveFonts(Some(family.to_string())));
                    }
                    theme::chip(ui, "Installed", p.good);
                } else if theme::primary(ui, &p, "Install").clicked() {
                    app.actions.push(Action::InstallFonts(Some(family.to_string())));
                }
            });
        });
        ui.add_space(4.0);
    }
}
