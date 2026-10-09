//! The list of ArtCraft apps CraftSpace knows how to install.
//!
//! A copy of `catalog.json` is compiled in so the manager works offline. On refresh,
//! a newer catalog is fetched from the CraftSpace repository, so new apps can show up in
//! Discover without shipping a new manager build.

use serde::{Deserialize, Serialize};

/// The catalog compiled into this build.
pub const BUILTIN_CATALOG: &str = include_str!("../../../catalog.json");

/// Where an up-to-date catalog is published.
pub const REMOTE_CATALOG_URL: &str = "https://raw.githubusercontent.com/emircesur/craftspace/HEAD/catalog.json";

/// The highest catalog schema this build understands.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub schema: u32,
    /// Bumped on every change; a fetched catalog only replaces a newer built-in one if its
    /// revision is at least as high.
    #[serde(default)]
    pub revision: u32,
    pub apps: Vec<AppEntry>,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub links: Vec<Link>,
    /// Fonts and other extras installed outside the apps.
    #[serde(default)]
    pub addons: Vec<Addon>,
    /// The site whose news and tutorials Discover shows (it must publish a `sitemap.xml`).
    #[serde(default)]
    pub news_site: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Addon {
    pub id: String,
    pub name: String,
    pub kind: AddonKind,
    #[serde(default)]
    pub description: String,
    /// For fonts, the manifest (`family | style | file | … | sha256 | …` lines); for packs, a zip.
    pub source: String,
    /// What file paths in the manifest are relative to.
    #[serde(default)]
    pub base_url: Option<String>,
    /// SHA-256 of `source`, for packs.
    #[serde(default)]
    pub sha256: Option<String>,
    /// For packs: the app whose preferences folder receives the files.
    #[serde(default)]
    pub app: Option<String>,
    /// For packs: the subfolder of the app's preferences folder, e.g. `Presets`.
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AddonKind {
    Fonts,
    PresetPack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppEntry {
    /// Stable identifier, also the asset prefix and the desktop binary name.
    pub id: String,
    pub name: String,
    /// Two-letter badge shown on the app tile.
    pub code: String,
    /// GitHub `owner/name`.
    pub repo: String,
    pub category: String,
    pub tagline: String,
    #[serde(default)]
    pub description: String,
    /// The well-known app this one feels like, shown as a hint ("Like Photoshop").
    #[serde(default)]
    pub like: Option<String>,
    pub colors: Colors,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Name of the desktop executable, when it differs from `id`.
    #[serde(default)]
    pub binary: Option<String>,
    #[serde(default)]
    pub featured: bool,
    /// The app's own icon (PNG).
    #[serde(default)]
    pub icon: Option<String>,
    /// On Windows, which installer to prefer when there is no portable build: `exe` or `msi`.
    #[serde(default)]
    pub windows_installer: Option<String>,
    /// Name of the app's preferences folder (`%APPDATA%\<name>`, `~/.config/<lowercase>`,
    /// `~/Library/Application Support/<name>`), for add-on packs.
    #[serde(default)]
    pub config_dir: Option<String>,
    /// The app's AppStream metainfo (screenshots, description), when not at the ArtCraft path.
    #[serde(default)]
    pub metainfo: Option<String>,
    /// Added by the user from a GitHub repository (not part of the ArtCraft catalog).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Colors {
    pub bg: String,
    pub fg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub title: String,
    pub url: String,
}

impl AppEntry {
    pub fn binary(&self) -> &str {
        self.binary.as_deref().unwrap_or(&self.id)
    }

    pub fn github_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }

    pub fn homepage(&self) -> String {
        self.homepage.clone().unwrap_or_else(|| format!("https://getartcraft.com/apps/{}", self.id))
    }

    /// Where the app keeps its preferences, if the catalog says.
    pub fn config_dir(&self) -> Option<std::path::PathBuf> {
        let name = self.config_dir.as_ref()?;
        let base = directories::BaseDirs::new()?;
        Some(if cfg!(windows) || cfg!(target_os = "macos") {
            base.config_dir().join(name)
        } else {
            base.config_dir().join(name.to_lowercase())
        })
    }

    /// Whether this app is a good default for opening files with `ext` (no leading dot).
    pub fn handles_extension(&self, ext: &str) -> bool {
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }
}

impl Catalog {
    pub fn builtin() -> Catalog {
        Catalog::parse(BUILTIN_CATALOG).expect("built-in catalog.json is valid")
    }

    pub fn parse(json: &str) -> anyhow::Result<Catalog> {
        let catalog: Catalog = serde_json::from_str(json)?;
        anyhow::ensure!(
            catalog.schema <= SCHEMA,
            "catalog schema {} is newer than this CraftSpace understands ({SCHEMA}); update CraftSpace",
            catalog.schema
        );
        anyhow::ensure!(!catalog.apps.is_empty(), "catalog has no apps");
        Ok(catalog)
    }

    pub fn app(&self, id: &str) -> Option<&AppEntry> {
        self.apps.iter().find(|a| a.id.eq_ignore_ascii_case(id))
    }

    pub fn category_name(&self, id: &str) -> String {
        self.categories.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_else(|| id.to_string())
    }

    /// Apps able to open files with `ext`, best match first (catalog order).
    pub fn apps_for_extension(&self, ext: &str) -> Vec<&AppEntry> {
        self.apps.iter().filter(|a| a.handles_extension(ext)).collect()
    }

    /// Every extension any app in the catalog opens, lowercase.
    pub fn all_extensions(&self) -> Vec<String> {
        let mut all: Vec<String> =
            self.apps.iter().flat_map(|a| a.extensions.iter().map(|e| e.to_ascii_lowercase())).collect();
        all.sort();
        all.dedup();
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_parses() {
        let c = Catalog::builtin();
        assert!(c.app("photocraft").is_some());
        assert!(c.app("PhotoCraft").is_some());
        for app in &c.apps {
            assert_eq!(app.code.chars().count(), 2, "{} code", app.id);
            assert!(app.repo.contains('/'), "{} repo", app.id);
            assert!(
                c.categories.iter().any(|cat| cat.id == app.category),
                "{} has unknown category {}",
                app.id,
                app.category
            );
        }
    }

    #[test]
    fn extensions_map_to_apps() {
        let c = Catalog::builtin();
        assert_eq!(c.apps_for_extension("PSD")[0].id, "photocraft");
        assert_eq!(c.apps_for_extension("xlsx")[0].id, "gridcraft");
        assert!(c.apps_for_extension("nope").is_empty());
        assert!(c.all_extensions().contains(&"pdf".to_string()));
    }

    #[test]
    fn rejects_future_schema() {
        let json = r#"{"schema": 99, "apps": []}"#;
        assert!(Catalog::parse(json).is_err());
    }
}
