//! Fonts & add-ons: font families from craft-fonts (and preset packs, when the catalog has any).

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

    let packs: Vec<_> = catalog.addons.iter().filter(|a| a.kind == AddonKind::PresetPack).collect();
    ui.label(RichText::new("Preset packs").size(16.0).strong());
    if packs.is_empty() {
        ui.label(
            RichText::new("No brush or preset packs have been published for the ArtCraft apps yet. When they are, they appear here and install into each app's presets folder.")
                .color(p.weak),
        );
    }
    for pack in packs {
        theme::card_frame(&p, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&pack.name).strong());
            ui.label(RichText::new(&pack.description).color(p.weak));
        });
    }
    ui.add_space(30.0);
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
