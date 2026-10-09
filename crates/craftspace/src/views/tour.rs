//! The screenshot carousel and feature list at the top of an app's Overview.

use craftspace_core::AppState;
use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Ui, Vec2};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

pub fn show(app: &mut CraftSpaceApp, ui: &mut Ui, s: &AppState) {
    let p = app.palette;
    let id = s.app.id.clone();
    app.ensure_tour(&id);
    let Some(Ok(tour)) = app.tours.get(&id).cloned() else {
        if !app.tours.contains_key(&id) {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(RichText::new("Loading screenshots…").color(p.weak));
            });
            ui.add_space(8.0);
        }
        return;
    };

    if !tour.slides.is_empty() {
        let n = tour.slides.len();
        let index = app.tour_index.get(&id).copied().unwrap_or(0).min(n - 1);
        let slide = tour.slides[index].clone();
        app.ensure_shot(&slide.image);
        // Load the next one too, so paging feels instant.
        if n > 1 {
            app.ensure_shot(&tour.slides[(index + 1) % n].image);
        }

        let width = ui.available_width().min(960.0);
        let shot = app.shots.get(&slide.image).cloned();
        let height = match &shot {
            Some(Ok(t)) => (width * t.size()[1] as f32 / t.size()[0] as f32).min(560.0),
            _ => width * 9.0 / 16.0,
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(10), p.card);
        match &shot {
            Some(Ok(texture)) => {
                // Fit inside the frame, keeping the aspect ratio.
                let size = texture.size_vec2();
                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                let img_rect = egui::Rect::from_center_size(rect.center(), size * scale);
                egui::Image::new(texture).corner_radius(CornerRadius::same(10)).paint_at(ui, img_rect);
            }
            Some(Err(err)) => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("Couldn't load this screenshot: {err}"),
                    egui::FontId::proportional(13.0),
                    p.weak,
                );
            }
            None => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading…",
                    egui::FontId::proportional(14.0),
                    p.weak,
                );
            }
        }
        if response
            .on_hover_text("Open the full-size picture")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            app.actions.push(Action::OpenUrl(slide.image.clone()));
        }

        // Previous / next arrows over the picture.
        if n > 1 {
            for (dir, x) in [(-1i32, rect.left() + 26.0), (1, rect.right() - 26.0)] {
                let c = egui::pos2(x, rect.center().y);
                let r = ui.interact(
                    egui::Rect::from_center_size(c, Vec2::splat(36.0)),
                    ui.id().with(("tour-arrow", dir, &id)),
                    Sense::click(),
                );
                let alpha = if r.hovered() { 220 } else { 150 };
                ui.painter().circle_filled(c, 17.0, Color32::from_black_alpha(alpha));
                let s = 5.0 * dir as f32;
                ui.painter().line_segment(
                    [c + Vec2::new(-s * 0.5, -7.0), c + Vec2::new(s * 0.7, 0.0)],
                    Stroke::new(2.5, Color32::WHITE),
                );
                ui.painter().line_segment(
                    [c + Vec2::new(s * 0.7, 0.0), c + Vec2::new(-s * 0.5, 7.0)],
                    Stroke::new(2.5, Color32::WHITE),
                );
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    let next = (index as i32 + dir).rem_euclid(n as i32) as usize;
                    app.tour_index.insert(id.clone(), next);
                }
            }
        }

        ui.add_space(6.0);
        ui.allocate_ui_with_layout(Vec2::new(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.horizontal(|ui| {
                if let Some(section) = &slide.section {
                    theme::chip(ui, section, p.accent);
                }
                ui.label(RichText::new(format!("{} of {n}", index + 1)).size(12.0).color(p.weak));
                // Dots.
                if n > 1 && n <= 30 {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for i in (0..n).rev() {
                            let (r, resp) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::click());
                            ui.painter().circle_filled(
                                r.center(),
                                if i == index { 4.0 } else { 3.0 },
                                if i == index { p.accent } else { p.stroke },
                            );
                            if resp.clicked() {
                                app.tour_index.insert(id.clone(), i);
                            }
                        }
                    });
                }
            });
            if !slide.caption.is_empty() {
                ui.add(egui::Label::new(RichText::new(&slide.caption).color(p.weak)).wrap());
            }
        });
        ui.add_space(14.0);
    }

    for para in &tour.paragraphs {
        ui.add(egui::Label::new(RichText::new(para).size(14.5)).wrap());
        ui.add_space(4.0);
    }
    if !tour.features.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new("Highlights").size(16.0).strong());
        for f in &tour.features {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("•").color(p.accent));
                ui.add(egui::Label::new(f).wrap());
            });
        }
    }
    if !tour.links.is_empty() {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for (kind, url) in &tour.links {
                let label = match kind.as_str() {
                    "homepage" => "Website ↗",
                    "bugtracker" => "Issues ↗",
                    "help" => "Documentation ↗",
                    "contact" => "Community ↗",
                    "donation" => "Donate ↗",
                    "translate" => "Translate ↗",
                    _ => continue,
                };
                if ui.add(egui::Button::new(RichText::new(label).color(p.accent)).frame(false)).clicked() {
                    app.actions.push(Action::OpenUrl(url.clone()));
                }
            }
        });
    }
    ui.add_space(12.0);
}
