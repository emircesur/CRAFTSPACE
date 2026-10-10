//! The Apps tab: the card grid, the Updates page and the app detail page.

use craftspace_core::download::format_bytes;
use craftspace_core::platform::AssetKind;
use craftspace_core::settings::Channel;
use craftspace_core::{AppState, Stage};
use eframe::egui::{self, Align, Layout, RichText, Sense, Ui, Vec2};

use crate::app::{Action, AppsView, CraftSpaceApp, DetailTab};
use crate::theme::{self, Palette};
use crate::worker::Job;

pub fn sidebar(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    let installed = app.states.iter().filter(|s| s.installed.is_some()).count();
    let updates = app.update_count();

    theme::section_label(ui, &p, "Apps");
    nav(app, ui, "All apps", AppsView::All, Some(app.states.len()), false);
    nav(app, ui, "Updates", AppsView::Updates, (updates > 0).then_some(updates), true);
    nav(app, ui, "Installed", AppsView::Installed, Some(installed), false);
    if !app.manager.catalog().addons.is_empty() {
        nav(app, ui, "Fonts & add-ons", AppsView::Addons, None, false);
    }

    theme::section_label(ui, &p, "Categories");
    let catalog = app.manager.catalog();
    for cat in &catalog.categories {
        let n = app.states.iter().filter(|s| s.app.category == cat.id).count();
        if n > 0 {
            nav(app, ui, &cat.name, AppsView::Category(cat.id.clone()), None, false);
        }
    }

    theme::section_label(ui, &p, "Resource links");
    for link in &catalog.links {
        if ui.add(egui::Button::new(RichText::new(format!("↗  {}", link.title)).color(p.text)).frame(false)).clicked()
        {
            app.actions.push(Action::OpenUrl(link.url.clone()));
        }
    }

    ui.add_space(16.0);
    ui.separator();
    ui.label(
        RichText::new(format!("{} · CraftSpace {}", app.manager.platform().display(), crate::app::version_string()))
            .size(11.5)
            .color(p.weak),
    );
    if let Some(v) = app.self_update_ready.clone() {
        ui.add_space(4.0);
        theme::chip(ui, &format!("CraftSpace {v} is ready"), p.good);
        if ui.add(egui::Button::new("Restart to use it").small()).clicked() {
            app.actions.push(Action::RestartCraftSpace);
        }
    } else if let Some(update) = &app.self_update {
        ui.add_space(4.0);
        theme::chip(ui, &format!("CraftSpace {} available", update.version), p.accent);
        if app.self_updating {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(RichText::new("Updating CraftSpace…").size(12.0));
            });
        } else if ui.add(egui::Button::new("Update CraftSpace").small()).clicked() {
            app.actions.push(Action::ApplySelfUpdate);
        }
    }
}

fn nav(app: &mut CraftSpaceApp, ui: &mut Ui, label: &str, view: AppsView, count: Option<usize>, bubble: bool) {
    let p = app.palette;
    let selected =
        app.apps_view == view || matches!((&app.apps_view, &view), (AppsView::Detail(_), _) if app.back_view == view);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 6, if selected { p.card_hover } else { p.card });
    }
    ui.painter().text(
        rect.left_center() + Vec2::new(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        p.text,
    );
    if let Some(n) = count {
        if bubble {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(Vec2::new(8.0, 6.0)))
                    .layout(Layout::right_to_left(Align::Center)),
            );
            theme::count_bubble(&mut child, &p, n);
        } else {
            ui.painter().text(
                rect.right_center() - Vec2::new(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                n.to_string(),
                egui::FontId::proportional(12.0),
                p.weak,
            );
        }
    }
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.actions.push(Action::GoApps(view));
    }
}

fn matches_search(s: &AppState, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let q = q.to_lowercase();
    let a = &s.app;
    [&a.name, &a.id, &a.tagline, &a.description, a.like.as_deref().unwrap_or("")]
        .iter()
        .any(|f| f.to_lowercase().contains(&q))
        || a.extensions.iter().any(|e| e.eq_ignore_ascii_case(q.trim_start_matches('.')))
}

pub fn content(app: &mut CraftSpaceApp, ui: &mut Ui) {
    if let AppsView::Detail(id) = app.apps_view.clone() {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| detail(app, ui, &id));
        return;
    }
    if app.apps_view == AppsView::Updates {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| updates(app, ui));
        return;
    }
    if app.apps_view == AppsView::Addons {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| super::addons::content(app, ui));
        return;
    }
    let p = app.palette;
    let catalog = app.manager.catalog();
    let title = match &app.apps_view {
        AppsView::Installed => "Installed".to_string(),
        AppsView::Category(c) => catalog.category_name(c),
        _ => "All apps".into(),
    };
    ui.horizontal(|ui| {
        ui.heading(title);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let installable = app.states.iter().filter(|s| s.installed.is_none() && s.installable.is_some()).count();
            if installable > 1 && theme::pill(ui, &p, "Install several…").clicked() {
                app.actions.push(Action::OpenMultiInstall);
            }
            if app.refreshing {
                ui.label(RichText::new("Checking for updates…").color(p.weak));
                ui.add(egui::Spinner::new());
            }
        });
    });
    ui.add_space(6.0);
    self_install_banner(app, ui);
    offline_banner(app, ui);

    let q = app.search.trim().to_string();
    let states: Vec<AppState> = app
        .states
        .iter()
        .filter(|s| match &app.apps_view {
            AppsView::Installed => s.installed.is_some(),
            AppsView::Category(c) => &s.app.category == c,
            _ => true,
        })
        .filter(|s| matches_search(s, &q))
        .cloned()
        .collect();

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if states.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(if q.is_empty() { "Nothing here yet" } else { "No apps match your search" })
                        .size(16.0),
                );
                if app.apps_view == AppsView::Installed {
                    ui.label(RichText::new("Install an app from All apps and it shows up here.").color(p.weak));
                }
            });
            return;
        }
        let (installed, available): (Vec<_>, Vec<_>) = states.into_iter().partition(|s| s.installed.is_some());
        if !installed.is_empty() && app.apps_view != AppsView::Installed {
            sub_heading(ui, &p, "Installed");
        }
        grid(app, ui, &installed);
        if !available.is_empty() {
            if !installed.is_empty() {
                ui.add_space(12.0);
            }
            sub_heading(ui, &p, "Available apps");
            grid(app, ui, &available);
        }
        ui.add_space(24.0);
    });
}

fn sub_heading(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(15.0).strong().color(p.text));
    ui.add_space(4.0);
}

fn self_install_banner(app: &mut CraftSpaceApp, ui: &mut Ui) {
    if !app.offer_self_install {
        return;
    }
    let p = app.palette;
    theme::card_frame(&p, false).fill(p.accent.gamma_multiply(0.12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Install CraftSpace on this computer?").strong());
                ui.label(RichText::new("Adds it to your Start menu / app menu so it can keep your apps up to date. No admin rights needed.").color(p.weak));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::primary(ui, &p, "Install").clicked() {
                    app.actions.push(Action::SelfInstall);
                }
                if ui.add(egui::Button::new("Not now").frame(false)).clicked() {
                    app.actions.push(Action::DismissSelfInstall);
                }
            });
        });
    });
    ui.add_space(8.0);
}

fn offline_banner(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    let unknown: Vec<String> = app.states.iter().filter(|s| !s.known).map(|s| s.app.name.clone()).collect();
    if app.refreshing || unknown.is_empty() || app.refresh_errors.is_empty() {
        return;
    }
    theme::card_frame(&p, false).fill(p.warn.gamma_multiply(0.12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.set_max_width(ui.available_width());
            let text_width = ui.available_width() - 80.0;
            ui.allocate_ui_with_layout(Vec2::new(text_width, 0.0), Layout::top_down(Align::Min), |ui| ui.add(egui::Label::new(format!(
                "⚠  Couldn't get release information for {} from GitHub. Check your connection, or add a GitHub token in Settings if you hit the rate limit.",
                unknown.join(", ")
            )).wrap()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Retry").clicked() {
                    app.actions.push(Action::Refresh);
                }
            });
        });
    });
    ui.add_space(8.0);
}

const CARD_W: f32 = 300.0;
const CARD_H: f32 = 168.0;

fn grid(app: &mut CraftSpaceApp, ui: &mut Ui, states: &[AppState]) {
    let spacing = 14.0;
    let avail = ui.available_width();
    let cols = ((avail + spacing) / (CARD_W + spacing)).floor().max(1.0) as usize;
    let w = ((avail - spacing * (cols as f32 - 1.0)) / cols as f32).min(420.0);
    for row in states.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            for s in row {
                ui.allocate_ui_with_layout(Vec2::new(w, CARD_H), Layout::top_down(Align::Min), |ui| {
                    card(app, ui, s, w)
                });
            }
        });
        ui.add_space(spacing - ui.spacing().item_spacing.y);
    }
}

fn card(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState, w: f32) {
    let p = app.palette;
    let id = s.app.id.clone();
    let background = theme::click_area(ui, Vec2::new(w, CARD_H), ("card", &id));
    let hovered = ui.rect_contains_pointer(background.rect);
    theme::card_frame(&p, hovered).show(ui, |ui| {
        ui.set_width(w - 34.0);
        ui.set_height(CARD_H - 34.0);
        ui.horizontal(|ui| {
            if theme::badge(ui, &s.app, 40.0).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.actions.push(Action::ShowApp(id.clone()));
            }
            ui.vertical(|ui| {
                ui.add_space(1.0);
                let name =
                    ui.add(egui::Label::new(RichText::new(&s.app.name).size(16.0).strong()).sense(Sense::click()));
                if name.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.actions.push(Action::ShowApp(id.clone()));
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if s.update_available {
                    theme::chip(ui, "Update", p.accent);
                } else if s.installed.is_some() {
                    theme::chip(ui, "Installed", p.good);
                }
            });
        });
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(&s.app.tagline).color(p.weak)).wrap());

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            if !job_row_for(app, ui, &id) {
                action_row(app, ui, s);
            }
            ui.separator();
        });
    });
    if background.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.actions.push(Action::ShowApp(id));
    }
}

/// The stage line and progress bar of a job.
pub fn job_row_body(p: &Palette, ui: &mut Ui, job: &Job) {
    let label = if job.queued {
        "Waiting…".to_string()
    } else {
        match (job.stage, job.total) {
            (Stage::Downloading, Some(t)) => {
                format!("{} {} / {}", job.stage.label(), format_bytes(job.done), format_bytes(t))
            }
            (Stage::Downloading, None) => format!("{} {}", job.stage.label(), format_bytes(job.done)),
            (Stage::Resolving, _) => format!("{}…", job.kind.verb()),
            (stage, _) => format!("{}…", stage.label()),
        }
    };
    ui.label(RichText::new(label).size(12.0).color(p.weak));
    let bar = match job.fraction() {
        Some(f) => egui::ProgressBar::new(f),
        None if job.queued => egui::ProgressBar::new(0.0),
        None => egui::ProgressBar::new(0.0).animate(true),
    };
    ui.add(bar.desired_height(6.0).fill(p.accent));
}

/// Progress for a job; returns true when the user clicks cancel.
pub fn job_row(p: &Palette, ui: &mut Ui, job: &Job) -> bool {
    let mut cancel = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width() - 34.0);
            job_row_body(p, ui, job);
        });
        cancel = ui.add(egui::Button::new("×").frame(false)).on_hover_text("Cancel").clicked();
    });
    cancel
}

/// Progress row for `id` if it has a job (or waits for the app to close). Returns false when
/// there is nothing to show.
fn job_row_for(app: &mut CraftSpaceApp, ui: &mut Ui, id: &str) -> bool {
    let p = app.palette;
    if let Some(job) = app.jobs.get(id) {
        if job_row(&p, ui, job) {
            app.actions.push(Action::Cancel(id.to_string()));
        }
        return true;
    }
    if let Some((_, kind)) = app.waiting_for_close.iter().find(|(k, _)| k == id).cloned() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} when you close it", kind.verb())).size(12.0).color(p.weak));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new("×").frame(false)).on_hover_text("Don't").clicked() {
                    app.actions.push(Action::Cancel(id.to_string()));
                }
            });
        });
        return true;
    }
    false
}

/// The version line plus the ⋯ menu and main button.
fn action_row(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState) {
    let p = app.palette;
    let id = s.app.id.clone();
    ui.horizontal(|ui| {
        let info = match (&s.installed, s.latest_version()) {
            (Some(i), Some(l)) if s.update_available => format!("{} › {}", i.current.version, l),
            (Some(i), _) => format!("Version {}", i.current.version),
            (None, Some(l)) => match &s.installable {
                Some((asset, _)) => match asset.size {
                    Some(size) => format!("{l} · {}", format_bytes(size)),
                    None => format!("Version {l}"),
                },
                None => format!("{l} · not for this platform"),
            },
            (None, None) if !s.known => "Checking…".into(),
            (None, None) => "No release yet".into(),
        };
        ui.label(RichText::new(info).size(12.5).color(p.weak));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            main_button(app, ui, s);
            more_menu(app, ui, s);
        });
    });
    let _ = id;
}

pub fn main_button(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState) {
    let p = app.palette;
    let id = s.app.id.clone();
    if app.jobs.contains_key(&id) {
        ui.add_enabled(false, egui::Button::new("Working…").corner_radius(15));
    } else if s.update_available {
        if theme::primary(ui, &p, "Update").clicked() {
            app.actions.push(Action::Update(id));
        }
    } else if s.installed.is_some() {
        if theme::pill(ui, &p, "Open").clicked() {
            app.actions.push(Action::Launch(id, vec![]));
        }
    } else if s.installable.is_some() {
        if theme::primary(ui, &p, "Install").clicked() {
            app.actions.push(Action::Install(id, None));
        }
    } else if theme::pill(ui, &p, "Learn more").clicked() {
        app.actions.push(Action::OpenUrl(s.app.homepage()));
    }
}

pub fn more_menu(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState) {
    let id = s.app.id.clone();
    let busy = app.jobs.contains_key(&id);
    let managed = app.managed();
    let button = theme::icon_button(ui, &app.palette, theme::Icon::More, 28.0).on_hover_text("More");
    egui::Popup::menu(&button).show(|ui| {
        ui.set_min_width(200.0);
        if ui.button("Details and versions").clicked() {
            app.actions.push(Action::ShowApp(id.clone()));
        }
        if let Some(installed) = &s.installed {
            if s.update_available && ui.add_enabled(!busy, egui::Button::new("Update")).clicked() {
                app.actions.push(Action::Update(id.clone()));
            }
            if let Some(prev) = installed.previous.as_ref().filter(|_| !managed) {
                if ui.add_enabled(!busy, egui::Button::new(format!("Roll back to {}", prev.version))).clicked() {
                    app.actions.push(Action::Rollback(id.clone()));
                }
            }
            if let Some(dir) = installed
                .current
                .dir
                .clone()
                .or_else(|| installed.current.executable.as_ref().and_then(|e| e.parent().map(|p| p.to_path_buf())))
            {
                if ui.button("Show install folder").clicked() {
                    app.actions.push(Action::OpenPath(dir));
                }
            }
        }
        if s.installed.is_none()
            && !managed
            && s.installable.as_ref().is_some_and(|(_, kind)| kind.is_managed())
            && ui.add_enabled(!busy, egui::Button::new("Install in a folder…")).clicked()
        {
            app.actions.push(Action::InstallTo(id.clone()));
        }
        if app.settings.other_sources.enabled {
            if s.app.custom {
                if s.installed.is_none() && ui.button("Remove from CraftSpace").clicked() {
                    app.actions.push(Action::Source(crate::worker::SourceOp::Remove { id: id.clone() }));
                }
            } else if ui.button("Update source…").on_hover_text("Update it from a fork, a mirror or a backup").clicked()
            {
                app.actions.push(Action::OpenSource(crate::views::sources::Target::App(id.clone())));
            }
        }
        if s.installed.is_some() && ui.add_enabled(!busy, egui::Button::new("Verify and repair")).clicked() {
            app.actions.push(Action::VerifyAndRepair(id.clone()));
        }
        if let Some(r) = &s.latest {
            if ui.button("Release notes ↗").clicked() {
                app.actions.push(Action::OpenUrl(r.html_url.clone()));
            }
        }
        if ui.button("Report a problem ↗").clicked() {
            app.actions.push(Action::ReportBug(id.clone()));
        }
        if ui.button("Source on GitHub ↗").clicked() {
            app.actions.push(Action::OpenUrl(s.app.github_url()));
        }
        if ui.button("Website ↗").clicked() {
            app.actions.push(Action::OpenUrl(s.app.homepage()));
        }
        if craftspace_core::profiles::spec(&id).is_some() {
            ui.separator();
            if ui
                .button("Save its setup…")
                .on_hover_text("Layouts, shortcuts, preferences and presets, for another computer or a classroom")
                .clicked()
            {
                app.actions.push(Action::OpenProfileExport(id.clone()));
            }
            if ui.button("Bring in a setup…").clicked() {
                app.actions.push(Action::OpenProfileImport);
            }
        }
        if s.installed.is_some() {
            ui.separator();
            if ui
                .add_enabled(!busy, egui::Button::new("Reset settings…"))
                .on_hover_text("Start the app with fresh settings; the old ones are kept in a backup folder")
                .clicked()
            {
                app.actions.push(Action::AskReset(id.clone()));
            }
            if !managed
                && ui
                    .add_enabled(!busy, egui::Button::new(RichText::new("Uninstall…").color(app.palette.bad)))
                    .clicked()
            {
                app.actions.push(Action::AskUninstall(id.clone()));
            }
        }
    });
}

fn updates(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    let list: Vec<AppState> = app
        .states
        .iter()
        .filter(|s| s.update_available || (s.installed.is_some() && app.jobs.contains_key(&s.app.id)))
        .cloned()
        .collect();
    ui.horizontal(|ui| {
        ui.heading("Updates");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let pending = list.iter().filter(|s| s.update_available && !app.jobs.contains_key(&s.app.id)).count();
            if pending > 1 && theme::primary(ui, &p, &format!("Update all ({pending})")).clicked() {
                app.actions.push(Action::UpdateAll);
            }
            if theme::pill(ui, &p, if app.refreshing { "Checking…" } else { "Check for updates" }).clicked()
                && !app.refreshing
            {
                app.actions.push(Action::Refresh);
            }
        });
    });
    let last = app.last_check.map(|t| {
        let mins = t.elapsed().as_secs() / 60;
        if mins == 0 {
            "just now".to_string()
        } else {
            format!("{mins} min ago")
        }
    });
    ui.label(RichText::new(format!("Last checked {}", last.unwrap_or_else(|| "never".into()))).color(p.weak));
    ui.add_space(10.0);

    if list.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("✔").size(40.0).color(p.good));
            ui.label(RichText::new("All your apps are up to date").size(17.0));
        });
    }
    for s in &list {
        theme::card_frame(&p, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::badge(ui, &s.app, 44.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&s.app.name).size(16.0).strong());
                    let from = s.installed_version().map(|v| v.to_string()).unwrap_or_default();
                    let to = s.latest_version().map(|v| v.to_string()).unwrap_or_default();
                    let date = s
                        .latest
                        .as_ref()
                        .and_then(|r| r.date().map(|d| format!(" · released {d}")))
                        .unwrap_or_default();
                    ui.label(RichText::new(format!("{from}  ›  {to}{date}")).color(p.weak));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if app.jobs.contains_key(&s.app.id) {
                        ui.allocate_ui_with_layout(Vec2::new(240.0, 40.0), Layout::top_down(Align::Min), |ui| {
                            job_row_for(app, ui, &s.app.id)
                        });
                    } else {
                        main_button(app, ui, s);
                        more_menu(app, ui, s);
                    }
                });
            });
            if let Some(notes) = s.latest.as_ref().map(|r| r.body.trim()).filter(|b| !b.is_empty()) {
                egui::CollapsingHeader::new("What's new").id_salt(("notes", &s.app.id)).show(ui, |ui| {
                    egui_commonmark::CommonMarkViewer::new().show(ui, &mut app.md_cache, notes);
                });
            }
        });
        ui.add_space(10.0);
    }

    if !app.refresh_errors.is_empty() {
        ui.add_space(10.0);
        ui.label(RichText::new("Couldn't check").strong());
        for (id, err) in &app.refresh_errors {
            ui.label(RichText::new(format!("{id}: {err}")).size(12.0).color(p.weak));
        }
    }
}

fn detail(app: &mut CraftSpaceApp, ui: &mut Ui, id: &str) {
    let p = app.palette;
    let Some(s) = app.state(id).cloned() else {
        ui.label("This app is no longer in the catalog.");
        return;
    };
    let back_label = match &app.back_view {
        AppsView::Updates => "Updates".to_string(),
        AppsView::Installed => "Installed".into(),
        AppsView::Category(c) => app.manager.catalog().category_name(c),
        _ => "All apps".into(),
    };
    if ui.add(egui::Button::new(RichText::new(format!("‹  {back_label}")).color(p.weak)).frame(false)).clicked() {
        app.actions.push(Action::GoApps(app.back_view.clone()));
    }
    ui.add_space(8.0);

    // Opened a file through CraftSpace whose app isn't installed yet.
    if let Some((_, file)) = app.pending_open.clone().filter(|(app_id, _)| app_id == id) {
        theme::card_frame(&p, false).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 240.0).max(200.0));
                    let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    ui.label(RichText::new(format!("{name} opens in {}", s.app.name)).strong());
                    ui.label(
                        RichText::new(format!("Install {} and the file opens as soon as it's ready.", s.app.name))
                            .color(p.weak),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::pill(ui, &p, "Not now").clicked() {
                        app.pending_open = None;
                    }
                    let busy = app.jobs.contains_key(id);
                    if !busy
                        && s.installable.is_some()
                        && theme::primary(ui, &p, &format!("Install {}", s.app.name)).clicked()
                    {
                        app.actions.push(Action::Install(id.to_string(), None));
                    }
                });
            });
        });
        ui.add_space(10.0);
    }

    ui.horizontal(|ui| {
        theme::badge(ui, &s.app, 76.0);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(&s.app.name).size(26.0).strong());
            ui.label(RichText::new(&s.app.tagline).size(15.0).color(p.weak));
            ui.horizontal(|ui| {
                let catalog = app.manager.catalog();
                theme::chip(ui, &catalog.category_name(&s.app.category), p.weak);
                theme::chip(ui, "Open source", p.good);
                if s.app.custom {
                    ui.scope(|ui| theme::chip(ui, "From GitHub", p.accent))
                        .response
                        .on_hover_text(format!("github.com/{}", s.app.repo));
                } else if let Some(repo) =
                    app.settings.other_sources.overrides.get(&s.app.id).filter(|_| app.settings.other_sources.enabled)
                {
                    ui.scope(|ui| theme::chip(ui, &format!("Updates from {repo}"), p.warn)).response.on_hover_text(
                        "Updated from this repository instead of the official one (Settings › Other sources)",
                    );
                }
                if let Some(external) = s.installed.as_ref().and_then(|i| i.current.external.as_ref()) {
                    let (label, tip) = match &external.flatpak {
                        Some(id) => (
                            "Updated by Flatpak".to_string(),
                            format!("Installed from Flatpak ({id}); update it with your software center"),
                        ),
                        None => (
                            "Found on this computer".to_string(),
                            format!("Installed {} without CraftSpace; CraftSpace updates it there", external.how),
                        ),
                    };
                    ui.scope(|ui| theme::chip(ui, &label, p.accent)).response.on_hover_text(tip);
                }
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if app.jobs.contains_key(id) {
                ui.allocate_ui_with_layout(Vec2::new(260.0, 40.0), Layout::top_down(Align::Min), |ui| {
                    job_row_for(app, ui, id)
                });
            } else {
                main_button(app, ui, &s);
                more_menu(app, ui, &s);
            }
        });
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        for (tab, label) in
            [(DetailTab::Overview, "Overview"), (DetailTab::Versions, "Versions"), (DetailTab::Readme, "README")]
        {
            let selected = app.detail_tab == tab;
            let r = ui.add(
                egui::Button::new(RichText::new(label).size(14.5).color(if selected { p.text } else { p.weak }))
                    .frame(false),
            );
            if selected {
                let y = r.rect.bottom() + 3.0;
                ui.painter().line_segment(
                    [egui::pos2(r.rect.left() + 6.0, y), egui::pos2(r.rect.right() - 6.0, y)],
                    egui::Stroke::new(2.0, p.accent),
                );
            }
            if r.clicked() {
                app.detail_tab = tab;
            }
        }
    });
    ui.add_space(10.0);
    match app.detail_tab {
        DetailTab::Overview => overview(app, ui, &s),
        DetailTab::Versions => versions(app, ui, &s),
        DetailTab::Readme => readme(app, ui, &s),
    }
    ui.add_space(30.0);
}

fn overview(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState) {
    let p = app.palette;
    let id = s.app.id.as_str();
    super::tour::show(app, ui, s);
    if !s.app.description.is_empty()
        && !app.tours.get(id).is_some_and(|t| t.as_ref().is_ok_and(|t| !t.paragraphs.is_empty()))
    {
        ui.add(egui::Label::new(RichText::new(&s.app.description).size(14.5)).wrap());
        ui.add_space(12.0);
    }
    theme::card_frame(&p, false).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("facts").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
            let row = |ui: &mut Ui, k: &str, v: String| {
                ui.label(RichText::new(k).color(p.weak));
                ui.add(egui::Label::new(v).wrap());
                ui.end_row();
            };
            if let Some(i) = &s.installed {
                row(ui, "Installed", format!("{} ({})", i.current.version, i.current.kind.label()));
                if let Some(prev) = &i.previous {
                    row(ui, "Kept for rollback", prev.version.to_string());
                }
                if let Some(size) = i.current.size_bytes {
                    row(ui, "Size on disk", format_bytes(size));
                }
                if let Some(exe) = &i.current.executable {
                    row(ui, "Program", exe.display().to_string());
                }
                if let Some(report) = app.verify_results.get(id) {
                    let text = if report.is_ok() {
                        format!("All {} files intact", report.checked)
                    } else {
                        format!("{} missing, {} changed", report.missing.len(), report.changed.len())
                    };
                    row(ui, "Last check", text);
                }
            }
            match s.latest_version() {
                Some(v) => row(
                    ui,
                    "Latest",
                    format!(
                        "{v}{}",
                        s.latest.as_ref().and_then(|r| r.date()).map(|d| format!(" · {d}")).unwrap_or_default()
                    ),
                ),
                None if !s.known => row(ui, "Latest", "Checking…".into()),
                None => row(ui, "Latest", "No release yet".into()),
            }
            if let Some((asset, kind)) = &s.installable {
                let size = asset.size.map(|n| format!(" · {}", format_bytes(n))).unwrap_or_default();
                let verified =
                    if asset.sha256.is_some() { " · SHA-256 verified" } else { " · no checksum published" };
                row(ui, "Package", format!("{} ({}{size}{verified})", asset.name, kind.label()));
            }
            // Update channel.
            ui.label(RichText::new("Updates").color(p.weak));
            let mut channel = s.channel.clone();
            let installed_version = s.installed_version().cloned();
            if app.manager.policy().locks_channel(id) || app.managed() {
                ui.label(match &channel {
                    Channel::Pinned(v) => format!("Kept on {v} by {}", app.organization()),
                    other => format!("{} (set by {})", other.label(), app.organization()),
                });
                ui.end_row();
            } else {
                egui::ComboBox::from_id_salt(("channel", id)).selected_text(channel.label()).show_ui(ui, |ui| {
                    ui.selectable_value(&mut channel, Channel::Default, "Default (as in Settings)");
                    ui.selectable_value(&mut channel, Channel::Stable, "Stable releases");
                    ui.selectable_value(&mut channel, Channel::Prerelease, "Pre-releases too");
                    if let Some(v) = installed_version {
                        let label = format!("Stay on {v}");
                        ui.selectable_value(&mut channel, Channel::Pinned(v), label);
                    }
                });
                if channel != s.channel {
                    app.actions.push(Action::SetChannel(id.to_string(), channel));
                }
                ui.end_row();
            }
            if !s.app.extensions.is_empty() {
                row(ui, "Opens", s.app.extensions.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join("  "));
            }
            row(ui, "Source", s.app.github_url());
        });
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if ui.button("Report a problem ↗").clicked() {
            app.actions.push(Action::ReportBug(id.to_string()));
        }
        if s.installed.is_some() && ui.button("Verify and repair").clicked() {
            app.actions.push(Action::VerifyAndRepair(id.to_string()));
        }
    });

    // What's new in the latest release.
    if let Some(r) = s.latest.as_ref().filter(|r| !r.body.trim().is_empty()) {
        ui.add_space(14.0);
        ui.label(RichText::new(format!("What's new in {}", r.name)).size(17.0).strong());
        theme::card_frame(&p, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui_commonmark::CommonMarkViewer::new().max_image_width(Some(720)).show(ui, &mut app.md_cache, &r.body);
        });
    }
}

fn versions(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState) {
    let p = app.palette;
    let id = s.app.id.as_str();
    let Some(list) = app.manager.releases(id) else {
        ui.label(RichText::new("No release information yet.").color(p.weak));
        return;
    };
    let settings = app.settings.clone();
    let prefs = settings.asset_prefs(&s.app);
    if list.source == craftspace_core::github::Source::LatestRedirect {
        ui.label(
            RichText::new("Only the latest release is listed while the GitHub API is unavailable.")
                .size(12.5)
                .color(p.weak),
        );
    }
    ui.add_space(4.0);
    let selected_tag = app.detail_release.clone().or_else(|| s.latest.as_ref().map(|r| r.tag.clone()));
    let show_pre = settings.wants_prereleases(id);
    for r in list.releases.iter().take(20) {
        if r.prerelease
            && !show_pre
            && s.installed.as_ref().is_none_or(|i| Some(&i.current.version) != r.version.as_ref())
        {
            continue;
        }
        let asset = app.manager.platform().select_asset(id, &r.asset_names(), prefs);
        let is_current = s.installed.as_ref().is_some_and(|i| Some(&i.current.version) == r.version.as_ref());
        let selected = selected_tag.as_deref() == Some(r.tag.as_str());
        let frame = theme::card_frame(&p, selected).inner_margin(egui::Margin::symmetric(14, 8));
        let row = theme::click_area(ui, Vec2::new(ui.available_width(), 46.0), ("release", &r.tag));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&r.name).strong());
                if let Some(d) = r.date() {
                    ui.label(RichText::new(d).color(p.weak));
                }
                if r.prerelease {
                    theme::chip(ui, "Pre-release", p.warn);
                }
                if is_current {
                    theme::chip(ui, "Installed", p.good);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let (Some((_, kind)), false, Some(v)) = (asset, is_current, r.version.clone()) {
                        let label = match s.installed_version() {
                            Some(cur) if r.version.as_ref().is_some_and(|rv| rv > cur) => "Update to this version",
                            Some(_) => "Switch to this version",
                            None => "Install this version",
                        };
                        if !app.jobs.contains_key(id) && ui.button(label).on_hover_text(kind.label()).clicked() {
                            app.actions.push(Action::Install(id.to_string(), Some(v)));
                        }
                    } else if asset.is_none() {
                        ui.label(RichText::new("No build for this platform").size(12.0).color(p.weak));
                    }
                });
            });
        });
        if row.clicked() {
            app.detail_release = Some(r.tag.clone());
        }
        ui.add_space(4.0);
    }

    if let Some(r) = list.releases.iter().find(|r| Some(&r.tag) == selected_tag.as_ref()) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("What's new in {}", r.name)).size(17.0).strong());
            if ui.add(egui::Button::new(RichText::new("View on GitHub ↗").color(p.accent)).frame(false)).clicked() {
                app.actions.push(Action::OpenUrl(r.html_url.clone()));
            }
        });
        theme::card_frame(&p, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if r.body.trim().is_empty() {
                ui.label(RichText::new("Release notes aren't available offline or without the GitHub API. Open the release on GitHub to read them.").color(p.weak));
            } else {
                egui_commonmark::CommonMarkViewer::new().max_image_width(Some(720)).show(ui, &mut app.md_cache, &r.body);
            }
        });
        if let Some((_, kind)) = app.manager.platform().select_asset(id, &r.asset_names(), prefs) {
            if !kind.is_managed() && kind != AssetKind::Dmg {
                ui.label(
                    RichText::new(format!("This version installs with the {}.", kind.label())).size(12.0).color(p.weak),
                );
            }
        }
    }
}

fn readme(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState) {
    let p = app.palette;
    app.ensure_readme(&s.app.id);
    match app.readmes.get(&s.app.id).cloned() {
        None => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Loading the README…").color(p.weak));
            });
        }
        Some(Err(err)) => {
            ui.label(RichText::new(format!("Couldn't load the README: {err}")).color(p.weak));
        }
        Some(Ok(text)) => {
            theme::card_frame(&p, false).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui_commonmark::CommonMarkViewer::new().max_image_width(Some(760)).show(ui, &mut app.md_cache, &text);
            });
        }
    }
}

/// Pick several apps and install them in one go.
pub fn multi_install_modal(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    let p = app.palette;
    let Some(mut selected) = app.multi_install.take() else { return };
    let candidates: Vec<AppState> =
        app.states.iter().filter(|s| s.installed.is_none() && s.installable.is_some()).cloned().collect();
    let mut keep = true;
    let modal = egui::Modal::new(egui::Id::new("multi-install")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading("Install several apps");
        ui.label(
            RichText::new(format!("They download {} at a time.", app.settings.max_parallel_downloads.max(1)))
                .color(p.weak),
        );
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            for s in &candidates {
                ui.horizontal(|ui| {
                    let mut on = selected.contains(&s.app.id);
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            selected.insert(s.app.id.clone());
                        } else {
                            selected.remove(&s.app.id);
                        }
                    }
                    theme::badge(ui, &s.app, 28.0);
                    ui.label(RichText::new(&s.app.name).strong());
                    let size = s.installable.as_ref().and_then(|(a, _)| a.size).map(format_bytes).unwrap_or_default();
                    ui.label(RichText::new(size).size(12.0).color(p.weak));
                });
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Select all").clicked() {
                selected = candidates.iter().map(|s| s.app.id.clone()).collect();
            }
            if ui.button("None").clicked() {
                selected.clear();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = selected.len();
                if ui
                    .add_enabled(
                        n > 0,
                        egui::Button::new(RichText::new(format!("Install {n}")).color(p.accent_text).strong())
                            .fill(p.accent)
                            .corner_radius(15),
                    )
                    .clicked()
                {
                    app.actions.push(Action::InstallMany(selected.iter().cloned().collect()));
                    keep = false;
                }
                if theme::pill(ui, &p, "Cancel").clicked() {
                    keep = false;
                }
            });
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if keep {
        app.multi_install = Some(selected);
    }
}
