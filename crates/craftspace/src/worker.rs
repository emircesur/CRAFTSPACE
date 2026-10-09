//! Background work. Each long operation runs on its own thread and reports back over a channel;
//! the UI drains the channel every frame (and while hidden in the tray, on each wake-up).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use craftspace_core::download::{format_bytes, is_cancelled};
use craftspace_core::files::FileEntry;
use craftspace_core::fonts::FontFile;
use craftspace_core::manager::VerifyReport;
use craftspace_core::news::Article;
use craftspace_core::selfupdate::{self, SelfUpdate};
use craftspace_core::{Manager, Progress, ProgressEvent, Stage};
use eframe::egui;
use semver::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobKind {
    Install(Option<Version>),
    Update,
    Uninstall,
    Rollback,
    Repair,
    InstallFonts(Option<String>),
    RemoveFonts(Option<String>),
}

impl JobKind {
    pub fn verb(&self) -> &'static str {
        match self {
            JobKind::Install(_) | JobKind::InstallFonts(_) => "Installing",
            JobKind::Update => "Updating",
            JobKind::Uninstall | JobKind::RemoveFonts(_) => "Removing",
            JobKind::Rollback => "Rolling back",
            JobKind::Repair => "Repairing",
        }
    }

    /// Whether the job replaces or removes an app's files (and so cares if the app is open).
    pub fn touches_app(&self) -> bool {
        matches!(self, JobKind::Update | JobKind::Uninstall | JobKind::Rollback | JobKind::Repair)
    }
}

/// An operation in progress (or waiting its turn), shown on the app's card and in Downloads.
pub struct Job {
    pub kind: JobKind,
    /// What the user sees: "PhotoCraft", "Noto Sans Arabic".
    pub label: String,
    pub stage: Stage,
    pub done: u64,
    pub total: Option<u64>,
    pub cancel: Arc<AtomicBool>,
    /// Waiting for a download slot.
    pub queued: bool,
    /// A folder the user chose for a new install.
    pub location: Option<std::path::PathBuf>,
}

impl Job {
    pub fn new(kind: JobKind, label: String) -> Job {
        Job {
            kind,
            label,
            stage: Stage::Resolving,
            done: 0,
            total: None,
            cancel: Arc::new(AtomicBool::new(false)),
            queued: true,
            location: None,
        }
    }

    pub fn fraction(&self) -> Option<f32> {
        match (self.stage, self.total) {
            (Stage::Downloading, Some(t)) if t > 0 => Some(self.done as f32 / t as f32),
            _ => None,
        }
    }
}

pub enum Msg {
    Progress {
        key: String,
        event: ProgressEvent,
    },
    JobDone {
        key: String,
        kind: JobKind,
        result: Result<String, String>,
    },
    Refreshed {
        errors: Vec<(String, String)>,
        manual: bool,
    },
    Icons(Vec<String>),
    Articles(Vec<Article>),
    Readme(String, Result<String, String>),
    Fonts(Result<Vec<FontFile>, String>),
    Verified(String, Result<VerifyReport, String>),
    Tour(String, Result<craftspace_core::tour::Tour, String>),
    Shot(String, Result<egui::ColorImage, String>),
    Files(Vec<FileEntry>),
    /// Something changed in a watched folder.
    FilesChanged,
    Thumb(std::path::PathBuf, Option<egui::ColorImage>),
    Portable(Vec<craftspace_core::app_data::PortableData>),
    Imported(String, Result<String, String>),
    SelfUpdateFound(Option<Box<SelfUpdate>>),
    SelfUpdateDone(Result<String, String>),
    TestNotification(Result<(), String>),
    /// Apps found on this computer and adopted: (name, version).
    Adopted(Vec<(String, Option<Version>)>),
}

/// Sends messages and wakes the UI.
#[derive(Clone)]
pub struct Bus {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Bus {
    pub fn new(tx: Sender<Msg>, ctx: egui::Context) -> Bus {
        Bus { tx, ctx }
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
        self.ctx.request_repaint();
    }

    fn spawn(&self, name: &str, f: impl FnOnce(Bus) + Send + 'static) {
        let bus = self.clone();
        std::thread::Builder::new().name(name.into()).spawn(move || f(bus)).expect("spawn worker thread");
    }
}

/// Run a job on its own thread. `key` is the app id (or `addon:<id>` for fonts).
pub fn start(bus: &Bus, manager: &Manager, key: &str, job: &Job) {
    let (manager, key, kind, cancel) = (manager.clone(), key.to_string(), job.kind.clone(), job.cancel.clone());
    let location = job.location.clone();
    bus.spawn(&format!("job-{key}"), move |bus| {
        let report = {
            let bus = bus.clone();
            let key = key.clone();
            move |event: ProgressEvent| bus.send(Msg::Progress { key: key.clone(), event })
        };
        let progress = Progress { report: &report, cancel: &cancel };
        let name = manager.app(&key).map(|a| a.name).unwrap_or_else(|| key.clone());
        let saved = |r: &craftspace_core::state::InstalledApp| match (r.current.delta_downloaded, r.current.size_bytes)
        {
            (Some(d), Some(total)) if total > d => {
                format!(" (delta update: downloaded {} of {})", format_bytes(d), format_bytes(total))
            }
            (Some(d), _) => format!(" (delta update: downloaded {})", format_bytes(d)),
            _ => String::new(),
        };
        let result = match &kind {
            JobKind::Install(version) => manager
                .plan(&key, version.as_ref())
                .and_then(|mut plan| {
                    plan.location = location;
                    manager.install(&plan, &progress)
                })
                .map(|r| format!("{name} {} is installed{}", r.current.version, saved(&r))),
            JobKind::Update => manager
                .plan(&key, None)
                .and_then(|plan| manager.install(&plan, &progress))
                .map(|r| format!("{name} was updated to {}{}", r.current.version, saved(&r))),
            JobKind::Repair => {
                manager.repair(&key, &progress).map(|r| format!("{name} {} was repaired", r.current.version))
            }
            JobKind::Rollback => manager.rollback(&key).map(|r| format!("{name} is back on {}", r.current.version)),
            JobKind::Uninstall => manager.uninstall(&key).map(|()| format!("{name} was uninstalled")),
            JobKind::InstallFonts(family) => {
                let addon = key.trim_start_matches("addon:").split(':').next().unwrap_or_default().to_string();
                manager.install_fonts(&addon, family.as_deref(), &progress).map(|n| {
                    format!(
                        "Installed {} ({n} file{})",
                        family.as_deref().unwrap_or("the fonts"),
                        if n == 1 { "" } else { "s" }
                    )
                })
            }
            JobKind::RemoveFonts(family) => manager
                .uninstall_fonts(family.as_deref())
                .map(|n| format!("Removed {n} font file{}", if n == 1 { "" } else { "s" })),
        };
        let result = result.map_err(|err| {
            if is_cancelled(&err) {
                "Cancelled".to_string()
            } else {
                log::error!("{} {key}: {err:#}", kind.verb());
                format!("{err:#}")
            }
        });
        bus.send(Msg::JobDone { key, kind, result });
    });
}

/// Refresh the app list, every app's releases, icons, news and CraftSpace's own updates.
pub fn refresh(bus: &Bus, manager: &Manager, force: bool, manual: bool) {
    let manager = manager.clone();
    bus.spawn("refresh", move |bus| {
        if let Err(err) = manager.refresh_catalog() {
            log::info!("using the built-in app list: {err:#}");
        }
        match manager.adopt_installed() {
            Ok(found) if !found.is_empty() => bus.send(Msg::Adopted(found)),
            Ok(_) => {}
            Err(err) => log::warn!("couldn't look for apps installed without CraftSpace: {err:#}"),
        }
        let errors = manager.refresh_all(force).into_iter().map(|(id, e)| (id, format!("{e:#}"))).collect();
        bus.send(Msg::Refreshed { errors, manual });
        let changed = manager.fetch_icons();
        if !changed.is_empty() {
            bus.send(Msg::Icons(changed));
        }
        if let Ok(articles) = manager.articles(manual) {
            bus.send(Msg::Articles(articles));
        }
        if let Ok(found) = selfupdate::check(&manager) {
            bus.send(Msg::SelfUpdateFound(found.map(Box::new)));
        }
    });
}

pub fn fetch_readme(bus: &Bus, manager: &Manager, id: &str) {
    let (manager, id) = (manager.clone(), id.to_string());
    bus.spawn("readme", move |bus| {
        let result = manager.readme(&id).map_err(|e| format!("{e:#}"));
        bus.send(Msg::Readme(id, result));
    });
}

pub fn fetch_tour(bus: &Bus, manager: &Manager, id: &str) {
    let (manager, id) = (manager.clone(), id.to_string());
    bus.spawn("tour", move |bus| {
        let result = manager.tour(&id, false).map_err(|e| format!("{e:#}"));
        bus.send(Msg::Tour(id, result));
    });
}

/// Download (or read from the cache) and decode a screenshot, scaled to at most 1600 px wide.
pub fn fetch_shot(bus: &Bus, manager: &Manager, url: &str) {
    let (manager, url) = (manager.clone(), url.to_string());
    bus.spawn("screenshot", move |bus| {
        let result = manager.tour_image(&url).and_then(|path| {
            let img = image::open(&path)?;
            let img = if img.width() > 1600 {
                img.resize(1600, 1600 * img.height() / img.width(), image::imageops::FilterType::Triangle)
            } else {
                img
            };
            let rgba = img.to_rgba8();
            Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
        });
        bus.send(Msg::Shot(url, result.map_err(|e: anyhow::Error| format!("{e:#}"))));
    });
}

pub fn fetch_fonts(bus: &Bus, manager: &Manager, addon: &str) {
    let (manager, addon) = (manager.clone(), addon.to_string());
    bus.spawn("fonts", move |bus| bus.send(Msg::Fonts(manager.font_list(&addon).map_err(|e| format!("{e:#}")))));
}

pub fn verify(bus: &Bus, manager: &Manager, id: &str) {
    let (manager, id) = (manager.clone(), id.to_string());
    bus.spawn("verify", move |bus| {
        let result = manager.verify(&id).map_err(|e| format!("{e:#}"));
        bus.send(Msg::Verified(id, result));
    });
}

pub fn scan_files(bus: &Bus, manager: &Manager, cancel: Arc<AtomicBool>) {
    let manager = manager.clone();
    bus.spawn("files", move |bus| {
        let found = manager.scan_files(&cancel);
        if !cancel.load(Ordering::Relaxed) {
            bus.send(Msg::Files(found));
        }
    });
}

/// The largest picture decoded for a thumbnail; bigger ones show the app icon instead.
const MAX_THUMB_SOURCE: u64 = 80 << 20;
const THUMB_SIZE: u32 = 192;

/// Make (or read from the cache) the thumbnail of `path`. `None`: no thumbnail for this file.
pub fn fetch_thumb(bus: &Bus, manager: &Manager, path: std::path::PathBuf) {
    let cache = manager.paths().cache.join("thumbs");
    bus.spawn("thumbnail", move |bus| {
        let image = thumbnail(&cache, &path)
            .map_err(|e| log::debug!("no thumbnail for {}: {e:#}", path.display()))
            .ok()
            .flatten();
        bus.send(Msg::Thumb(path, image));
    });
}

fn thumbnail(cache: &std::path::Path, path: &std::path::Path) -> anyhow::Result<Option<egui::ColorImage>> {
    use craftspace_core::thumbs;
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !thumbs::supported(&ext) {
        return Ok(None);
    }
    let cached = thumbs::cache_path(cache, path);
    let img = match cached.as_ref().filter(|c| c.is_file()).and_then(|c| image::open(c).ok()) {
        Some(img) => img,
        None => {
            let source = if ext == "psd" || ext == "psb" {
                match thumbs::psd_preview(path)? {
                    Some(jpeg) => image::load_from_memory(&jpeg)?,
                    None => return Ok(None),
                }
            } else {
                anyhow::ensure!(std::fs::metadata(path)?.len() <= MAX_THUMB_SOURCE, "too big");
                image::ImageReader::open(path)?.with_guessed_format()?.decode()?
            };
            let thumb = source.thumbnail(THUMB_SIZE, THUMB_SIZE);
            if let Some(c) = &cached {
                let _ = std::fs::create_dir_all(cache);
                if let Err(err) = thumb.save(c) {
                    log::debug!("couldn't cache a thumbnail: {err}");
                }
            }
            thumb
        }
    };
    let rgba = img.to_rgba8();
    Ok(Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())))
}

/// Watch the Files folders (and the apps' settings folders, for their recent lists); every
/// relevant change sends [`Msg::FilesChanged`]. Dropping the watcher stops it.
pub fn watch_files(bus: &Bus, manager: &Manager) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let settings = manager.settings();
    let catalog = manager.catalog();
    let extensions = catalog.all_extensions();
    let mut settings_dirs = Vec::new();
    if settings.files.app_recents {
        for app in &catalog.apps {
            settings_dirs.extend(craftspace_core::app_data::data_dirs(app, &[]));
        }
    }
    let bus = bus.clone();
    let dirs = settings_dirs.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        let relevant = event.paths.iter().any(|p| {
            let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
            match ext {
                Some(ext) if extensions.binary_search(&ext).is_ok() => true,
                Some(ext) if ext == "json" => p.parent().is_some_and(|d| dirs.iter().any(|s| s == d)),
                _ => false,
            }
        });
        if relevant {
            bus.send(Msg::FilesChanged);
        }
    })
    .map_err(|e| log::info!("can't watch folders: {e}"))
    .ok()?;
    for dir in &settings.file_locations {
        if let Err(err) = watcher.watch(dir, RecursiveMode::Recursive) {
            log::info!("can't watch {}: {err}", dir.display());
        }
    }
    for dir in &settings_dirs {
        let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
    }
    Some(watcher)
}

pub fn find_portable(bus: &Bus, manager: &Manager) {
    let manager = manager.clone();
    bus.spawn("portable", move |bus| bus.send(Msg::Portable(manager.find_portable_data())));
}

pub fn import_portable(bus: &Bus, manager: &Manager, found: craftspace_core::app_data::PortableData) {
    let manager = manager.clone();
    bus.spawn("import", move |bus| {
        let name = manager.app(&found.app_id).map(|a| a.name).unwrap_or_else(|| found.app_id.clone());
        let result = manager
            .import_portable(&found)
            .map(|r| {
                let mut text =
                    format!("Moved {} file{} into {name}'s settings", r.copied, if r.copied == 1 { "" } else { "s" });
                if r.recents_merged > 0 {
                    text.push_str(&format!(
                        ", with {} more recent file{}",
                        r.recents_merged,
                        if r.recents_merged == 1 { "" } else { "s" }
                    ));
                }
                if !r.replaced.is_empty() {
                    text.push_str(" (its old settings are kept as .before-import files)");
                }
                text
            })
            .map_err(|e| format!("{e:#}"));
        bus.send(Msg::Imported(found.app_id, result));
    });
}

pub fn apply_self_update(bus: &Bus, manager: &Manager, update: SelfUpdate) {
    let manager = manager.clone();
    bus.spawn("self-update", move |bus| {
        let cancel = AtomicBool::new(false);
        let report = |_| {};
        let result = selfupdate::apply(&manager, &update, &Progress { report: &report, cancel: &cancel })
            .map(|()| format!("CraftSpace {} is ready; restart to use it", update.version))
            .map_err(|e| format!("{e:#}"));
        bus.send(Msg::SelfUpdateDone(result));
    });
}

/// Wake the UI regularly so update checks and "update when it closes" keep running while the
/// window is hidden in the tray.
pub fn heartbeat(ctx: egui::Context) {
    std::thread::Builder::new()
        .name("heartbeat".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(20));
            ctx.request_repaint();
        })
        .expect("spawn heartbeat");
}

/// macOS shows notifications as coming from an app bundle. Without one set, the library asks
/// AppleScript for an app called "use_default", which pops up a "Where is…?" dialog.
#[cfg(target_os = "macos")]
fn set_notification_sender() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // The library takes only one try, so pick before asking: CraftSpace.app's own identity,
        // or Finder's for a development build that isn't in a bundle.
        let bundled = std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"));
        let id = if bundled { "io.github.emircesur.craftspace" } else { "com.apple.Finder" };
        if let Err(err) = notify_rust::set_application(id) {
            log::info!("notifications will come from another app: {err}");
        }
    });
}

fn show_notification(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    set_notification_sender();
    let mut n = notify_rust::Notification::new();
    n.appname("CraftSpace").summary("CraftSpace").body(text);
    #[cfg(all(unix, not(target_os = "macos")))]
    n.icon("craftspace");
    n.show().map(|_| ()).map_err(|e| e.to_string())
}

/// A system notification (best effort; not every desktop has a notification service).
pub fn notify(text: String) {
    std::thread::spawn(move || {
        if let Err(err) = show_notification(&text) {
            log::info!("couldn't show a notification: {err}");
        }
    });
}

/// Send a notification and report whether the desktop accepted it (Settings › Send a test
/// notification, and `CRAFTSPACE_TEST_NOTIFICATION=1` at startup).
pub fn test_notification(bus: &Bus) {
    bus.spawn("test-notification", |bus| {
        let result = show_notification(
            "Notifications are working. You'll see one like this when updates are found or installed.",
        );
        if let Err(err) = &result {
            log::warn!("test notification failed: {err}");
        } else {
            log::info!("test notification sent");
        }
        bus.send(Msg::TestNotification(result));
    });
}
