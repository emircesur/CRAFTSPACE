//! Workspace sync: save an app's layouts, shortcuts, preferences and presets to a file, or bring
//! a file's into the app on this computer.

use std::collections::BTreeSet;
use std::path::PathBuf;

use craftspace_core::profiles::{Manifest, Part};
use eframe::egui::{self, Align, Layout, RichText};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

pub enum Mode {
    Export { app: String, available: Vec<Part>, note: Option<&'static str> },
    Import { file: PathBuf, manifest: Manifest },
}

pub struct Dialog {
    pub mode: Mode,
    pub parts: BTreeSet<Part>,
    pub working: bool,
}

impl Dialog {
    pub fn export(app: String, available: Vec<Part>, note: Option<&'static str>) -> Dialog {
        let parts = available.iter().copied().collect();
        Dialog { mode: Mode::Export { app, available, note }, parts, working: false }
    }

    pub fn import(file: PathBuf, manifest: Manifest) -> Dialog {
        let parts = manifest.parts.iter().copied().collect();
        Dialog { mode: Mode::Import { file, manifest }, parts, working: false }
    }
}

pub fn window(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    let Some(mut d) = app.profile_dialog.take() else { return };
    let p = app.palette;
    let mut keep = true;
    let modal = egui::Modal::new(egui::Id::new("profile-dialog")).show(ctx, |ui| {
        ui.set_width(460.0);
        let (available, app_id) = match &d.mode {
            Mode::Export { app: id, available, note } => {
                ui.heading(format!("Save {}'s setup", app.app_name(id)));
                ui.add(
                    egui::Label::new(
                        RichText::new("To set up another computer, or every computer in a classroom, the same way. Recent files, window positions, devices and folder paths stay on this computer.")
                            .color(p.weak),
                    )
                    .wrap(),
                );
                if let Some(note) = note {
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(RichText::new(*note).size(12.0).color(p.warn)).wrap());
                }
                (available.clone(), id.clone())
            }
            Mode::Import { manifest, .. } => {
                let name = app.app_name(&manifest.app);
                ui.heading(format!("Bring a setup into {name}"));
                let from = match &manifest.app_version {
                    Some(v) => format!("Saved from {name} {v} on {}.", os_name(&manifest.os)),
                    None => format!("Saved on {}.", os_name(&manifest.os)),
                };
                ui.label(RichText::new(from).color(p.weak));
                ui.label(
                    RichText::new("This computer's current setup is saved first, so you can go back.").size(12.0).color(p.weak),
                );
                (manifest.parts.clone(), manifest.app.clone())
            }
        };
        ui.add_space(10.0);
        for part in available {
            let mut on = d.parts.contains(&part);
            if ui.add_enabled(!d.working, egui::Checkbox::new(&mut on, part.label())).changed() {
                if on {
                    d.parts.insert(part);
                } else {
                    d.parts.remove(&part);
                }
            }
        }
        if matches!(d.mode, Mode::Import { .. }) {
            let backups = app.manager.profile_backups(&app_id);
            if let Some(latest) = backups.first() {
                ui.add_space(6.0);
                if ui
                    .add_enabled(!d.working, egui::Button::new(RichText::new("Go back to the setup before the last import").size(12.5)).frame(false))
                    .clicked()
                {
                    app.actions.push(Action::ImportProfileFile(latest.clone()));
                    keep = false;
                }
            }
        }
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let parts: Vec<Part> = d.parts.iter().copied().collect();
            if d.working {
                ui.add(egui::Spinner::new());
            } else {
                let label = if matches!(d.mode, Mode::Export { .. }) { "Save…" } else { "Bring in" };
                if ui.add_enabled_ui(!parts.is_empty(), |ui| theme::primary(ui, &p, label)).inner.clicked() {
                    match &d.mode {
                        Mode::Export { app: id, .. } => {
                            let name = format!("{id}.{}", craftspace_core::profiles::EXTENSION);
                            if let Some(out) = rfd::FileDialog::new()
                                .set_title("Save the setup")
                                .set_file_name(name)
                                .add_filter("CraftSpace profile", &[craftspace_core::profiles::EXTENSION])
                                .save_file()
                            {
                                app.actions.push(Action::ExportProfile(id.clone(), parts, out));
                                d.working = true;
                            }
                        }
                        Mode::Import { file, .. } => {
                            app.actions.push(Action::ImportProfile(file.clone(), parts));
                            d.working = true;
                        }
                    }
                }
            }
            if theme::pill(ui, &p, "Cancel").clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() && !d.working {
        keep = false;
    }
    if keep {
        app.profile_dialog = Some(d);
    }
}

fn os_name(os: &str) -> &str {
    match os {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        "" => "another computer",
        other => other,
    }
}
