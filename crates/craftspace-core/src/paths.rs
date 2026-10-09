//! Where CraftSpace keeps its state, downloads and installed apps.
//!
//! | | Windows | Linux |
//! |---|---|---|
//! | root | `%LOCALAPPDATA%\CraftSpace` | `$XDG_DATA_HOME/craftspace` (`~/.local/share/craftspace`) |
//! | apps | `<root>\Apps\<app>\<version>` | `<root>/apps/<app>/<version>` |
//! | cache | `<root>\Cache` | `$XDG_CACHE_HOME/craftspace` |
//!
//! `CRAFTSPACE_HOME` overrides the root (and puts everything under it), which tests and
//! portable setups use.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub apps: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    pub fn detect() -> anyhow::Result<Paths> {
        if let Some(home) = std::env::var_os("CRAFTSPACE_HOME").filter(|v| !v.is_empty()) {
            return Ok(Paths::under(PathBuf::from(home)));
        }
        let base =
            directories::BaseDirs::new().ok_or_else(|| anyhow::anyhow!("could not determine the home directory"))?;
        if cfg!(windows) {
            let root = base.data_local_dir().join("CraftSpace");
            Ok(Paths { apps: root.join("Apps"), cache: root.join("Cache"), root })
        } else {
            let root = base.data_local_dir().join("craftspace");
            Ok(Paths { apps: root.join("apps"), cache: base.cache_dir().join("craftspace"), root })
        }
    }

    /// Everything under one directory.
    pub fn under(root: PathBuf) -> Paths {
        Paths { apps: root.join("apps"), cache: root.join("cache"), root }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn installed_file(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    pub fn downloads(&self) -> PathBuf {
        self.cache.join("downloads")
    }

    pub fn release_cache(&self) -> PathBuf {
        self.cache.join("releases")
    }

    pub fn catalog_cache(&self) -> PathBuf {
        self.cache.join("catalog.json")
    }

    pub fn app_dir(&self, apps_root: &Path, app_id: &str) -> PathBuf {
        apps_root.join(app_id)
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.root, &self.apps, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// Write `contents` to `path` via a temporary file and rename, so readers never see half a file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}
