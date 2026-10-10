//! Colors, the egui style, and small shared widgets (app badges, pill buttons, cards).

use craftspace_core::settings::Theme;
use craftspace_core::AppEntry;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Margin, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub stroke: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub dark: bool,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x1B, 0x1B, 0x1D),
    panel: Color32::from_rgb(0x23, 0x23, 0x26),
    card: Color32::from_rgb(0x2A, 0x2A, 0x2E),
    card_hover: Color32::from_rgb(0x32, 0x32, 0x37),
    stroke: Color32::from_rgb(0x3A, 0x3A, 0x40),
    text: Color32::from_rgb(0xEC, 0xEC, 0xEE),
    weak: Color32::from_rgb(0xA0, 0xA0, 0xA8),
    // The lemon lime of CraftSpace's octopus logo, with dark text on it.
    accent: Color32::from_rgb(0xD4, 0xF4, 0x24),
    accent_text: Color32::from_rgb(0x17, 0x19, 0x0A),
    good: Color32::from_rgb(0x34, 0xC7, 0x7B),
    warn: Color32::from_rgb(0xF5, 0xA5, 0x24),
    bad: Color32::from_rgb(0xF0, 0x5A, 0x5A),
    dark: true,
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF4, 0xF4, 0xF6),
    panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    card_hover: Color32::from_rgb(0xF7, 0xF8, 0xFA),
    stroke: Color32::from_rgb(0xDE, 0xDE, 0xE3),
    text: Color32::from_rgb(0x1D, 0x1D, 0x22),
    weak: Color32::from_rgb(0x6B, 0x6B, 0x75),
    // The logo's lime, deepened so it reads on white.
    accent: Color32::from_rgb(0x5C, 0x7F, 0x00),
    accent_text: Color32::WHITE,
    good: Color32::from_rgb(0x1E, 0x9E, 0x5A),
    warn: Color32::from_rgb(0xC2, 0x7A, 0x00),
    bad: Color32::from_rgb(0xD0, 0x3B, 0x3B),
    dark: false,
};

pub fn palette_for(theme: Theme, ctx: &egui::Context) -> Palette {
    match theme {
        Theme::Dark => DARK,
        Theme::Light => LIGHT,
        Theme::System => {
            if ctx.system_theme() == Some(egui::Theme::Light) {
                LIGHT
            } else {
                DARK
            }
        }
    }
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut visuals = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.panel;
    visuals.extreme_bg_color =
        if p.dark { Color32::from_rgb(0x15, 0x15, 0x17) } else { Color32::from_rgb(0xEC, 0xEE, 0xF2) };
    visuals.faint_bg_color = p.card;
    visuals.window_stroke = Stroke::new(1.0, p.stroke);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.hyperlink_color = p.accent;
    visuals.override_text_color = Some(p.text);
    for w in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.stroke);
    visuals.widgets.inactive.weak_bg_fill = p.card;
    // Checkboxes and sliders use `bg_fill`; keep them visible on cards and dialogs.
    visuals.widgets.inactive.bg_fill = p.stroke;
    visuals.widgets.hovered.weak_bg_fill = p.card_hover;
    ctx.set_visuals(visuals);

    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        style.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(24.0));
        style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
    });
}

pub fn hex(color: &str) -> Color32 {
    Color32::from_hex(color).unwrap_or(Color32::GRAY)
}

fn icon_id(app_id: &str) -> egui::Id {
    egui::Id::new(("app-icon", app_id))
}

/// CraftSpace's own logo as a texture (loaded once).
pub fn logo(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("craftspace-logo");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = image::load_from_memory(include_bytes!("../../../assets/craftspace-64.png"))
        .expect("bundled logo decodes")
        .to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    let texture = ctx.load_texture("craftspace-logo", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// The logo at 256 px, for the About window.
pub fn logo_large(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("craftspace-logo-256");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    let img = image::load_from_memory(craftspace_core::selfupdate::ICON_PNG).expect("bundled logo decodes").to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    let texture = ctx.load_texture("craftspace-logo-256", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

pub fn icon_texture(ctx: &egui::Context, app_id: &str) -> Option<egui::TextureHandle> {
    ctx.data(|d| d.get_temp(icon_id(app_id)))
}

/// Turn cached icon PNGs into textures (all apps, or just `only`).
pub fn load_icons(ctx: &egui::Context, manager: &craftspace_core::Manager, only: Option<&[String]>) {
    for app in manager.catalog().apps {
        if only.is_some_and(|ids| !ids.contains(&app.id)) {
            continue;
        }
        let Some(bytes) = manager.icon(&app.id) else { continue };
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        let img = img.resize(128, 128, image::imageops::FilterType::Lanczos3).to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        let texture = ctx.load_texture(format!("icon-{}", app.id), color, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(icon_id(&app.id), texture));
    }
}

/// The app's own icon, or (until it's downloaded) a rounded square with its two-letter code.
pub fn badge(ui: &mut Ui, app: &AppEntry, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let texture: Option<egui::TextureHandle> = ui.ctx().data(|d| d.get_temp(icon_id(&app.id)));
    if let Some(texture) = texture {
        if ui.is_rect_visible(rect) {
            egui::Image::new(&texture).corner_radius(CornerRadius::same((size * 0.22) as u8)).paint_at(ui, rect);
        }
        return response;
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let radius = CornerRadius::same((size * 0.2) as u8);
        let fg = hex(&app.colors.fg);
        painter.rect_filled(rect, radius, hex(&app.colors.bg));
        painter.rect_stroke(
            rect.shrink(size * 0.04),
            radius,
            Stroke::new((size * 0.045).max(1.0), fg),
            StrokeKind::Inside,
        );
        painter.text(rect.center(), Align2::CENTER_CENTER, &app.code, FontId::proportional(size * 0.44), fg);
    }
    response
}

/// A rounded, outlined button like Creative Cloud's "Open".
pub fn pill(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.text))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.5, p.weak))
            .corner_radius(CornerRadius::same(15))
            .min_size(Vec2::new(64.0, 30.0)),
    )
}

/// A filled accent button for the main action.
pub fn primary(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.accent_text).strong())
            .fill(p.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(15))
            .min_size(Vec2::new(64.0, 30.0)),
    )
}

pub fn card_frame(p: &Palette, hovered: bool) -> Frame {
    Frame::new()
        .fill(if hovered { p.card_hover } else { p.card })
        .stroke(Stroke::new(1.0, p.stroke))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(16))
}

/// A small rounded label, e.g. "Update" or "Pre-release".
pub fn chip(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.18))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(color));
        });
}

/// A count bubble for sidebar entries.
pub fn count_bubble(ui: &mut Ui, p: &Palette, n: usize) {
    let text = n.to_string();
    let size = Vec2::new(10.0 + 7.0 * text.len() as f32, 18.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(9), p.accent);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(11.5), p.accent_text);
}

pub fn section_label(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text.to_uppercase()).size(11.0).color(p.weak).strong());
    ui.add_space(2.0);
}

#[derive(Clone, Copy)]
pub enum Icon {
    Bell,
    More,
    /// A text glyph from the default fonts (⚙, ⟳).
    Glyph(&'static str),
}

/// A small painted icon button (glyphs for these aren't in egui's default fonts).
pub fn icon_button(ui: &mut Ui, p: &Palette, icon: Icon, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() || response.is_pointer_button_down_on() {
            painter.rect_filled(rect, CornerRadius::same(6), p.card_hover);
        }
        let color = if response.hovered() { p.text } else { p.text.gamma_multiply(0.85) };
        let c = rect.center();
        match icon {
            Icon::More => {
                for dx in [-5.0, 0.0, 5.0] {
                    painter.circle_filled(c + Vec2::new(dx, 0.0), 1.8, color);
                }
            }
            Icon::Glyph(g) => {
                painter.text(c, egui::Align2::CENTER_CENTER, g, egui::FontId::proportional(size * 0.6), color);
            }
            Icon::Bell => {
                let s = size / 30.0;
                let pt = |x: f32, y: f32| c + Vec2::new(x * s, y * s);
                let body = vec![
                    pt(-1.5, -8.0),
                    pt(-4.5, -6.5),
                    pt(-6.0, -3.5),
                    pt(-6.0, 1.5),
                    pt(-8.0, 4.5),
                    pt(8.0, 4.5),
                    pt(6.0, 1.5),
                    pt(6.0, -3.5),
                    pt(4.5, -6.5),
                    pt(1.5, -8.0),
                ];
                painter.add(egui::Shape::closed_line(body, Stroke::new(1.7, color)));
                painter.circle_filled(pt(0.0, 7.5), 2.0 * s, color);
                painter.circle_filled(pt(0.0, -9.0), 1.2 * s, color);
            }
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A top-bar tab: weak text that brightens on hover, underlined when selected.
pub fn tab(ui: &mut Ui, p: &Palette, label: &str, selected: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(15.0), p.text);
    let size = galley.size() + Vec2::new(16.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() && !selected {
            painter.rect_filled(rect, CornerRadius::same(6), p.card);
        }
        let color = if selected || response.hovered() { p.text } else { p.weak };
        painter.galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, color);
        if selected {
            let y = rect.bottom() + 4.0;
            painter.line_segment(
                [egui::pos2(rect.left() + 8.0, y), egui::pos2(rect.right() - 8.0, y)],
                Stroke::new(2.5, p.text),
            );
        }
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A click target covering the next `size` of space, registered before the content drawn
/// there so buttons inside still get their own clicks.
pub fn click_area(ui: &mut Ui, size: Vec2, salt: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let rect = egui::Rect::from_min_size(ui.cursor().min, size);
    ui.interact(rect, ui.id().with(salt), Sense::click())
}
