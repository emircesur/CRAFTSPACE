//! Settings › Other sources: add an app from a GitHub repository, or update an ArtCraft app (or
//! CraftSpace) from a fork, mirror or backup.

use eframe::egui::{self, Align, Layout, RichText};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;
use crate::worker::SourceOp;

/// What the dialog is for.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A new app from GitHub.
    NewApp,
    /// Another repository for this ArtCraft app, or `craftspace` for CraftSpace itself.
    App(String),
}

pub struct Dialog {
    pub target: Target,
    pub repo: String,
    pub name: String,
    pub binary: String,
    pub checking: bool,
    pub check: Option<Result<craftspace_core::sources::SourceCheck, String>>,
    pub working: bool,
}

impl Dialog {
    pub fn new(target: Target, repo: String) -> Dialog {
        Dialog {
            target,
            repo,
            name: String::new(),
            binary: String::new(),
            checking: false,
            check: None,
            working: false,
        }
    }
}

pub fn window(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    let Some(mut d) = app.source_dialog.take() else { return };
    let p = app.palette;
    let mut keep = true;
    let title = match &d.target {
        Target::NewApp => "Add an app from GitHub".to_string(),
        Target::App(id) if id == craftspace_core::selfupdate::NAME => "Where CraftSpace updates from".to_string(),
        Target::App(id) => format!("Where {} updates from", app.app_name(id)),
    };
    let modal = egui::Modal::new(egui::Id::new("source-dialog")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(title);
        ui.add(
            egui::Label::new(
                RichText::new(match d.target {
                    Target::NewApp => "CraftSpace installs and updates it from the repository's releases, like the ArtCraft apps.",
                    Target::App(_) => "A fork, a mirror or a backup of the official repository. Its releases must be named like the official ones.",
                })
                .color(p.weak),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        theme::chip(ui, "Only add repositories you trust: their programs run on this computer.", p.warn);
        ui.add_space(10.0);
        ui.label("Repository");
        let before = d.repo.clone();
        ui.add(egui::TextEdit::singleline(&mut d.repo).hint_text("owner/repo or https://github.com/owner/repo").desired_width(f32::INFINITY));
        if d.repo != before {
            d.check = None;
        }
        if d.target == Target::NewApp {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label("Name (optional)");
                    ui.add(egui::TextEdit::singleline(&mut d.name).hint_text("from the repository's name").desired_width(230.0));
                });
                ui.vertical(|ui| {
                    ui.label("Program (optional)");
                    ui.add(egui::TextEdit::singleline(&mut d.binary).hint_text("e.g. rg for ripgrep").desired_width(230.0));
                });
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let can_check = !d.repo.trim().is_empty() && !d.checking;
            if ui.add_enabled(can_check, egui::Button::new("Check")).clicked() {
                let id = match &d.target {
                    Target::App(id) => id.clone(),
                    Target::NewApp => d.repo.trim().trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_ascii_lowercase(),
                };
                d.checking = true;
                d.check = None;
                app.actions.push(Action::Source(SourceOp::Check { repo: d.repo.clone(), id }));
            }
            if d.checking {
                ui.add(egui::Spinner::new().size(14.0));
            }
            match &d.check {
                Some(Ok(c)) => {
                    ui.label(RichText::new(format!("✓ {} {}: {} ({})", c.repo, c.tag, c.asset, c.kind.label())).color(p.good).size(12.5));
                }
                Some(Err(e)) => {
                    ui.add(egui::Label::new(RichText::new(e).color(p.bad).size(12.5)).wrap());
                }
                None => {}
            }
        });
        ui.add_space(12.0);
        ui.separator();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let ready = matches!(d.check, Some(Ok(_))) && !d.working;
            let label = if d.target == Target::NewApp { "Add" } else { "Use this repository" };
            if ui.add_enabled(ready, egui::Button::new(RichText::new(label).color(egui::Color32::WHITE)).fill(p.accent).corner_radius(15)).clicked() {
                d.working = true;
                let op = match &d.target {
                    Target::NewApp => SourceOp::Add { repo: d.repo.clone(), name: d.name.clone(), binary: d.binary.clone() },
                    Target::App(id) => SourceOp::Set { id: id.clone(), repo: Some(d.repo.clone()) },
                };
                app.actions.push(Action::Source(op));
            }
            if let Target::App(id) = &d.target {
                if ui.button("Use the official repository").clicked() {
                    d.working = true;
                    app.actions.push(Action::Source(SourceOp::Set { id: id.clone(), repo: None }));
                }
            }
            if theme::pill(ui, &p, "Cancel").clicked() {
                keep = false;
            }
            if d.working {
                ui.add(egui::Spinner::new().size(14.0));
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if keep {
        app.source_dialog = Some(d);
    }
}
