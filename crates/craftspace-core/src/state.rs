//! What is installed, stored as `installed.json` in the CraftSpace root.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::paths::write_atomic;
use crate::platform::AssetKind;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstalledDb {
    #[serde(default)]
    pub apps: BTreeMap<String, InstalledApp>,
    /// Folders that could not be removed yet (usually because the app was running).
    #[serde(default)]
    pub pending_removal: Vec<PathBuf>,
    /// Installed font files, by file name.
    #[serde(default)]
    pub fonts: BTreeMap<String, crate::fonts::InstalledFont>,
    /// Files written by add-on packs, by add-on id.
    #[serde(default)]
    pub packs: BTreeMap<String, Vec<PathBuf>>,
    /// Add-ons from the registry and stores, by id.
    #[serde(default)]
    pub addons: BTreeMap<String, crate::addons::Installed>,
    /// Workspace profiles from the policy that were applied: app id → the profile's SHA-256.
    #[serde(default)]
    pub profiles_applied: BTreeMap<String, String>,
}

/// One installed version of an app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledVersion {
    pub version: Version,
    pub tag: String,
    pub asset: String,
    pub kind: AssetKind,
    pub sha256: Option<String>,
    /// The version folder, for apps CraftSpace manages itself.
    pub dir: Option<PathBuf>,
    /// The desktop executable, when known.
    pub executable: Option<PathBuf>,
    /// The installer package kept for uninstalling, for system-installer installs.
    #[serde(default)]
    pub package: Option<PathBuf>,
    /// Unix seconds.
    pub installed_at: u64,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// For delta updates: how many bytes were downloaded instead of the whole package.
    #[serde(default)]
    pub delta_downloaded: Option<u64>,
    /// For `.rpm` / `.deb` installs: the package's name in the system package manager.
    #[serde(default)]
    pub system_package: Option<String>,
    /// Installed without CraftSpace and adopted (see [`crate::detect`]).
    #[serde(default)]
    pub external: Option<crate::detect::External>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstalledApp {
    pub id: String,
    pub current: InstalledVersion,
    /// The version that was current before the last update, kept for rollback.
    #[serde(default)]
    pub previous: Option<InstalledVersion>,
    /// Files and links created outside the version folder (shortcuts, desktop entries, symlinks).
    #[serde(default)]
    pub integration: Vec<PathBuf>,
    /// Registry keys created under HKEY_CURRENT_USER (Windows).
    #[serde(default)]
    pub registry_keys: Vec<String>,
    #[serde(default)]
    pub last_launched: Option<u64>,
}

impl InstalledDb {
    pub fn load(path: &Path) -> InstalledDb {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                log::error!("{} is unreadable ({err}); keeping a copy and starting empty", path.display());
                let _ = std::fs::copy(path, path.with_extension("json.bad"));
                InstalledDb::default()
            }),
            Err(_) => InstalledDb::default(),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&InstalledApp> {
        self.apps.get(id)
    }

    /// Try again to delete folders left behind by earlier updates.
    pub fn retry_pending_removal(&mut self) {
        self.pending_removal.retain(|dir| match std::fs::remove_dir_all(dir) {
            Ok(()) => false,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => true,
        });
    }

    /// Delete `dir`, or remember to try again later if something holds it open.
    pub fn remove_dir_or_defer(&mut self, dir: &Path) {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                log::warn!("could not remove {} yet: {err}", dir.display());
                if !self.pending_removal.iter().any(|d| d == dir) {
                    self.pending_removal.push(dir.to_path_buf());
                }
            }
        }
    }
}

/// Total size of a folder in bytes.
pub fn dir_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}
