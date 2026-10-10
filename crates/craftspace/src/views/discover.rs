//! The Discover tab: a featured app, what's new across all apps, apps to try, and links.

use craftspace_core::AppState;
use eframe::egui::{self, Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Ui, Vec2};

use crate::app::{Action, CraftSpaceApp};
use crate::theme;

pub fn content(app: &mut CraftSpaceApp, ui: &mut Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(1100.0);
        hero(app, ui);
        ui.add_space(22.0);
        whats_new(app, ui);
        ui.add_space(22.0);
        learn(app, ui);
        ui.add_space(22.0);
        apps_to_try(app, ui);
        ui.add_space(22.0);
        links(app, ui);
        ui.add_space(30.0);
    });
}

/// The app with the newest release, or the catalog's featured app.
fn featured(app: &CraftSpaceApp) -> Option<AppState> {
    let newest = app
        .states
        .iter()
        .filter(|s| s.latest.as_ref().and_then(|r| r.published_at.as_ref()).is_some())
        .max_by_key(|s| s.latest.as_ref().and_then(|r| r.published_at.clone()));
    newest.or_else(|| app.states.iter().find(|s| s.app.featured)).or_else(|| app.states.first()).cloned()
}

fn hero(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let Some(s) = featured(app) else { return };
    let fg = theme::hex(&s.app.colors.fg);
    let bg = theme::hex(&s.app.colors.bg);
    let width = ui.available_width();
    let height = 200.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

    // Gradient from the app's dark color into its accent.
    let mut mesh = egui::Mesh::default();
    let mix = |t: f32| {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
        Color32::from_rgb(l(bg.r(), fg.r()), l(bg.g(), fg.g()), l(bg.b(), fg.b()))
    };
    mesh.colored_vertex(rect.left_top(), bg);
    mesh.colored_vertex(rect.right_top(), mix(0.55));
    mesh.colored_vertex(rect.right_bottom(), mix(0.35));
    mesh.colored_vertex(rect.left_bottom(), bg);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    let painter = ui.painter_at(rect);
    painter.add(egui::Shape::mesh(mesh));
    // Big translucent badge on the right as artwork.
    let art = egui::Rect::from_center_size(rect.right_center() - Vec2::new(150.0, 0.0), Vec2::splat(150.0));
    match theme::icon_texture(ui.ctx(), &s.app.id) {
        Some(texture) => egui::Image::new(&texture).corner_radius(CornerRadius::same(32)).paint_at(ui, art),
        None => {
            painter.rect_filled(art, CornerRadius::same(30), bg.gamma_multiply(0.85));
            painter.rect_stroke(
                art.shrink(6.0),
                CornerRadius::same(26),
                egui::Stroke::new(6.0, fg),
                egui::StrokeKind::Inside,
            );
            painter.text(art.center(), egui::Align2::CENTER_CENTER, &s.app.code, egui::FontId::proportional(68.0), fg);
        }
    }

    let mut text_rect = rect.shrink2(Vec2::new(32.0, 28.0));
    text_rect.max.x = (art.left() - 40.0).max(text_rect.min.x + 200.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::top_down(Align::Min)));
    let p = app.palette;
    child.label(RichText::new(format!("Make it in {}", s.app.name)).size(28.0).strong().color(Color32::WHITE));
    child.add_space(2.0);
    child.label(RichText::new(&s.app.description).size(14.0).color(Color32::from_white_alpha(220)));
    if let Some(r) = &s.latest {
        child.label(
            RichText::new(format!("New: {}{}", r.name, r.date().map(|d| format!(" · {d}")).unwrap_or_default()))
                .size(12.5)
                .color(Color32::from_white_alpha(190)),
        );
    }
    child.add_space(10.0);
    child.horizontal(|ui| {
        let label = if s.update_available {
            "Update"
        } else if s.installed.is_some() {
            "Open"
        } else {
            "Install"
        };
        let b = ui.add(
            egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(1.5, Color32::WHITE))
                .corner_radius(15)
                .min_size(Vec2::new(96.0, 30.0)),
        );
        if b.clicked() {
            app.actions.push(match label {
                "Update" => Action::Update(s.app.id.clone()),
                "Open" => Action::Launch(s.app.id.clone(), vec![]),
                _ => Action::Install(s.app.id.clone(), None),
            });
        }
        if ui.add(egui::Button::new(RichText::new("Learn more").color(Color32::WHITE)).frame(false)).clicked() {
            app.actions.push(Action::ShowApp(s.app.id.clone()));
        }
        let _ = p;
    });
}

fn whats_new(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    ui.label(RichText::new("What's new").size(18.0).strong());
    ui.label(RichText::new("The latest releases across the ArtCraft apps.").color(p.weak));
    ui.add_space(6.0);
    let mut items: Vec<AppState> = app.states.iter().filter(|s| s.latest.is_some()).cloned().collect();
    items.sort_by(|a, b| {
        let key = |s: &AppState| s.latest.as_ref().and_then(|r| r.published_at.clone());
        key(b).cmp(&key(a)).then_with(|| a.app.name.cmp(&b.app.name))
    });
    if items.is_empty() {
        ui.label(
            RichText::new(if app.refreshing {
                "Loading…"
            } else {
                "No release information yet. Check your connection and refresh."
            })
            .color(p.weak),
        );
        return;
    }
    let spacing = 14.0;
    let cols = ((ui.available_width() + spacing) / (340.0 + spacing)).floor().max(1.0) as usize;
    let w = (ui.available_width() - spacing * (cols as f32 - 1.0)) / cols as f32;
    for row in items.chunks(cols).take(3) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            // Short cards when there are no notes to show (e.g. while the GitHub API is unavailable).
            let has_notes = row.iter().any(|s| s.latest.as_ref().is_some_and(|r| !r.body.trim().is_empty()));
            let h = if has_notes { 150.0 } else { 104.0 };
            for s in row {
                let r = s.latest.as_ref().expect("filtered");
                let resp = ui.allocate_ui_with_layout(Vec2::new(w, h), Layout::top_down(Align::Min), |ui| {
                    theme::card_frame(&p, false).show(ui, |ui| {
                        ui.set_width(w - 34.0);
                        ui.set_height(h - 34.0);
                        ui.horizontal(|ui| {
                            theme::badge(ui, &s.app, 30.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} {}",
                                        s.app.name,
                                        s.latest_version().map(|v| v.to_string()).unwrap_or_default()
                                    ))
                                    .strong(),
                                );
                                ui.label(RichText::new(r.date().unwrap_or("Latest release")).size(12.0).color(p.weak));
                            });
                        });
                        let excerpt = excerpt(&r.body, 180);
                        ui.add(
                            egui::Label::new(
                                RichText::new(if excerpt.is_empty() { s.app.tagline.clone() } else { excerpt })
                                    .size(13.0)
                                    .color(p.weak),
                            )
                            .wrap(),
                        );
                    })
                });
                if resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.actions.push(Action::ShowApp(s.app.id.clone()));
                }
            }
        });
        ui.add_space(spacing - 8.0);
    }
}

/// Plain-text start of Markdown release notes.
fn excerpt(md: &str, max: usize) -> String {
    let text: String = md
        .lines()
        .map(|l| l.trim().trim_start_matches(['#', '-', '*', '>', ' ']).trim())
        .filter(|l| !l.is_empty() && !l.starts_with("```") && !l.starts_with('|') && !l.starts_with("<"))
        .collect::<Vec<_>>()
        .join(" · ")
        .replace("**", "")
        .replace('`', "");
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>().trim_end())
    } else {
        text
    }
}

/// News and tutorials from the ArtCraft website.
fn learn(app: &mut CraftSpaceApp, ui: &mut Ui) {
    use craftspace_core::news::ArticleKind;
    let p = app.palette;
    if app.articles.is_empty() {
        return;
    }
    for (kind, title, blurb) in [
        (ArticleKind::News, "News", "From the ArtCraft blog."),
        (ArticleKind::Tutorial, "Tutorials", "Learn the tools step by step."),
    ] {
        let items: Vec<_> = app.articles.iter().filter(|a| a.kind == kind).cloned().collect();
        if items.is_empty() {
            continue;
        }
        ui.label(RichText::new(title).size(18.0).strong());
        ui.label(RichText::new(blurb).color(p.weak));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
            for a in items.iter().take(6) {
                let size = Vec2::new(330.0, 112.0);
                let tile = theme::click_area(ui, size, ("article", &a.url));
                let hovered = ui.rect_contains_pointer(tile.rect);
                ui.allocate_ui_with_layout(size, Layout::top_down(Align::Min), |ui| {
                    theme::card_frame(&p, hovered).show(ui, |ui| {
                        ui.set_width(size.x - 34.0);
                        ui.set_height(size.y - 34.0);
                        if let Some(d) = &a.date {
                            ui.label(RichText::new(d).size(12.0).color(p.weak));
                        }
                        ui.add(egui::Label::new(RichText::new(&a.title).strong()).truncate());
                        ui.add(egui::Label::new(RichText::new(&a.description).size(12.5).color(p.weak)).wrap());
                    });
                });
                if tile.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.actions.push(Action::OpenUrl(a.url.clone()));
                }
            }
        });
        ui.add_space(18.0);
    }
}

fn apps_to_try(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    let to_try: Vec<AppState> = app.states.iter().filter(|s| s.installed.is_none()).cloned().collect();
    if to_try.is_empty() {
        return;
    }
    ui.label(RichText::new("Apps to try").size(18.0).strong());
    ui.label(RichText::new("Free and open source, installed per user, no admin rights needed.").color(p.weak));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
        for s in &to_try {
            let tile = theme::click_area(ui, Vec2::new(250.0, 64.0), ("try", &s.app.id));
            ui.allocate_ui_with_layout(Vec2::new(250.0, 64.0), Layout::top_down(Align::Min), |ui| {
                theme::card_frame(&p, false).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
                    ui.set_width(226.0);
                    ui.horizontal(|ui| {
                        theme::badge(ui, &s.app, 36.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&s.app.name).strong());
                            ui.label(RichText::new(&s.app.tagline).size(12.0).color(p.weak));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if s.installable.is_some()
                                && !app.jobs.contains_key(&s.app.id)
                                && ui
                                    .add(egui::Button::new(RichText::new("Install").size(12.5)).corner_radius(12))
                                    .clicked()
                            {
                                app.actions.push(Action::Install(s.app.id.clone(), None));
                            }
                            if app.jobs.contains_key(&s.app.id) {
                                ui.add(egui::Spinner::new().size(14.0));
                            }
                        });
                    });
                })
            });
            if tile.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.actions.push(Action::ShowApp(s.app.id.clone()));
            }
        }
    });
}

fn links(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    ui.label(RichText::new("Learn and connect").size(18.0).strong());
    ui.add_space(6.0);
    let catalog = app.manager.catalog();
    // Fixed-size cards in rows, so long titles never squeeze into a narrow column.
    let card = Vec2::new(250.0, 64.0);
    let gap = 12.0;
    let per_row = (((ui.available_width() + gap) / (card.x + gap)).floor() as usize).max(1);
    for row in catalog.links.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for link in row {
                let (rect, resp) = ui.allocate_exact_size(card, Sense::click());
                let hovered = resp.hovered();
                ui.painter().rect(
                    rect,
                    CornerRadius::same(10),
                    if hovered { p.card_hover } else { p.card },
                    egui::Stroke::new(1.0, p.stroke),
                    egui::StrokeKind::Inside,
                );
                let mut inner = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.shrink2(Vec2::new(16.0, 12.0)))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                inner.add(
                    egui::Label::new(RichText::new(format!("{}  ↗", link.title)).strong()).truncate().selectable(false),
                );
                inner.add(
                    egui::Label::new(RichText::new(link.url.trim_start_matches("https://")).size(12.0).color(p.weak))
                        .truncate()
                        .selectable(false),
                );
                if resp.on_hover_text(&link.url).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.actions.push(Action::OpenUrl(link.url.clone()));
                }
            }
        });
        ui.add_space(gap);
    }
}

#[cfg(test)]
mod tests {
    use super::excerpt;

    #[test]
    fn excerpts_markdown() {
        let md = "## Highlights\n\n- **Faster** brushes\n- New `Liquify` filter\n\n```\ncode\n```";
        assert_eq!(excerpt(md, 100), "Highlights · Faster brushes · New Liquify filter · code");
        assert_eq!(excerpt("abcdef", 3), "abc…");
    }
}
