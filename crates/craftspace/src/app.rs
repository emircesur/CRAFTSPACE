//! The window: top bar, sidebars, routing between views, the job queue, the tray and
//! notifications.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use craftspace_core::files::FileEntry;
use craftspace_core::fonts::FontFile;
use craftspace_core::manager::{AppList, VerifyReport};
use craftspace_core::news::Article;
use craftspace_core::selfupdate::{self, SelfUpdate};
use craftspace_core::settings::Channel;
use craftspace_core::{AppState, Manager, ProgressEvent, Settings};
use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke, Vec2};
use egui_commonmark::CommonMarkCache;
use semver::Version;

use crate::theme::{self, Palette};
use crate::tray::{Tray, TrayCommand};
use crate::views;
use crate::worker::{self, Bus, Job, JobKind, Msg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Apps,
    Files,
    Discover,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppsView {
    All,
    Updates,
    Installed,
    Category(String),
    Detail(String),
    Addons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Overview,
    Versions,
    Readme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesView {
    Recent,
    /// Files on the apps' own recent lists.
    Opened,
    Pinned,
}

/// A file's thumbnail: being made, made, or there is none.
pub enum Thumb {
    Loading,
    Ready(egui::TextureHandle),
    None,
}

/// Things views ask for; handled after drawing so views only need shared access.
#[derive(Debug, Clone)]
pub enum Action {
    Install(String, Option<Version>),
    /// Install into a folder the user picks.
    InstallTo(String),
    InstallMany(Vec<String>),
    Update(String),
    UpdateAll,
    AskUninstall(String),
    Uninstall(String),
    Rollback(String),
    VerifyAndRepair(String),
    Cancel(String),
    Launch(String, Vec<PathBuf>),
    OpenUrl(String),
    OpenPath(PathBuf),
    Reveal(PathBuf),
    ShowApp(String),
    GoApps(AppsView),
    Refresh,
    RescanFiles,
    TogglePin(PathBuf),
    AddLocation,
    RemoveLocation(PathBuf),
    OpenSettings,
    SaveSettings(Box<Settings>),
    ApplySelfUpdate,
    SelfInstall,
    DismissSelfInstall,
    Cleanup,
    TestNotification,
    OpenAbout,
    /// Save the Files features (from the first-run card or the Files window).
    SaveFilesFeatures(Box<craftspace_core::settings::FilesFeatures>),
    /// Close the first-run card without changing anything.
    FilesSetupLater,
    /// Settings › Files › Choose…
    OpenFilesSetup,
    SetFilesGrid(bool),
    ImportPortable(craftspace_core::app_data::PortableData),
    DismissPortable(PathBuf),
    CopyText(String),
    SetChannel(String, Channel),
    ReportBug(String),
    InstallFonts(Option<String>),
    RemoveFonts(Option<String>),
    ExportList,
    ImportList,
    OpenMultiInstall,
    /// From the "app is open" dialog.
    WhenClosed(String, JobKind),
    RunNow(String, JobKind),
}

#[derive(Clone, Copy, PartialEq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub at: Instant,
}

pub struct FilesState {
    pub entries: Vec<FileEntry>,
    pub scanning: bool,
    pub scanned: bool,
    pub view: FilesView,
    pub app_filter: Option<String>,
    pub cancel: Arc<AtomicBool>,
    pub thumbs: HashMap<PathBuf, Thumb>,
    /// Thumbnails waiting for a worker (a few are made at a time).
    thumb_queue: std::collections::VecDeque<PathBuf>,
    thumbs_in_flight: usize,
    watcher: Option<notify::RecommendedWatcher>,
    /// A watched folder changed; rescan once things settle.
    changed_at: Option<Instant>,
    /// Settings left by portable copies, offered for import.
    pub portable: Vec<craftspace_core::app_data::PortableData>,
    portable_checked: bool,
    /// The Files features being edited (the first-run card, or Settings › Files).
    pub setup_draft: Option<craftspace_core::settings::FilesFeatures>,
    /// Edit them in a window (from Settings) rather than the card.
    pub setup_window: bool,
}

pub struct CraftSpaceApp {
    pub manager: Manager,
    bus: Bus,
    rx: Receiver<Msg>,
    pub palette: Palette,
    applied_theme: Option<(craftspace_core::settings::Theme, bool)>,

    pub tab: Tab,
    pub apps_view: AppsView,
    /// Where "Back" goes from a detail page.
    pub back_view: AppsView,
    pub detail_tab: DetailTab,
    pub search: String,

    pub states: Vec<AppState>,
    /// Running and queued jobs, by app id (or `addon:<id>`), in queue order.
    pub jobs: HashMap<String, Job>,
    pub job_order: Vec<String>,
    /// Jobs waiting for an app to be closed.
    pub waiting_for_close: Vec<(String, JobKind)>,
    last_close_check: Instant,
    /// The "app is open" question.
    pub running_prompt: Option<(String, JobKind)>,
    pub multi_install: Option<BTreeSet<String>>,
    pub downloads_open: bool,

    pub refreshing: bool,
    pub refresh_errors: Vec<(String, String)>,
    pub last_check: Option<Instant>,
    pub self_update: Option<SelfUpdate>,
    pub self_updating: bool,
    required_queued: bool,

    pub files: FilesState,
    pub articles: Vec<Article>,
    pub readmes: HashMap<String, Result<String, String>>,
    readme_loading: HashSet<String>,
    pub fonts: Option<Result<Vec<FontFile>, String>>,
    fonts_loading: bool,
    pub verify_results: HashMap<String, VerifyReport>,
    pub tours: HashMap<String, Result<craftspace_core::tour::Tour, String>>,
    tour_loading: HashSet<String>,
    pub tour_index: HashMap<String, usize>,
    pub shots: HashMap<String, Result<egui::TextureHandle, String>>,
    shots_loading: HashSet<String>,

    pub settings: Settings,
    pub settings_draft: Option<Settings>,
    pub confirm_uninstall: Option<String>,
    confirm_quit: bool,
    pub md_cache: CommonMarkCache,
    pub detail_release: Option<String>,
    /// Running from outside the CraftSpace folder (e.g. straight from Downloads).
    pub offer_self_install: bool,
    toasts: Vec<Toast>,
    pub actions: Vec<Action>,

    tray: Option<Tray>,
    tray_rx: Receiver<TrayCommand>,
    hidden: bool,
    pub about_open: bool,
    /// `craftspace open <file>` for an app that isn't installed: open it once it is.
    pub pending_open: Option<(String, PathBuf)>,
    /// Hidden while no tray icon is showing (the panel isn't there yet, or went away).
    hidden_without_tray: Option<Instant>,
    quitting: bool,
    told_about_tray: bool,
}

impl CraftSpaceApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        manager: Manager,
        background: bool,
        open_request: Option<(String, PathBuf)>,
    ) -> CraftSpaceApp {
        let (tx, rx) = channel();
        let bus = Bus::new(tx, cc.egui_ctx.clone());
        let settings = manager.settings();
        let palette = theme::palette_for(settings.theme, &cc.egui_ctx);
        let (tray_tx, tray_rx) = channel();
        let tray = Tray::new(&cc.egui_ctx, tray_tx);
        if tray.is_some() {
            log::info!("tray icon ready");
        }
        worker::heartbeat(cc.egui_ctx.clone());
        let mut app = CraftSpaceApp {
            states: manager.states(),
            articles: manager.cached_articles(),
            bus,
            rx,
            palette,
            applied_theme: None,
            tab: Tab::Apps,
            apps_view: AppsView::All,
            back_view: AppsView::All,
            detail_tab: DetailTab::Overview,
            search: String::new(),
            jobs: HashMap::new(),
            job_order: Vec::new(),
            waiting_for_close: Vec::new(),
            last_close_check: Instant::now(),
            running_prompt: None,
            multi_install: None,
            downloads_open: false,
            refreshing: false,
            refresh_errors: Vec::new(),
            last_check: None,
            self_update: None,
            self_updating: false,
            required_queued: false,
            files: FilesState {
                entries: Vec::new(),
                scanning: false,
                scanned: false,
                view: FilesView::Recent,
                app_filter: None,
                cancel: Arc::new(AtomicBool::new(false)),
                thumbs: HashMap::new(),
                thumb_queue: Default::default(),
                thumbs_in_flight: 0,
                watcher: None,
                changed_at: None,
                portable: Vec::new(),
                portable_checked: false,
                setup_draft: None,
                setup_window: false,
            },
            readmes: HashMap::new(),
            readme_loading: HashSet::new(),
            fonts: None,
            fonts_loading: false,
            verify_results: HashMap::new(),
            tours: HashMap::new(),
            tour_loading: HashSet::new(),
            tour_index: HashMap::new(),
            shots: HashMap::new(),
            shots_loading: HashSet::new(),
            settings,
            settings_draft: None,
            confirm_uninstall: None,
            confirm_quit: false,
            md_cache: CommonMarkCache::default(),
            detail_release: None,
            offer_self_install: false,
            toasts: Vec::new(),
            actions: Vec::new(),
            tray,
            tray_rx,
            hidden: false,
            pending_open: None,
            about_open: false,
            hidden_without_tray: None,
            quitting: false,
            told_about_tray: false,
            manager,
        };
        theme::load_icons(&cc.egui_ctx, &app.manager, None);
        app.offer_self_install = app.settings.offer_self_install && !selfupdate::is_installed_copy(&app.manager);
        if background {
            if app.tray.is_some() {
                app.hidden = true;
            } else {
                // Nowhere to hide: start minimized instead.
                cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        // `CRAFTSPACE_VIEW=files|discover|updates|installed|addons|app:<id>` opens a specific
        // page (handy for screenshots and for jumping straight somewhere from a shortcut).
        if let Ok(view) = std::env::var("CRAFTSPACE_VIEW") {
            match view.as_str() {
                "files" => app.tab = Tab::Files,
                "discover" => app.tab = Tab::Discover,
                "updates" => app.apps_view = AppsView::Updates,
                "installed" => app.apps_view = AppsView::Installed,
                "addons" => app.apps_view = AppsView::Addons,
                "about" => app.about_open = true,
                other => {
                    if let Some(rest) = other.strip_prefix("app:") {
                        let (id, tab) = rest.split_once(':').unwrap_or((rest, ""));
                        app.apps_view = AppsView::Detail(id.to_string());
                        app.detail_tab = match tab {
                            "versions" => DetailTab::Versions,
                            "readme" => DetailTab::Readme,
                            _ => DetailTab::Overview,
                        };
                    }
                }
            }
        }
        if let Some((id, file)) = open_request {
            app.apps_view = AppsView::Detail(id.clone());
            app.detail_tab = DetailTab::Overview;
            app.pending_open = Some((id, file));
        }
        if std::env::var_os("CRAFTSPACE_TEST_NOTIFICATION").is_some() {
            worker::test_notification(&app.bus);
        }
        // Always fetch on start when nothing is cached yet; otherwise follow the setting.
        if app.settings.auto_check_updates || app.states.iter().all(|s| !s.known) {
            app.start_refresh(false, false);
        }
        app
    }

    fn start_refresh(&mut self, force: bool, manual: bool) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        self.last_check = Some(Instant::now());
        worker::refresh(&self.bus, &self.manager, force, manual);
    }

    /// Make CraftSpace the opener for the chosen file types (or stop being it).
    fn register_file_types(&mut self) {
        let types = self.settings.files.open_with_craftspace.clone();
        match self.manager.register_file_types(&types) {
            Ok(r) if types.is_empty() => {
                let _ = r;
                self.toast(ToastKind::Info, "File types open in their apps directly again");
            }
            Ok(r) => {
                let n = r.types;
                match r.confirm_url {
                    Some(url) => {
                        self.toast(
                            ToastKind::Info,
                            format!("CraftSpace can open {n} file type{}. Windows asks you to confirm: choose CraftSpace for them in Default apps.", if n == 1 { "" } else { "s" }),
                        );
                        let _ = open::that_detached(url);
                    }
                    None => self.toast(
                        ToastKind::Success,
                        format!("CraftSpace now opens {n} file type{} in the right app", if n == 1 { "" } else { "s" }),
                    ),
                }
            }
            Err(err) => self.toast(ToastKind::Error, format!("Couldn't set up file types: {err:#}")),
        }
    }

    pub fn files_watching(&self) -> bool {
        self.files.watcher.is_some()
    }

    fn tray_shown(&self) -> bool {
        self.tray.as_ref().is_some_and(Tray::shown)
    }

    /// A window hidden in the tray must stay reachable: if no tray icon shows up for a while
    /// (started at login on a desktop without a tray), minimize it to the taskbar instead.
    fn keep_findable(&mut self, ctx: &egui::Context) {
        if !self.hidden || self.tray_shown() {
            self.hidden_without_tray = None;
            return;
        }
        let since = *self.hidden_without_tray.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(30) {
            log::info!("no tray icon is showing; minimizing the window instead");
            self.hidden = false;
            self.hidden_without_tray = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        } else {
            ctx.request_repaint_after(Duration::from_secs(5));
        }
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), kind, at: Instant::now() });
    }

    /// A toast, plus a system notification when the window isn't in front.
    fn announce(&mut self, ctx: &egui::Context, kind: ToastKind, text: String) {
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if self.settings.notifications && (self.hidden || !focused) {
            worker::notify(text.clone());
        }
        self.toast(kind, text);
    }

    pub fn state(&self, id: &str) -> Option<&AppState> {
        self.states.iter().find(|s| s.app.id == id)
    }

    pub fn update_count(&self) -> usize {
        self.states.iter().filter(|s| s.update_available).count()
    }

    fn reload_states(&mut self) {
        self.states = self.manager.states();
        if let Some(tray) = &self.tray {
            tray.set_updates(self.update_count());
        }
    }

    pub fn app_name(&self, key: &str) -> String {
        self.manager.app(key).map(|a| a.name).unwrap_or_else(|| key.to_string())
    }

    fn process_messages(&mut self, ctx: &egui::Context) {
        let mut reload = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Progress { key, event } => {
                    if let Some(job) = self.jobs.get_mut(&key) {
                        match event {
                            ProgressEvent::Stage(s) => job.stage = s,
                            ProgressEvent::Bytes { done, total } => {
                                job.done = done;
                                job.total = total;
                            }
                        }
                    }
                }
                Msg::JobDone { key, kind, result } => {
                    self.jobs.remove(&key);
                    self.job_order.retain(|k| *k != key);
                    reload = true;
                    if result.is_ok() && self.pending_open.as_ref().is_some_and(|(id, _)| *id == key) {
                        if let Some((id, file)) = self.pending_open.take() {
                            self.actions.push(Action::Launch(id, vec![file]));
                        }
                    }
                    match result {
                        Ok(text) => self.announce(ctx, ToastKind::Success, text),
                        Err(text) if text == "Cancelled" => self.toast(ToastKind::Info, "Cancelled"),
                        Err(text) => {
                            let name = self.app_name(&key);
                            self.announce(ctx, ToastKind::Error, format!("{} {name} failed: {text}", kind.verb()));
                        }
                    }
                    self.pump_queue();
                }
                Msg::Refreshed { errors, manual } => {
                    self.refreshing = false;
                    reload = true;
                    if manual && !errors.is_empty() {
                        self.toast(
                            ToastKind::Error,
                            format!("Couldn't check {} app(s); see the Updates page", errors.len()),
                        );
                    }
                    self.refresh_errors = errors;
                    self.after_refresh(ctx, manual);
                }
                Msg::Icons(ids) => theme::load_icons(ctx, &self.manager, Some(&ids)),
                Msg::Articles(articles) => self.articles = articles,
                Msg::Readme(id, result) => {
                    self.readme_loading.remove(&id);
                    self.readmes.insert(id, result);
                }
                Msg::Fonts(result) => {
                    self.fonts_loading = false;
                    self.fonts = Some(result);
                }
                Msg::Verified(id, result) => match result {
                    Ok(report) if report.is_ok() => {
                        self.toast(
                            ToastKind::Success,
                            format!("{}: all {} files are intact", self.app_name(&id), report.checked),
                        );
                        self.verify_results.insert(id, report);
                    }
                    Ok(report) => {
                        let n = report.missing.len() + report.changed.len();
                        self.toast(
                            ToastKind::Info,
                            format!("{}: {n} damaged or missing file(s); repairing", self.app_name(&id)),
                        );
                        self.verify_results.insert(id.clone(), report);
                        self.request(&id, JobKind::Repair);
                    }
                    Err(err) => {
                        self.toast(ToastKind::Info, format!("Couldn't check the files ({err}); reinstalling"));
                        self.request(&id, JobKind::Repair);
                    }
                },
                Msg::Tour(id, result) => {
                    self.tour_loading.remove(&id);
                    self.tours.insert(id, result);
                }
                Msg::Shot(url, result) => {
                    self.shots_loading.remove(&url);
                    let texture = result.map(|img| ctx.load_texture(url.clone(), img, egui::TextureOptions::LINEAR));
                    self.shots.insert(url, texture);
                }
                Msg::Files(entries) => {
                    self.files.entries = entries;
                    self.files.scanning = false;
                    self.files.scanned = true;
                }
                Msg::FilesChanged => {
                    self.files.changed_at.get_or_insert_with(Instant::now);
                }
                Msg::Thumb(path, image) => {
                    self.files.thumbs_in_flight = self.files.thumbs_in_flight.saturating_sub(1);
                    let thumb = match image {
                        Some(img) => Thumb::Ready(ctx.load_texture(
                            format!("thumb:{}", path.display()),
                            img,
                            egui::TextureOptions::LINEAR,
                        )),
                        None => Thumb::None,
                    };
                    self.files.thumbs.insert(path, thumb);
                    self.pump_thumbs();
                }
                Msg::Portable(found) => self.files.portable = found,
                Msg::Imported(id, result) => match result {
                    Ok(text) => self.toast(ToastKind::Success, text),
                    Err(err) => self
                        .toast(ToastKind::Error, format!("Couldn't import {}'s settings: {err}", self.app_name(&id))),
                },
                Msg::SelfUpdateFound(found) => self.self_update = found.map(|b| *b),
                Msg::TestNotification(result) => match result {
                    Ok(()) => self.toast(ToastKind::Success, "Sent a test notification"),
                    Err(err) => {
                        self.toast(ToastKind::Error, format!("This desktop didn't accept the notification: {err}"))
                    }
                },
                Msg::SelfUpdateDone(result) => {
                    self.self_updating = false;
                    match result {
                        Ok(text) => {
                            self.self_update = None;
                            self.toast(ToastKind::Success, text);
                        }
                        Err(text) => self.toast(ToastKind::Error, format!("CraftSpace update failed: {text}")),
                    }
                }
            }
        }
        if reload {
            self.reload_states();
        }
    }

    fn after_refresh(&mut self, ctx: &egui::Context, manual: bool) {
        self.reload_states();
        // Apps a machine policy requires, once per run.
        if !self.required_queued {
            self.required_queued = true;
            for id in self.manager.required_missing() {
                self.start_job(&id, JobKind::Install(None));
            }
        }
        let updates: Vec<String> =
            self.states.iter().filter(|s| s.update_available).map(|s| s.app.id.clone()).collect();
        if updates.is_empty() {
            if manual {
                self.toast(ToastKind::Info, "All your apps are up to date");
            }
            return;
        }
        if self.settings.auto_install_updates {
            for id in updates {
                self.request_quietly(&id, JobKind::Update);
            }
        } else {
            let n = updates.len();
            self.announce(ctx, ToastKind::Info, format!("{n} update{} available", if n == 1 { "" } else { "s" }));
        }
    }

    // ---- jobs ------------------------------------------------------------------------------

    /// Queue a job; it starts when a download slot is free.
    fn start_job(&mut self, key: &str, kind: JobKind) {
        self.start_job_in(key, kind, None);
    }

    fn start_job_in(&mut self, key: &str, kind: JobKind, location: Option<PathBuf>) {
        if self.jobs.contains_key(key) {
            return;
        }
        let label = match &kind {
            JobKind::InstallFonts(Some(f)) | JobKind::RemoveFonts(Some(f)) => f.clone(),
            JobKind::InstallFonts(None) | JobKind::RemoveFonts(None) => "Fonts".into(),
            _ => self.app_name(key),
        };
        let mut job = Job::new(kind, label);
        job.location = location;
        self.jobs.insert(key.to_string(), job);
        self.job_order.push(key.to_string());
        self.pump_queue();
    }

    fn pump_queue(&mut self) {
        let limit = self.settings.max_parallel_downloads.max(1) as usize;
        let mut active = self.jobs.values().filter(|j| !j.queued).count();
        for key in self.job_order.clone() {
            if active >= limit {
                break;
            }
            if let Some(job) = self.jobs.get_mut(&key).filter(|j| j.queued) {
                job.queued = false;
                worker::start(&self.bus, &self.manager, &key, job);
                active += 1;
            }
        }
    }

    /// Start a job, asking first if it would change an app that is open.
    fn request(&mut self, id: &str, kind: JobKind) {
        if kind.touches_app() && self.manager.is_running(id) {
            self.running_prompt = Some((id.to_string(), kind));
        } else {
            self.start_job(id, kind);
        }
    }

    /// Like [`Self::request`] but without a dialog: open apps are updated once they close.
    fn request_quietly(&mut self, id: &str, kind: JobKind) {
        if kind.touches_app() && self.manager.is_running(id) {
            if !self.waiting_for_close.iter().any(|(k, _)| k == id) {
                self.toast(ToastKind::Info, format!("{} is open; it will update when you close it", self.app_name(id)));
                self.waiting_for_close.push((id.to_string(), kind));
            }
        } else {
            self.start_job(id, kind);
        }
    }

    fn check_waiting(&mut self) {
        if self.waiting_for_close.is_empty() || self.last_close_check.elapsed() < Duration::from_secs(4) {
            return;
        }
        self.last_close_check = Instant::now();
        let (ready, still): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.waiting_for_close).into_iter().partition(|(id, _)| !self.manager.is_running(id));
        self.waiting_for_close = still;
        for (id, kind) in ready {
            self.start_job(&id, kind);
        }
    }

    /// Open the Files tab: the cached list right away, a fresh scan, the folder watcher and the
    /// portable-settings search.
    pub fn files_opened(&mut self) {
        if !self.files.scanned && !self.files.scanning {
            if self.files.entries.is_empty() {
                if let Some(cached) = self.manager.cached_files() {
                    self.files.entries = cached;
                }
            }
            self.start_file_scan();
        }
        self.update_watcher();
        if self.settings.files.find_portable && !self.files.portable_checked {
            self.files.portable_checked = true;
            worker::find_portable(&self.bus, &self.manager);
        }
    }

    /// Start or stop watching the Files folders to match the settings.
    fn update_watcher(&mut self) {
        self.files.watcher = None;
        if self.settings.files.watch {
            self.files.watcher = worker::watch_files(&self.bus, &self.manager);
        }
    }

    /// Ask for the thumbnail of `path` (shown once it's made).
    pub fn want_thumb(&mut self, path: &std::path::Path) {
        if !self.settings.files.thumbnails || self.files.thumbs.contains_key(path) {
            return;
        }
        // Keep memory in check on huge folders: thumbnails reload quickly from the disk cache.
        if self.files.thumbs.len() > 800 {
            self.files.thumbs.retain(|_, t| matches!(t, Thumb::Loading));
        }
        self.files.thumbs.insert(path.to_path_buf(), Thumb::Loading);
        self.files.thumb_queue.push_back(path.to_path_buf());
        self.pump_thumbs();
    }

    fn pump_thumbs(&mut self) {
        while self.files.thumbs_in_flight < 4 {
            let Some(path) = self.files.thumb_queue.pop_back() else { break };
            self.files.thumbs_in_flight += 1;
            worker::fetch_thumb(&self.bus, &self.manager, path);
        }
    }

    /// Rescan a moment after the last change in a watched folder.
    fn rescan_if_changed(&mut self) {
        if let Some(at) = self.files.changed_at {
            if at.elapsed() >= Duration::from_millis(1500) && !self.files.scanning {
                self.files.changed_at = None;
                self.start_file_scan();
            }
        }
    }

    pub fn start_file_scan(&mut self) {
        self.files.cancel.store(true, Ordering::Relaxed);
        self.files.cancel = Arc::new(AtomicBool::new(false));
        self.files.scanning = true;
        worker::scan_files(&self.bus, &self.manager, self.files.cancel.clone());
    }

    pub fn ensure_readme(&mut self, id: &str) {
        if !self.readmes.contains_key(id) && self.readme_loading.insert(id.to_string()) {
            worker::fetch_readme(&self.bus, &self.manager, id);
        }
    }

    pub fn ensure_tour(&mut self, id: &str) {
        if !self.tours.contains_key(id) && self.tour_loading.insert(id.to_string()) {
            worker::fetch_tour(&self.bus, &self.manager, id);
        }
    }

    pub fn ensure_shot(&mut self, url: &str) {
        if !self.shots.contains_key(url) && self.shots_loading.insert(url.to_string()) {
            worker::fetch_shot(&self.bus, &self.manager, url);
        }
    }

    pub fn ensure_fonts(&mut self, addon: &str) {
        if self.fonts.is_none() && !self.fonts_loading {
            self.fonts_loading = true;
            worker::fetch_fonts(&self.bus, &self.manager, addon);
        }
    }

    fn save_settings(&mut self, settings: Settings) {
        let old_files = self.settings.files.clone();
        let files_changed = settings.files != old_files;
        let rescan = settings.file_locations != self.settings.file_locations
            || settings.files.app_recents != old_files.app_recents;
        let rewatch = rescan || settings.files.watch != old_files.watch;
        let types_changed = settings.files.open_with_craftspace != old_files.open_with_craftspace;
        let refetch = settings.include_prereleases != self.settings.include_prereleases
            || settings.prefer_system_installer != self.settings.prefer_system_installer
            || settings.github_token != self.settings.github_token;
        let autostart = settings.start_at_login != self.settings.start_at_login;
        match self.manager.set_settings(settings) {
            Ok(()) => {
                self.settings = self.manager.settings();
                self.reload_states();
                if rescan && self.files.scanned {
                    self.start_file_scan();
                }
                if files_changed {
                    if rewatch && (self.files.scanned || self.files.watcher.is_some()) {
                        self.update_watcher();
                    }
                    if !self.settings.files.thumbnails {
                        self.files.thumbs.clear();
                        self.files.thumb_queue.clear();
                    }
                    if self.settings.files.find_portable && !old_files.find_portable {
                        self.files.portable_checked = false;
                        if self.tab == Tab::Files {
                            self.files_opened();
                        }
                    } else if !self.settings.files.find_portable {
                        self.files.portable.clear();
                    }
                }
                if types_changed {
                    self.register_file_types();
                }
                if refetch {
                    self.start_refresh(true, false);
                }
                if autostart {
                    if let Err(err) =
                        craftspace_core::autostart::set(self.settings.start_at_login, &launcher_path(&self.manager))
                    {
                        self.toast(ToastKind::Error, format!("Couldn't change start at login: {err:#}"));
                    }
                }
                self.pump_queue();
            }
            Err(err) => self.toast(ToastKind::Error, format!("Couldn't save settings: {err:#}")),
        }
    }

    fn handle_actions(&mut self, ctx: &egui::Context) {
        for action in std::mem::take(&mut self.actions) {
            match action {
                Action::Install(id, version) => {
                    let installed = self.manager.installed_app(&id).is_some();
                    let kind = if installed && version.is_none() { JobKind::Update } else { JobKind::Install(version) };
                    if installed {
                        self.request(&id, kind);
                    } else {
                        self.start_job(&id, kind);
                    }
                }
                Action::InstallTo(id) => {
                    let name = self.app_name(&id);
                    let start = self.settings.install_dir.clone().unwrap_or_else(|| self.manager.apps_root());
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title(format!("Install {name} in"))
                        .set_directory(&start)
                        .pick_folder()
                    {
                        self.toast(ToastKind::Info, format!("Installing {name} in {}", dir.join(&id).display()));
                        self.start_job_in(&id, JobKind::Install(None), Some(dir));
                    }
                }
                Action::InstallMany(ids) => {
                    let n = ids.len();
                    for id in ids {
                        self.start_job(&id, JobKind::Install(None));
                    }
                    self.toast(ToastKind::Info, format!("Installing {n} app{}", if n == 1 { "" } else { "s" }));
                }
                Action::Update(id) => self.request(&id, JobKind::Update),
                Action::UpdateAll => {
                    let ids: Vec<String> =
                        self.states.iter().filter(|s| s.update_available).map(|s| s.app.id.clone()).collect();
                    for id in ids {
                        self.request_quietly(&id, JobKind::Update);
                    }
                }
                Action::AskUninstall(id) => self.confirm_uninstall = Some(id),
                Action::Uninstall(id) => self.request(&id, JobKind::Uninstall),
                Action::Rollback(id) => self.request(&id, JobKind::Rollback),
                Action::VerifyAndRepair(id) => {
                    self.toast(ToastKind::Info, format!("Checking {}'s files…", self.app_name(&id)));
                    worker::verify(&self.bus, &self.manager, &id);
                }
                Action::Cancel(key) => {
                    match self.jobs.get(&key) {
                        Some(job) if job.queued => {
                            self.jobs.remove(&key);
                            self.job_order.retain(|k| *k != key);
                        }
                        Some(job) => job.cancel.store(true, Ordering::Relaxed),
                        None => {}
                    }
                    self.waiting_for_close.retain(|(k, _)| *k != key);
                }
                Action::WhenClosed(id, kind) => {
                    self.toast(
                        ToastKind::Info,
                        format!("{} will continue when you close {}", kind.verb(), self.app_name(&id)),
                    );
                    self.waiting_for_close.push((id, kind));
                }
                Action::RunNow(id, kind) => self.start_job(&id, kind),
                Action::Launch(id, files) => match self.manager.launch(&id, &files) {
                    Ok(()) => {
                        let name = self.app_name(&id);
                        self.toast(ToastKind::Info, format!("Opening {name}…"));
                    }
                    Err(err) => self.toast(ToastKind::Error, format!("{err:#}")),
                },
                // Straight to the system's browser, so a failure can be reported.
                Action::OpenUrl(url) => {
                    if let Err(err) = open::that_detached(&url) {
                        self.toast(ToastKind::Error, format!("Couldn't open {url}: {err}"));
                    }
                }
                Action::OpenPath(path) => {
                    if let Err(err) = open::that_detached(&path) {
                        self.toast(ToastKind::Error, format!("Couldn't open {}: {err}", path.display()));
                    }
                }
                Action::Reveal(path) => reveal(&path),
                Action::ShowApp(id) => {
                    if !matches!(self.apps_view, AppsView::Detail(_)) {
                        self.back_view = self.apps_view.clone();
                    }
                    self.tab = Tab::Apps;
                    self.detail_release = None;
                    self.detail_tab = DetailTab::Overview;
                    self.apps_view = AppsView::Detail(id);
                }
                Action::GoApps(view) => {
                    self.tab = Tab::Apps;
                    self.apps_view = view;
                }
                Action::Refresh => self.start_refresh(true, true),
                Action::RescanFiles => self.start_file_scan(),
                Action::TogglePin(path) => {
                    let mut s = self.settings.clone();
                    if let Some(i) = s.pinned_files.iter().position(|p| *p == path) {
                        s.pinned_files.remove(i);
                    } else {
                        s.pinned_files.insert(0, path);
                    }
                    self.save_settings(s);
                }
                Action::AddLocation => {
                    if let Some(dir) = rfd::FileDialog::new().set_title("Add a folder to Files").pick_folder() {
                        let mut s = self.settings.clone();
                        if !s.file_locations.contains(&dir) {
                            s.file_locations.push(dir);
                            self.save_settings(s);
                            self.start_file_scan();
                        }
                    }
                }
                Action::RemoveLocation(dir) => {
                    let mut s = self.settings.clone();
                    s.file_locations.retain(|d| *d != dir);
                    self.save_settings(s);
                    self.start_file_scan();
                }
                Action::OpenSettings => self.settings_draft = Some(self.settings.clone()),
                Action::SaveSettings(mut s) => {
                    // The Settings window doesn't edit these (its own window does, maybe while
                    // Settings is open), so keep what's saved.
                    s.files = self.settings.files.clone();
                    self.save_settings(*s)
                }
                Action::ApplySelfUpdate => {
                    if let Some(update) = self.self_update.clone() {
                        self.self_updating = true;
                        worker::apply_self_update(&self.bus, &self.manager, update);
                    }
                }
                Action::SelfInstall => {
                    self.offer_self_install = false;
                    match selfupdate::self_install(&self.manager) {
                        Ok(_) => self.toast(
                            ToastKind::Success,
                            "CraftSpace is installed. Find it in your Start menu or app menu.",
                        ),
                        Err(err) => self.toast(ToastKind::Error, format!("Couldn't install CraftSpace: {err:#}")),
                    }
                }
                Action::DismissSelfInstall => {
                    self.offer_self_install = false;
                    let s = Settings { offer_self_install: false, ..self.settings.clone() };
                    self.save_settings(s);
                }
                Action::TestNotification => worker::test_notification(&self.bus),
                Action::OpenAbout => self.about_open = true,
                Action::SaveFilesFeatures(features) => {
                    let mut s = self.settings.clone();
                    s.files = *features;
                    s.files.setup_done = true;
                    self.files.setup_draft = None;
                    self.files.setup_window = false;
                    self.save_settings(s);
                }
                Action::FilesSetupLater => {
                    let mut s = self.settings.clone();
                    s.files.setup_done = true;
                    self.files.setup_draft = None;
                    self.save_settings(s);
                    self.toast(ToastKind::Info, "You can change the Files features any time in Settings › Files");
                }
                Action::OpenFilesSetup => {
                    self.files.setup_draft = Some(self.settings.files.clone());
                    self.files.setup_window = true;
                }
                Action::SetFilesGrid(grid) => {
                    let mut s = self.settings.clone();
                    s.files.grid = grid;
                    self.save_settings(s);
                }
                Action::ImportPortable(found) => {
                    self.files.portable.retain(|p| p.dir != found.dir);
                    worker::import_portable(&self.bus, &self.manager, found);
                }
                Action::DismissPortable(dir) => {
                    self.files.portable.retain(|p| p.dir != dir);
                    if let Err(err) = self.manager.forget_portable(&dir) {
                        self.toast(ToastKind::Error, format!("{err:#}"));
                    }
                    self.settings = self.manager.settings();
                }
                Action::Cleanup => match self.manager.cleanup() {
                    Ok(freed) => self
                        .toast(ToastKind::Success, format!("Freed {}", craftspace_core::download::format_bytes(freed))),
                    Err(err) => self.toast(ToastKind::Error, format!("{err:#}")),
                },
                Action::CopyText(text) => {
                    ctx.copy_text(text);
                    self.toast(ToastKind::Info, "Copied");
                }
                Action::SetChannel(id, channel) => {
                    let mut s = self.settings.clone();
                    if channel == Channel::Default {
                        s.channels.remove(&id);
                    } else {
                        s.channels.insert(id, channel);
                    }
                    self.save_settings(s);
                }
                Action::ReportBug(id) => {
                    if let Some(url) = self.manager.bug_report_url(&id) {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                    }
                }
                Action::InstallFonts(family) => {
                    if let Some(addon) = self.font_addon_id() {
                        self.start_job(
                            &format!("addon:{addon}:{}", family.as_deref().unwrap_or("*")),
                            JobKind::InstallFonts(family),
                        );
                    }
                }
                Action::RemoveFonts(family) => {
                    if let Some(addon) = self.font_addon_id() {
                        self.start_job(
                            &format!("addon:{addon}:{}", family.as_deref().unwrap_or("*")),
                            JobKind::RemoveFonts(family),
                        );
                    }
                }
                Action::ExportList => {
                    let list = self.manager.export_list();
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Export app list")
                        .set_file_name("craftspace-apps.json")
                        .save_file()
                    {
                        let result = serde_json::to_vec_pretty(&list)
                            .map_err(anyhow::Error::from)
                            .and_then(|j| Ok(std::fs::write(&path, j)?));
                        match result {
                            Ok(()) => self.toast(
                                ToastKind::Success,
                                format!("Saved {} apps to {}", list.apps.len(), path.display()),
                            ),
                            Err(err) => self.toast(ToastKind::Error, format!("Couldn't save: {err:#}")),
                        }
                    }
                }
                Action::ImportList => {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("Import app list")
                        .add_filter("App list", &["json"])
                        .pick_file()
                    else {
                        continue;
                    };
                    let parsed = std::fs::read(&path)
                        .map_err(anyhow::Error::from)
                        .and_then(|b| serde_json::from_slice::<AppList>(&b).map_err(Into::into))
                        .and_then(|list| self.manager.import_list(&list, false));
                    match parsed {
                        Ok(todo) if todo.is_empty() => {
                            self.toast(ToastKind::Info, "Everything in that list is already installed")
                        }
                        Ok(todo) => {
                            self.settings = self.manager.settings();
                            let ids = todo.into_iter().map(|(id, _)| id).collect();
                            self.actions.push(Action::InstallMany(ids));
                        }
                        Err(err) => {
                            self.toast(ToastKind::Error, format!("Couldn't import {}: {err:#}", path.display()))
                        }
                    }
                }
                Action::OpenMultiInstall => self.multi_install = Some(BTreeSet::new()),
            }
        }
        // Actions queued while handling (import → install).
        if !self.actions.is_empty() {
            self.handle_actions(ctx);
        }
    }

    pub fn font_addon_id(&self) -> Option<String> {
        self.manager
            .catalog()
            .addons
            .into_iter()
            .find(|a| a.kind == craftspace_core::catalog::AddonKind::Fonts)
            .map(|a| a.id)
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCommand::Show => {
                    log::info!("opened from the tray");
                    self.hidden = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayCommand::CheckUpdates => self.start_refresh(true, true),
                TrayCommand::UpdateAll => self.actions.push(Action::UpdateAll),
                TrayCommand::Quit => {
                    for job in self.jobs.values() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.palette;
        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            let (rect, logo) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::click());
            egui::Image::new(&theme::logo(ui.ctx())).paint_at(ui, rect);
            let name =
                ui.add(egui::Label::new(RichText::new("CraftSpace").size(17.0).strong()).sense(egui::Sense::click()));
            if (logo | name).on_hover_text("All apps").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.search.clear();
                self.actions.push(Action::GoApps(AppsView::All));
            }
            ui.add_space(18.0);
            for (tab, label) in [(Tab::Apps, "Apps"), (Tab::Files, "Files"), (Tab::Discover, "Discover")] {
                let r = theme::tab(ui, &p, label, self.tab == tab);
                if r.clicked() {
                    self.tab = tab;
                    if tab == Tab::Files {
                        self.files_opened();
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                // The menu: settings, about, update check.
                let menu = theme::icon_button(ui, &p, theme::Icon::Glyph("⚙"), 30.0).on_hover_text("Menu");
                egui::Popup::menu(&menu).show(|ui| {
                    ui.set_min_width(190.0);
                    if ui.button("Settings…").clicked() {
                        self.actions.push(Action::OpenSettings);
                    }
                    if ui.add_enabled(!self.refreshing, egui::Button::new("Check for updates")).clicked() {
                        self.actions.push(Action::Refresh);
                    }
                    ui.separator();
                    if ui.button("About CraftSpace").clicked() {
                        self.actions.push(Action::OpenAbout);
                    }
                });
                if self.refreshing {
                    ui.add(egui::Spinner::new().size(14.0));
                } else if theme::icon_button(ui, &p, theme::Icon::Glyph("⟳"), 30.0)
                    .on_hover_text("Check for updates")
                    .clicked()
                {
                    self.actions.push(Action::Refresh);
                }
                let updates = self.update_count();
                let bell = theme::icon_button(ui, &p, theme::Icon::Bell, 30.0);
                if updates > 0 {
                    let c = bell.rect.right_top() + Vec2::new(-3.0, 4.0);
                    ui.painter().circle_filled(c, 7.0, p.bad);
                    ui.painter().text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        updates.to_string(),
                        egui::FontId::proportional(10.0),
                        Color32::WHITE,
                    );
                }
                if bell
                    .on_hover_text(if updates > 0 {
                        format!("{updates} update(s) available")
                    } else {
                        "No updates".into()
                    })
                    .clicked()
                {
                    self.actions.push(Action::GoApps(AppsView::Updates));
                }
                if !self.jobs.is_empty() || !self.waiting_for_close.is_empty() {
                    views::downloads::button(self, ui);
                }
                ui.add_space(12.0);
                let hint = match self.tab {
                    Tab::Files => "Search files",
                    _ => "Search ArtCraft apps",
                };
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(format!("🔍  {hint}"))
                        .desired_width(ui.available_width().clamp(120.0, 340.0))
                        .margin(Margin::symmetric(10, 6)),
                );
            });
        });
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_secs(if t.kind == ToastKind::Error { 9 } else { 5 }));
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let mut dismiss = None;
                for (i, t) in self.toasts.iter().enumerate() {
                    let color = match t.kind {
                        ToastKind::Info => p.accent,
                        ToastKind::Success => p.good,
                        ToastKind::Error => p.bad,
                    };
                    Frame::new()
                        .fill(p.panel)
                        .stroke(Stroke::new(1.0, p.stroke))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(14, 10))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 4],
                            blur: 16,
                            spread: 0,
                            color: Color32::from_black_alpha(80),
                        })
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 18.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, CornerRadius::same(2), color);
                                ui.add(egui::Label::new(&t.text).wrap());
                                if ui.add(egui::Button::new(RichText::new("×").color(p.weak)).frame(false)).clicked() {
                                    dismiss = Some(i);
                                }
                            });
                        });
                    ui.add_space(6.0);
                }
                if let Some(i) = dismiss {
                    self.toasts.remove(i);
                }
            });
    }

    fn modals(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        if let Some(id) = self.confirm_uninstall.clone() {
            let name = self.app_name(&id);
            let modal = egui::Modal::new(egui::Id::new("confirm-uninstall")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.heading(format!("Uninstall {name}?"));
                ui.add_space(6.0);
                ui.label(RichText::new("The app, its shortcuts and any version kept for rollback are removed. Your documents and app preferences are kept.").color(p.weak));
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if danger_button(ui, &p, "Uninstall").clicked() {
                        self.actions.push(Action::Uninstall(id.clone()));
                        ui.close();
                    }
                    if theme::pill(ui, &p, "Cancel").clicked() {
                        ui.close();
                    }
                });
            });
            if modal.should_close() {
                self.confirm_uninstall = None;
            }
        }

        if let Some((id, kind)) = self.running_prompt.clone() {
            let name = self.app_name(&id);
            let managed = self.manager.installed_app(&id).is_some_and(|i| i.current.kind.is_managed());
            let modal = egui::Modal::new(egui::Id::new("app-running")).show(ctx, |ui| {
                ui.set_width(440.0);
                ui.heading(format!("{name} is open"));
                ui.add_space(6.0);
                let explain = match (&kind, managed) {
                    (JobKind::Uninstall, _) => format!("Close {name} before uninstalling it, or let CraftSpace do it as soon as you close it."),
                    (_, true) => format!("You can go ahead: the new version installs next to the open one and is used the next time you start {name}. Or wait until you close it."),
                    (_, false) => format!("{name}'s installer needs it to be closed. CraftSpace can continue as soon as you close it."),
                };
                ui.label(RichText::new(explain).color(p.weak));
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if managed && kind != JobKind::Uninstall && theme::primary(ui, &p, &format!("{} now", short_verb(&kind))).clicked() {
                        self.actions.push(Action::RunNow(id.clone(), kind.clone()));
                        ui.close();
                    }
                    if theme::pill(ui, &p, "When it closes").clicked() {
                        self.actions.push(Action::WhenClosed(id.clone(), kind.clone()));
                        ui.close();
                    }
                    if ui.add(egui::Button::new("Cancel").frame(false)).clicked() {
                        ui.close();
                    }
                });
            });
            if modal.should_close() {
                self.running_prompt = None;
            }
        }

        if self.multi_install.is_some() {
            views::apps::multi_install_modal(self, ctx);
        }

        if self.confirm_quit {
            let modal = egui::Modal::new(egui::Id::new("confirm-quit")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.heading("Quit while apps are installing?");
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Installs in progress will be cancelled. Partial downloads resume next time.")
                        .color(p.weak),
                );
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if danger_button(ui, &p, "Quit").clicked() {
                        for job in self.jobs.values() {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                        self.jobs.clear();
                        self.quitting = true;
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close();
                    }
                    if theme::pill(ui, &p, "Keep going").clicked() {
                        ui.close();
                    }
                });
            });
            if modal.should_close() {
                self.confirm_quit = false;
            }
        }

        views::settings::window(self, ctx);
        views::files::setup_window(self, ctx);
        views::about::window(self, ctx);
    }
}

fn short_verb(kind: &JobKind) -> &'static str {
    match kind {
        JobKind::Update => "Update",
        JobKind::Rollback => "Roll back",
        JobKind::Repair => "Repair",
        _ => "Continue",
    }
}

pub fn danger_button(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(p.bad).corner_radius(CornerRadius::same(15)),
    )
}

impl eframe::App for CraftSpaceApp {
    /// Everything that must keep happening while the window is hidden in the tray.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_messages(ctx);
        self.handle_tray(ctx);
        self.rescan_if_changed();
        if self.files.changed_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        self.keep_findable(ctx);
        if self.settings.auto_check_updates {
            let every = Duration::from_secs(u64::from(self.settings.check_interval_hours.max(1)) * 3600);
            if self.last_check.is_none_or(|t| t.elapsed() >= every) {
                self.start_refresh(false, false);
            }
        }
        self.check_waiting();
        if !self.actions.is_empty() {
            self.handle_actions(ctx);
        }
        if !self.jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Theme.
        let system_light = ctx.system_theme() == Some(egui::Theme::Light);
        if self.applied_theme != Some((self.settings.theme, system_light)) {
            self.palette = theme::palette_for(self.settings.theme, &ctx);
            theme::apply(&ctx, &self.palette);
            self.applied_theme = Some((self.settings.theme, system_light));
        }

        // Closing: confirm during installs, else hide to the tray if that's on.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            if !self.jobs.is_empty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm_quit = true;
            } else if self.settings.keep_running_in_tray && self.tray_shown() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.hidden = true;
                log::info!("window closed; still running in the tray");
                if !self.told_about_tray {
                    self.told_about_tray = true;
                    worker::notify(
                        "CraftSpace keeps checking for updates in the background. Quit it from the tray icon.".into(),
                    );
                }
            }
        }

        let p = self.palette;
        egui::Panel::top("top")
            .exact_size(56.0)
            .frame(Frame::new().fill(p.panel).inner_margin(Margin::symmetric(12, 0)).stroke(Stroke::new(1.0, p.stroke)))
            .show(ui, |ui| self.top_bar(ui));

        if self.tab != Tab::Discover {
            egui::Panel::left("sidebar")
                .exact_size(232.0)
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(p.panel)
                        .inner_margin(Margin::symmetric(14, 12))
                        .stroke(Stroke::new(1.0, p.stroke)),
                )
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                        Tab::Apps => views::apps::sidebar(self, ui),
                        Tab::Files => views::files::sidebar(self, ui),
                        Tab::Discover => {}
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg).inner_margin(Margin { left: 28, right: 20, top: 20, bottom: 0 }))
            .show(ui, |ui| match self.tab {
                Tab::Apps => views::apps::content(self, ui),
                Tab::Files => views::files::content(self, ui),
                Tab::Discover => views::discover::content(self, ui),
            });

        if self.downloads_open {
            views::downloads::panel(self, &ctx);
        }
        self.modals(&ctx);
        self.toasts(&ctx);
        self.handle_actions(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for job in self.jobs.values() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.files.cancel.store(true, Ordering::Relaxed);
    }
}

/// What start-at-login should run: the installed copy when there is one.
fn launcher_path(manager: &Manager) -> PathBuf {
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("craftspace"));
    if let Some(bundle) = selfupdate::running_bundle() {
        return bundle.join("Contents/MacOS").join(current.file_name().unwrap_or_default());
    }
    let installed = selfupdate::install_dir(manager).join("bin").join(current.file_name().unwrap_or_default());
    if installed.is_file() {
        installed
    } else {
        current
    }
}

/// Show a file in the system file manager.
fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        let _ = std::process::Command::new("explorer").arg(arg).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        let _ = open::that_detached(dir);
    }
}

pub fn version_string() -> String {
    selfupdate::current_version().to_string()
}
