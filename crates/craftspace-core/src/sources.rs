//! Optional: apps from any GitHub repository, and other repositories (forks, mirrors, backups) to
//! update the ArtCraft apps and CraftSpace itself from. Off by default (Settings › Other sources),
//! so CraftSpace stays an ArtCraft app manager unless someone asks for more.

use semver::Version;

use crate::catalog::{AppEntry, Catalog, Category, Colors};
use crate::platform::AssetKind;
use crate::settings::{CustomApp, OtherSources};

/// The category apps added from GitHub are listed under.
pub const CATEGORY: &str = "github";

/// `owner/repo` from what someone typed: `owner/repo`, a github.com URL, with or without `.git`.
pub fn normalize_repo(input: &str) -> anyhow::Result<String> {
    let s = input.trim().trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let s = ["https://github.com/", "http://github.com/", "github.com/", "git@github.com:"]
        .iter()
        .find_map(|p| s.strip_prefix(p))
        .unwrap_or(s);
    let mut parts = s.split('/');
    let (owner, repo) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    anyhow::ensure!(
        ok(owner) && ok(repo) && !owner.starts_with('.') && !repo.starts_with('.'),
        "\"{input}\" isn't a GitHub repository; write it as owner/repo or paste its github.com address"
    );
    Ok(format!("{owner}/{repo}"))
}

/// An id for a new app from `repo`, not taken by `taken`.
pub fn new_id(repo: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let (owner, name) = repo.split_once('/').unwrap_or(("", repo));
    let clean = |s: &str| {
        s.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>()
    };
    let base = clean(name);
    if !taken(&base) {
        return base;
    }
    let with_owner = format!("{base}-{}", clean(owner));
    if !taken(&with_owner) {
        return with_owner;
    }
    (2..).map(|n| format!("{with_owner}-{n}")).find(|id| !taken(id)).expect("some number is free")
}

/// A readable name from a repository name: `my-cool_tool` → "My Cool Tool".
pub fn name_from_repo(repo: &str) -> String {
    let name = repo.rsplit('/').next().unwrap_or(repo);
    name.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The catalog entry for an app added from GitHub.
pub fn custom_entry(app: &CustomApp) -> AppEntry {
    let owner = app.repo.split('/').next().unwrap_or_default();
    let code: String = app.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
    // A stable color per repository.
    let hash = app.repo.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    let palette = ["#3B82F6", "#10B981", "#F59E0B", "#EF4444", "#8B5CF6", "#EC4899", "#14B8A6", "#F97316"];
    AppEntry {
        id: app.id.clone(),
        name: app.name.clone(),
        code: if code.is_empty() { "Gh".into() } else { code },
        repo: app.repo.clone(),
        category: CATEGORY.into(),
        tagline: format!("From github.com/{}", app.repo),
        description: String::new(),
        like: None,
        colors: Colors { bg: palette[(hash % palette.len() as u32) as usize].into(), fg: "#FFFFFF".into() },
        extensions: Vec::new(),
        homepage: Some(format!("https://github.com/{}", app.repo)),
        binary: app.binary.clone().filter(|b| !b.trim().is_empty()),
        featured: false,
        icon: (!owner.is_empty()).then(|| format!("https://github.com/{owner}.png?size=128")),
        windows_installer: None,
        config_dir: None,
        metainfo: None,
        custom: true,
    }
}

/// Apply the other sources to the catalog: other repositories for ArtCraft apps, and the apps
/// added from GitHub. Nothing changes while they're turned off.
pub fn apply(catalog: &mut Catalog, other: &OtherSources) {
    if !other.enabled {
        return;
    }
    for app in &mut catalog.apps {
        if let Some(repo) = other.overrides.get(&app.id) {
            app.repo.clone_from(repo);
        }
    }
    for custom in &other.apps {
        if !catalog.apps.iter().any(|a| a.id == custom.id) {
            catalog.apps.push(custom_entry(custom));
        }
    }
    if !other.apps.is_empty() && !catalog.categories.iter().any(|c| c.id == CATEGORY) {
        catalog.categories.push(Category { id: CATEGORY.into(), name: "From GitHub".into() });
    }
}

/// What a repository offers this computer: its latest release and the file CraftSpace would
/// install from it.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceCheck {
    pub repo: String,
    pub tag: String,
    pub version: Option<Version>,
    pub asset: String,
    pub kind: AssetKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repositories_are_normalized() {
        for input in [
            "BurntSushi/ripgrep",
            " https://github.com/BurntSushi/ripgrep/ ",
            "github.com/BurntSushi/ripgrep.git",
            "git@github.com:BurntSushi/ripgrep.git",
            "https://github.com/BurntSushi/ripgrep/releases",
        ] {
            assert_eq!(normalize_repo(input).unwrap(), "BurntSushi/ripgrep", "{input}");
        }
        for bad in ["ripgrep", "https://gitlab.com/x", "a b/c", "/x", "owner/", "../etc"] {
            assert!(normalize_repo(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn custom_apps_join_the_catalog_only_when_turned_on() {
        let mut other = OtherSources::default();
        other.apps.push(CustomApp {
            id: "ripgrep".into(),
            name: name_from_repo("BurntSushi/ripgrep"),
            repo: "BurntSushi/ripgrep".into(),
            binary: Some("rg".into()),
        });
        other.overrides.insert("photocraft".into(), "someone/photocraft-fork".into());

        let mut off = Catalog::builtin();
        apply(&mut off, &other);
        assert!(off.app("ripgrep").is_none());
        assert_eq!(off.app("photocraft").unwrap().repo, "storytold/photocraft");

        other.enabled = true;
        let mut on = Catalog::builtin();
        apply(&mut on, &other);
        let rg = on.app("ripgrep").unwrap();
        assert!(rg.custom);
        assert_eq!((rg.name.as_str(), rg.binary(), rg.category.as_str()), ("Ripgrep", "rg", CATEGORY));
        assert_eq!(on.app("photocraft").unwrap().repo, "someone/photocraft-fork");
        assert!(on.categories.iter().any(|c| c.id == CATEGORY));
    }

    #[test]
    fn ids_avoid_existing_apps() {
        let taken = |id: &str| id == "photocraft" || id == "photocraft-someone";
        assert_eq!(new_id("someone/PhotoCraft", &taken), "photocraft-someone-2");
        assert_eq!(new_id("BurntSushi/ripgrep", &taken), "ripgrep");
        assert_eq!(name_from_repo("me/my-cool_tool"), "My Cool Tool");
    }
}
