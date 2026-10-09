//! The Files tab: recent and pinned documents that ArtCraft apps open, with "start something new".
//! Its optional features (the apps' own recent lists, thumbnails, live updates, portable-settings
//! import, opening files through CraftSpace) are offered on a card the first time it opens, and
//! in Settings › Files.

use std::collections::HashMap;

use craftspace_core::download::format_bytes;
use craftspace_core::file_types;
use craftspace_core::files::{self, FileEntry};
use craftspace_core::settings::FilesFeatures;
use eframe::egui::{self, Align, Layout, RichText, Sense, Ui, Vec2};

use crate::app::{Action, CraftSpaceApp, FilesView, Thumb};
use crate::theme;

pub fn sidebar(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    theme::section_label(ui, &p, "Files");
    let recent = app.files.entries.len();
    let opened = app.files.entries.iter().filter(|f| f.opened.is_some()).count();
    let pinned = app.settings.pinned_files.len();
    if nav(app, ui, "🕘  Recent", app.files.view == FilesView::Recent && app.files.app_filter.is_none(), Some(recent))
    {
        app.files.view = FilesView::Recent;
        app.files.app_filter = None;
    }
    if app.settings.files.app_recents
        && nav(
            app,
            ui,
            "↺  Opened in apps",
            app.files.view == FilesView::Opened && app.files.app_filter.is_none(),
            Some(opened),
        )
    {
        app.files.view = FilesView::Opened;
        app.files.app_filter = None;
    }
    if nav(app, ui, "★  Pinned", app.files.view == FilesView::Pinned, Some(pinned)) {
        app.files.view = FilesView::Pinned;
        app.files.app_filter = None;
    }

    let mut per_app: HashMap<String, usize> = HashMap::new();
    for f in &app.files.entries {
        if let Some(id) = &f.app_id {
            *per_app.entry(id.clone()).or_default() += 1;
        }
    }
    if !per_app.is_empty() {
        theme::section_label(ui, &p, "By app");
        for state in app.states.clone() {
            if let Some(&n) = per_app.get(&state.app.id) {
                let selected = app.files.app_filter.as_deref() == Some(state.app.id.as_str());
                if nav(app, ui, &state.app.name, selected, Some(n)) {
                    app.files.view = FilesView::Recent;
                    app.files.app_filter = Some(state.app.id.clone());
                }
            }
        }
    }

    theme::section_label(ui, &p, "Locations");
    for dir in app.settings.file_locations.clone() {
        ui.horizontal(|ui| {
            let name =
                dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
            let r = ui.add(egui::Label::new(RichText::new(name.clone())).truncate().sense(Sense::click()));
            if r.on_hover_text(dir.display().to_string()).clicked() {
                app.actions.push(Action::OpenPath(dir.clone()));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(RichText::new("×").size(11.0).color(p.weak)).frame(false))
                    .on_hover_text("Stop looking here")
                    .clicked()
                {
                    app.actions.push(Action::RemoveLocation(dir.clone()));
                }
            });
        });
    }
    ui.add_space(4.0);
    if ui.button("+ Add folder").clicked() {
        app.actions.push(Action::AddLocation);
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(!app.files.scanning, egui::Button::new("⟳ Rescan")).clicked() {
            app.actions.push(Action::RescanFiles);
        }
        if app.files.scanning {
            ui.add(egui::Spinner::new().size(14.0));
        }
    });
    if app.settings.files.watch && app.files_watching() {
        ui.label(RichText::new("Watching for changes").size(11.5).color(p.weak));
    }
}

fn nav(app: &CraftSpaceApp, ui: &mut Ui, label: &str, selected: bool, count: Option<usize>) -> bool {
    let p = app.palette;
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
        ui.painter().text(
            rect.right_center() - Vec2::new(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            n.to_string(),
            egui::FontId::proportional(12.0),
            p.weak,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

pub fn content(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    app.files_opened();
    ui.heading("Your files");
    ui.label(RichText::new("Documents in your folders that ArtCraft apps can open.").color(p.weak));
    ui.add_space(12.0);

    if !app.settings.files.setup_done && !app.files.setup_window {
        setup_card(app, ui);
        ui.add_space(14.0);
    }
    portable_offers(app, ui);

    create_new(app, ui);
    ui.add_space(16.0);

    // The list.
    let catalog = app.manager.catalog();
    let q = app.search.trim().to_lowercase();
    let pinned = app.settings.pinned_files.clone();
    let entries: Vec<FileEntry> = match app.files.view {
        FilesView::Pinned => pinned.iter().filter_map(|path| files::describe(path, &catalog)).collect(),
        FilesView::Opened => app.files.entries.iter().filter(|f| f.opened.is_some()).cloned().collect(),
        FilesView::Recent => app.files.entries.clone(),
    };
    let entries: Vec<FileEntry> = entries
        .into_iter()
        .filter(|f| app.files.app_filter.is_none() || f.app_id == app.files.app_filter)
        .filter(|f| {
            q.is_empty() || f.name.to_lowercase().contains(&q) || f.path.to_string_lossy().to_lowercase().contains(&q)
        })
        .collect();

    let grid = app.settings.files.grid;
    ui.horizontal(|ui| {
        let title = match (&app.files.view, &app.files.app_filter) {
            (FilesView::Pinned, _) => "Pinned".to_string(),
            (_, Some(id)) => app.manager.app(id).map(|a| format!("{} files", a.name)).unwrap_or_default(),
            (FilesView::Opened, _) => "Opened in apps".into(),
            _ => "Recent".into(),
        };
        ui.label(RichText::new(title).size(16.0).strong());
        ui.label(RichText::new(format!("{}", entries.len())).color(p.weak));
        if app.files.scanning {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(RichText::new("Looking through your folders…").color(p.weak));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.selectable_label(grid, "▦ Grid").on_hover_text("Show thumbnails in a grid").clicked() && !grid {
                app.actions.push(Action::SetFilesGrid(true));
            }
            if ui.selectable_label(!grid, "☰ List").clicked() && grid {
                app.actions.push(Action::SetFilesGrid(false));
            }
        });
    });
    ui.add_space(6.0);

    if entries.is_empty() && !app.files.scanning {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            let text = match app.files.view {
                FilesView::Pinned => "Pin files to keep them here.",
                FilesView::Opened => "Files you open in the ArtCraft apps show up here.",
                FilesView::Recent if app.settings.file_locations.is_empty() => {
                    "Add a folder to see your documents here."
                }
                FilesView::Recent => "No matching documents in your folders yet.",
            };
            ui.label(RichText::new(text).size(15.0).color(p.weak));
        });
        return;
    }

    let shown = &entries[..entries.len().min(1000)];
    if grid {
        let tile = Vec2::new(176.0, 196.0);
        let gap = 10.0;
        let per_row = (((ui.available_width() + gap) / (tile.x + gap)).floor() as usize).max(1);
        let rows = shown.len().div_ceil(per_row);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, tile.y + gap, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for f in shown.iter().skip(row * per_row).take(per_row) {
                        file_tile(app, ui, f, pinned.contains(&f.path), tile);
                    }
                });
                ui.add_space(gap);
            }
        });
    } else {
        let row_h = 58.0;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, row_h, shown.len(), |ui, range| {
            for f in &shown[range] {
                file_row(app, ui, f, pinned.contains(&f.path), row_h);
            }
        });
    }
}

// ---- first-run card and Settings › Files --------------------------------------------------------

/// The card offered the first time Files opens.
fn setup_card(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    let mut draft = app.files.setup_draft.take().unwrap_or_else(|| app.settings.files.clone());
    theme::card_frame(&p, false).inner_margin(egui::Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Set up Files").size(17.0).strong());
        ui.label(
            RichText::new("Choose what this tab does. Everything here is optional, and you can change it later in Settings › Files.")
                .color(p.weak),
        );
        ui.add_space(8.0);
        features_editor(app, ui, &mut draft);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if theme::primary(ui, &p, "Save").clicked() {
                app.actions.push(Action::SaveFilesFeatures(Box::new(draft.clone())));
            }
            if theme::pill(ui, &p, "Not now").on_hover_text("Keep the current choices; this card won't show again").clicked()
            {
                app.actions.push(Action::FilesSetupLater);
            }
        });
    });
    app.files.setup_draft = Some(draft);
}

/// Settings › Files › Choose…: the same choices in a window.
pub fn setup_window(app: &mut CraftSpaceApp, ctx: &egui::Context) {
    if !app.files.setup_window {
        return;
    }
    let p = app.palette;
    let mut draft = app.files.setup_draft.take().unwrap_or_else(|| app.settings.files.clone());
    let mut keep = true;
    let modal = egui::Modal::new(egui::Id::new("files-setup")).show(ctx, |ui| {
        ui.set_width(600.0);
        ui.heading("Files");
        ui.label(RichText::new("What the Files tab does.").color(p.weak));
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() - 220.0).show(ui, |ui| {
            features_editor(app, ui, &mut draft);
        });
        ui.add_space(12.0);
        ui.separator();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::primary(ui, &p, "Save").clicked() {
                app.actions.push(Action::SaveFilesFeatures(Box::new(draft.clone())));
                keep = false;
            }
            if theme::pill(ui, &p, "Cancel").clicked() {
                keep = false;
            }
        });
    });
    if modal.should_close() {
        keep = false;
    }
    if keep {
        app.files.setup_draft = Some(draft);
    } else {
        app.files.setup_window = false;
    }
}

fn features_editor(app: &mut CraftSpaceApp, ui: &mut Ui, f: &mut FilesFeatures) {
    let p = app.palette;
    let policy = app.manager.policy().clone();
    let free = |key: &str| !policy.locks(&format!("files.{key}"));
    let hint = |ui: &mut Ui, text: &str| {
        ui.add_space(-4.0);
        egui::Frame::NONE.inner_margin(egui::Margin { left: 26, right: 0, top: 0, bottom: 4 }).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).size(12.0).color(p.weak)).wrap());
        });
    };
    ui.add_enabled(free("app_recents"), egui::Checkbox::new(&mut f.app_recents, "Show what I opened in each app"));
    hint(ui, "Reads each ArtCraft app's own recent-files list, so files outside your folders appear too, marked \"Opened in PhotoCraft · 2 hours ago\".");
    ui.add_enabled(free("thumbnails"), egui::Checkbox::new(&mut f.thumbnails, "Thumbnails for pictures and PSDs"));
    hint(ui, "Small previews, kept in CraftSpace's cache. Switch between a list and a grid above the files.");
    ui.add_enabled(free("watch"), egui::Checkbox::new(&mut f.watch, "Update the list as files change"));
    hint(ui, "New and changed files appear without Rescan. The last list is saved, so the tab opens instantly.");
    ui.add_enabled(
        free("find_portable"),
        egui::Checkbox::new(&mut f.find_portable, "Look for settings from portable copies"),
    );
    hint(ui, "Finds folders like PhotoCraftData left by a portable copy and offers to move their preferences, presets and recent files into the installed app.");

    if file_types::supported() {
        let mut on = !f.open_with_craftspace.is_empty();
        let was_on = on;
        ui.add_enabled(
            free("open_with_craftspace"),
            egui::Checkbox::new(&mut on, "Open ArtCraft file types with CraftSpace"),
        );
        hint(
            ui,
            if cfg!(windows) {
                "Double-clicking a file opens it in the app that handles it, or offers to install that app. Windows asks you to confirm in Default apps."
            } else {
                "Double-clicking a file opens it in the app that handles it, or offers to install that app."
            },
        );
        if on && !was_on {
            f.open_with_craftspace = default_types(app);
        } else if !on {
            f.open_with_craftspace.clear();
        }
        if on {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                egui::CollapsingHeader::new(format!("File types ({} chosen)", f.open_with_craftspace.len()))
                    .id_salt("file-types")
                    .show(ui, |ui| type_chooser(app, ui, &mut f.open_with_craftspace));
            });
        }
    }
}

/// The ArtCraft apps' own formats (not pictures, PDFs and other files other programs open).
fn default_types(app: &CraftSpaceApp) -> Vec<String> {
    let mut out: Vec<String> =
        app.manager.catalog().all_extensions().into_iter().filter(|e| file_types::default_on(e)).collect();
    out.sort();
    out
}

fn type_chooser(app: &CraftSpaceApp, ui: &mut Ui, chosen: &mut Vec<String>) {
    let p = app.palette;
    for state in &app.states {
        if state.app.extensions.is_empty() {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&state.app.name).strong());
            for ext in &state.app.extensions {
                let ext = ext.to_ascii_lowercase();
                let mut on = chosen.contains(&ext);
                let label = if file_types::default_on(&ext) { format!(".{ext}") } else { format!(".{ext}*") };
                if ui.checkbox(&mut on, label).changed() {
                    if on {
                        chosen.push(ext.clone());
                        chosen.sort();
                        chosen.dedup();
                    } else {
                        chosen.retain(|e| *e != ext);
                    }
                }
            }
        });
    }
    ui.label(
        RichText::new("* Also opened by many other programs (pictures, PDFs, office files), so not chosen by default.")
            .size(11.5)
            .color(p.weak),
    );
}

/// "Found PhotoCraft settings from a portable copy": import or dismiss.
fn portable_offers(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    for found in app.files.portable.clone() {
        let Some(entry) = app.manager.app(&found.app_id) else { continue };
        theme::card_frame(&p, false).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::badge(ui, &entry, 36.0);
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 260.0).max(200.0));
                    ui.label(RichText::new(format!("{} settings from a portable copy", entry.name)).strong());
                    let mut items = found.items.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
                    if found.items.len() > 4 {
                        items.push_str(", …");
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{} ({items}). Move them into the installed {}?", found.dir.display(), entry.name))
                                .size(12.5)
                                .color(p.weak),
                        )
                        .wrap(),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::pill(ui, &p, "Not now").on_hover_text("Don't offer this folder again").clicked() {
                        app.actions.push(Action::DismissPortable(found.dir.clone()));
                    }
                    if theme::primary(ui, &p, "Import")
                        .on_hover_text(format!(
                            "Copies its preferences, presets and recent files into {}'s settings (anything replaced is kept as a .before-import file), then renames the folder to {}.imported",
                            entry.name,
                            found.dir.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .clicked()
                    {
                        app.actions.push(Action::ImportPortable(found.clone()));
                    }
                });
            });
        });
        ui.add_space(10.0);
    }
}

// ---- the files ----------------------------------------------------------------------------------

/// "Start something new": one tile per app that makes documents.
fn create_new(app: &mut CraftSpaceApp, ui: &mut Ui) {
    let p = app.palette;
    ui.label(RichText::new("Start something new").size(16.0).strong());
    ui.add_space(4.0);
    let states = app.states.clone();
    egui::ScrollArea::horizontal().id_salt("create-new").show(ui, |ui| {
        ui.horizontal(|ui| {
            for s in states.iter().filter(|s| !s.app.extensions.is_empty()) {
                let installed = s.installed.is_some();
                let resp = ui.allocate_ui_with_layout(Vec2::new(128.0, 112.0), Layout::top_down(Align::Min), |ui| {
                    theme::card_frame(&p, false).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(104.0);
                        ui.vertical_centered(|ui| {
                            theme::badge(ui, &s.app, 40.0);
                            ui.label(RichText::new(&s.app.name).size(13.0));
                            ui.label(
                                RichText::new(if installed { "New" } else { "Get app" })
                                    .size(11.5)
                                    .color(if installed { p.weak } else { p.accent }),
                            );
                        });
                    })
                });
                let r = resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if r.clicked() {
                    app.actions.push(if installed {
                        Action::Launch(s.app.id.clone(), vec![])
                    } else {
                        Action::ShowApp(s.app.id.clone())
                    });
                }
            }
        });
    });
}

/// "Opened in PhotoCraft · 2 hours ago", or "Changed 5 minutes ago".
fn when(app: &CraftSpaceApp, f: &FileEntry) -> String {
    match &f.opened {
        Some(o) => {
            let name = app.manager.app(&o.app_id).map(|a| a.name).unwrap_or_else(|| o.app_id.clone());
            match o.at {
                Some(t) => format!("Opened in {name} · {}", files::relative_time(t).to_lowercase()),
                None => format!("Opened in {name}"),
            }
        }
        None => f.modified.map(files::relative_time).unwrap_or_default(),
    }
}

/// The file's thumbnail, if there is (or will be) one.
fn thumbnail(app: &mut CraftSpaceApp, f: &FileEntry) -> Option<egui::TextureHandle> {
    if !app.settings.files.thumbnails || !craftspace_core::thumbs::supported(&f.ext) {
        return None;
    }
    app.want_thumb(&f.path);
    match app.files.thumbs.get(&f.path) {
        Some(Thumb::Ready(t)) => Some(t.clone()),
        _ => None,
    }
}

/// Draw `texture` inside `rect`, keeping its proportions.
fn paint_fit(ui: &Ui, texture: &egui::TextureHandle, rect: egui::Rect, rounding: u8) {
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let r = egui::Rect::from_center_size(rect.center(), size * scale);
    egui::Image::new(texture).corner_radius(rounding).paint_at(ui, r);
}

fn open_file(app: &mut CraftSpaceApp, f: &FileEntry) {
    let state = f.app_id.as_ref().and_then(|id| app.state(id));
    match state {
        Some(s) if s.installed.is_some() => app.actions.push(Action::Launch(s.app.id.clone(), vec![f.path.clone()])),
        Some(s) => app.actions.push(Action::ShowApp(s.app.id.clone())),
        None => app.actions.push(Action::OpenPath(f.path.clone())),
    }
}

fn file_menu(app: &mut CraftSpaceApp, ui: &mut Ui, f: &FileEntry, pinned: bool) {
    ui.set_min_width(200.0);
    let state = f.app_id.as_ref().and_then(|id| app.state(id)).cloned();
    if let Some(s) = &state {
        if s.installed.is_some() && ui.button(format!("Open in {}", s.app.name)).clicked() {
            app.actions.push(Action::Launch(s.app.id.clone(), vec![f.path.clone()]));
        }
    }
    let catalog = app.manager.catalog();
    let others: Vec<_> =
        catalog.apps_for_extension(&f.ext).into_iter().filter(|a| Some(&a.id) != f.app_id.as_ref()).cloned().collect();
    if !others.is_empty() {
        ui.menu_button("Open with", |ui| {
            for other in others {
                let ok = app.state(&other.id).is_some_and(|s| s.installed.is_some());
                if ui.add_enabled(ok, egui::Button::new(&other.name)).clicked() {
                    app.actions.push(Action::Launch(other.id.clone(), vec![f.path.clone()]));
                }
            }
        });
    }
    if ui.button("Open with system default").clicked() {
        app.actions.push(Action::OpenPath(f.path.clone()));
    }
    if ui.button("Show in folder").clicked() {
        app.actions.push(Action::Reveal(f.path.clone()));
    }
    if ui.button(if pinned { "Unpin" } else { "Add to pinned" }).clicked() {
        app.actions.push(Action::TogglePin(f.path.clone()));
    }
    if ui.button("Copy path").clicked() {
        app.actions.push(Action::CopyText(f.path.display().to_string()));
    }
}

fn file_tile(app: &mut CraftSpaceApp, ui: &mut Ui, f: &FileEntry, pinned: bool, size: Vec2) {
    let p = app.palette;
    let state = f.app_id.as_ref().and_then(|id| app.state(id)).cloned();
    let thumb = thumbnail(app, f);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    ui.painter().rect_filled(rect, 10, if hovered { p.card_hover } else { p.card });
    let preview = egui::Rect::from_min_size(rect.min + Vec2::splat(8.0), Vec2::new(size.x - 16.0, 120.0));
    ui.painter().rect_filled(
        preview,
        6,
        if p.dark { egui::Color32::from_gray(0x1F) } else { egui::Color32::from_gray(0xE8) },
    );
    match (&thumb, &state) {
        (Some(t), _) => paint_fit(ui, t, preview.shrink(2.0), 4),
        (None, Some(s)) => {
            let icon = egui::Rect::from_center_size(preview.center(), Vec2::splat(56.0));
            theme::badge(&mut ui.new_child(egui::UiBuilder::new().max_rect(icon)), &s.app, 56.0);
        }
        _ => {}
    }
    // A small app badge on the thumbnail's corner.
    if let (Some(_), Some(s)) = (&thumb, &state) {
        let corner = egui::Rect::from_min_size(preview.right_bottom() - Vec2::splat(30.0), Vec2::splat(24.0));
        theme::badge(&mut ui.new_child(egui::UiBuilder::new().max_rect(corner)), &s.app, 24.0);
    }
    if pinned {
        ui.painter().text(
            preview.left_top() + Vec2::new(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            "★",
            egui::FontId::proportional(14.0),
            p.accent,
        );
    }
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 10.0, preview.bottom() + 6.0),
        rect.right_bottom() - Vec2::new(10.0, 6.0),
    );
    {
        let mut text = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::top_down(Align::Min)));
        text.spacing_mut().item_spacing.y = 2.0;
        text.add(egui::Label::new(RichText::new(&f.name).size(13.5)).truncate());
        let color = if f.opened.is_some() { p.accent } else { p.weak };
        text.add(egui::Label::new(RichText::new(when(app, f)).size(11.5).color(color)).truncate());
        text.add(egui::Label::new(RichText::new(format_bytes(f.size)).size(11.0).color(p.weak)).truncate());
    }
    let response = response.on_hover_text(f.path.display().to_string()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.double_clicked() || response.clicked() {
        open_file(app, f);
    }
    response.context_menu(|ui| file_menu(app, ui, f, pinned));
}

fn file_row(app: &mut CraftSpaceApp, ui: &mut Ui, f: &FileEntry, pinned: bool, row_h: f32) {
    let p = app.palette;
    let state = f.app_id.as_ref().and_then(|id| app.state(id)).cloned();
    let installed = state.as_ref().is_some_and(|s| s.installed.is_some());
    let thumb = thumbnail(app, f);
    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), row_h - 6.0), Layout::top_down(Align::Min), |ui| {
        let background = theme::click_area(ui, Vec2::new(ui.available_width(), row_h - 6.0), ("file", &f.path));
        let hovered = ui.rect_contains_pointer(background.rect);
        let frame = theme::card_frame(&p, hovered).inner_margin(egui::Margin::symmetric(14, 7));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                match (&thumb, &state) {
                    (Some(t), _) => {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                        paint_fit(ui, t, r, 4);
                    }
                    (None, Some(s)) => {
                        theme::badge(ui, &s.app, 32.0);
                    }
                    (None, None) => {
                        ui.add_space(32.0);
                    }
                }
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 330.0).max(160.0));
                    ui.add(egui::Label::new(RichText::new(&f.name).size(14.5)).truncate());
                    let folder = f.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let color = if f.opened.is_some() { p.accent } else { p.weak };
                        ui.label(RichText::new(when(app, f)).size(12.0).color(color));
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(" · {} · {folder}", format_bytes(f.size)))
                                    .size(12.0)
                                    .color(p.weak),
                            )
                            .truncate(),
                        );
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let more = theme::icon_button(ui, &p, theme::Icon::More, 28.0).on_hover_text("More");
                    egui::Popup::menu(&more).show(|ui| file_menu(app, ui, f, pinned));
                    if let Some(s) = &state {
                        if installed {
                            if theme::pill(ui, &p, "Open").clicked() {
                                app.actions.push(Action::Launch(s.app.id.clone(), vec![f.path.clone()]));
                            }
                        } else if theme::primary(ui, &p, &format!("Get {}", s.app.name))
                            .on_hover_text(format!("Install {} to open this file", s.app.name))
                            .clicked()
                        {
                            app.actions.push(Action::ShowApp(s.app.id.clone()));
                        }
                    }
                    let pin_label = if pinned { "★ Pinned" } else { "Add to pinned" };
                    if ui.add(egui::Button::new(pin_label).corner_radius(6)).clicked() {
                        app.actions.push(Action::TogglePin(f.path.clone()));
                    }
                });
            });
        });
        if background.double_clicked() {
            if let Some(s) = state.as_ref().filter(|_| installed) {
                app.actions.push(Action::Launch(s.app.id.clone(), vec![f.path.clone()]));
            }
        }
    });
}
