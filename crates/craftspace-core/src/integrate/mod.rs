//! Making installed apps feel installed: Start menu / app menu entries, desktop shortcuts,
//! command-line links and "Open with" registrations. Everything created here is recorded so
//! uninstalling removes it again.

use std::path::{Path, PathBuf};

use crate::catalog::AppEntry;

#[cfg(unix)]
pub mod linux;
#[cfg(unix)]
pub mod linux_packages;
pub mod macos;
#[cfg(windows)]
pub mod windows;

pub struct Request<'a> {
    pub app: &'a AppEntry,
    pub version: &'a semver::Version,
    /// The unpacked version folder.
    pub dir: &'a Path,
    pub executable: &'a Path,
    pub desktop_shortcut: bool,
    /// A command that uninstalls the app (`[exe, args...]`), for the system's app list.
    pub uninstall_command: Option<Vec<String>>,
    pub size_bytes: u64,
    /// macOS: where the bundle goes, when not `~/Applications` (an app found in /Applications is
    /// updated there).
    pub applications: Option<&'a Path>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Integration {
    pub files: Vec<PathBuf>,
    pub registry_keys: Vec<String>,
    /// Where the program ended up, when integrating moved it (macOS: into `~/Applications`).
    pub executable: Option<PathBuf>,
}

/// Create shortcuts and registrations for a freshly installed version.
pub fn integrate(req: &Request) -> anyhow::Result<Integration> {
    if std::env::var_os("CRAFTSPACE_NO_INTEGRATION").is_some() {
        return Ok(Integration::default());
    }
    #[cfg(windows)]
    return windows::integrate(req);
    #[cfg(all(unix, not(target_os = "macos")))]
    return linux::integrate(&linux::Dirs::detect()?, req);
    #[cfg(target_os = "macos")]
    {
        let backup = crate::manager::versions_dir(req.dir, &req.app.id).join("replaced");
        let applications = req.applications.map(Path::to_path_buf).unwrap_or_else(macos::applications_dir);
        let active = macos::activate(req.executable, &applications, &backup)?;
        return Ok(Integration { files: vec![active.clone()], registry_keys: Vec::new(), executable: Some(active) });
    }
    #[allow(unreachable_code)]
    {
        let _ = req;
        Ok(Integration::default())
    }
}

/// Undo the parts of [`integrate`] that moved the program, so another version can take its
/// place. Returns the program's path afterwards when it moved (macOS).
pub fn deactivate(dir: Option<&Path>, executable: Option<&Path>) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") || std::env::var_os("CRAFTSPACE_NO_INTEGRATION").is_some() {
        return None;
    }
    let (dir, exe) = (dir?, executable?);
    if exe.starts_with(dir) || !exe.exists() {
        return None;
    }
    match macos::deactivate(exe, dir) {
        Ok(path) => Some(path),
        Err(err) => {
            log::warn!("couldn't move {} back into {}: {err:#}", exe.display(), dir.display());
            None
        }
    }
}

/// Remove what [`integrate`] created. Missing files are fine.
pub fn remove(files: &[PathBuf], registry_keys: &[String]) {
    for file in files {
        let result = if file.is_dir() && !file.is_symlink() {
            std::fs::remove_dir_all(file)
        } else {
            std::fs::remove_file(file)
        };
        match result {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => log::warn!("could not remove {}: {err}", file.display()),
        }
    }
    #[cfg(windows)]
    windows::remove_registry(registry_keys);
    #[cfg(not(windows))]
    let _ = registry_keys;
    #[cfg(all(unix, not(target_os = "macos")))]
    if !files.is_empty() {
        if let Ok(dirs) = linux::Dirs::detect() {
            linux::refresh_caches(&dirs);
        }
    }
}
