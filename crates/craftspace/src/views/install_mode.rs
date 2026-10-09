//! Asked once, before the first install: portable builds or the apps' own installers (system
//! packages on Linux). Settings › Installation changes it later.

use craftspace_core::platform::{AssetKind, Os};
use craftspace_core::Manager;
use eframe::egui::{self, Align, Layout, RichText, Sense, Ui, Vec2};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

/// Whether this computer has a choice to make.
pub fn has_choice(manager: &Manager) -> bool {
    match manager.platform().os {
        Os::Windows => true,
        Os::Linux => craftspace_core::platform::linux_package_format().is_some(),
        _ => false,
    }
}

pub fn window(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    if app.install_mode_pending.is_none() {
        return;
    }
    let p = app.palette;
    let windows = app.manager.platform().os == Os::Windows;
    let installer_name = if windows {
        "Installer".to_string()
    } else if craftspace_core::platform::linux_package_format() == Some(AssetKind::Rpm) {
        "System package (.rpm)".to_string()
    } else {
        "System package (.deb)".to_string()
    };
    let id = egui::Id::new("install-mode-choice");
    let mut installer = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(app.settings.prefer_system_installer);
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("install-mode")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading("How should apps be installed?");
        ui.label(RichText::new("CraftSpace asks once. You can change it any time in Settings › Installation.").color(p.weak));
        ui.add_space(12.0);
        let portable_text = if windows {
            "Into your user folder. No administrator rights needed, and updates install side by side, so you can go back to the previous version. Apps still get Start menu and Settings › Apps entries."
        } else {
            "Into your home folder. No password needed, and updates install side by side, so you can go back to the previous version. Apps still get menu entries."
        };
        let installer_text = if windows {
            "With each app's own Windows installer (MSI or Setup), into Program Files, like a traditional install. May ask for administrator rights. Apps without an installer are installed portable."
        } else {
            "Through your package manager (dnf, zypper or apt), so the system owns the files. Asks for your password. Apps without a package are installed portable."
        };
        if option(ui, &p, "Portable (recommended)", portable_text, !installer) {
            installer = false;
        }
        ui.add_space(8.0);
        if option(ui, &p, &installer_name, installer_text, installer) {
            installer = true;
        }
        ui.add_space(14.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::primary(ui, &p, "Continue").clicked() {
                app.actions.push(Action::SetInstallMode(installer));
                close = true;
            }
            if theme::pill(ui, &p, "Cancel").clicked() {
                app.install_mode_pending = None;
                close = true;
            }
        });
    });
    if modal.should_close() && !close {
        app.install_mode_pending = None;
        close = true;
    }
    if close {
        ctx.data_mut(|d| d.remove::<bool>(id));
    } else {
        ctx.data_mut(|d| d.insert_temp(id, installer));
    }
}

/// A selectable card with a title and a description. Returns whether it was clicked.
fn option(ui: &mut Ui, p: &theme::Palette, title: &str, text: &str, selected: bool) -> bool {
    let width = ui.available_width();
    let galley_h = 74.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, galley_h), Sense::click());
    let stroke = if selected { egui::Stroke::new(2.0, p.accent) } else { egui::Stroke::new(1.0, p.stroke) };
    ui.painter().rect(rect, 10, if resp.hovered() { p.card_hover } else { p.card }, stroke, egui::StrokeKind::Inside);
    // The radio dot.
    let c = rect.left_top() + Vec2::new(22.0, 22.0);
    ui.painter().circle_stroke(c, 7.0, egui::Stroke::new(1.5, if selected { p.accent } else { p.weak }));
    if selected {
        ui.painter().circle_filled(c, 4.0, p.accent);
    }
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                rect.left_top() + Vec2::new(40.0, 12.0),
                rect.right_bottom() - Vec2::new(14.0, 8.0),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    inner.label(RichText::new(title).strong());
    inner.add(egui::Label::new(RichText::new(text).size(12.0).color(p.weak)).wrap().selectable(false));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}
