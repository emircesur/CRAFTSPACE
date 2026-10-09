//! The high-level API the GUI and CLI share: refresh, install, update, roll back, repair,
//! uninstall, launch, plus fonts, add-ons and app lists.
//!
//! A [`Manager`] is cheap to clone and safe to use from several threads; each app can have one
//! operation in flight at a time.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Context;
use semver::Version;
use serde::{Deserialize, Serialize};
use ureq::Agent;

use crate::addons;
use crate::archive;
use crate::catalog::{Addon, AddonKind, AppEntry, Catalog, REMOTE_CATALOG_URL};
use crate::download::{self, Progress, ProgressEvent, Stage};
use crate::fonts::{self, FontFile, InstalledFont};
use crate::github::{self, Asset, GitHub, Release, ReleaseList};
use crate::http::{self, check_status};
use crate::integrate;
use crate::news::{self, Article};
use crate::paths::{write_atomic, Paths};
use crate::platform::{AssetKind, Os, Platform};
use crate::policy::Policy;
use crate::profiles;
use crate::settings::{Channel, Settings};
use crate::state::{dir_size, InstalledApp, InstalledDb, InstalledVersion};
use crate::zsync;

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

struct Inner {
    paths: Paths,
    platform: Platform,
    agent: Agent,
    /// What the user chose; [`Manager::settings`] applies the policy on top.
    settings: RwLock<Settings>,
    policy: Policy,
    catalog: RwLock<Catalog>,
    installed: Mutex<InstalledDb>,
    releases: RwLock<HashMap<String, ReleaseList>>,
    busy: Mutex<HashSet<String>>,
    /// Run installers without any UI.
    quiet: AtomicBool,
}

/// What to install: a release and the asset chosen for this machine.
#[derive(Debug, Clone)]
pub struct Plan {
    pub app: AppEntry,
    pub release: Release,
    pub asset: Asset,
    pub kind: AssetKind,
    /// Install into `<location>/<app id>/<version>` instead of the usual folder (a new install
    /// only: updates stay where the app is).
    pub location: Option<PathBuf>,
}

/// Everything the UI needs to draw one app.
#[derive(Debug, Clone)]
pub struct AppState {
    pub app: AppEntry,
    pub installed: Option<InstalledApp>,
    pub latest: Option<Release>,
    /// The asset this machine would install from `latest`.
    pub installable: Option<(Asset, AssetKind)>,
    pub update_available: bool,
    /// Release information has been fetched (or loaded from cache).
    pub known: bool,
    pub channel: Channel,
}

impl AppState {
    pub fn installed_version(&self) -> Option<&Version> {
        self.installed.as_ref().map(|i| &i.current.version)
    }

    pub fn latest_version(&self) -> Option<&Version> {
        self.latest.as_ref().and_then(|r| r.version.as_ref())
    }
}

/// The result of checking an installed app's files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VerifyReport {
    pub checked: usize,
    pub missing: Vec<PathBuf>,
    pub changed: Vec<PathBuf>,
}

impl VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.changed.is_empty()
    }
}

/// A portable list of apps, for setting up another computer (`craftspace-cli export`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppList {
    /// The CraftSpace version that wrote it.
    pub craftspace: String,
    pub apps: Vec<AppListEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppListEntry {
    pub id: String,
    #[serde(default)]
    pub version: Option<Version>,
    #[serde(default)]
    pub channel: Option<Channel>,
}

/// Files recorded at install time for [`Manager::verify`].
#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    root: PathBuf,
    files: BTreeMap<String, String>,
}

/// Releases a busy marker when dropped.
struct BusyGuard<'a> {
    set: &'a Mutex<HashSet<String>>,
    id: String,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.id);
    }
}

impl Manager {
    pub fn open() -> anyhow::Result<Manager> {
        Manager::open_at(Paths::detect()?)
    }

    pub fn open_at(paths: Paths) -> anyhow::Result<Manager> {
        Manager::open_with_policy(paths, Policy::load())
    }

    pub fn open_with_policy(paths: Paths, mut policy: Policy) -> anyhow::Result<Manager> {
        paths.ensure().with_context(|| format!("creating {}", paths.root.display()))?;
        // The centrally published policy, as last fetched.
        if policy.policy_url.is_some() {
            if let Some(remote) =
                std::fs::read(remote_policy_file(&paths)).ok().and_then(|b| serde_json::from_slice::<Policy>(&b).ok())
            {
                policy.overlay(remote);
            }
        }
        let settings = Settings::load(&paths.settings_file());
        let mut installed = InstalledDb::load(&paths.installed_file());
        installed.retry_pending_removal();

        let mut catalog = Catalog::builtin();
        if policy.apply(&settings).remote_catalog {
            if let Some(cached) =
                std::fs::read_to_string(paths.catalog_cache()).ok().and_then(|t| Catalog::parse(&t).ok())
            {
                catalog = merge_catalogs(catalog, cached);
            }
        }

        let mut releases = HashMap::new();
        for app in &catalog.apps {
            if let Some(list) = github::read_cache(&paths.release_cache(), &app.repo) {
                releases.insert(app.id.clone(), list);
            }
        }

        let manager = Manager {
            inner: Arc::new(Inner {
                platform: Platform::current(),
                agent: http::agent(),
                quiet: AtomicBool::new(policy.quiet),
                settings: RwLock::new(settings),
                policy,
                catalog: RwLock::new(catalog),
                installed: Mutex::new(installed),
                releases: RwLock::new(releases),
                busy: Mutex::new(HashSet::new()),
                paths,
            }),
        };
        manager.apply_speed_limit();
        Ok(manager)
    }

    pub fn paths(&self) -> &Paths {
        &self.inner.paths
    }

    pub fn platform(&self) -> Platform {
        self.inner.platform
    }

    pub fn agent(&self) -> &Agent {
        &self.inner.agent
    }

    pub fn policy(&self) -> &Policy {
        &self.inner.policy
    }

    /// Run installers without showing their UI (for scripts and managed machines).
    pub fn set_quiet(&self, quiet: bool) {
        self.inner.quiet.store(quiet, Ordering::Relaxed);
    }

    fn quiet(&self) -> bool {
        self.inner.quiet.load(Ordering::Relaxed)
    }

    /// The settings in effect: the user's, with the machine policy forced on top.
    pub fn settings(&self) -> Settings {
        self.inner.policy.apply(&self.inner.settings.read().unwrap())
    }

    pub fn set_settings(&self, settings: Settings) -> anyhow::Result<()> {
        settings.save(&self.inner.paths.settings_file())?;
        *self.inner.settings.write().unwrap() = settings;
        self.apply_speed_limit();
        Ok(())
    }

    fn apply_speed_limit(&self) {
        download::set_speed_limit(self.settings().download_limit_kbps.map(|k| u64::from(k) * 1024));
    }

    /// The app list, without apps the policy doesn't allow.
    pub fn catalog(&self) -> Catalog {
        let mut catalog = self.inner.catalog.read().unwrap().clone();
        crate::sources::apply(&mut catalog, &self.settings().other_sources);
        catalog.apps.retain(|a| self.inner.policy.allows(&a.id));
        catalog
    }

    pub fn app(&self, id: &str) -> Option<AppEntry> {
        let other = self.settings().other_sources;
        let mut app = self.inner.catalog.read().unwrap().app(id).cloned().or_else(|| {
            other
                .enabled
                .then(|| other.apps.iter().find(|a| a.id.eq_ignore_ascii_case(id)).map(crate::sources::custom_entry))?
        })?;
        if other.enabled {
            if let Some(repo) = other.overrides.get(&app.id) {
                app.repo.clone_from(repo);
            }
        }
        self.inner.policy.allows(&app.id).then_some(app)
    }

    // ---- other sources (optional) --------------------------------------------------------

    /// What `repo` offers this computer: its latest release and the file that would be
    /// installed. `id` is the app's id, for release files named the ArtCraft way.
    pub fn check_source(&self, repo: &str, id: &str) -> anyhow::Result<crate::sources::SourceCheck> {
        let repo = crate::sources::normalize_repo(repo)?;
        let settings = self.settings();
        let list = self.github().releases(&repo, true)?;
        let release = list
            .latest(settings.include_prereleases)
            .or_else(|| list.latest(true))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{repo} has no releases"))?;
        let prefs = self.app(id).map(|a| settings.asset_prefs(&a)).unwrap_or_default();
        let (i, kind) = self.inner.platform.select_asset(id, &release.asset_names(), prefs).ok_or_else(|| {
            anyhow::anyhow!("{repo} {} has no file for {}", release.tag, self.inner.platform.display())
        })?;
        Ok(crate::sources::SourceCheck {
            repo,
            tag: release.tag.clone(),
            version: release.version.clone(),
            asset: release.assets[i].name.clone(),
            kind,
        })
    }

    fn require_other_sources(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.settings().other_sources.enabled,
            "other sources are off; turn them on in Settings › Other sources (or: craftspace-cli source enable)"
        );
        anyhow::ensure!(
            !self.inner.policy.locks("other_sources"),
            "your organization manages other sources on this computer"
        );
        Ok(())
    }

    /// Add an app from a GitHub repository. Returns it once its releases check out.
    pub fn add_source_app(
        &self,
        repo: &str,
        name: Option<&str>,
        binary: Option<&str>,
    ) -> anyhow::Result<(crate::settings::CustomApp, crate::sources::SourceCheck)> {
        self.require_other_sources()?;
        let repo = crate::sources::normalize_repo(repo)?;
        let mut settings = self.settings();
        anyhow::ensure!(
            !settings.other_sources.apps.iter().any(|a| a.repo.eq_ignore_ascii_case(&repo)),
            "{repo} is already added"
        );
        let catalog = self.inner.catalog.read().unwrap().clone();
        let taken = |id: &str| catalog.app(id).is_some() || settings.other_sources.apps.iter().any(|a| a.id == id);
        let id = crate::sources::new_id(&repo, &taken);
        let app = crate::settings::CustomApp {
            id: id.clone(),
            name: name
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| crate::sources::name_from_repo(&repo)),
            repo: repo.clone(),
            binary: binary.map(str::trim).filter(|b| !b.is_empty()).map(str::to_string),
        };
        settings.other_sources.apps.push(app.clone());
        self.set_settings(settings.clone())?;
        match self.check_source(&repo, &id) {
            Ok(check) => {
                let _ = self.refresh(&id, true);
                Ok((app, check))
            }
            Err(err) => {
                settings.other_sources.apps.retain(|a| a.id != id);
                self.set_settings(settings)?;
                Err(err)
            }
        }
    }

    /// Remove an app added from GitHub (uninstall it first).
    pub fn remove_source_app(&self, id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(self.installed_app(id).is_none(), "uninstall {id} first");
        let mut settings = self.settings();
        let before = settings.other_sources.apps.len();
        settings.other_sources.apps.retain(|a| a.id != id);
        anyhow::ensure!(settings.other_sources.apps.len() < before, "{id} wasn't added from GitHub");
        self.inner.releases.write().unwrap().remove(id);
        self.set_settings(settings)
    }

    /// Update an ArtCraft app (or CraftSpace, as `craftspace`) from another repository: a fork,
    /// a mirror, a backup. `None` goes back to the official one.
    pub fn set_app_source(&self, id: &str, repo: Option<&str>) -> anyhow::Result<Option<crate::sources::SourceCheck>> {
        self.require_other_sources()?;
        let repo = repo.map(crate::sources::normalize_repo).transpose()?;
        let check = match &repo {
            Some(r) => Some(self.check_source(r, id)?),
            None => None,
        };
        let mut settings = self.settings();
        if id == crate::selfupdate::NAME {
            settings.other_sources.self_repo = repo;
        } else {
            anyhow::ensure!(self.inner.catalog.read().unwrap().app(id).is_some(), "unknown app '{id}'");
            match repo {
                Some(r) => settings.other_sources.overrides.insert(id.to_string(), r),
                None => settings.other_sources.overrides.remove(id),
            };
        }
        self.set_settings(settings)?;
        if id != crate::selfupdate::NAME {
            let _ = self.refresh(id, true);
        }
        Ok(check)
    }

    fn require_app(&self, id: &str) -> anyhow::Result<AppEntry> {
        self.app(id).ok_or_else(|| anyhow::anyhow!("unknown app '{id}' (see `craftspace-cli list`)"))
    }

    pub fn installed(&self) -> InstalledDb {
        self.inner.installed.lock().unwrap().clone()
    }

    pub fn installed_app(&self, id: &str) -> Option<InstalledApp> {
        self.inner.installed.lock().unwrap().get(id).cloned()
    }

    /// Apply `f` to the installed-apps database (re-read from disk first, so a CLI and the GUI
    /// running side by side don't overwrite each other) and save it.
    fn with_db<R>(&self, f: impl FnOnce(&mut InstalledDb) -> R) -> anyhow::Result<R> {
        let mut guard = self.inner.installed.lock().unwrap();
        let path = self.inner.paths.installed_file();
        if path.exists() {
            *guard = InstalledDb::load(&path);
        }
        let result = f(&mut guard);
        guard.save(&path)?;
        Ok(result)
    }

    pub fn github(&self) -> GitHub {
        GitHub::new(
            self.inner.agent.clone(),
            self.settings().effective_github_token(),
            self.inner.paths.release_cache(),
        )
    }

    pub fn apps_root(&self) -> PathBuf {
        self.settings().install_dir.unwrap_or_else(|| self.inner.paths.apps.clone())
    }

    /// The folder holding an app's versions: a chosen location for a new install, else where the
    /// installed version already is (so updates don't move it), else the install folder.
    fn app_dir(&self, plan: &Plan) -> PathBuf {
        if let Some(location) = &plan.location {
            return location.join(&plan.app.id);
        }
        // A portable copy or AppImage found on this computer: new versions go next to it.
        if let Some(home) = self.installed_app(&plan.app.id).and_then(|a| a.current.external).and_then(|e| e.home) {
            return home;
        }
        self.installed_app(&plan.app.id)
            .and_then(|a| a.current.dir.filter(|_| a.current.kind.is_managed()))
            .and_then(|d| d.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.apps_root().join(&plan.app.id))
    }

    // ---- discovery -------------------------------------------------------------------------

    /// Fetch the latest app list from the CraftSpace repository. Returns whether apps were added.
    pub fn refresh_catalog(&self) -> anyhow::Result<bool> {
        if !self.settings().remote_catalog {
            return Ok(false);
        }
        let mut resp = self.inner.agent.get(REMOTE_CATALOG_URL).call()?;
        check_status(REMOTE_CATALOG_URL, resp.status().as_u16())?;
        let text = resp.body_mut().read_to_string()?;
        let remote = Catalog::parse(&text)?;
        write_atomic(&self.inner.paths.catalog_cache(), text.as_bytes())?;
        let mut catalog = self.inner.catalog.write().unwrap();
        let before = catalog.apps.len();
        *catalog = merge_catalogs(Catalog::builtin(), remote);
        Ok(catalog.apps.len() > before)
    }

    /// Fetch releases for one app.
    pub fn refresh(&self, id: &str, force: bool) -> anyhow::Result<ReleaseList> {
        let app = self.require_app(id)?;
        let list = self.github().releases(&app.repo, force)?;
        self.inner.releases.write().unwrap().insert(app.id.clone(), list.clone());
        Ok(list)
    }

    /// Fetch releases for every app in parallel. Returns the apps that failed.
    pub fn refresh_all(&self, force: bool) -> Vec<(String, anyhow::Error)> {
        let ids: Vec<String> = self.catalog().apps.into_iter().map(|a| a.id).collect();
        let errors = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for chunk in ids.chunks(ids.len().div_ceil(6).max(1)) {
                let errors = &errors;
                scope.spawn(move || {
                    for id in chunk {
                        if let Err(err) = self.refresh(id, force) {
                            errors.lock().unwrap().push((id.clone(), err));
                        }
                    }
                });
            }
        });
        errors.into_inner().unwrap()
    }

    pub fn releases(&self, id: &str) -> Option<ReleaseList> {
        self.inner.releases.read().unwrap().get(id).cloned()
    }

    fn wants_prereleases(&self, settings: &Settings, id: &str, installed: Option<&InstalledApp>) -> bool {
        settings.wants_prereleases(id)
            || (settings.channel(id) == Channel::Default
                && installed.is_some_and(|i| !i.current.version.pre.is_empty()))
    }

    /// Which kinds of package to prefer for `app`. An app installed without CraftSpace keeps its
    /// kind (its installer, package, AppImage or portable copy), so it updates where it is.
    fn prefs_for(
        &self,
        app: &AppEntry,
        settings: &Settings,
        installed: Option<&InstalledApp>,
    ) -> crate::platform::AssetPrefs {
        let mut prefs = settings.asset_prefs(app);
        if let Some(current) = installed.map(|i| &i.current).filter(|c| c.external.is_some()) {
            match current.kind {
                AssetKind::Msi => (prefs.prefer_system_installer, prefs.prefer_exe_installer) = (true, false),
                AssetKind::Exe => (prefs.prefer_system_installer, prefs.prefer_exe_installer) = (true, true),
                AssetKind::Rpm | AssetKind::Deb => prefs.prefer_system_installer = true,
                AssetKind::AppImage => (prefs.prefer_system_installer, prefs.prefer_appimage) = (false, true),
                AssetKind::PortableZip | AssetKind::TarGz => {
                    (prefs.prefer_system_installer, prefs.prefer_appimage) = (false, false)
                }
                AssetKind::Dmg => {}
            }
        }
        prefs
    }

    pub fn state(&self, id: &str) -> Option<AppState> {
        let app = self.app(id)?;
        let settings = self.settings();
        let installed = self.installed_app(id);
        let list = self.releases(id);
        let channel = settings.channel(id);
        let latest =
            list.as_ref().and_then(|l| l.latest(self.wants_prereleases(&settings, id, installed.as_ref()))).cloned();
        let installable = latest.as_ref().and_then(|r| {
            self.inner
                .platform
                .select_asset(&app.id, &r.asset_names(), self.prefs_for(&app, &settings, installed.as_ref()))
                .map(|(i, kind)| (r.assets[i].clone(), kind))
        });
        let from_flatpak =
            installed.as_ref().is_some_and(|i| i.current.external.as_ref().is_some_and(|e| e.flatpak.is_some()));
        let update_available = match (&installed, latest.as_ref().and_then(|r| r.version.as_ref())) {
            (_, _) if matches!(channel, Channel::Pinned(_)) || from_flatpak => false,
            (Some(i), Some(latest)) => installable.is_some() && latest > &i.current.version,
            _ => false,
        };
        Some(AppState { app, installed, latest, installable, update_available, known: list.is_some(), channel })
    }

    pub fn states(&self) -> Vec<AppState> {
        self.catalog().apps.iter().filter_map(|a| self.state(&a.id)).collect()
    }

    /// Apps with an update ready to install.
    pub fn updates(&self) -> Vec<AppState> {
        self.states().into_iter().filter(|s| s.update_available).collect()
    }

    /// Apps the policy requires that aren't installed yet.
    pub fn required_missing(&self) -> Vec<String> {
        self.inner.policy.required_apps.iter().filter(|id| self.installed_app(id).is_none()).cloned().collect()
    }

    /// Installed apps that aren't on the version the policy pins them to: (id, pinned version).
    pub fn off_pinned_version(&self) -> Vec<(String, Version)> {
        self.installed()
            .apps
            .values()
            .filter_map(|a| {
                self.inner.policy.pinned(&a.id).filter(|v| *v != a.current.version).map(|v| (a.id.clone(), v))
            })
            .collect()
    }

    // ---- IT and classrooms -----------------------------------------------------------------

    /// Fetch the centrally published policy (`policy_url`) and keep it for the next start.
    /// Returns the new policy when it changed.
    pub fn refresh_policy(&self) -> anyhow::Result<Option<Policy>> {
        let Some(url) = self.inner.policy.policy_url.clone() else { return Ok(None) };
        anyhow::ensure!(url.starts_with("https://"), "the policy address must start with https://");
        let mut resp = self.inner.agent.get(&url).call()?;
        check_status(&url, resp.status().as_u16())?;
        let text = resp.body_mut().read_to_string()?;
        let remote: Policy = serde_json::from_str(&text).with_context(|| format!("the policy at {url} isn't valid"))?;
        let file = remote_policy_file(&self.inner.paths);
        let old = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Policy>(&b).ok());
        write_atomic(&file, text.as_bytes())?;
        Ok((old.as_ref() != Some(&remote)).then_some(remote))
    }

    /// Whether this process may make changes the policy keeps from users (uninstalling, rolling
    /// back): only an administrator, when `prevent_uninstall` is set.
    fn guard_managed(&self, what: &str) -> anyhow::Result<()> {
        if self.inner.policy.prevent_uninstall && !crate::platform::is_elevated() {
            let help = self.inner.policy.support.as_deref().map(|s| format!(" (help: {s})")).unwrap_or_default();
            let org = self.inner.policy.organization.as_deref().unwrap_or("Your organization");
            anyhow::bail!("{org} manages the apps on this computer, so {what} is turned off{help}");
        }
        Ok(())
    }

    /// What's installed here, for IT: computer, CraftSpace, policy and every app's version and
    /// update state.
    pub fn report(&self) -> serde_json::Value {
        let policy = &self.inner.policy;
        let apps: Vec<serde_json::Value> = self
            .states()
            .into_iter()
            .filter_map(|s| {
                let i = s.installed.as_ref()?;
                Some(serde_json::json!({
                    "id": s.app.id,
                    "name": s.app.name,
                    "version": i.current.version.to_string(),
                    "kind": i.current.kind,
                    "latest": s.latest.as_ref().and_then(|r| r.version.as_ref()).map(|v| v.to_string()),
                    "update_available": s.update_available,
                    "pinned": policy.pinned(&s.app.id).map(|v| v.to_string()),
                    "found_on_computer": i.current.external.is_some(),
                    "location": i.current.executable,
                    "installed_at": i.current.installed_at,
                }))
            })
            .collect();
        serde_json::json!({
            "computer": sysinfo::System::host_name().unwrap_or_default(),
            "user": std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default(),
            "platform": self.inner.platform.display(),
            "os": sysinfo::System::long_os_version().unwrap_or_default(),
            "craftspace": crate::selfupdate::current_version().to_string(),
            "reported_at": github::now_secs(),
            "policy": {
                "source": policy.source,
                "organization": policy.organization,
                "required_missing": self.required_missing(),
                "off_pinned_version": self.off_pinned_version().into_iter().map(|(id, v)| (id, v.to_string())).collect::<BTreeMap<_, _>>(),
                "update_window": policy.update_window.as_ref().map(|w| w.describe()),
            },
            "apps": apps,
        })
    }

    /// Write the report into the policy's `report_dir` as `<computer name>.json`.
    pub fn write_report(&self) -> anyhow::Result<Option<PathBuf>> {
        let Some(dir) = self.inner.policy.report_dir.clone() else { return Ok(None) };
        std::fs::create_dir_all(&dir)?;
        let name = sysinfo::System::host_name().unwrap_or_else(|| "computer".into());
        let safe: String =
            name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let file = dir.join(format!("{safe}.json"));
        write_atomic(&file, serde_json::to_string_pretty(&self.report())?.as_bytes())?;
        Ok(Some(file))
    }

    // ---- workspace profiles ------------------------------------------------------------------

    fn profile_target(&self, id: &str) -> anyhow::Result<(&'static profiles::AppSpec, profiles::Context, AppEntry)> {
        let app = self.require_app(id)?;
        let spec =
            profiles::spec(&app.id).ok_or_else(|| anyhow::anyhow!("{}", profiles::unsupported_reason(&app.id)))?;
        let flatpak = self.installed_app(id).and_then(|a| a.current.external).and_then(|e| e.flatpak);
        let ctx = profiles::Context { programs: self.programs(id), flatpak, name: app.name.clone() };
        Ok((spec, ctx, app))
    }

    /// The parts of an app's setup a profile can carry, and anything to know about them.
    pub fn profile_parts(&self, id: &str) -> anyhow::Result<(Vec<profiles::Part>, Option<&'static str>)> {
        let (spec, _, _) = self.profile_target(id)?;
        Ok((spec.parts(), spec.note))
    }

    /// Save `parts` of an app's setup (layouts, shortcuts, preferences, presets) to a profile.
    pub fn export_profile(
        &self,
        id: &str,
        parts: &[profiles::Part],
        out: &Path,
    ) -> anyhow::Result<profiles::ExportReport> {
        let (spec, ctx, _) = self.profile_target(id)?;
        let version = self.installed_app(id).map(|a| a.current.version.to_string());
        profiles::export(spec, &ctx, parts, version, out)
    }

    /// Bring `parts` of a profile into the app on this computer. The current setup is saved
    /// first; the returned path is that backup (import it to go back).
    pub fn import_profile(
        &self,
        file: &Path,
        parts: &[profiles::Part],
    ) -> anyhow::Result<(profiles::ImportReport, Option<PathBuf>)> {
        let manifest = profiles::read_manifest(file)?;
        let (spec, ctx, app) = self.profile_target(&manifest.app)?;
        anyhow::ensure!(
            !self.is_running(&app.id),
            "{} is open; close it first (it saves its settings when it quits)",
            app.name
        );
        let backup =
            self.profile_backup_dir().join(format!("{}-{}.{}", app.id, github::now_secs(), profiles::EXTENSION));
        let backup = match profiles::export(spec, &ctx, &profiles::Part::ALL, None, &backup) {
            Ok(_) => Some(backup),
            Err(err) => {
                log::info!("nothing to back up for {}: {err:#}", app.name);
                None
            }
        };
        let report = profiles::import(spec, &ctx, file, parts)?;
        // Keep the last ten.
        for old in self.profile_backups(&app.id).into_iter().skip(10) {
            let _ = std::fs::remove_file(old);
        }
        Ok((report, backup))
    }

    pub fn profile_backup_dir(&self) -> PathBuf {
        self.inner.paths.root.join("profiles").join("backups")
    }

    /// Saved setups to go back to, newest first.
    pub fn profile_backups(&self, id: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.profile_backup_dir())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&format!("{id}-")) && n.ends_with(profiles::EXTENSION))
            })
            .collect();
        files.sort();
        files.reverse();
        files
    }

    /// Apply the workspace profiles the policy hands out: new or changed ones, and the
    /// `every-start` ones when `starting`. Apps that are open are left for next time.
    pub fn apply_policy_profiles(&self, starting: bool) -> Vec<(String, anyhow::Result<String>)> {
        let mut results = Vec::new();
        for (id, wanted) in self.inner.policy.profiles.clone() {
            let result = (|| -> anyhow::Result<Option<String>> {
                let app = self.require_app(&id)?;
                let file = self.fetch_policy_profile(&id, &wanted)?;
                let sha = download::sha256_file(&file)?;
                if let Some(expected) = &wanted.sha256 {
                    anyhow::ensure!(
                        sha.eq_ignore_ascii_case(expected),
                        "the profile for {} doesn't match its checksum",
                        app.name
                    );
                }
                let applied = self.installed().profiles_applied.get(&id) == Some(&sha);
                let due = !applied || (starting && wanted.apply == crate::policy::ProfileApply::EveryStart);
                if !due {
                    return Ok(None);
                }
                if self.is_running(&id) {
                    return Ok(Some(format!("{} is open; its profile is applied next time", app.name)));
                }
                let parts = if wanted.parts.is_empty() { profiles::Part::ALL.to_vec() } else { wanted.parts.clone() };
                let (report, _) = self.import_profile(&file, &parts)?;
                self.with_db(|db| db.profiles_applied.insert(id.clone(), sha))?;
                Ok(Some(format!(
                    "{}: applied the organization's profile ({} file{})",
                    app.name,
                    report.changed.len(),
                    if report.changed.len() == 1 { "" } else { "s" }
                )))
            })();
            match result {
                Ok(Some(text)) => results.push((id, Ok(text))),
                Ok(None) => {}
                Err(err) => results.push((id, Err(err))),
            }
        }
        results
    }

    fn fetch_policy_profile(&self, id: &str, wanted: &crate::policy::ProfilePolicy) -> anyhow::Result<PathBuf> {
        if !wanted.source.starts_with("https://") {
            let path = PathBuf::from(&wanted.source);
            anyhow::ensure!(path.is_file(), "the profile {} isn't there", path.display());
            return Ok(path);
        }
        let file = self.inner.paths.cache.join("profiles").join(format!("{id}.{}", profiles::EXTENSION));
        std::fs::create_dir_all(file.parent().expect("has a parent"))?;
        let cancel = AtomicBool::new(false);
        let progress = Progress { report: &|_| {}, cancel: &cancel };
        let _ = std::fs::remove_file(&file);
        download::download(&self.inner.agent, &wanted.source, &file, wanted.sha256.as_deref(), &progress)?;
        Ok(file)
    }

    /// Put an app's settings back to how they were on first start (between classes): its
    /// settings folders are renamed to `<folder>.reset-<time>`, so nothing is lost.
    pub fn reset_app(&self, id: &str) -> anyhow::Result<Vec<PathBuf>> {
        let app = self.require_app(id)?;
        anyhow::ensure!(!self.is_running(id), "{} is open; close it first", app.name);
        let programs = self.programs(id);
        let stamp = github::now_secs();
        let mut moved = Vec::new();
        for dir in crate::app_data::data_dirs(&app, &programs) {
            let target =
                dir.with_file_name(format!("{}.reset-{stamp}", dir.file_name().unwrap_or_default().to_string_lossy()));
            std::fs::rename(&dir, &target).with_context(|| format!("moving {} aside", dir.display()))?;
            moved.push(target);
        }
        Ok(moved)
    }

    /// Download the packages of `ids` (every app when empty) into `dir` (default: the package
    /// cache), for this computer or, with `all_platforms`, for every platform the apps support.
    /// Returns the files written.
    pub fn fill_cache(
        &self,
        ids: &[String],
        dir: Option<&Path>,
        all_platforms: bool,
        progress: &Progress,
    ) -> anyhow::Result<Vec<PathBuf>> {
        let dir = dir
            .map(Path::to_path_buf)
            .or_else(|| self.settings().package_cache)
            .ok_or_else(|| anyhow::anyhow!("no package cache is set (Settings › IT & Classroom, or --dir)"))?;
        std::fs::create_dir_all(&dir)?;
        let ids: Vec<String> =
            if ids.is_empty() { self.catalog().apps.into_iter().map(|a| a.id).collect() } else { ids.to_vec() };
        let mut written = Vec::new();
        for id in ids {
            let plan = match self.plan(&id, None) {
                Ok(p) => p,
                Err(err) => {
                    log::warn!("{id}: {err:#}");
                    continue;
                }
            };
            let mut release = plan.release.clone();
            let assets: Vec<String> = if all_platforms {
                release
                    .assets
                    .iter()
                    .map(|a| a.name.clone())
                    .filter(|n| crate::platform::kind_from_name(&n.to_ascii_lowercase()).is_some())
                    .collect()
            } else {
                vec![plan.asset.name.clone()]
            };
            for name in assets {
                let _ = self.github().fill_checksum(&mut release, &name);
                let Some(asset) = release.asset(&name).cloned() else { continue };
                let Some(sha) = asset.sha256.clone() else {
                    log::warn!("{name} has no checksum; not cached");
                    continue;
                };
                let target = dir.join(&name);
                if target.exists() && download::sha256_file(&target).is_ok_and(|s| s.eq_ignore_ascii_case(&sha)) {
                    continue;
                }
                download::download(&self.inner.agent, &asset.url, &target, Some(&sha), progress)?;
                written.push(target);
            }
        }
        Ok(written)
    }

    // ---- install ---------------------------------------------------------------------------

    /// Decide what to install for `id`: the latest release (or the pinned one), or `version`.
    pub fn plan(&self, id: &str, version: Option<&Version>) -> anyhow::Result<Plan> {
        let app = self.require_app(id)?;
        let settings = self.settings();
        let pinned = match settings.channel(id) {
            Channel::Pinned(v) => Some(v),
            _ => None,
        };
        let version = version.or(pinned.as_ref());
        let mut list = match self.releases(id) {
            Some(l) if version.is_none_or(|v| l.find(v).is_some()) => l,
            _ => self.refresh(id, version.is_some())?,
        };
        // An older version the list doesn't have (the API is rate limited, so only the latest is
        // known): look the release up by its tag.
        if let Some(v) = version.filter(|v| list.find(v).is_none()) {
            let github = self.github();
            if let Some(release) =
                [format!("v{v}"), v.to_string()].iter().find_map(|tag| github.release_by_tag(&app.repo, tag).ok())
            {
                list.releases.push(release);
                self.inner.releases.write().unwrap().insert(app.id.clone(), list.clone());
            }
        }
        let mut release = match version {
            Some(v) => list.find(v).cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "{} {v} not found (known versions: {})",
                    app.name,
                    list.releases
                        .iter()
                        .filter_map(|r| r.version.as_ref())
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
            None => {
                let installed = self.installed_app(id);
                list.latest(self.wants_prereleases(&settings, id, installed.as_ref()))
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("{} has no releases yet", app.name))?
            }
        };
        let installed = self.installed_app(id);
        if installed.as_ref().is_some_and(|i| i.current.external.as_ref().is_some_and(|e| e.flatpak.is_some())) {
            anyhow::bail!("{} is installed from Flatpak, which keeps it up to date (flatpak update)", app.name);
        }
        let prefs = self.prefs_for(&app, &settings, installed.as_ref());
        let (index, kind) =
            self.inner.platform.select_asset(&app.id, &release.asset_names(), prefs).ok_or_else(|| {
                anyhow::anyhow!("{} {} has no build for {}", app.name, release.tag, self.inner.platform.display())
            })?;
        if release.assets[index].sha256.is_none() {
            let name = release.assets[index].name.clone();
            if let Err(err) = self.github().fill_checksum(&mut release, &name) {
                log::warn!("could not fetch checksums for {} {}: {err:#}", app.name, release.tag);
            }
        }
        let asset = release.assets[index].clone();
        Ok(Plan { app, release, asset, kind, location: None })
    }

    fn mark_busy(&self, id: &str) -> anyhow::Result<BusyGuard<'_>> {
        let mut busy = self.inner.busy.lock().unwrap();
        anyhow::ensure!(busy.insert(id.to_string()), "{id} is already being changed");
        Ok(BusyGuard { set: &self.inner.busy, id: id.to_string() })
    }

    pub fn is_busy(&self, id: &str) -> bool {
        self.inner.busy.lock().unwrap().contains(id)
    }

    /// Download, verify and install `plan`, replacing (and optionally keeping) the current version.
    pub fn install(&self, plan: &Plan, progress: &Progress) -> anyhow::Result<InstalledApp> {
        let _busy = self.mark_busy(&plan.app.id)?;
        let settings = self.settings();
        let version = plan
            .release
            .version
            .clone()
            .ok_or_else(|| anyhow::anyhow!("release {} has no version", plan.release.tag))?;
        progress.stage(Stage::Resolving);
        if plan.asset.sha256.is_none() && !settings.allow_unverified_downloads {
            anyhow::bail!(
                "{} publishes no checksum for {}, so it can't be verified. Turn on \"Allow unverified downloads\" in Settings to install it anyway.",
                plan.app.name,
                plan.asset.name
            );
        }
        let previous = self.installed_app(&plan.app.id);

        // Download: a delta update when possible, else the package (reusing a verified earlier
        // download, resuming a partial one).
        let downloads = self.inner.paths.downloads();
        let archive_path = downloads.join(&plan.asset.name);
        let mut delta_downloaded = None;
        // A classroom's shared package cache: take the package from there when it's the right one.
        let cache = settings.package_cache.clone();
        if let (Some(cache), Some(expected)) = (&cache, &plan.asset.sha256) {
            let cached = cache.join(&plan.asset.name);
            if !archive_path.exists()
                && cached.is_file()
                && download::sha256_file(&cached).is_ok_and(|s| s.eq_ignore_ascii_case(expected))
            {
                log::info!("{}: from the package cache {}", plan.asset.name, cache.display());
                if let Some(parent) = archive_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let tmp = archive_path.with_extension("from-cache");
                if std::fs::copy(&cached, &tmp).is_ok() {
                    std::fs::rename(&tmp, &archive_path)?;
                }
            }
        }
        let reusable = match &plan.asset.sha256 {
            Some(expected) if archive_path.exists() => {
                download::sha256_file(&archive_path).is_ok_and(|s| s.eq_ignore_ascii_case(expected))
            }
            _ => false,
        };
        if !reusable {
            if let Some(stats) = self.try_delta(plan, previous.as_ref(), &archive_path, progress) {
                delta_downloaded = Some(stats.downloaded_bytes);
            }
        }
        let sha256 = if reusable || delta_downloaded.is_some() {
            let size = std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(1);
            (progress.report)(ProgressEvent::Bytes { done: size, total: Some(size) });
            plan.asset.sha256.clone().unwrap_or_else(|| download::sha256_file(&archive_path).unwrap_or_default())
        } else {
            download::download(
                &self.inner.agent,
                &plan.asset.url,
                &archive_path,
                plan.asset.sha256.as_deref(),
                progress,
            )?
        };
        if plan.asset.sha256.is_none() {
            log::warn!("{} has no published checksum; installed without verification", plan.asset.name);
        }
        // Share it with the other computers.
        if let Some(cache) = cache.filter(|_| settings.package_cache_write && plan.asset.sha256.is_some()) {
            let target = cache.join(&plan.asset.name);
            if !target.exists() {
                let tmp = cache.join(format!(".{}.part-{}", plan.asset.name, std::process::id()));
                let copied = std::fs::create_dir_all(&cache)
                    .and_then(|()| std::fs::copy(&archive_path, &tmp))
                    .and_then(|_| std::fs::rename(&tmp, &target));
                if let Err(err) = copied {
                    let _ = std::fs::remove_file(&tmp);
                    log::warn!("couldn't put {} into the package cache: {err}", plan.asset.name);
                }
            }
        }
        progress.check_cancelled()?;

        let mut new_version = if plan.kind.is_managed() {
            self.install_managed(plan, &version, &archive_path, &sha256, progress)?
        } else {
            self.install_system(plan, &version, &archive_path, &sha256, progress)?
        };
        new_version.delta_downloaded = delta_downloaded;
        // An app found on this computer stays where it was found for later updates too.
        if let Some(prev) = previous.as_ref().filter(|p| p.current.kind == plan.kind) {
            new_version.external.clone_from(&prev.current.external);
        }

        // macOS: move the old bundle out of ~/Applications before the new one goes in.
        let deactivated = previous
            .as_ref()
            .and_then(|p| integrate::deactivate(p.current.dir.as_deref(), p.current.executable.as_deref()));

        // Shortcuts and registrations for the new version, then drop the ones it no longer makes.
        progress.stage(Stage::Integrating);
        let mut integration = integrate::Integration::default();
        // macOS: an app found in /Applications is replaced there, not added to ~/Applications.
        let applications = previous
            .as_ref()
            .filter(|p| p.current.external.is_some() && p.current.kind == AssetKind::Dmg && plan.kind == AssetKind::Dmg)
            .and_then(|p| p.current.executable.as_ref().and_then(|b| b.parent()).map(Path::to_path_buf));
        if let Some(exe) = new_version.executable.clone().filter(|_| plan.kind.is_managed()) {
            let dir = new_version.dir.clone().unwrap_or_default();
            let req = integrate::Request {
                applications: applications.as_deref(),
                app: &plan.app,
                version: &version,
                dir: &dir,
                executable: &exe,
                desktop_shortcut: settings.desktop_shortcuts,
                uninstall_command: Some(self.uninstall_command(&plan.app.id)),
                size_bytes: new_version.size_bytes.unwrap_or(0),
            };
            match integrate::integrate(&req) {
                Ok(i) => integration = i,
                Err(err) => log::warn!("{} installed, but adding shortcuts failed: {err:#}", plan.app.name),
            }
        }
        if let Some(exe) = integration.executable.take() {
            new_version.executable = Some(exe);
        }
        if let Some(prev) = &previous {
            let stale: Vec<PathBuf> =
                prev.integration.iter().filter(|f| !integration.files.contains(f)).cloned().collect();
            let stale_keys: Vec<String> =
                prev.registry_keys.iter().filter(|k| !integration.registry_keys.contains(k)).cloned().collect();
            integrate::remove(&stale, &stale_keys);
        }

        // Record it, keeping the old version for rollback if asked.
        progress.stage(Stage::Cleaning);
        let id = plan.app.id.clone();
        let keep_previous = settings.keep_previous_version;
        let record = self.with_db(|db| {
            let old = db.apps.remove(&id);
            let mut to_remove = Vec::new();
            let mut kept = None;
            if let Some(mut old) = old {
                if let Some(p) = old.previous {
                    to_remove.push(p);
                }
                if let Some(exe) = deactivated {
                    old.current.executable = Some(exe);
                }
                if old.current.external.is_some() {
                    // Found on this computer: never deleted. A portable copy stays as the version
                    // to roll back to; others were replaced in place.
                    if old.current.dir.as_ref().is_some_and(|d| d.exists()) && old.current.dir != new_version.dir {
                        kept = Some(old.current);
                    }
                } else if old.current.dir != new_version.dir {
                    if keep_previous && old.current.kind.is_managed() && old.current.version != new_version.version {
                        kept = Some(old.current);
                    } else {
                        to_remove.push(old.current);
                    }
                }
            }
            for v in to_remove.into_iter().filter(|v| v.external.is_none()) {
                if let Some(dir) = v.dir.filter(|d| Some(d) != new_version.dir.as_ref()) {
                    db.remove_dir_or_defer(&dir);
                }
            }
            let record = InstalledApp {
                id: id.clone(),
                current: new_version,
                previous: kept,
                integration: integration.files,
                registry_keys: integration.registry_keys,
                last_launched: previous.as_ref().and_then(|p| p.last_launched),
            };
            db.apps.insert(id.clone(), record.clone());
            record
        })?;
        if plan.kind.is_managed() {
            let _ = std::fs::remove_file(&archive_path);
        }
        // Switching between a system package and CraftSpace's own copy: remove the other one.
        if let Some(prev) = previous.as_ref().filter(|p| p.current.kind != plan.kind && !p.current.kind.is_managed()) {
            if let Err(err) = self.uninstall_system(&plan.app.id, prev) {
                log::warn!("couldn't remove the previous {} package: {err:#}", plan.app.name);
            }
        }
        Ok(record)
    }

    /// Build the new AppImage from the installed one plus the changed blocks, when the release
    /// publishes a `.zsync` for it. Any failure falls back to a full download.
    fn try_delta(
        &self,
        plan: &Plan,
        previous: Option<&InstalledApp>,
        dest: &Path,
        progress: &Progress,
    ) -> Option<zsync::DeltaStats> {
        if plan.kind != AssetKind::AppImage {
            return None;
        }
        let prev = previous.filter(|p| p.current.kind == AssetKind::AppImage)?;
        let seed = prev.current.executable.as_ref().filter(|e| e.is_file())?;
        let zs = plan.release.asset(&format!("{}.zsync", plan.asset.name))?;
        match zsync::update_from_seed(&self.inner.agent, &zs.url, &plan.asset.url, seed, dest, progress) {
            Ok(stats) => {
                let ok = plan
                    .asset
                    .sha256
                    .as_ref()
                    .is_none_or(|exp| download::sha256_file(dest).is_ok_and(|s| s.eq_ignore_ascii_case(exp)));
                if ok {
                    log::info!(
                        "delta update for {}: downloaded {} and reused {}",
                        plan.app.name,
                        download::format_bytes(stats.downloaded_bytes),
                        download::format_bytes(stats.reused_bytes)
                    );
                    Some(stats)
                } else {
                    let _ = std::fs::remove_file(dest);
                    log::warn!(
                        "delta update for {} didn't match the release checksum; downloading in full",
                        plan.app.name
                    );
                    None
                }
            }
            Err(err) if download::is_cancelled(&err) => None,
            Err(err) if err.downcast_ref::<zsync::LowReuse>().is_some() => {
                log::info!("delta update for {}: {err}; downloading in full", plan.app.name);
                None
            }
            Err(err) => {
                log::warn!("delta update for {} failed ({err:#}); downloading in full", plan.app.name);
                None
            }
        }
    }

    fn install_managed(
        &self,
        plan: &Plan,
        version: &Version,
        archive_path: &Path,
        sha256: &str,
        progress: &Progress,
    ) -> anyhow::Result<InstalledVersion> {
        progress.stage(Stage::Installing);
        let app_dir = self.app_dir(plan);
        std::fs::create_dir_all(&app_dir)?;
        // Next to a copy found on this computer, the folder says which app it is.
        let beside_external = self
            .installed_app(&plan.app.id)
            .is_some_and(|a| a.current.external.as_ref().is_some_and(|e| e.home.is_some()));
        let name = if beside_external { format!("{}-{version}", plan.app.id) } else { version.to_string() };
        let dir = free_dir(&app_dir, &name);

        let result = (|| -> anyhow::Result<PathBuf> {
            match plan.kind {
                AssetKind::PortableZip | AssetKind::TarGz => archive::unpack(archive_path, &dir, progress)?,
                AssetKind::AppImage => {
                    std::fs::create_dir_all(&dir)?;
                    let target = dir.join(format!("{}.AppImage", plan.app.binary()));
                    std::fs::copy(archive_path, &target)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
                    }
                }
                AssetKind::Dmg => {
                    integrate::macos::install_dmg(archive_path, &dir)?;
                }
                other => anyhow::bail!("{other:?} is not a managed package"),
            }
            // Portable builds keep settings next to the exe while `portable.txt` is there;
            // without it they use the per-user folder, so settings survive updates.
            let marker = dir.join("portable.txt");
            if marker.exists() {
                let _ = std::fs::remove_file(marker);
            }
            find_executable(&dir, plan.app.binary(), self.inner.platform.os)
                .or_else(|| {
                    plan.app
                        .custom
                        .then(|| find_any_executable(&dir, plan.app.binary(), self.inner.platform.os))
                        .flatten()
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("installed {} but could not find its program in {}", plan.app.name, dir.display())
                })
        })();
        let executable = match result {
            Ok(e) => e,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(err);
            }
        };
        // For repairs: what every file should look like.
        let manifest_root = if plan.kind == AssetKind::Dmg { executable.clone() } else { dir.clone() };
        if let Err(err) = self.write_manifest(&plan.app.id, &dir, &manifest_root) {
            log::warn!("couldn't record {}'s files for repairs: {err:#}", plan.app.name);
        }
        Ok(InstalledVersion {
            version: version.clone(),
            tag: plan.release.tag.clone(),
            asset: plan.asset.name.clone(),
            kind: plan.kind,
            sha256: Some(sha256.to_string()),
            size_bytes: Some(dir_size(&dir)),
            dir: Some(dir),
            executable: Some(executable),
            package: None,
            installed_at: github::now_secs(),
            delta_downloaded: None,
            system_package: None,
            external: None,
        })
    }

    /// Hand an MSI or setup program to Windows and keep the package for uninstalling.
    fn install_system(
        &self,
        plan: &Plan,
        version: &Version,
        package: &Path,
        sha256: &str,
        progress: &Progress,
    ) -> anyhow::Result<InstalledVersion> {
        progress.stage(Stage::Installing);
        #[cfg(unix)]
        if matches!(plan.kind, AssetKind::Rpm | AssetKind::Deb) {
            use crate::integrate::linux_packages as pkg;
            let name = pkg::package_name(plan.kind, package)?;
            let downgrade = self.installed_app(&plan.app.id).is_some_and(|i| i.current.version > *version);
            pkg::install(plan.kind, package, downgrade, self.quiet())?;
            let _ = std::fs::remove_file(package);
            return Ok(InstalledVersion {
                version: version.clone(),
                tag: plan.release.tag.clone(),
                asset: plan.asset.name.clone(),
                kind: plan.kind,
                sha256: Some(sha256.to_string()),
                dir: None,
                executable: pkg::find_program(plan.kind, &name, plan.app.binary()),
                package: None,
                installed_at: github::now_secs(),
                size_bytes: plan.asset.size,
                delta_downloaded: None,
                system_package: Some(name),
                external: None,
            });
        }
        anyhow::ensure!(self.inner.platform.os == Os::Windows, "{:?} packages only install on Windows", plan.kind);
        let keep_dir = self.inner.paths.cache.join("packages").join(&plan.app.id);
        std::fs::create_dir_all(&keep_dir)?;
        let kept = keep_dir.join(&plan.asset.name);
        std::fs::rename(package, &kept).or_else(|_| std::fs::copy(package, &kept).map(|_| ()))?;
        let quiet = self.quiet();
        let status = match plan.kind {
            AssetKind::Msi => std::process::Command::new("msiexec")
                .arg("/i")
                .arg(&kept)
                .args([if quiet { "/qn" } else { "/passive" }, "/norestart"])
                .status()?,
            // NSIS (what Tauri apps like ArtCraft ship) installs silently, per user, with /S.
            AssetKind::Exe => std::process::Command::new(&kept).arg("/S").status()?,
            other => anyhow::bail!("{other:?} packages can't be installed here"),
        };
        // 3010: success, restart required.
        anyhow::ensure!(status.success() || status.code() == Some(3010), "the installer exited with {status}");
        Ok(InstalledVersion {
            version: version.clone(),
            tag: plan.release.tag.clone(),
            asset: plan.asset.name.clone(),
            kind: plan.kind,
            sha256: Some(sha256.to_string()),
            dir: None,
            executable: find_system_install(&plan.app),
            package: Some(kept),
            installed_at: github::now_secs(),
            size_bytes: plan.asset.size,
            delta_downloaded: None,
            system_package: None,
            external: None,
        })
    }

    /// Install or update to the latest release.
    pub fn install_latest(&self, id: &str, progress: &Progress) -> anyhow::Result<InstalledApp> {
        let plan = self.plan(id, None)?;
        self.install(&plan, progress)
    }

    /// Switch back to the version kept by the last update.
    pub fn rollback(&self, id: &str) -> anyhow::Result<InstalledApp> {
        self.guard_managed("going back to an earlier version")?;
        let _busy = self.mark_busy(id)?;
        let app = self.require_app(id)?;
        let installed = self.installed_app(id).ok_or_else(|| anyhow::anyhow!("{} is not installed", app.name))?;
        let previous =
            installed.previous.clone().ok_or_else(|| anyhow::anyhow!("no earlier version of {} is kept", app.name))?;
        let (Some(dir), Some(exe)) = (previous.dir.clone(), previous.executable.clone()) else {
            anyhow::bail!("the earlier version of {} can't be restored", app.name)
        };
        anyhow::ensure!(exe.exists(), "the earlier version's files are gone ({})", dir.display());
        let deactivated =
            integrate::deactivate(installed.current.dir.as_deref(), installed.current.executable.as_deref());
        let req = integrate::Request {
            app: &app,
            version: &previous.version,
            dir: &dir,
            executable: &exe,
            desktop_shortcut: self.settings().desktop_shortcuts,
            uninstall_command: Some(self.uninstall_command(id)),
            applications: None,
            size_bytes: previous.size_bytes.unwrap_or(0),
        };
        let mut integration = integrate::integrate(&req).unwrap_or_else(|err| {
            log::warn!("rolling back {}: adding shortcuts failed: {err:#}", app.name);
            integrate::Integration::default()
        });
        self.with_db(|db| {
            let entry = db.apps.get_mut(id).expect("installed");
            let mut restored = previous;
            if let Some(exe) = integration.executable.take() {
                restored.executable = Some(exe);
            }
            let mut current = std::mem::replace(&mut entry.current, restored);
            if let Some(exe) = deactivated {
                current.executable = Some(exe);
            }
            entry.previous = Some(current);
            entry.integration = integration.files;
            entry.registry_keys = integration.registry_keys;
            entry.clone()
        })
    }

    /// Remove an app, its shortcuts and every version CraftSpace kept.
    pub fn uninstall(&self, id: &str) -> anyhow::Result<()> {
        let _busy = self.mark_busy(id)?;
        let installed = self.installed_app(id).ok_or_else(|| anyhow::anyhow!("{id} is not installed"))?;
        self.guard_managed("uninstalling")?;
        let app_dir = installed.current.dir.as_ref().and_then(|d| d.parent()).map(Path::to_path_buf);
        if let Some(external) = &installed.current.external {
            self.uninstall_external(&installed, external)?;
        }
        if !installed.current.kind.is_managed() {
            self.uninstall_system(id, &installed)?;
        }
        integrate::remove(&installed.integration, &installed.registry_keys);
        self.with_db(|db| {
            if let Some(app) = db.apps.remove(id) {
                for v in std::iter::once(app.current).chain(app.previous) {
                    if let Some(dir) = v.dir {
                        db.remove_dir_or_defer(&dir);
                    }
                    if let Some(pkg) = v.package {
                        let _ = std::fs::remove_file(pkg);
                    }
                }
            }
            // The app's folder, only if empty.
            for dir in app_dir.iter().cloned().chain([self.apps_root().join(id)]) {
                let _ = std::fs::remove_dir(&dir);
            }
        })?;
        let _ = std::fs::remove_dir_all(self.inner.paths.root.join("manifests").join(id));
        Ok(())
    }

    #[cfg(windows)]
    fn uninstall_system(&self, id: &str, installed: &InstalledApp) -> anyhow::Result<()> {
        let name = self.app(id).map(|a| a.name).unwrap_or_else(|| id.to_string());
        if let Some(entry) = integrate::windows::find_uninstall_entry(&name) {
            return integrate::windows::run_uninstaller(&entry, self.quiet());
        }
        if let Some(package) = installed.current.package.as_ref().filter(|_| installed.current.kind == AssetKind::Msi) {
            let status = std::process::Command::new("msiexec")
                .arg("/x")
                .arg(package)
                .args([if self.quiet() { "/qn" } else { "/passive" }, "/norestart"])
                .status()?;
            anyhow::ensure!(
                status.success() || status.code() == Some(3010),
                "the Windows Installer exited with {status}"
            );
            return Ok(());
        }
        log::warn!("{name} has no uninstaller registered; forgetting it");
        Ok(())
    }

    #[cfg(not(windows))]
    fn uninstall_system(&self, _id: &str, installed: &InstalledApp) -> anyhow::Result<()> {
        #[cfg(unix)]
        if let Some(name) = &installed.current.system_package {
            use crate::integrate::linux_packages as pkg;
            if pkg::is_installed(installed.current.kind, name) {
                pkg::remove(installed.current.kind, name, self.quiet())?;
            }
        }
        let _ = installed;
        Ok(())
    }

    /// Find ArtCraft apps installed without CraftSpace and record them, so they show as installed
    /// and update where they are. Adopted apps that are gone since are forgotten. Returns what was
    /// newly found, as (name, version).
    pub fn adopt_installed(&self) -> anyhow::Result<Vec<(String, Option<Version>)>> {
        if !self.settings().detect_installed {
            return Ok(Vec::new());
        }
        let own = vec![
            self.inner.paths.root.clone(),
            self.inner.paths.apps.clone(),
            self.apps_root(),
            crate::selfupdate::install_dir(self),
        ];
        let db = self.installed();
        let found = crate::detect::find(&self.catalog(), &|id| db.apps.contains_key(id), &own);
        let gone: Vec<String> = db
            .apps
            .values()
            .filter(|a| {
                a.current.external.as_ref().is_some_and(|e| e.flatpak.is_none())
                    && a.current.executable.as_ref().is_none_or(|e| !e.exists())
            })
            .map(|a| a.id.clone())
            .collect();
        if found.is_empty() && gone.is_empty() {
            return Ok(Vec::new());
        }
        let catalog = self.catalog();
        self.with_db(|db| {
            for id in &gone {
                log::info!("{id} isn't where it was found anymore; forgetting it");
                db.apps.remove(id);
            }
            let mut adopted = Vec::new();
            for f in found {
                if db.apps.contains_key(&f.app_id) {
                    continue;
                }
                let name = catalog.app(&f.app_id).map(|a| a.name.clone()).unwrap_or_else(|| f.app_id.clone());
                adopted.push((name, f.version.clone()));
                let version = f.version.clone().unwrap_or_else(|| Version::new(0, 0, 0));
                db.apps.insert(
                    f.app_id.clone(),
                    InstalledApp {
                        id: f.app_id.clone(),
                        current: InstalledVersion {
                            tag: format!("v{version}"),
                            version,
                            asset: String::new(),
                            kind: f.kind,
                            sha256: None,
                            dir: f.dir,
                            executable: Some(f.executable),
                            package: None,
                            installed_at: github::now_secs(),
                            size_bytes: None,
                            delta_downloaded: None,
                            system_package: f.system_package,
                            external: Some(f.external),
                        },
                        previous: None,
                        integration: Vec::new(),
                        registry_keys: Vec::new(),
                        last_launched: None,
                    },
                );
            }
            adopted
        })
    }

    /// Remove an app CraftSpace adopted: a Flatpak through flatpak, a bundle or AppImage by
    /// deleting it. (Installers and packages go through [`Self::uninstall_system`]; portable
    /// folders are removed with the other version folders.)
    fn uninstall_external(&self, installed: &InstalledApp, external: &crate::detect::External) -> anyhow::Result<()> {
        if let Some(id) = &external.flatpak {
            let status =
                std::process::Command::new("flatpak").args(["uninstall", "--noninteractive", "-y", id]).status()?;
            anyhow::ensure!(status.success(), "flatpak uninstall {id} failed ({status})");
            return Ok(());
        }
        let current = &installed.current;
        if current.kind == AssetKind::Dmg || (current.kind == AssetKind::AppImage && current.dir.is_none()) {
            if let Some(exe) = current.executable.as_ref().filter(|e| e.exists()) {
                integrate::remove(std::slice::from_ref(exe), &[]);
            }
        }
        Ok(())
    }

    /// Record CraftSpace's own installation (see [`crate::selfupdate::self_install`]).
    pub fn record_self_install(&self, record: InstalledApp) -> anyhow::Result<()> {
        self.with_db(|db| {
            db.apps.insert(record.id.clone(), record);
        })
    }

    /// The command Windows' app list runs to uninstall `id`.
    pub fn uninstall_command(&self, id: &str) -> Vec<String> {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("craftspace"));
        let cli = exe.with_file_name(if cfg!(windows) { "craftspace-cli.exe" } else { "craftspace-cli" });
        let program = if cli.exists() { cli } else { exe };
        vec![program.to_string_lossy().into_owned(), "uninstall".into(), id.into()]
    }

    // ---- repair ----------------------------------------------------------------------------

    fn manifest_path(&self, id: &str, version_dir: &Path) -> PathBuf {
        let name = version_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.inner.paths.root.join("manifests").join(id).join(format!("{name}.json"))
    }

    fn write_manifest(&self, id: &str, version_dir: &Path, root: &Path) -> anyhow::Result<()> {
        let mut files = BTreeMap::new();
        for entry in walkdir::WalkDir::new(root).into_iter().filter_map(Result::ok) {
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(root)?.to_string_lossy().replace('\\', "/");
                files.insert(rel, download::sha256_file(entry.path())?);
            }
        }
        let manifest = Manifest { root: root.to_path_buf(), files };
        write_atomic(&self.manifest_path(id, version_dir), &serde_json::to_vec(&manifest)?)?;
        Ok(())
    }

    /// Check the installed files against what was installed.
    pub fn verify(&self, id: &str) -> anyhow::Result<VerifyReport> {
        let installed = self.installed_app(id).ok_or_else(|| anyhow::anyhow!("{id} is not installed"))?;
        let current = &installed.current;
        #[cfg(unix)]
        if let Some(name) = &current.system_package {
            let (missing, changed) = crate::integrate::linux_packages::verify(current.kind, name)?;
            return Ok(VerifyReport { checked: 1, missing, changed });
        }
        let Some(dir) = current.dir.as_ref().filter(|_| current.kind.is_managed() && current.external.is_none()) else {
            // System installs: all we can tell is whether the program is still there.
            let exe = current.executable.clone().or_else(|| self.executable(id));
            let missing: Vec<PathBuf> = exe.iter().filter(|e| !e.exists()).cloned().collect();
            return Ok(VerifyReport { checked: exe.iter().count(), missing, changed: Vec::new() });
        };
        let manifest: Manifest = serde_json::from_slice(
            &std::fs::read(self.manifest_path(id, dir))
                .context("no record of the installed files; reinstall to repair")?,
        )?;
        // macOS bundles move into ~/Applications after the manifest is written.
        let root = if current.kind == AssetKind::Dmg {
            current.executable.clone().unwrap_or(manifest.root.clone())
        } else {
            dir.clone()
        };
        let mut report = VerifyReport::default();
        for (rel, sha) in &manifest.files {
            let path = root.join(rel);
            report.checked += 1;
            match download::sha256_file(&path) {
                Ok(actual) if actual == *sha => {}
                Ok(_) => report.changed.push(path),
                Err(_) => report.missing.push(path),
            }
        }
        Ok(report)
    }

    /// Reinstall the current version from a fresh download.
    pub fn repair(&self, id: &str, progress: &Progress) -> anyhow::Result<InstalledApp> {
        let installed = self.installed_app(id).ok_or_else(|| anyhow::anyhow!("{id} is not installed"))?;
        let plan = self.plan(id, Some(&installed.current.version))?;
        self.install(&plan, progress)
    }

    // ---- running apps and launching -----------------------------------------------------

    /// Folders whose programs belong to `id`.
    fn program_roots(&self, id: &str) -> Vec<PathBuf> {
        let Some(installed) = self.installed_app(id) else { return Vec::new() };
        let mut roots = Vec::new();
        for v in std::iter::once(&installed.current).chain(installed.previous.as_ref()) {
            if let Some(dir) = &v.dir {
                roots.push(dir.clone());
            }
            if let Some(exe) = &v.executable {
                // A bundle (macOS) is a folder; a program's folder otherwise.
                if exe.is_dir() || v.system_package.is_some() {
                    // A bundle (macOS) or a program in a shared folder like /usr/bin.
                    roots.push(exe.clone());
                } else if v.dir.is_none() {
                    if let Some(parent) = exe.parent().filter(|p| !p.as_os_str().is_empty()) {
                        roots.push(parent.to_path_buf());
                    }
                }
            }
        }
        roots
    }

    /// Whether the app is open right now.
    pub fn is_running(&self, id: &str) -> bool {
        // A Flatpak runs inside its sandbox; ask Flatpak.
        if let Some(flatpak) = self.installed_app(id).and_then(|a| a.current.external).and_then(|e| e.flatpak) {
            return std::process::Command::new("flatpak")
                .args(["ps", "--columns=application"])
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == flatpak));
        }
        crate::running::any_running_under(&self.program_roots(id))
    }

    pub fn executable(&self, id: &str) -> Option<PathBuf> {
        let installed = self.installed_app(id)?;
        installed.current.executable.clone().or_else(|| self.app(id).and_then(|a| find_system_install(&a)))
    }

    // ---- files -----------------------------------------------------------------------------

    fn files_cache(&self) -> PathBuf {
        self.inner.paths.cache.join("files.json")
    }

    /// The last Files scan, for showing the tab before a new scan finishes.
    pub fn cached_files(&self) -> Option<Vec<crate::files::FileEntry>> {
        crate::files::load_cache(&self.files_cache())
    }

    /// Scan the Files folders (and, if turned on, the apps' recent lists) and cache the result.
    pub fn scan_files(&self, cancel: &std::sync::atomic::AtomicBool) -> Vec<crate::files::FileEntry> {
        let settings = self.settings();
        let catalog = self.catalog();
        let mut files =
            crate::files::scan(&settings.file_locations, &catalog, &crate::files::ScanOptions::default(), cancel);
        if settings.files.app_recents {
            crate::files::merge_recents(&mut files, self.app_recents(), &catalog);
        }
        if !cancel.load(std::sync::atomic::Ordering::Relaxed) {
            if let Err(err) = crate::files::save_cache(&self.files_cache(), &files) {
                log::warn!("couldn't cache the file list: {err:#}");
            }
        }
        files
    }

    /// The programs of an installed app (current and kept versions), whose folders may hold
    /// portable settings.
    fn programs(&self, id: &str) -> Vec<PathBuf> {
        self.installed_app(id)
            .map(|a| std::iter::once(a.current).chain(a.previous).filter_map(|v| v.executable).collect())
            .unwrap_or_default()
    }

    /// Every app's recent files that still exist.
    pub fn app_recents(&self) -> Vec<crate::app_data::RecentFile> {
        let times_path = self.inner.paths.cache.join("recent-opened.json");
        let mut times = crate::app_data::OpenedTimes::load(&times_path);
        let mut out = Vec::new();
        for app in &self.catalog().apps {
            out.extend(crate::app_data::recent_files(app, &self.programs(&app.id), &mut times));
        }
        if let Err(err) = times.save(&times_path) {
            log::debug!("couldn't save recent-file times: {err:#}");
        }
        out
    }

    /// Settings left by portable copies of installed apps, in the usual places (Downloads,
    /// Desktop, Documents, the Files folders, the home folder and CraftSpace's app folder).
    pub fn find_portable_data(&self) -> Vec<crate::app_data::PortableData> {
        let settings = self.settings();
        let mut roots: Vec<PathBuf> = settings.file_locations.clone();
        if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
            roots.push(home);
        }
        roots.push(self.apps_root());
        roots.push(self.inner.paths.apps.clone());
        roots.sort();
        roots.dedup();
        let db = self.installed();
        crate::app_data::find_portable_data(&self.catalog(), &roots, &settings.files.portable_ignored)
            .into_iter()
            .filter(|p| db.apps.contains_key(&p.app_id))
            .collect()
    }

    /// Move a portable copy's settings into the installed app's settings folder.
    pub fn import_portable(
        &self,
        found: &crate::app_data::PortableData,
    ) -> anyhow::Result<crate::app_data::ImportReport> {
        let app = self.require_app(&found.app_id)?;
        let dest =
            crate::app_data::config_dir(&app).ok_or_else(|| anyhow::anyhow!("no settings folder for {}", app.name))?;
        let report = crate::app_data::import_portable(&found.dir, &dest)?;
        self.forget_portable(&found.dir)?;
        Ok(report)
    }

    /// Don't offer this portable folder again.
    pub fn forget_portable(&self, dir: &Path) -> anyhow::Result<()> {
        let mut s = self.settings();
        if !s.files.portable_ignored.iter().any(|d| d == dir) {
            s.files.portable_ignored.push(dir.to_path_buf());
            self.set_settings(s)?;
        }
        Ok(())
    }

    /// `craftspace open <file>`: open it in the installed app that handles it. `Ok(Some(id))`
    /// when the app that opens it isn't installed yet.
    pub fn open_file(&self, path: &Path) -> anyhow::Result<Option<String>> {
        let db = self.installed();
        let (id, installed) = crate::file_types::choose_app(&self.catalog(), path, |id| db.apps.contains_key(id))
            .ok_or_else(|| {
                anyhow::anyhow!("no ArtCraft app opens {}", path.file_name().unwrap_or_default().to_string_lossy())
            })?;
        if !installed {
            return Ok(Some(id));
        }
        self.launch(&id, &[path.to_path_buf()])?;
        Ok(None)
    }

    /// Register (or, with an empty list, unregister) CraftSpace as the opener for `extensions`.
    pub fn register_file_types(&self, extensions: &[String]) -> anyhow::Result<crate::file_types::Registered> {
        let exe = crate::file_types::program(Some(crate::selfupdate::installed_exe(self)))?;
        crate::file_types::register(&exe, extensions)
    }

    /// Start an installed app, optionally opening `files`.
    pub fn launch(&self, id: &str, files: &[PathBuf]) -> anyhow::Result<()> {
        let app = self.require_app(id)?;
        let exe = self
            .executable(id)
            .ok_or_else(|| anyhow::anyhow!("{} is not installed (or its program could not be found)", app.name))?;
        if let Some(flatpak) = self.installed_app(id).and_then(|a| a.current.external).and_then(|e| e.flatpak) {
            std::process::Command::new("flatpak").arg("run").arg(&flatpak).args(files).spawn()?;
            return Ok(());
        }
        anyhow::ensure!(exe.exists(), "{} is missing; repair or reinstall {}", exe.display(), app.name);
        let mut cmd = if exe.is_dir() {
            integrate::macos::launch_command(&exe, files)
        } else {
            let mut cmd = std::process::Command::new(&exe);
            cmd.args(files);
            if let Some(dir) = exe.parent() {
                cmd.current_dir(dir);
            }
            cmd
        };
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().with_context(|| format!("starting {}", exe.display()))?;
        // Reap the child when it exits so it doesn't linger as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let _ = self.with_db(|db| {
            if let Some(app) = db.apps.get_mut(id) {
                app.last_launched = Some(github::now_secs());
            }
        });
        Ok(())
    }

    /// Delete leftover downloads and folders that could not be removed earlier.
    pub fn cleanup(&self) -> anyhow::Result<u64> {
        let mut freed = 0;
        if let Ok(rd) = std::fs::read_dir(self.inner.paths.downloads()) {
            for entry in rd.filter_map(Result::ok) {
                freed += entry.metadata().map(|m| m.len()).unwrap_or(0);
                let path = entry.path();
                let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
            }
        }
        self.with_db(|db| db.retry_pending_removal())?;
        Ok(freed)
    }

    // ---- app lists -------------------------------------------------------------------------

    /// The installed apps, their versions and channels, for `import_list` elsewhere.
    pub fn export_list(&self) -> AppList {
        let settings = self.settings();
        let db = self.installed();
        let apps = self
            .catalog()
            .apps
            .iter()
            .filter_map(|a| db.get(&a.id))
            .map(|i| AppListEntry {
                id: i.id.clone(),
                version: Some(i.current.version.clone()),
                channel: Some(settings.channel(&i.id)).filter(|c| *c != Channel::Default),
            })
            .collect();
        AppList { craftspace: env!("CARGO_PKG_VERSION").into(), apps }
    }

    /// Adopt the channels in `list`, and return what to install: `(app, version)`, where the
    /// version is `None` for "latest" unless `exact_versions`.
    pub fn import_list(&self, list: &AppList, exact_versions: bool) -> anyhow::Result<Vec<(String, Option<Version>)>> {
        let mut settings = self.inner.settings.read().unwrap().clone();
        let mut todo = Vec::new();
        for entry in &list.apps {
            let Some(app) = self.app(&entry.id) else {
                log::warn!("skipping unknown app {}", entry.id);
                continue;
            };
            if let Some(channel) = &entry.channel {
                settings.channels.insert(app.id.clone(), channel.clone());
            }
            let version = if exact_versions { entry.version.clone() } else { None };
            let installed = self.installed_app(&app.id).map(|i| i.current.version);
            let wanted_installed = match (&version, &installed) {
                (Some(v), Some(i)) => v == i,
                (None, Some(_)) => true,
                _ => false,
            };
            if !wanted_installed {
                todo.push((app.id.clone(), version));
            }
        }
        self.set_settings(settings)?;
        Ok(todo)
    }

    // ---- icons, news, READMEs ----------------------------------------------------------------

    /// The app's icon, from the cache (call [`Manager::fetch_icons`] to fill it).
    pub fn icon(&self, id: &str) -> Option<Vec<u8>> {
        crate::icons::cached(&self.inner.paths, id)
    }

    /// Download icons that aren't cached (or are old). Returns the apps whose icon changed.
    pub fn fetch_icons(&self) -> Vec<String> {
        let apps = self.catalog().apps;
        std::thread::scope(|scope| {
            let handles: Vec<_> = apps
                .iter()
                .map(|app| {
                    scope.spawn(move || {
                        let before = self.icon(&app.id);
                        match crate::icons::fetch(&self.inner.agent, &self.inner.paths, app) {
                            Ok(Some(bytes)) if before.as_ref() != Some(&bytes) => Some(app.id.clone()),
                            Ok(_) => None,
                            Err(err) => {
                                log::info!("no icon for {}: {err:#}", app.id);
                                None
                            }
                        }
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
        })
    }

    pub fn articles(&self, force: bool) -> anyhow::Result<Vec<Article>> {
        let site = self.catalog().news_site.ok_or_else(|| anyhow::anyhow!("no news site configured"))?;
        news::fetch(&self.inner.agent, &site, &self.inner.paths.cache.join("news.json"), force)
    }

    pub fn cached_articles(&self) -> Vec<Article> {
        news::cached(&self.inner.paths.cache.join("news.json")).unwrap_or_default()
    }

    pub fn readme(&self, id: &str) -> anyhow::Result<String> {
        let app = self.require_app(id)?;
        news::fetch_readme(
            &self.inner.agent,
            &app.repo,
            &self.inner.paths.cache.join("readmes").join(format!("{id}.md")),
        )
    }

    /// Screenshots, description and feature list for the app's page.
    pub fn tour(&self, id: &str, force: bool) -> anyhow::Result<crate::tour::Tour> {
        let app = self.require_app(id)?;
        crate::tour::fetch(&self.inner.agent, &app, &self.inner.paths.cache.join("tours"), force)
    }

    /// A tour screenshot on disk (downloaded once).
    pub fn tour_image(&self, url: &str) -> anyhow::Result<PathBuf> {
        crate::tour::image(&self.inner.agent, url, &self.inner.paths.cache.join("screenshots"))
    }

    /// A "new issue" link for the app, pre-filled with versions and platform.
    pub fn bug_report_url(&self, id: &str) -> Option<String> {
        let app = self.app(id)?;
        let installed = self.installed_app(id);
        Some(news::issue_url(
            &app.repo,
            &app.name,
            installed.as_ref().map(|i| i.current.version.to_string()).as_deref(),
            installed.as_ref().map(|i| i.current.asset.as_str()),
            &self.inner.platform.display(),
        ))
    }

    // ---- fonts and add-ons ---------------------------------------------------------------

    pub fn addon(&self, id: &str) -> Option<Addon> {
        self.catalog().addons.into_iter().find(|a| a.id == id)
    }

    /// The fonts an add-on offers.
    pub fn font_list(&self, addon_id: &str) -> anyhow::Result<Vec<FontFile>> {
        let addon = self.addon(addon_id).ok_or_else(|| anyhow::anyhow!("unknown add-on {addon_id}"))?;
        anyhow::ensure!(addon.kind == AddonKind::Fonts, "{} isn't a font add-on", addon.name);
        let cache = self.inner.paths.cache.join(format!("fonts-{addon_id}.json"));
        match fonts::fetch_manifest(&self.inner.agent, &addon) {
            Ok(list) => {
                let _ = write_atomic(&cache, &serde_json::to_vec(&list)?);
                Ok(list)
            }
            Err(err) => std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or(err),
        }
    }

    pub fn installed_fonts(&self) -> BTreeMap<String, InstalledFont> {
        self.installed().fonts
    }

    /// Install every font of `family` (or all of them) from a font add-on.
    pub fn install_fonts(&self, addon_id: &str, family: Option<&str>, progress: &Progress) -> anyhow::Result<usize> {
        let _busy = self.mark_busy(&format!("addon:{addon_id}"))?;
        let addon = self.addon(addon_id).ok_or_else(|| anyhow::anyhow!("unknown add-on {addon_id}"))?;
        let wanted: Vec<FontFile> =
            self.font_list(addon_id)?.into_iter().filter(|f| family.is_none_or(|fam| f.family == fam)).collect();
        anyhow::ensure!(!wanted.is_empty(), "no fonts called {}", family.unwrap_or("that"));
        let staging = self.inner.paths.downloads();
        let mut count = 0;
        for font in &wanted {
            progress.check_cancelled()?;
            let installed = fonts::install_font(&self.inner.agent, &addon, font, &staging, progress)?;
            self.with_db(|db| db.fonts.insert(font.file_name().to_string(), installed))?;
            count += 1;
        }
        fonts::refresh_font_cache();
        Ok(count)
    }

    /// Remove installed fonts of `family` (or all CraftSpace installed).
    pub fn uninstall_fonts(&self, family: Option<&str>) -> anyhow::Result<usize> {
        let removed = self.with_db(|db| {
            let names: Vec<String> = db
                .fonts
                .iter()
                .filter(|(_, f)| family.is_none_or(|fam| f.family == fam))
                .map(|(k, _)| k.clone())
                .collect();
            names.into_iter().filter_map(|n| db.fonts.remove(&n)).collect::<Vec<_>>()
        })?;
        for font in &removed {
            fonts::uninstall_font(font);
        }
        fonts::refresh_font_cache();
        Ok(removed.len())
    }

    // ---- add-ons (registry and stores) ---------------------------------------------------

    fn addons_cache(&self) -> PathBuf {
        self.inner.paths.cache.join("addons")
    }

    /// The CraftSpace add-on registry: the copy last fetched, else the one this build ships.
    pub fn addon_registry(&self) -> addons::Registry {
        std::fs::read_to_string(self.addons_cache().join("registry.json"))
            .ok()
            .and_then(|t| addons::parse_registry(&t, "CraftSpace").ok())
            .unwrap_or_else(addons::Registry::builtin)
    }

    /// Every add-on store, with whether it's turned on.
    pub fn addon_stores(&self) -> Vec<(addons::Store, bool)> {
        let settings = self.settings().addon_stores;
        let mut stores: Vec<(addons::Store, bool)> = self
            .addon_registry()
            .stores
            .into_iter()
            .map(|s| {
                let on = !settings.disabled.contains(&s.id);
                (s, on)
            })
            .collect();
        for s in settings.custom {
            if !stores.iter().any(|(x, _)| x.id == s.id) {
                let on = !settings.disabled.contains(&s.id);
                stores.push((s, on));
            }
        }
        stores
    }

    /// Add-ons from the registry and the stores that are on (as last fetched).
    pub fn available_addons(&self) -> Vec<addons::Addon> {
        let mut list = self.addon_registry().addons;
        for (store, on) in self.addon_stores() {
            if !on {
                continue;
            }
            let file = self.addons_cache().join(format!("store-{}.json", store.id));
            if let Some(found) = std::fs::read_to_string(file).ok().and_then(|t| addons::parse_store(&store, &t).ok()) {
                list.extend(found);
            }
        }
        list
    }

    /// Fetch the registry and the stores that are on. Returns the stores that failed.
    pub fn refresh_addons(&self) -> Vec<(String, anyhow::Error)> {
        let mut errors = Vec::new();
        let dir = self.addons_cache();
        let _ = std::fs::create_dir_all(&dir);
        let fetch = |url: &str| -> anyhow::Result<String> {
            let mut resp = self.inner.agent.get(url).call()?;
            check_status(url, resp.status().as_u16())?;
            Ok(resp.body_mut().with_config().limit(16 << 20).read_to_string()?)
        };
        if self.settings().remote_catalog {
            match fetch(addons::REGISTRY_URL).and_then(|t| addons::parse_registry(&t, "CraftSpace").map(|_| t)) {
                Ok(text) => {
                    if let Err(err) = write_atomic(&dir.join("registry.json"), text.as_bytes()) {
                        errors.push(("CraftSpace".to_string(), err.into()));
                    }
                }
                Err(err) => errors.push(("CraftSpace".to_string(), err)),
            }
        }
        for (store, on) in self.addon_stores() {
            if !on {
                continue;
            }
            let result = fetch(&store.url)
                .and_then(|t| addons::parse_store(&store, &t).map(|_| t))
                .and_then(|t| Ok(write_atomic(&dir.join(format!("store-{}.json", store.id)), t.as_bytes())?));
            if let Err(err) = result {
                errors.push((store.name.clone(), err));
            }
        }
        errors
    }

    pub fn installed_addons(&self) -> BTreeMap<String, addons::Installed> {
        self.installed().addons
    }

    /// `Documents/CraftSpace Add-ons`: content the apps import themselves.
    pub fn addon_library(&self) -> PathBuf {
        let docs = directories::UserDirs::new()
            .and_then(|u| u.document_dir().map(Path::to_path_buf))
            .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join("Documents")))
            .unwrap_or_else(|| self.inner.paths.root.join("Documents"));
        docs.join("CraftSpace Add-ons")
    }

    /// Install an add-on. Add-ons not checked by CraftSpace need `accept_unchecked` (the person
    /// agreed); they still have to match their checksum when the listing has one.
    pub fn install_addon(
        &self,
        id: &str,
        accept_unchecked: bool,
        progress: &Progress,
    ) -> anyhow::Result<addons::Report> {
        let _busy = self.mark_busy(&format!("addon:{id}"))?;
        let addon = self
            .available_addons()
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown add-on {id} (see `craftspace-cli addons list`)"))?;
        if !addon.checked() {
            anyhow::ensure!(
                !self.inner.policy.block_unchecked_addons,
                "{} allows only add-ons checked by CraftSpace on this computer",
                self.inner.policy.organization.as_deref().unwrap_or("Your organization")
            );
            anyhow::ensure!(
                accept_unchecked,
                "{} isn't checked by CraftSpace; agree to install it anyway (--yes)",
                addon.name
            );
        }
        let platform = self.platform();
        let file = addon
            .file_for(platform)
            .ok_or_else(|| anyhow::anyhow!("{} has no download for {}", addon.name, platform.display()))?
            .clone();
        anyhow::ensure!(file.url.starts_with("https://"), "{} isn't downloaded over https", addon.name);
        anyhow::ensure!(file.sha256.is_some() || !addon.checked(), "{} has no checksum in the registry", addon.name);

        // Download, and unpack archives.
        let staging = self.inner.paths.downloads().join(format!("addon-{}", addons::slug(&addon.id)));
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;
        let name =
            file.url.rsplit('/').next().unwrap_or("download").split('?').next().unwrap_or("download").to_string();
        let download = staging.join(&name);
        download::download(&self.inner.agent, &file.url, &download, file.sha256.as_deref(), progress)?;
        progress.stage(Stage::Installing);
        let lower = name.to_ascii_lowercase();
        let content = if [".zip", ".tar.gz", ".tgz", ".tar.xz", ".txz"].iter().any(|e| lower.ends_with(e)) {
            let dir = staging.join("content");
            archive::unpack(&download, &dir, progress)?;
            dir
        } else {
            let dir = staging.join("content");
            std::fs::create_dir_all(&dir)?;
            std::fs::rename(&download, dir.join(&name))?;
            dir
        };

        let mut report = addons::Report::default();
        let result = self.place_addon(&addon, &content, &mut report);
        let _ = std::fs::remove_dir_all(&staging);
        if let Err(err) = result {
            for path in &report.written {
                remove_any(path);
            }
            return Err(err);
        }
        anyhow::ensure!(!report.written.is_empty(), "{} has nothing for this computer", addon.name);
        let installed = addons::Installed {
            version: addon.version.clone(),
            source: addon.source.clone(),
            paths: report.written.clone(),
            installed_at: github::now_secs(),
        };
        self.with_db(|db| db.addons.insert(addon.id.clone(), installed))?;
        Ok(report)
    }

    fn place_addon(&self, addon: &addons::Addon, content: &Path, report: &mut addons::Report) -> anyhow::Result<()> {
        let os = self.platform().os;
        for step in &addon.install {
            let app = self.require_app(&step.app)?;
            let mut written = Vec::new();
            match step.to.as_str() {
                "clap" | "vst3" | "au" => {
                    let Some(dir) = addons::audio_plugin_dir(&step.to, os) else { continue };
                    let ext = if step.to == "au" { "component" } else { step.to.as_str() };
                    for bundle in addons::bundles(content, ext) {
                        let dest = dir.join(bundle.file_name().expect("a bundle has a name"));
                        addons::copy_any(&bundle, &dest)?;
                        written.push(dest);
                    }
                    if !written.is_empty() {
                        report.notes.push(format!(
                            "{}: {} plug-ins in {}",
                            app.name,
                            step.to.to_uppercase(),
                            dir.display()
                        ));
                    }
                }
                "plugins" => {
                    let dir = self.plugin_folder(&app, report)?;
                    for (rel, src) in addons::matching_files(content, &step.files) {
                        let name = Path::new(&rel).file_name().expect("a file has a name").to_owned();
                        let dest = dir.join(name);
                        addons::copy_any(&src, &dest)?;
                        written.push(dest);
                    }
                    if !written.is_empty() {
                        report.notes.push(format!("{}: loads them the next time it starts", app.name));
                    }
                }
                "library" => {
                    let dir = self.addon_library().join(addons::safe_name(&addon.name));
                    for (rel, src) in addons::matching_files(content, &step.files) {
                        let dest = dir.join(&rel);
                        if !dest.exists() {
                            addons::copy_any(&src, &dest)?;
                        }
                        written.push(dest);
                    }
                    if step.open && self.installed_app(&app.id).is_some() && !written.is_empty() {
                        match self.launch(&app.id, &written) {
                            Ok(()) => report.notes.push(format!("{}: opened them so it imports them", app.name)),
                            Err(err) => log::warn!("couldn't open {} in {}: {err:#}", addon.name, app.name),
                        }
                    }
                    if !written.is_empty() {
                        report.notes.push(match &step.hint {
                            Some(hint) => format!("{}: {hint} (files in {})", app.name, dir.display()),
                            None => format!("{}: files in {}", app.name, dir.display()),
                        });
                    }
                }
                to => {
                    let Some(rest) = to.strip_prefix("app:") else {
                        anyhow::bail!("{}: unknown place {to}", addon.name)
                    };
                    let (root_key, sub) = rest.split_once('/').unwrap_or((rest, ""));
                    let (spec, ctx, _) = self.profile_target(&app.id)?;
                    let root = spec
                        .roots
                        .iter()
                        .find(|r| r.key == root_key)
                        .and_then(|r| profiles::root_dir(r, &ctx))
                        .ok_or_else(|| anyhow::anyhow!("{}: {} has no {root_key} folder here", addon.name, app.name))?;
                    let dir = root.join(sub);
                    for (rel, src) in addons::matching_files(content, &step.files) {
                        let name = Path::new(&rel).file_name().expect("a file has a name").to_owned();
                        let dest = dir.join(name);
                        addons::copy_any(&src, &dest)?;
                        written.push(dest);
                    }
                    if !written.is_empty() {
                        report.notes.push(match &step.hint {
                            Some(hint) => format!("{}: {hint}", app.name),
                            None => format!("{}: in {}", app.name, dir.display()),
                        });
                    }
                }
            }
            for w in written {
                if !report.written.contains(&w) {
                    report.written.push(w);
                }
            }
        }
        Ok(())
    }

    /// Where an app loads WebAssembly plug-ins from: the folder chosen in its preferences, else
    /// CraftSpace's own folder, which is then chosen for it.
    fn plugin_folder(&self, app: &AppEntry, report: &mut addons::Report) -> anyhow::Result<PathBuf> {
        let ours = self.inner.paths.root.join("addons").join("plug-ins").join(&app.id);
        let (spec, ctx, _) = self.profile_target(&app.id)?;
        let config = spec
            .roots
            .iter()
            .find(|r| r.key == "config")
            .and_then(|r| profiles::root_dir(r, &ctx))
            .ok_or_else(|| anyhow::anyhow!("{} has no settings folder here", app.name))?;
        let read =
            |file: &Path| std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        match app.id.as_str() {
            "photocraft" => {
                let file = config.join("preferences.json");
                let mut prefs = read(&file).unwrap_or_else(|| serde_json::json!({}));
                let current = prefs
                    .pointer("/plugIns/additionalPluginsFolder")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let used =
                    prefs.pointer("/plugIns/useAdditionalPluginsFolder").and_then(|v| v.as_bool()).unwrap_or(false);
                if used && !current.is_empty() {
                    return Ok(PathBuf::from(current));
                }
                // PhotoCraft merges its preferences with the file when it saves, so this holds
                // even while it's open.
                let section = prefs
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("PhotoCraft's preferences aren't readable"))?
                    .entry("plugIns")
                    .or_insert_with(|| serde_json::json!({}));
                section["useAdditionalPluginsFolder"] = serde_json::json!(true);
                section["additionalPluginsFolder"] = serde_json::json!(ours.to_string_lossy());
                std::fs::create_dir_all(&config)?;
                write_atomic(&file, serde_json::to_vec_pretty(&prefs)?.as_slice())?;
                report
                    .notes
                    .push(format!("PhotoCraft now loads plug-ins from {} (Preferences › Plug-ins)", ours.display()));
                Ok(ours)
            }
            "vectorcraft" => {
                let file = config.join("ui.json");
                let mut ui = read(&file).unwrap_or_else(|| serde_json::json!({}));
                let current =
                    ui.pointer("/engine_prefs/pluginsFolder").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if !current.trim().is_empty() {
                    return Ok(PathBuf::from(current.trim()));
                }
                anyhow::ensure!(
                    !self.is_running("vectorcraft"),
                    "VectorCraft is open; close it first (it saves its settings when it quits)"
                );
                let map =
                    ui.as_object_mut().ok_or_else(|| anyhow::anyhow!("VectorCraft's settings aren't readable"))?;
                let prefs = map.entry("engine_prefs").or_insert_with(|| serde_json::json!({}));
                if !prefs.is_object() {
                    *prefs = serde_json::json!({});
                }
                prefs["pluginsFolder"] = serde_json::json!(ours.to_string_lossy());
                std::fs::create_dir_all(&config)?;
                write_atomic(&file, serde_json::to_vec_pretty(&ui)?.as_slice())?;
                report.notes.push(format!(
                    "VectorCraft now loads plug-ins from {} (Preferences › Performance & Storage)",
                    ours.display()
                ));
                Ok(ours)
            }
            "effectcraft" => Ok(config.join("Plug-ins")),
            _ => anyhow::bail!("{} doesn't take plug-ins", app.name),
        }
    }

    pub fn uninstall_addon(&self, id: &str) -> anyhow::Result<usize> {
        let _busy = self.mark_busy(&format!("addon:{id}"))?;
        let installed =
            self.with_db(|db| db.addons.remove(id))?.ok_or_else(|| anyhow::anyhow!("{id} isn't installed"))?;
        let library = self.addon_library();
        for path in &installed.paths {
            remove_any(path);
            // Folders the add-on made in the library, when empty.
            let mut dir = path.parent();
            while let Some(d) = dir.filter(|d| d.starts_with(&library) && *d != library.as_path()) {
                if std::fs::remove_dir(d).is_err() {
                    break;
                }
                dir = d.parent();
            }
        }
        Ok(installed.paths.len())
    }

    /// Install a preset pack into its app's preferences folder.
    pub fn install_pack(&self, addon_id: &str, progress: &Progress) -> anyhow::Result<usize> {
        let _busy = self.mark_busy(&format!("addon:{addon_id}"))?;
        let addon = self.addon(addon_id).ok_or_else(|| anyhow::anyhow!("unknown add-on {addon_id}"))?;
        anyhow::ensure!(addon.kind == AddonKind::PresetPack, "{} isn't a pack", addon.name);
        let app = addon
            .app
            .as_deref()
            .and_then(|id| self.app(id))
            .ok_or_else(|| anyhow::anyhow!("{} names no app", addon.name))?;
        let base = app.config_dir().ok_or_else(|| anyhow::anyhow!("{} has no known preferences folder", app.name))?;
        let target = base.join(addon.target.as_deref().unwrap_or(""));
        let files = fonts::install_pack(&self.inner.agent, &addon, &target, &self.inner.paths.downloads(), progress)?;
        let n = files.len();
        self.with_db(|db| db.packs.insert(addon.id.clone(), files))?;
        Ok(n)
    }

    pub fn uninstall_pack(&self, addon_id: &str) -> anyhow::Result<()> {
        let files = self.with_db(|db| db.packs.remove(addon_id))?.unwrap_or_default();
        for f in files {
            let _ = std::fs::remove_file(f);
        }
        Ok(())
    }
}

/// Remove a file or a folder.
fn remove_any(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// Built-in entries, overridden and extended by a newer remote catalog.
fn merge_catalogs(mut base: Catalog, newer: Catalog) -> Catalog {
    if newer.revision < base.revision {
        // This build ships a newer list than the one online (or cached).
        return base;
    }
    base.revision = newer.revision;
    for app in newer.apps {
        match base.apps.iter_mut().find(|a| a.id == app.id) {
            Some(existing) => *existing = app,
            None => base.apps.push(app),
        }
    }
    for cat in newer.categories {
        if !base.categories.iter().any(|c| c.id == cat.id) {
            base.categories.push(cat);
        }
    }
    if !newer.links.is_empty() {
        base.links = newer.links;
    }
    for addon in newer.addons {
        match base.addons.iter_mut().find(|a| a.id == addon.id) {
            Some(existing) => *existing = addon,
            None => base.addons.push(addon),
        }
    }
    if newer.news_site.is_some() {
        base.news_site = newer.news_site;
    }
    base
}

/// `parent/name`, or `parent/name-2`, `-3`… if taken.
fn remote_policy_file(paths: &Paths) -> PathBuf {
    paths.cache.join("policy-remote.json")
}

fn free_dir(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    (2..).map(|n| parent.join(format!("{name}-{n}"))).find(|p| !p.exists()).expect("some free name")
}

/// Find the app's program in an unpacked version folder: an `.app` bundle on macOS, else the
/// executable (`<binary>.exe`, `bin/<binary>` or `<binary>.AppImage`).
pub fn find_executable(dir: &Path, binary: &str, os: Os) -> Option<PathBuf> {
    if os == Os::Macos {
        if let Some(bundle) = integrate::macos::find_app_bundle(dir) {
            return Some(bundle);
        }
    }
    let wanted = if os == Os::Windows { format!("{binary}.exe") } else { binary.to_string() };
    let appimage = format!("{binary}.AppImage");
    for candidate in [dir.join(&wanted), dir.join("bin").join(&wanted), dir.join(&appimage)] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    walkdir::WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| e.file_type().is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(&wanted))
        .map(|e| e.into_path())
}

/// For apps added from GitHub, whose program may be named anything: the only program in `dir`,
/// or the one whose name is closest to `hint`.
pub fn find_any_executable(dir: &Path, hint: &str, os: Os) -> Option<PathBuf> {
    if os == Os::Macos {
        if let Some(bundle) = integrate::macos::find_app_bundle(dir) {
            return Some(bundle);
        }
    }
    let is_program = |p: &Path| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if os == Os::Windows {
            return name.ends_with(".exe")
                && !["unins", "uninstall", "setup", "install"].iter().any(|w| name.starts_with(w));
        }
        if name.ends_with(".appimage") {
            return true;
        }
        if name.contains('.') && !name.ends_with(".bin") {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            return std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
        }
        #[allow(unreachable_code)]
        false
    };
    let programs: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && is_program(e.path()))
        .map(|e| e.into_path())
        .collect();
    let hint = hint.to_ascii_lowercase();
    let stem = |p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    programs
        .iter()
        .find(|p| stem(p) == hint)
        .or_else(|| programs.iter().find(|p| stem(p).contains(&hint) || hint.contains(&stem(p))))
        .or_else(|| (programs.len() == 1).then(|| &programs[0]))
        .cloned()
}

/// Where a system installer put the app: its Settings › Apps entry, or the usual folders.
pub(crate) fn find_system_install(app: &AppEntry) -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(exe) = integrate::windows::find_uninstall_entry(&app.name).and_then(|e| e.executable(app.binary())) {
        return Some(exe);
    }
    if !cfg!(windows) {
        return None;
    }
    let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .flat_map(|root| {
            [root.join(&app.name), root.join("ArtCraft").join(&app.name), root.join("Programs").join(&app.name)]
        })
        .collect();
    roots.into_iter().filter(|r| r.is_dir()).find_map(|r| find_executable(&r, app.binary(), Os::Windows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::tests::{make_tar_gz, make_zip, no_progress};

    fn manager(tmp: &Path) -> Manager {
        std::env::set_var("CRAFTSPACE_NO_INTEGRATION", "1");
        let m = Manager::open_with_policy(Paths::under(tmp.join("home")), Policy::default()).unwrap();
        let mut s = m.settings();
        s.remote_catalog = false;
        m.set_settings(s).unwrap();
        m
    }

    /// A plan whose asset is already sitting in the downloads folder with the right checksum,
    /// so `install` runs end to end without the network.
    fn local_plan(m: &Manager, version: &str) -> Plan {
        let os = m.platform().os;
        let token = m.platform().token().unwrap();
        let (name, kind) = match os {
            Os::Windows => (format!("photocraft-{version}-{token}-portable.zip"), AssetKind::PortableZip),
            _ => (format!("photocraft-{version}-{token}.tar.gz"), AssetKind::TarGz),
        };
        let path = m.paths().downloads().join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let top = format!("photocraft-{version}-{token}");
        match kind {
            AssetKind::PortableZip => {
                make_zip(&path, &[(&format!("{top}/photocraft.exe"), b"exe"), (&format!("{top}/portable.txt"), b"p")])
            }
            _ => make_tar_gz(
                &path,
                &[(&format!("{top}/bin/photocraft"), b"#!/bin/sh\n"), (&format!("{top}/bin/photocraft-cli"), b"x")],
            ),
        }
        let sha = download::sha256_file(&path).unwrap();
        let v = Version::parse(version).unwrap();
        Plan {
            app: m.app("photocraft").unwrap(),
            release: Release {
                tag: format!("v{version}"),
                name: format!("v{version}"),
                version: Some(v),
                prerelease: false,
                published_at: None,
                body: String::new(),
                html_url: String::new(),
                assets: vec![],
            },
            asset: Asset { name, size: None, url: "http://invalid.invalid/".into(), sha256: Some(sha) },
            kind,
            location: None,
        }
    }

    #[test]
    fn a_shared_package_cache_is_used_and_filled() {
        let tmp = tempfile::tempdir().unwrap();
        let share = tmp.path().join("share");
        // Computer A downloads (here: has the package) and shares it.
        let a = manager(&tmp.path().join("a"));
        let mut s = a.settings();
        s.package_cache = Some(share.clone());
        s.package_cache_write = true;
        a.set_settings(s).unwrap();
        let plan = local_plan(&a, "0.3.0");
        no_progress(|p| a.install(&plan, p)).unwrap();
        assert!(share.join(&plan.asset.name).is_file(), "put into the share");

        // Computer B has nothing downloaded and no network (the URL is invalid): the share has it.
        let b = manager(&tmp.path().join("b"));
        let mut s = b.settings();
        s.package_cache = Some(share.clone());
        b.set_settings(s).unwrap();
        let mut plan_b = plan.clone();
        plan_b.app = b.app("photocraft").unwrap();
        assert!(!b.paths().downloads().join(&plan.asset.name).exists());
        let installed = no_progress(|p| b.install(&plan_b, p)).unwrap();
        assert_eq!(installed.current.version, Version::new(0, 3, 0));

        // A tampered file in the share isn't used.
        std::fs::write(share.join(&plan.asset.name), b"not the package").unwrap();
        let c = manager(&tmp.path().join("c"));
        let mut s = c.settings();
        s.package_cache = Some(share);
        c.set_settings(s).unwrap();
        assert!(no_progress(|p| c.install(&plan_b, p)).is_err());
    }

    #[test]
    fn the_report_lists_installed_apps() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        no_progress(|p| m.install(&local_plan(&m, "0.3.0"), p)).unwrap();
        let report = m.report();
        assert_eq!(report["apps"][0]["id"], "photocraft");
        assert_eq!(report["apps"][0]["version"], "0.3.0");
        assert!(report["craftspace"].is_string() && report["platform"].is_string());
    }

    #[test]
    fn a_managed_computer_keeps_its_apps() {
        if crate::platform::is_elevated() {
            return; // Administrators may; the check is for everyone else.
        }
        let tmp = tempfile::tempdir().unwrap();
        let policy: Policy = serde_json::from_str(
            r#"{"prevent_uninstall": true, "organization": "Riverside", "support": "it@riverside.example"}"#,
        )
        .unwrap();
        let m = Manager::open_with_policy(Paths::under(tmp.path().join("p")), policy).unwrap();
        let mut plan = local_plan(&m, "0.3.0");
        plan.app = m.app("photocraft").unwrap();
        no_progress(|p| m.install(&plan, p)).unwrap();
        let err = m.uninstall("photocraft").unwrap_err().to_string();
        assert!(err.contains("Riverside") && err.contains("it@riverside.example"), "{err}");
        assert!(m.installed_app("photocraft").is_some());
    }

    #[test]
    fn programs_of_apps_from_github_are_found_by_any_name() {
        let tmp = tempfile::tempdir().unwrap();
        let os = Platform::current().os;
        let dir = tmp.path().join("ripgrep-14.1.1-x86_64");
        std::fs::create_dir_all(dir.join("doc")).unwrap();
        let exe = |name: &str| if os == Os::Windows { format!("{name}.exe") } else { name.to_string() };
        let write = |p: &Path| {
            std::fs::write(p, b"x").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        };
        std::fs::write(dir.join("README.md"), b"x").unwrap();
        std::fs::write(dir.join("doc/rg.1"), b"x").unwrap();
        write(&dir.join(exe("rg")));
        if os != Os::Macos {
            // The only program, though named differently from the repository.
            assert_eq!(find_any_executable(&dir, "ripgrep", os), Some(dir.join(exe("rg"))));
            // With several, the one matching the name wins.
            write(&dir.join(exe("helper")));
            assert_eq!(find_any_executable(&dir, "rg", os), Some(dir.join(exe("rg"))));
            assert_eq!(find_any_executable(&dir, "ripgrep", os), None);
        }
    }

    #[test]
    fn a_copy_found_on_the_computer_updates_where_it_is() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        let downloads = tmp.path().join("Downloads");
        let found = downloads.join("photocraft-0.3.0-portable");
        let exe = found.join(if cfg!(windows) { "photocraft.exe" } else { "photocraft" });
        std::fs::create_dir_all(&found).unwrap();
        std::fs::write(&exe, b"old").unwrap();
        let kind = local_plan(&m, "0.3.0").kind;
        m.with_db(|db| {
            db.apps.insert(
                "photocraft".into(),
                InstalledApp {
                    id: "photocraft".into(),
                    current: InstalledVersion {
                        version: Version::new(0, 3, 0),
                        tag: "v0.3.0".into(),
                        asset: String::new(),
                        kind,
                        sha256: None,
                        dir: Some(found.clone()),
                        executable: Some(exe.clone()),
                        package: None,
                        installed_at: 0,
                        size_bytes: None,
                        delta_downloaded: None,
                        system_package: None,
                        external: Some(crate::detect::External {
                            how: "in Downloads".into(),
                            home: Some(downloads.clone()),
                            flatpak: None,
                        }),
                    },
                    previous: None,
                    integration: Vec::new(),
                    registry_keys: Vec::new(),
                    last_launched: None,
                },
            );
        })
        .unwrap();
        assert_eq!(m.verify("photocraft").unwrap().missing.len(), 0);

        let updated = no_progress(|p| m.install(&local_plan(&m, "0.5.0"), p)).unwrap();
        let dir = updated.current.dir.clone().unwrap();
        assert_eq!(dir, downloads.join("photocraft-0.5.0"), "next to the copy that was found");
        assert!(found.join(exe.file_name().unwrap()).is_file(), "the found copy is kept");
        assert_eq!(updated.previous.as_ref().and_then(|p| p.dir.clone()), Some(found.clone()));
        assert!(!m.apps_root().join("photocraft").exists());
        assert!(updated.current.external.is_some(), "later updates go there too");
    }

    #[test]
    fn a_chosen_folder_is_kept_for_updates() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        let chosen = tmp.path().join("My Apps");
        let mut plan = local_plan(&m, "0.3.0");
        plan.location = Some(chosen.clone());
        let installed = no_progress(|p| m.install(&plan, p)).unwrap();
        assert!(installed.current.dir.as_ref().unwrap().starts_with(chosen.join("photocraft")));

        // The update goes next to it, not into the default folder.
        let updated = no_progress(|p| m.install(&local_plan(&m, "0.5.0"), p)).unwrap();
        assert!(updated.current.dir.as_ref().unwrap().starts_with(chosen.join("photocraft")));
        assert!(!m.apps_root().join("photocraft").exists());

        m.uninstall("photocraft").unwrap();
        assert!(!chosen.join("photocraft").exists());
    }

    #[test]
    fn install_update_rollback_uninstall() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());

        let first = no_progress(|p| m.install(&local_plan(&m, "0.4.0"), p)).unwrap();
        let dir1 = first.current.dir.clone().unwrap();
        assert!(first.current.executable.as_ref().unwrap().is_file());
        assert!(!dir1.join("portable.txt").exists());
        assert!(first.previous.is_none());

        let second = no_progress(|p| m.install(&local_plan(&m, "0.5.0"), p)).unwrap();
        assert_eq!(second.current.version, Version::new(0, 5, 0));
        assert_eq!(second.previous.as_ref().unwrap().version, Version::new(0, 4, 0));
        assert!(dir1.exists(), "previous version kept for rollback");

        // Persisted.
        let reopened = Manager::open_with_policy(m.paths().clone(), Policy::default()).unwrap();
        assert_eq!(reopened.installed_app("photocraft").unwrap().current.version, Version::new(0, 5, 0));

        let rolled = m.rollback("photocraft").unwrap();
        assert_eq!(rolled.current.version, Version::new(0, 4, 0));
        assert_eq!(rolled.previous.as_ref().unwrap().version, Version::new(0, 5, 0));

        // A third version drops the oldest kept one.
        let dir2 = second.current.dir.clone().unwrap();
        let third = no_progress(|p| m.install(&local_plan(&m, "0.6.0"), p)).unwrap();
        assert_eq!(third.previous.as_ref().unwrap().version, Version::new(0, 4, 0));
        assert!(!dir2.exists());

        m.uninstall("photocraft").unwrap();
        assert!(m.installed_app("photocraft").is_none());
        assert!(!third.current.dir.unwrap().exists());
        assert!(!dir1.exists());
    }

    #[test]
    fn reinstalling_same_version_uses_fresh_folder_and_bad_checksums_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        let a = no_progress(|p| m.install(&local_plan(&m, "0.5.0"), p)).unwrap();
        let b = no_progress(|p| m.install(&local_plan(&m, "0.5.0"), p)).unwrap();
        assert_ne!(a.current.dir, b.current.dir);
        assert!(!a.current.dir.unwrap().exists(), "same version isn't kept as a rollback target");
        assert!(b.previous.is_none());

        let mut plan = local_plan(&m, "0.7.0");
        plan.asset.sha256 = Some("0".repeat(64));
        // The cached file doesn't match, so it is re-downloaded from an unreachable URL.
        assert!(no_progress(|p| m.install(&plan, p)).is_err());
        assert_eq!(m.installed_app("photocraft").unwrap().current.version, Version::new(0, 5, 0));
    }

    #[test]
    fn state_reports_updates() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        no_progress(|p| m.install(&local_plan(&m, "0.4.0"), p)).unwrap();
        let token = m.platform().token().unwrap();
        let name = match m.platform().os {
            Os::Windows => format!("photocraft-0.5.0-{token}-portable.zip"),
            Os::Macos => format!("photocraft-0.5.0-{token}.dmg"),
            _ => format!("photocraft-0.5.0-{token}.tar.gz"),
        };
        let release = Release {
            tag: "v0.5.0".into(),
            name: "v0.5.0".into(),
            version: Some(Version::new(0, 5, 0)),
            prerelease: false,
            published_at: None,
            body: String::new(),
            html_url: String::new(),
            assets: vec![Asset { name, size: Some(1), url: String::new(), sha256: None }],
        };
        let release_for_pin = release.clone();
        m.inner.releases.write().unwrap().insert(
            "photocraft".into(),
            ReleaseList {
                repo: "storytold/photocraft".into(),
                releases: vec![release],
                source: github::Source::Api,
                fetched_at: 0,
                etag: None,
            },
        );
        let s = m.state("photocraft").unwrap();
        assert!(s.update_available);
        assert_eq!(m.updates().len(), 1);
        assert!(!m.state("gridcraft").unwrap().known);

        // Pinning stops update offers.
        let mut settings = m.settings();
        settings.channels.insert("photocraft".into(), Channel::Pinned(Version::new(0, 4, 0)));
        m.set_settings(settings).unwrap();
        assert!(!m.state("photocraft").unwrap().update_available);
        assert!(m.updates().is_empty());
        let _ = release_for_pin;
    }

    #[test]
    fn verifies_installed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        let installed = no_progress(|p| m.install(&local_plan(&m, "0.5.0"), p)).unwrap();
        let report = m.verify("photocraft").unwrap();
        assert!(report.is_ok(), "{report:?}");
        assert!(report.checked >= 1);
        let exe = installed.current.executable.unwrap();
        std::fs::write(&exe, b"tampered").unwrap();
        let report = m.verify("photocraft").unwrap();
        assert_eq!(report.changed, vec![exe.clone()]);
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(m.verify("photocraft").unwrap().missing, vec![exe]);
    }

    #[test]
    fn exports_and_imports_app_lists() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        no_progress(|p| m.install(&local_plan(&m, "0.4.0"), p)).unwrap();
        let mut s = m.settings();
        s.channels.insert("photocraft".into(), Channel::Prerelease);
        m.set_settings(s).unwrap();
        let list = m.export_list();
        assert_eq!(list.apps.len(), 1);
        assert_eq!(list.apps[0].version, Some(Version::new(0, 4, 0)));
        let json = serde_json::to_string(&list).unwrap();

        // On a fresh machine: photocraft (latest) plus gridcraft at a set version.
        let other = manager(&tmp.path().join("other"));
        let mut list: AppList = serde_json::from_str(&json).unwrap();
        list.apps.push(AppListEntry { id: "gridcraft".into(), version: Some(Version::new(0, 3, 0)), channel: None });
        list.apps.push(AppListEntry { id: "nonsense".into(), version: None, channel: None });
        let todo = other.import_list(&list, false).unwrap();
        assert_eq!(todo, vec![("photocraft".to_string(), None), ("gridcraft".to_string(), None)]);
        assert_eq!(other.settings().channel("photocraft"), Channel::Prerelease);
        let exact = other.import_list(&list, true).unwrap();
        assert_eq!(exact[1], ("gridcraft".to_string(), Some(Version::new(0, 3, 0))));
        // Already installed at that version: nothing to do.
        assert!(m.import_list(&m.export_list(), true).unwrap().is_empty());
    }

    #[test]
    fn refuses_unverified_packages_unless_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manager(tmp.path());
        let mut plan = local_plan(&m, "0.5.0");
        plan.asset.sha256 = None;
        let err = no_progress(|p| m.install(&plan, p)).unwrap_err();
        assert!(format!("{err:#}").contains("no checksum"), "{err:#}");
        let mut s = m.settings();
        s.allow_unverified_downloads = true;
        m.set_settings(s).unwrap();
        // The local file can't be confirmed without a checksum, so it's fetched (and fails here).
        assert!(no_progress(|p| m.install(&plan, p)).is_err());
    }

    #[test]
    fn policy_limits_apps_and_forces_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let policy: Policy =
            serde_json::from_str(r#"{"allowed_apps": ["photocraft", "gridcraft"], "required_apps": ["gridcraft"], "settings": {"keep_previous_version": false}}"#).unwrap();
        let m = Manager::open_with_policy(Paths::under(tmp.path().join("p")), policy).unwrap();
        assert_eq!(m.catalog().apps.len(), 2);
        assert!(m.app("pdfcraft").is_none());
        assert_eq!(m.required_missing(), vec!["gridcraft".to_string()]);
        let mut s = m.settings();
        s.keep_previous_version = true;
        m.set_settings(s).unwrap();
        assert!(!m.settings().keep_previous_version, "the policy wins");
    }

    #[test]
    fn merges_catalogs() {
        let base = Catalog::builtin();
        let mut newer = Catalog::builtin();
        newer.apps.truncate(1);
        newer.apps[0].tagline = "new tagline".into();
        let mut extra = newer.apps[0].clone();
        extra.id = "newcraft".into();
        newer.apps.push(extra);
        let merged = merge_catalogs(base.clone(), newer.clone());
        assert_eq!(merged.apps.len(), base.apps.len() + 1);
        assert_eq!(merged.apps[0].tagline, "new tagline");
        // An older online list doesn't undo what this build knows.
        let mut older = newer;
        older.revision = base.revision - 1;
        let merged = merge_catalogs(base.clone(), older);
        assert_eq!(merged.apps, base.apps);
    }
}
