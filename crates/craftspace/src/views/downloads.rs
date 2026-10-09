//! The Downloads button in the top bar and its panel: every running, queued and waiting job with
//! one combined progress bar.

use craftspace_core::download::format_bytes;
use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, Ui, Vec2};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

/// Combined progress over every job that knows its size.
fn overall(app: &CraftSpaceApp) -> (u64, u64) {
    app.jobs.values().filter(|j| !j.queued).fold((0, 0), |(d, t), j| match j.total {
        Some(total) => (d + j.done.min(total), t + total),
        None => (d, t),
    })
}

pub fn button(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let (done, total) = overall(app);
    let painter = ui.painter();
    let c = rect.center();
    painter.circle_stroke(c, 11.0, Stroke::new(2.5, p.stroke));
    if total > 0 {
        let frac = done as f32 / total as f32;
        let points: Vec<egui::Pos2> = (0..=48)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU * i as f32 / 48.0;
                c + 11.0 * Vec2::angled(a)
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(2.5, p.accent)));
    }
    // A down arrow.
    painter.line_segment([c + Vec2::new(0.0, -5.0), c + Vec2::new(0.0, 4.0)], Stroke::new(1.8, p.text));
    painter.line_segment([c + Vec2::new(-3.5, 1.0), c + Vec2::new(0.0, 4.5)], Stroke::new(1.8, p.text));
    painter.line_segment([c + Vec2::new(3.5, 1.0), c + Vec2::new(0.0, 4.5)], Stroke::new(1.8, p.text));
    let n = app.jobs.len() + app.waiting_for_close.len();
    let hover = format!("{n} download{} and install{}", if n == 1 { "" } else { "s" }, if n == 1 { "" } else { "s" });
    if response.on_hover_text(hover).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.downloads_open = !app.downloads_open;
    }
}

pub fn panel(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    let p = app.palette;
    if app.jobs.is_empty() && app.waiting_for_close.is_empty() {
        app.downloads_open = false;
        return;
    }
    let mut open = true;
    egui::Window::new(RichText::new("Downloads").size(16.0).strong())
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-16.0, 64.0))
        .default_width(380.0)
        .show(ctx, |ui| {
            let (done, total) = overall(app);
            if total > 0 {
                ui.label(RichText::new(format!("{} of {}", format_bytes(done), format_bytes(total))).color(p.weak));
                ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0).fill(p.accent));
                ui.add_space(6.0);
            }
            let limit =
                app.settings.download_limit_kbps.map(|k| format!(" · limited to {} KB/s", k)).unwrap_or_default();
            ui.label(
                RichText::new(format!("{} at a time{limit}", app.settings.max_parallel_downloads.max(1)))
                    .size(12.0)
                    .color(p.weak),
            );
            ui.separator();
            for key in app.job_order.clone() {
                let Some(job) = app.jobs.get(&key) else { continue };
                let icon = app.manager.app(&key);
                let mut cancel = false;
                ui.horizontal(|ui| {
                    match &icon {
                        Some(a) => {
                            theme::badge(ui, a, 28.0);
                        }
                        None => {
                            ui.add_space(28.0);
                        }
                    }
                    ui.vertical(|ui| {
                        ui.set_width(270.0);
                        ui.label(RichText::new(&job.label).strong());
                        if job.queued {
                            ui.label(RichText::new("Waiting for a free slot").size(12.0).color(p.weak));
                        } else {
                            super::apps::job_row_body(&p, ui, job);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        cancel = ui.add(egui::Button::new("×").frame(false)).on_hover_text("Cancel").clicked();
                    });
                });
                if cancel {
                    app.actions.push(Action::Cancel(key));
                }
                ui.add_space(4.0);
            }
            for (id, kind) in app.waiting_for_close.clone() {
                ui.horizontal(|ui| {
                    if let Some(a) = app.manager.app(&id) {
                        theme::badge(ui, &a, 28.0);
                    }
                    ui.vertical(|ui| {
                        ui.set_width(270.0);
                        ui.label(RichText::new(app.app_name(&id)).strong());
                        ui.label(RichText::new(format!("{} when you close it", kind.verb())).size(12.0).color(p.weak));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new("×").frame(false)).on_hover_text("Don't").clicked() {
                            app.actions.push(Action::Cancel(id.clone()));
                        }
                    });
                });
            }
        });
    if !open {
        app.downloads_open = false;
    }
}
