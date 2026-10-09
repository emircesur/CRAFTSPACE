//! About CraftSpace: the logo, version, what it is and where it comes from.

use eframe::egui::{self, Align, Layout, RichText, Vec2};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

const REPO: &str = "https://github.com/emircesur/craftspace";

pub fn window(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    if !app.about_open {
        return;
    }
    let p = app.palette;
    let mut keep = true;
    let modal = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(128.0), egui::Sense::hover());
            egui::Image::new(&theme::logo_large(ctx)).corner_radius(24).paint_at(ui, rect);
            ui.add_space(10.0);
            ui.label(RichText::new("CraftSpace").size(28.0).strong());
            ui.label(RichText::new(format!("Version {}", crate::app::version_string())).color(p.weak));
            ui.label(RichText::new(app.manager.platform().display()).size(12.0).color(p.weak));
            ui.add_space(10.0);
            ui.add(
                egui::Label::new(
                    "An independent installer and update manager for the open-source ArtCraft creative apps: install, update and roll back every app in one place, and find your files.",
                )
                .wrap(),
            );
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Free and open source (MIT or Apache-2.0). A community project, done for education purposes only. Not affiliated with the ArtCraft developers, Adobe or Microsoft. App names and icons belong to their owners.",
                    )
                    .size(12.0)
                    .color(p.weak),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "No accounts, no tracking: CraftSpace only talks to GitHub and getartcraft.com to find releases, news and icons. Your files are never uploaded.",
                    )
                    .size(12.0)
                    .color(p.weak),
                )
                .wrap(),
            );
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                for (label, url) in [
                    ("Source code ↗", REPO.to_string()),
                    ("Releases ↗", format!("{REPO}/releases")),
                    ("Report a problem ↗", format!("{REPO}/issues/new")),
                    ("ArtCraft ↗", "https://getartcraft.com".to_string()),
                ] {
                    if ui.add(egui::Button::new(RichText::new(label).color(p.accent)).frame(false)).clicked() {
                        app.actions.push(Action::OpenUrl(url));
                    }
                }
            });
        });
        ui.add_space(10.0);
        ui.separator();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::pill(ui, &p, "Close").clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    app.about_open = keep;
}
