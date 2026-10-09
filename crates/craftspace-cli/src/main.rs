//! `craftspace-cli`: install, update and launch ArtCraft apps from a terminal or a script.
//!
//! For IT: `--quiet` runs without progress output and with silent installers, `install --all`
//! installs everything, `import` sets up a machine from an exported list, and `apply-policy`
//! installs the apps a machine policy requires (see `craftspace_core::policy`).

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use craftspace_core::download::format_bytes;
use craftspace_core::manager::AppList;
use craftspace_core::settings::Channel;
use craftspace_core::{autostart, fonts, selfupdate, AppState, Manager, Progress, ProgressEvent, Stage};
use semver::Version;

/// No progress output (set by `--quiet`).
static QUIET: AtomicBool = AtomicBool::new(false);

macro_rules! say {
    ($($arg:tt)*) => {
        if !QUIET.load(Ordering::Relaxed) {
            println!($($arg)*);
        }
    };
}

#[derive(Parser)]
#[command(name = "craftspace-cli", version, about = "Install, update and launch ArtCraft creative apps")]
struct Cli {
    /// Ask GitHub for fresh release information instead of using the 10-minute cache.
    #[arg(long, global = true)]
    refresh: bool,
    /// No progress output, and installers run without any windows (for scripts and IT).
    #[arg(short, long, global = true)]
    quiet: bool,
    /// Log more detail (repeat for even more).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the apps, what is installed and what is available.
    #[command(alias = "ls")]
    List {
        /// Only installed apps.
        #[arg(long)]
        installed: bool,
    },
    /// Details and release history for one app.
    Info { app: String },
    /// Install apps (the latest release, or `--version`).
    #[command(alias = "add")]
    Install {
        apps: Vec<String>,
        /// Every app that has a build for this computer.
        #[arg(long, conflicts_with = "apps")]
        all: bool,
        #[arg(long)]
        version: Option<Version>,
        /// Update even if the app is open (Windows installers may fail while it runs).
        #[arg(long)]
        force: bool,
        /// Install into this folder instead of the usual one (new installs; updates stay there).
        #[arg(long, value_name = "FOLDER")]
        dir: Option<PathBuf>,
    },
    /// Update installed apps (all of them when none are named).
    #[command(alias = "upgrade")]
    Update {
        apps: Vec<String>,
        /// Update apps that are open too.
        #[arg(long)]
        force: bool,
    },
    /// List available updates without installing them. Exits with 10 when there are some.
    Check,
    /// Go back to the version that was installed before the last update.
    Rollback { app: String },
    /// Check an app's files against what was installed.
    Verify { app: String },
    /// Reinstall an app's current version from a fresh download.
    Repair { app: String },
    /// Show or set which releases an app follows.
    Channel {
        app: String,
        #[arg(value_enum)]
        channel: Option<ChannelArg>,
        /// With `pin`: the version to stay on (default: the installed one).
        version: Option<Version>,
    },
    /// Uninstall an app and remove its shortcuts.
    #[command(alias = "remove")]
    Uninstall {
        app: String,
        /// Don't ask for confirmation.
        #[arg(short, long)]
        yes: bool,
        /// Uninstall even if the app is open.
        #[arg(long)]
        force: bool,
    },
    /// Start an installed app, optionally opening files.
    #[command(alias = "open", alias = "run")]
    Launch { app: String, files: Vec<PathBuf> },
    /// Write the installed apps (versions and channels) to a file, or stdout.
    Export { file: Option<PathBuf> },
    /// Install the apps in an exported list.
    Import {
        file: PathBuf,
        /// Install the exact versions in the list instead of the latest.
        #[arg(long)]
        exact: bool,
    },
    /// Install the apps the machine policy requires.
    ApplyPolicy,
    /// Open a file in the ArtCraft app that handles it.
    OpenFile { file: PathBuf },
    /// Find ArtCraft apps installed without CraftSpace and keep them up to date where they are.
    Detect,
    /// Open ArtCraft file types through CraftSpace (double-clicking opens the right app, or
    /// offers to install it).
    FileTypes {
        #[command(subcommand)]
        action: FileTypesAction,
    },
    /// Fonts from the ArtCraft font collection.
    Fonts {
        #[command(subcommand)]
        action: FontsAction,
    },
    /// Latest news and tutorials from the ArtCraft website.
    News,
    /// Open a pre-filled bug report for an app (prints the link with --quiet).
    ReportBug { app: String },
    /// Start CraftSpace in the background when you log in.
    Autostart {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Update CraftSpace itself.
    SelfUpdate,
    /// Install CraftSpace itself for this user (menu entry, `craftspace-cli` on PATH on Linux).
    SelfInstall,
    /// Show or change a setting (`config`, `config download_limit_kbps 2048`).
    Config { key: Option<String>, value: Option<String> },
    /// Show where CraftSpace keeps apps, downloads and settings.
    Paths,
    /// Delete leftover downloads and folders from earlier updates.
    Cleanup,
}

#[derive(Clone, Copy, ValueEnum)]
enum ChannelArg {
    Default,
    Stable,
    Prerelease,
    Pin,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Subcommand)]
enum FileTypesAction {
    /// Show which file types CraftSpace opens.
    List,
    /// Open these extensions (default: the apps' own formats) through CraftSpace.
    On { extensions: Vec<String> },
    /// Stop: files open in their apps directly again.
    Off,
}

#[derive(Subcommand)]
enum FontsAction {
    /// The fonts on offer and which are installed.
    List,
    /// Install a font family, or all of them.
    Install { family: Option<String> },
    /// Remove a font family CraftSpace installed, or all of them.
    Uninstall { family: Option<String> },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    QUIET.store(cli.quiet, Ordering::Relaxed);
    let level = match cli.verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level)).init();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let manager = Manager::open()?;
    if cli.quiet {
        manager.set_quiet(true);
    }
    let mut failed = 0;
    match cli.command {
        Command::List { installed } => {
            refresh_all(&manager, cli.refresh);
            print_table(
                &manager.states().into_iter().filter(|s| !installed || s.installed.is_some()).collect::<Vec<_>>(),
            );
        }
        Command::Info { app } => {
            manager.refresh(&app, cli.refresh)?;
            print_info(&manager, &app)?;
        }
        Command::Install { apps, all, version, force, dir } => {
            anyhow::ensure!(version.is_none() || apps.len() == 1, "--version works with one app at a time");
            let apps = if all {
                refresh_all(&manager, cli.refresh);
                manager.states().into_iter().filter(|s| s.installable.is_some()).map(|s| s.app.id).collect()
            } else {
                anyhow::ensure!(!apps.is_empty(), "name the apps to install, or use --all");
                apps
            };
            for app in &apps {
                if let Err(err) = install_in(&manager, app, version.as_ref(), cli.refresh, force, dir.as_deref()) {
                    eprintln!("error: {app}: {err:#}");
                    failed += 1;
                }
            }
        }
        Command::Update { apps, force } => {
            refresh_all(&manager, cli.refresh);
            let updates: Vec<AppState> = manager
                .updates()
                .into_iter()
                .filter(|s| apps.is_empty() || apps.iter().any(|a| a.eq_ignore_ascii_case(&s.app.id)))
                .collect();
            if updates.is_empty() {
                say!("Everything is up to date.");
            }
            for state in updates {
                if let Err(err) = install(&manager, &state.app.id, None, false, force) {
                    eprintln!("error: {}: {err:#}", state.app.id);
                    failed += 1;
                }
            }
        }
        Command::Check => {
            refresh_all(&manager, cli.refresh);
            let updates = manager.updates();
            if updates.is_empty() {
                say!("Everything is up to date.");
            } else {
                for s in &updates {
                    println!("{:<12} {} → {}", s.app.id, fmt_opt(s.installed_version()), fmt_opt(s.latest_version()));
                }
                return Ok(ExitCode::from(10));
            }
        }
        Command::Rollback { app } => {
            let record = manager.rollback(&app)?;
            say!("{} is back on {}.", app, record.current.version);
        }
        Command::Verify { app } => {
            let report = manager.verify(&app)?;
            if report.is_ok() {
                say!("{app}: all {} files are intact.", report.checked);
            } else {
                for p in &report.missing {
                    println!("missing  {}", p.display());
                }
                for p in &report.changed {
                    println!("changed  {}", p.display());
                }
                println!("Run `craftspace-cli repair {app}` to fix it.");
                return Ok(ExitCode::from(11));
            }
        }
        Command::Repair { app } => {
            let record = with_progress(&app, |p| manager.repair(&app, p))?;
            say!("Repaired {} {}.", app, record.current.version);
        }
        Command::Channel { app, channel, version } => {
            let entry = manager.app(&app).with_context(|| format!("unknown app '{app}'"))?;
            let mut settings = manager.settings();
            match channel {
                None => println!("{}: {}", entry.id, settings.channel(&entry.id).label()),
                Some(c) => {
                    let channel = match c {
                        ChannelArg::Default => Channel::Default,
                        ChannelArg::Stable => Channel::Stable,
                        ChannelArg::Prerelease => Channel::Prerelease,
                        ChannelArg::Pin => Channel::Pinned(
                            version
                                .or_else(|| manager.installed_app(&entry.id).map(|i| i.current.version))
                                .context("give the version to pin")?,
                        ),
                    };
                    say!("{}: {}", entry.id, channel.label());
                    settings.channels.insert(entry.id.clone(), channel);
                    manager.set_settings(settings)?;
                }
            }
        }
        Command::Uninstall { app, yes, force } => {
            let entry = manager.app(&app).with_context(|| format!("unknown app '{app}'"))?;
            let installed =
                manager.installed_app(&entry.id).with_context(|| format!("{} is not installed", entry.name))?;
            if !force && manager.is_running(&entry.id) {
                anyhow::bail!("{} is open; close it first (or pass --force)", entry.name);
            }
            if !yes && !cli.quiet && !confirm(&format!("Uninstall {} {}?", entry.name, installed.current.version))? {
                return Ok(ExitCode::SUCCESS);
            }
            manager.uninstall(&entry.id)?;
            say!("Uninstalled {}.", entry.name);
        }
        Command::Launch { app, files } => {
            let files: Vec<PathBuf> = files.into_iter().map(|f| std::fs::canonicalize(&f).unwrap_or(f)).collect();
            manager.launch(&app, &files)?;
        }
        Command::Export { file } => {
            let json = serde_json::to_string_pretty(&manager.export_list())?;
            match file {
                Some(f) => {
                    std::fs::write(&f, json)?;
                    say!("Wrote {}.", f.display());
                }
                None => println!("{json}"),
            }
        }
        Command::Import { file, exact } => {
            let list: AppList = serde_json::from_slice(&std::fs::read(&file)?).context("not a CraftSpace app list")?;
            let todo = manager.import_list(&list, exact)?;
            if todo.is_empty() {
                say!("Everything in the list is already installed.");
            }
            for (id, version) in todo {
                if let Err(err) = install(&manager, &id, version.as_ref(), cli.refresh, false) {
                    eprintln!("error: {id}: {err:#}");
                    failed += 1;
                }
            }
        }
        Command::ApplyPolicy => {
            let policy = manager.policy();
            match &policy.source {
                Some(src) => say!("Policy: {}", src.display()),
                None => say!("No machine policy is set."),
            }
            for id in manager.required_missing() {
                if let Err(err) = install(&manager, &id, None, cli.refresh, false) {
                    eprintln!("error: {id}: {err:#}");
                    failed += 1;
                }
            }
        }
        Command::Fonts { action } => {
            let addon = manager
                .catalog()
                .addons
                .into_iter()
                .find(|a| a.kind == craftspace_core::catalog::AddonKind::Fonts)
                .context("the catalog offers no fonts")?;
            match action {
                FontsAction::List => {
                    let installed = manager.installed_fonts();
                    for f in manager.font_list(&addon.id)? {
                        let scripts: Vec<&str> = f.scripts.iter().map(|s| fonts::script_name(s)).collect();
                        let mark = if installed.contains_key(f.file_name()) { "installed" } else { "" };
                        println!("{:<20} {:<8} {:<34} {mark}", f.family, f.style, scripts.join(", "));
                    }
                }
                FontsAction::Install { family } => {
                    let n = with_progress(&addon.name, |p| manager.install_fonts(&addon.id, family.as_deref(), p))?;
                    say!("Installed {n} font file(s).");
                }
                FontsAction::Uninstall { family } => {
                    let n = manager.uninstall_fonts(family.as_deref())?;
                    say!("Removed {n} font file(s).");
                }
            }
        }
        Command::News => {
            for a in manager.articles(cli.refresh)? {
                let kind = match a.kind {
                    craftspace_core::news::ArticleKind::News => "news",
                    craftspace_core::news::ArticleKind::Tutorial => "tutorial",
                };
                println!("{:<9} {:<10} {}\n          {}", kind, a.date.as_deref().unwrap_or(""), a.title, a.url);
            }
        }
        Command::ReportBug { app } => {
            let url = manager.bug_report_url(&app).with_context(|| format!("unknown app '{app}'"))?;
            if cli.quiet || open::that_detached(&url).is_err() {
                println!("{url}");
            }
        }
        Command::Autostart { state } => {
            let on = matches!(state, OnOff::On);
            let exe =
                std::env::current_exe()?.with_file_name(if cfg!(windows) { "craftspace.exe" } else { "craftspace" });
            autostart::set(on, &exe)?;
            let mut settings = manager.settings();
            settings.start_at_login = on;
            manager.set_settings(settings)?;
            say!("CraftSpace {} start when you log in.", if on { "will" } else { "won't" });
        }
        Command::SelfUpdate => match selfupdate::check(&manager)? {
            None => say!("CraftSpace {} is up to date.", selfupdate::current_version()),
            Some(update) => {
                say!("Updating CraftSpace {} → {}", selfupdate::current_version(), update.version);
                with_progress(&update.asset.name, |p| selfupdate::apply(&manager, &update, p))?;
                say!("Done. The new version starts next time.");
            }
        },
        Command::SelfInstall => {
            let exe = selfupdate::self_install(&manager)?;
            say!("CraftSpace is installed: {}", exe.display());
        }
        Command::Config { key, value } => {
            let mut json = serde_json::to_value(manager.settings())?;
            match (key, value) {
                (None, _) => println!("{}", serde_json::to_string_pretty(&json)?),
                (Some(k), None) => println!("{}", json.get(&k).with_context(|| format!("no setting {k}"))?),
                (Some(k), Some(v)) => {
                    anyhow::ensure!(!manager.policy().locks(&k), "{k} is set by your organization's policy");
                    let obj = json.as_object_mut().expect("settings are an object");
                    anyhow::ensure!(obj.contains_key(&k), "no setting {k}");
                    let parsed: serde_json::Value = serde_json::from_str(&v).unwrap_or(serde_json::Value::String(v));
                    obj.insert(k.clone(), parsed);
                    let settings = serde_json::from_value(json).with_context(|| format!("bad value for {k}"))?;
                    manager.set_settings(settings)?;
                    say!("{k} updated.");
                }
            }
        }
        Command::Detect => {
            let found = manager.adopt_installed()?;
            if found.is_empty() {
                say!("No other ArtCraft apps found (apps CraftSpace already knows aren't listed again).");
            }
            for (name, version) in found {
                say!("Found {name} {}", version.map(|v| v.to_string()).unwrap_or_else(|| "(version unknown)".into()));
            }
        }
        Command::OpenFile { file } => match manager.open_file(&file)? {
            None => {}
            Some(id) => {
                let name = manager.app(&id).map(|a| a.name).unwrap_or_else(|| id.clone());
                anyhow::bail!(
                    "{} opens in {name}, which isn't installed; run: craftspace-cli install {id}",
                    file.display()
                );
            }
        },
        Command::FileTypes { action } => {
            let mut settings = manager.settings();
            match action {
                FileTypesAction::List => {
                    let types = &settings.files.open_with_craftspace;
                    if types.is_empty() {
                        say!("CraftSpace doesn't open any file types; they open in their apps directly.");
                    } else {
                        say!(
                            "CraftSpace opens: {}",
                            types.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" ")
                        );
                    }
                    return Ok(ExitCode::SUCCESS);
                }
                FileTypesAction::On { extensions } => {
                    let all = manager.catalog().all_extensions();
                    let chosen: Vec<String> = if extensions.is_empty() {
                        all.into_iter().filter(|e| craftspace_core::file_types::default_on(e)).collect()
                    } else {
                        let chosen: Vec<String> =
                            extensions.iter().map(|e| e.trim_start_matches('.').to_ascii_lowercase()).collect();
                        if let Some(bad) = chosen.iter().find(|e| all.binary_search(e).is_err()) {
                            anyhow::bail!("no ArtCraft app opens .{bad}");
                        }
                        chosen
                    };
                    let r = manager.register_file_types(&chosen)?;
                    settings.files.open_with_craftspace = chosen;
                    manager.set_settings(settings)?;
                    say!("CraftSpace now opens {} file type(s).", r.types);
                    if let Some(url) = r.confirm_url {
                        say!("Windows asks you to confirm: choose CraftSpace in Settings › Default apps ({url}).");
                    }
                }
                FileTypesAction::Off => {
                    manager.register_file_types(&[])?;
                    settings.files.open_with_craftspace.clear();
                    manager.set_settings(settings)?;
                    say!("File types open in their apps directly again.");
                }
            }
        }
        Command::Paths => {
            let p = manager.paths();
            println!("platform   {}", manager.platform().display());
            println!("apps       {}", manager.apps_root().display());
            println!("downloads  {}", p.downloads().display());
            println!("settings   {}", p.settings_file().display());
            println!("installed  {}", p.installed_file().display());
            println!("policy     {}", craftspace_core::policy::Policy::default_path().display());
            println!("fonts      {}", fonts::fonts_dir().map(|d| d.display().to_string()).unwrap_or_default());
        }
        Command::Cleanup => {
            let freed = manager.cleanup()?;
            say!("Freed {}.", format_bytes(freed));
        }
    }
    Ok(if failed > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn refresh_all(manager: &Manager, force: bool) {
    if let Err(err) = manager.refresh_catalog() {
        log::info!("using the built-in app list: {err:#}");
    }
    // Apps installed without CraftSpace are listed and updated too.
    match manager.adopt_installed() {
        Ok(found) => {
            for (name, version) in found {
                say!("Found {name} {} on this computer", version.map(|v| v.to_string()).unwrap_or_default());
            }
        }
        Err(err) => log::warn!("couldn't look for apps installed without CraftSpace: {err:#}"),
    }
    for (id, err) in manager.refresh_all(force) {
        eprintln!("warning: could not check {id}: {err:#}");
    }
}

fn install(manager: &Manager, app: &str, version: Option<&Version>, refresh: bool, force: bool) -> anyhow::Result<()> {
    install_in(manager, app, version, refresh, force, None)
}

fn install_in(
    manager: &Manager,
    app: &str,
    version: Option<&Version>,
    refresh: bool,
    force: bool,
    dir: Option<&Path>,
) -> anyhow::Result<()> {
    if refresh {
        manager.refresh(app, true)?;
    }
    let mut plan = manager.plan(app, version)?;
    if let Some(dir) = dir {
        anyhow::ensure!(
            plan.kind.is_managed(),
            "{} installs as a system package ({}), which goes where the package manager puts it; --dir works with portable installs",
            plan.app.name,
            plan.kind.label()
        );
        std::fs::create_dir_all(dir)?;
        plan.location = Some(std::path::absolute(dir)?);
    }
    let from = manager.installed_app(&plan.app.id).map(|i| i.current.version);
    match &from {
        Some(v) if Some(v) == plan.release.version.as_ref() && version.is_none() => {
            say!("{} {} is already installed and up to date.", plan.app.name, v);
            return Ok(());
        }
        Some(v) => say!("{} {} → {} ({})", plan.app.name, v, plan.release.tag, plan.kind.label()),
        None => say!("Installing {} {} ({})", plan.app.name, plan.release.tag, plan.kind.label()),
    }
    if from.is_some() && manager.is_running(&plan.app.id) {
        if !plan.kind.is_managed() && !force {
            anyhow::bail!("{} is open; close it first (or pass --force)", plan.app.name);
        }
        eprintln!("note: {} is open; the new version is used the next time it starts", plan.app.name);
    }
    let record = with_progress(&plan.asset.name, |p| manager.install(&plan, p))?;
    if let Some(n) = record.current.delta_downloaded {
        match plan.asset.size.or(record.current.size_bytes).filter(|t| *t > 0) {
            Some(total) => say!(
                "Delta update: downloaded {} of {} ({}% reused from the installed version).",
                format_bytes(n),
                format_bytes(total),
                100u64.saturating_sub(n * 100 / total)
            ),
            None => say!("Delta update: downloaded {}.", format_bytes(n)),
        }
    }
    match &record.current.executable {
        Some(exe) => say!("Installed {} {} → {}", plan.app.name, record.current.version, exe.display()),
        None => say!("Installed {} {}", plan.app.name, record.current.version),
    }
    Ok(())
}

/// Run `f` with a one-line progress display on stderr.
fn with_progress<R>(label: &str, f: impl FnOnce(&Progress) -> anyhow::Result<R>) -> anyhow::Result<R> {
    let quiet = QUIET.load(Ordering::Relaxed);
    let tty = std::io::stderr().is_terminal() && !quiet;
    let stage = Mutex::new(Stage::Resolving);
    let report = |event: ProgressEvent| {
        let mut err = std::io::stderr();
        match event {
            ProgressEvent::Stage(s) => {
                *stage.lock().unwrap() = s;
                if s != Stage::Downloading && !quiet {
                    if tty {
                        let _ = write!(err, "\r\x1b[2K");
                    }
                    let _ = writeln!(err, "  {}…", s.label());
                }
            }
            ProgressEvent::Bytes { done, total } if tty => {
                let line = match total {
                    Some(t) if t > 0 => {
                        let pct = done as f64 / t as f64;
                        let filled = (pct * 30.0) as usize;
                        format!(
                            "  {label} [{}{}] {:>3}% {} / {}",
                            "█".repeat(filled),
                            "░".repeat(30 - filled.min(30)),
                            (pct * 100.0) as u32,
                            format_bytes(done),
                            format_bytes(t)
                        )
                    }
                    _ => format!("  {label} {}", format_bytes(done)),
                };
                let _ = write!(err, "\r\x1b[2K{line}");
                let _ = err.flush();
            }
            ProgressEvent::Bytes { .. } => {}
        }
    };
    let cancel = AtomicBool::new(false);
    let result = f(&Progress { report: &report, cancel: &cancel });
    if tty {
        eprint!("\r\x1b[2K");
    }
    result
}

fn fmt_opt(v: Option<&Version>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

fn print_table(states: &[AppState]) {
    println!("{:<12} {:<13} {:<11} {:<11} STATUS", "APP", "LIKE", "INSTALLED", "LATEST");
    for s in states {
        let status = if !s.known {
            "unknown (offline?)".to_string()
        } else if s.latest.is_none() {
            "no releases yet".into()
        } else if s.installable.is_none() && s.installed.is_none() {
            "no build for this platform".into()
        } else if s.update_available {
            "update available".into()
        } else if s.installed.is_some() {
            "up to date".into()
        } else {
            "available".into()
        };
        println!(
            "{:<12} {:<13} {:<11} {:<11} {status}",
            s.app.id,
            s.app.like.as_deref().unwrap_or(""),
            fmt_opt(s.installed_version()),
            fmt_opt(s.latest_version()),
        );
    }
}

fn print_info(manager: &Manager, id: &str) -> anyhow::Result<()> {
    let state = manager.state(id).with_context(|| format!("unknown app '{id}'"))?;
    let app = &state.app;
    println!("{} — {}", app.name, app.tagline);
    if !app.description.is_empty() {
        println!("{}", app.description);
    }
    println!();
    println!("source     {}", app.github_url());
    println!("website    {}", app.homepage());
    if !app.extensions.is_empty() {
        println!("opens      {}", app.extensions.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" "));
    }
    if let Some(i) = &state.installed {
        println!("installed  {} ({})", i.current.version, i.current.kind.label());
        if let Some(exe) = &i.current.executable {
            println!("program    {}", exe.display());
        }
        if let Some(p) = &i.previous {
            println!("rollback   {}", p.version);
        }
    }
    if let Some((asset, kind)) = &state.installable {
        let size = asset.size.map(|s| format!(", {}", format_bytes(s))).unwrap_or_default();
        println!("download   {} ({}{size})", asset.name, kind.label());
    }
    if let Some(list) = manager.releases(id) {
        println!();
        println!("releases:");
        for r in list.releases.iter().take(10) {
            println!("  {:<16} {}{}", r.tag, r.date().unwrap_or(""), if r.prerelease { "  (pre-release)" } else { "" });
        }
    }
    Ok(())
}

fn confirm(question: &str) -> anyhow::Result<bool> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("{question} Pass --yes to confirm when not running interactively.");
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}
