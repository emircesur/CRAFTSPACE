//! Updating CraftSpace itself, from this repository's releases (published with the same
//! `<name>-<version>-<platform>` asset convention as the ArtCraft apps).

use std::path::{Path, PathBuf};

use semver::Version;

use crate::archive;
use crate::download::{self, Progress, Stage};
use crate::github::{Asset, Release};
use crate::manager::{find_executable, Manager};
use crate::platform::{AssetKind, Os};

pub const REPO: &str = "emircesur/craftspace";
pub const NAME: &str = "craftspace";

/// Where CraftSpace looks for its own updates: the official repository, or a fork or mirror set
/// in Settings › Other sources.
pub fn update_repo(manager: &Manager) -> String {
    let other = manager.settings().other_sources;
    other.self_repo.filter(|_| other.enabled).unwrap_or_else(|| REPO.to_string())
}

/// CraftSpace's icon, for menu entries.
pub const ICON_PNG: &[u8] = include_bytes!("../../../assets/craftspace-256.png");

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
}

#[derive(Debug, Clone)]
pub struct SelfUpdate {
    pub release: Release,
    pub version: Version,
    pub asset: Asset,
    pub kind: AssetKind,
}

/// A newer CraftSpace for this platform, if there is one.
pub fn check(manager: &Manager) -> anyhow::Result<Option<SelfUpdate>> {
    if manager.policy().disable_self_update || from_system_package() {
        return Ok(None);
    }
    let settings = manager.settings();
    let list = manager.github().releases(&update_repo(manager), false)?;
    let Some(release) = list.latest(settings.include_prereleases).cloned() else { return Ok(None) };
    let Some(version) = release.version.clone().filter(|v| *v > current_version()) else { return Ok(None) };
    // Self-updates always use the archive builds, never an installer.
    let Some((i, kind)) = manager.platform().select_asset(NAME, &release.asset_names(), Default::default()) else {
        return Ok(None);
    };
    if !matches!(kind, AssetKind::PortableZip | AssetKind::TarGz | AssetKind::Dmg) {
        return Ok(None);
    }
    let mut release = release;
    if release.assets[i].sha256.is_none() {
        let name = release.assets[i].name.clone();
        let _ = manager.github().fill_checksum(&mut release, &name);
    }
    let asset = release.assets[i].clone();
    Ok(Some(SelfUpdate { release, version, asset, kind }))
}

/// Download `update` and swap it in for the running executables. Takes effect on restart.
pub fn apply(manager: &Manager, update: &SelfUpdate, progress: &Progress) -> anyhow::Result<()> {
    let downloads = manager.paths().downloads();
    let archive_path = downloads.join(&update.asset.name);
    download::download(manager.agent(), &update.asset.url, &archive_path, update.asset.sha256.as_deref(), progress)?;
    progress.stage(Stage::Installing);
    let staging = downloads.join(format!("craftspace-{}", update.version));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    let result = if update.kind == AssetKind::Dmg {
        crate::integrate::macos::install_dmg(&archive_path, &staging)
            .and_then(|bundle| replace_running_bundle(&bundle, &downloads))
    } else {
        archive::unpack(&archive_path, &staging, progress)?;
        replace_running(&staging, manager.platform().os)
    };
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(&archive_path);
    result
}

fn replace_running(staging: &Path, os: Os) -> anyhow::Result<()> {
    let current = std::env::current_exe()?;
    let dir = current.parent().map(Path::to_path_buf).unwrap_or_default();
    let ext = if os == Os::Windows { ".exe" } else { "" };
    for name in [NAME, "craftspace-cli"] {
        let Some(new) = find_executable(staging, name, os) else { continue };
        let target: PathBuf = dir.join(format!("{name}{ext}"));
        if target == current {
            // The running program can't simply be overwritten on Windows.
            self_replace::self_replace(&new)?;
        } else if target.exists() || name == NAME {
            let tmp = target.with_extension("new");
            std::fs::copy(&new, &tmp)?;
            std::fs::rename(&tmp, &target)?;
        }
    }
    Ok(())
}

/// Installed from an .rpm / .deb (or Copr): the package manager updates CraftSpace, not itself.
pub fn from_system_package() -> bool {
    cfg!(target_os = "linux") && std::env::current_exe().is_ok_and(|e| e.starts_with("/usr/"))
}

/// The `.app` bundle the running program is in (macOS).
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// Swap the running `CraftSpace.app` for `new_bundle`. The running copy keeps working until it
/// quits; the old bundle is set aside in `trash_dir`.
fn replace_running_bundle(new_bundle: &Path, trash_dir: &Path) -> anyhow::Result<()> {
    let running = running_bundle().ok_or_else(|| anyhow::anyhow!("CraftSpace isn't running from an .app bundle"))?;
    let old = trash_dir.join(format!("CraftSpace-old-{}.app", crate::github::now_secs()));
    crate::integrate::macos::move_bundle(&running, &old)?;
    if let Err(err) = crate::integrate::macos::move_bundle(new_bundle, &running) {
        // Put the old one back rather than leave nothing.
        let _ = crate::integrate::macos::move_bundle(&old, &running);
        return Err(err);
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// The catalog-style entry CraftSpace uses for its own shortcuts.
pub fn self_entry() -> crate::catalog::AppEntry {
    crate::catalog::AppEntry {
        id: NAME.into(),
        name: "CraftSpace".into(),
        code: "Cs".into(),
        repo: REPO.into(),
        category: "tools".into(),
        tagline: "Install and update ArtCraft apps".into(),
        description: String::new(),
        like: None,
        colors: crate::catalog::Colors { bg: "#1B1B1D".into(), fg: "#3B82F6".into() },
        extensions: Vec::new(),
        homepage: Some(format!("https://github.com/{REPO}")),
        binary: Some(NAME.into()),
        featured: false,
        icon: None,
        windows_installer: None,
        config_dir: None,
        metainfo: None,
        custom: false,
    }
}

/// Where a self-installed CraftSpace lives: `<root>/app/bin/`.
pub fn install_dir(manager: &Manager) -> PathBuf {
    manager.paths().root.join("app")
}

/// The installed CraftSpace program (`/usr/bin/craftspace` from a package, else the copy
/// `self-install` makes); it may not exist yet.
pub fn installed_exe(manager: &Manager) -> PathBuf {
    if from_system_package() {
        if let Ok(exe) = std::env::current_exe() {
            return exe;
        }
    }
    let ext = if manager.platform().os == Os::Windows { ".exe" } else { "" };
    install_dir(manager).join("bin").join(format!("{NAME}{ext}"))
}

/// Whether the running program is the installed copy.
pub fn is_installed_copy(manager: &Manager) -> bool {
    if from_system_package() {
        return true;
    }
    if manager.platform().os == Os::Macos {
        // Anywhere in an Applications folder counts.
        return running_bundle()
            .is_some_and(|b| b.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == "Applications")));
    }
    let Ok(exe) = std::env::current_exe().and_then(|e| e.canonicalize()) else { return false };
    let dir = install_dir(manager);
    dir.canonicalize().is_ok_and(|d| exe.starts_with(d))
}

/// Copy the running CraftSpace (and its CLI, if next to it) into the CraftSpace folder and add
/// a Start menu / app menu entry and, on Windows, an entry in Settings › Apps.
pub fn self_install(manager: &Manager) -> anyhow::Result<PathBuf> {
    let os = manager.platform().os;
    if os == Os::Macos {
        // On macOS installing means copying the bundle into ~/Applications.
        let bundle =
            running_bundle().ok_or_else(|| anyhow::anyhow!("run CraftSpace.app (not a bare binary) to install it"))?;
        let target = crate::integrate::macos::applications_dir().join("CraftSpace.app");
        if bundle != target {
            crate::integrate::macos::copy_bundle(&bundle, &target)?;
        }
        return Ok(target);
    }
    let ext = if os == Os::Windows { ".exe" } else { "" };
    let current = std::env::current_exe()?;
    let src_dir = current.parent().map(Path::to_path_buf).unwrap_or_default();
    let dir = install_dir(manager);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin)?;

    for name in [NAME, "craftspace-cli"] {
        let src = src_dir.join(format!("{name}{ext}"));
        if !src.is_file() {
            continue;
        }
        let dest = bin.join(format!("{name}{ext}"));
        if src.canonicalize().ok() == dest.canonicalize().ok() {
            continue;
        }
        let tmp = dest.with_extension("new");
        std::fs::copy(&src, &tmp)?;
        std::fs::rename(&tmp, &dest).or_else(|_| {
            // The installed copy is running; swap it the way updates do.
            if dest.exists() && name == NAME {
                let old = dest.with_extension("old");
                let _ = std::fs::remove_file(&old);
                std::fs::rename(&dest, &old)?;
                std::fs::rename(&tmp, &dest)
            } else {
                Err(std::io::Error::other(format!("can't replace {}", dest.display())))
            }
        })?;
    }
    let exe = bin.join(format!("{NAME}{ext}"));
    anyhow::ensure!(exe.is_file(), "couldn't find {NAME}{ext} next to the running program");

    // An icon for the menu entry.
    let icon_dir = dir.join("share/icons/hicolor/256x256/apps");
    std::fs::create_dir_all(&icon_dir)?;
    std::fs::write(icon_dir.join("craftspace.png"), ICON_PNG)?;

    let entry = self_entry();
    let version = current_version();
    let mut uninstall =
        vec![bin.join(format!("craftspace-cli{ext}")).to_string_lossy().into_owned(), "uninstall".into(), NAME.into()];
    if !bin.join(format!("craftspace-cli{ext}")).exists() {
        uninstall[0] = exe.to_string_lossy().into_owned();
    }
    let integration = crate::integrate::integrate(&crate::integrate::Request {
        app: &entry,
        version: &version,
        dir: &dir,
        executable: &exe,
        desktop_shortcut: manager.settings().desktop_shortcuts,
        uninstall_command: Some(uninstall),
        applications: None,
        size_bytes: crate::state::dir_size(&dir),
    })?;
    manager.record_self_install(crate::state::InstalledApp {
        id: NAME.into(),
        current: crate::state::InstalledVersion {
            version,
            tag: format!("v{}", current_version()),
            asset: String::new(),
            kind: if os == Os::Windows { AssetKind::PortableZip } else { AssetKind::TarGz },
            sha256: None,
            dir: Some(dir),
            executable: Some(exe.clone()),
            package: None,
            installed_at: crate::github::now_secs(),
            size_bytes: None,
            delta_downloaded: None,
            system_package: None,
            external: None,
        },
        previous: None,
        integration: integration.files,
        registry_keys: integration.registry_keys,
        last_launched: None,
    })?;
    Ok(exe)
}
