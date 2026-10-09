//! Add-ons: preset packs, palettes, LUTs and plug-ins for the ArtCraft apps.
//!
//! They come from the CraftSpace add-on registry (`addons/registry.json` in the CraftSpace
//! repository, where anyone can submit one) and from other add-on stores, such as the
//! community ArtCraft Store. Each add-on says where its files go in each app:
//!
//! - `app:<root>/<folder>`: a folder the app reads by itself (e.g. VectorCraft's `Swatches`),
//!   found the same way as for workspace profiles (portable copies and Flatpak included);
//! - `plugins`: the app's plug-in folder (PhotoCraft and VectorCraft: the "additional plug-ins
//!   folder" setting, set to CraftSpace's own folder when none is chosen; EffectCraft: `Plug-ins`);
//! - `clap`, `vst3`, `au`: the per-user audio plug-in folders SoundCraft (and other hosts) scan;
//! - `library`: `Documents/CraftSpace Add-ons/<name>`, for content the app imports itself
//!   (optionally opened in the app, which imports it).
//!
//! Only add-ons made and reviewed by CraftSpace are "checked". Everything else, including
//! open-source plug-ins pinned in the registry and every store's add-ons, is shown as not
//! checked for security and installed only after the person agrees.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::platform::{Os, Platform};

/// The registry, as published (raw file in the CraftSpace repository).
pub const REGISTRY_URL: &str = "https://raw.githubusercontent.com/emircesur/CRAFTSPACE/main/addons/registry.json";
/// Where people submit add-ons.
pub const SUBMIT_URL: &str = "https://github.com/emircesur/CRAFTSPACE/blob/main/addons/README.md";
/// The registry this build ships with, for offline use.
pub const BUILTIN_REGISTRY: &str = include_str!("../../../addons/registry.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trust {
    /// Made or reviewed by CraftSpace.
    Checked,
    /// Not reviewed for security; installed only after the person agrees.
    #[default]
    Unchecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Presets, palettes, LUTs, templates.
    #[default]
    Pack,
    /// An app plug-in (WebAssembly for PhotoCraft, VectorCraft and EffectCraft) or script.
    Plugin,
    /// CLAP, VST3 or Audio Units plug-ins for SoundCraft.
    AudioPlugin,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Pack => "Presets & content",
            Kind::Plugin => "Plug-in",
            Kind::AudioPlugin => "Audio plug-in",
        }
    }
}

/// One download; `platform` (`linux-x86_64`, `windows-x64`, `macos`, …) is absent when it fits
/// every computer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct File {
    #[serde(default)]
    pub platform: Option<String>,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Where (some of) an add-on's files go for one app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub app: String,
    pub to: String,
    /// Paths in the download (`*` matches within a folder); empty means everything.
    #[serde(default)]
    pub files: Vec<String>,
    /// Open the files in the app afterwards, so it imports them.
    #[serde(default)]
    pub open: bool,
    /// How to use them in the app.
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Addon {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub trust: Trust,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub install: Vec<Step>,
    /// Where it was listed: the registry, or a store's name.
    #[serde(default)]
    pub source: String,
}

impl Addon {
    /// The download for this computer.
    pub fn file_for(&self, platform: Platform) -> Option<&File> {
        let token = platform.token();
        let fits = |p: &str| {
            Some(p) == token
                || (p == "macos" && platform.os == Os::Macos)
                || (p == "linux" && platform.os == Os::Linux)
                || (p == "windows" && platform.os == Os::Windows)
        };
        self.files
            .iter()
            .find(|f| f.platform.as_deref().is_some_and(fits))
            .or_else(|| self.files.iter().find(|f| f.platform.is_none()))
    }

    pub fn checked(&self) -> bool {
        self.trust == Trust::Checked
    }
}

/// Another add-on store: a catalog in the CraftSpace registry format, or in the ArtCraft Store
/// format (`plugins` with `downloadUrl` / `releaseAsset` / `artifact`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub description: String,
}

/// A place with plug-ins CraftSpace lists but doesn't install.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repository {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub addons: Vec<Addon>,
    #[serde(default)]
    pub stores: Vec<Store>,
    #[serde(default)]
    pub repositories: Vec<Repository>,
}

impl Registry {
    pub fn builtin() -> Registry {
        parse_registry(BUILTIN_REGISTRY, "CraftSpace").expect("the built-in add-on registry is valid")
    }
}

pub fn parse_registry(text: &str, source: &str) -> anyhow::Result<Registry> {
    let mut reg: Registry = serde_json::from_str(text)?;
    anyhow::ensure!(reg.format == "craftspace-addons", "not a CraftSpace add-on registry");
    for a in &mut reg.addons {
        a.source = source.to_string();
    }
    Ok(reg)
}

/// A store's add-ons. Nothing from a store counts as checked, whatever it says.
pub fn parse_store(store: &Store, text: &str) -> anyhow::Result<Vec<Addon>> {
    let value: Value = serde_json::from_str(text)?;
    let mut addons = if value.get("format").and_then(Value::as_str) == Some("craftspace-addons") {
        parse_registry(text, &store.name)?.addons
    } else if let Some(plugins) = value.get("plugins").and_then(Value::as_array) {
        plugins.iter().filter_map(|p| artcraft_store_entry(store, p)).collect()
    } else {
        anyhow::bail!("{} isn't an add-on catalog CraftSpace can read", store.url);
    };
    for a in &mut addons {
        a.trust = Trust::Unchecked;
        a.source = store.name.clone();
        a.id = format!("{}/{}", store.id, a.id.trim_start_matches(&format!("{}/", store.id)));
    }
    Ok(addons)
}

/// One ArtCraft Store listing (see its `api/v1/README.md`).
fn artcraft_store_entry(store: &Store, p: &Value) -> Option<Addon> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
    let id = s("id")?;
    let app = s("app")?;
    let kind = s("kind").unwrap_or_default();
    let root = store.url.rsplit_once('/').map(|(r, _)| r.to_string()).unwrap_or_default();
    let url = match (s("downloadUrl"), s("releaseAsset").or_else(|| s("artifact"))) {
        (Some(u), _) if u.starts_with("https://") => u,
        (Some(u), _) => format!("{root}/{}", u.trim_start_matches("./").trim_start_matches('/')),
        (None, Some(asset)) => format!("{root}/downloads/{asset}"),
        (None, None) => return None,
    };
    // PhotoCraft plug-ins are WebAssembly; After Effects scripts run in EffectCraft.
    let (apps, step) = match app.as_str() {
        "photocraft" if url.ends_with(".wasm") => (
            vec!["photocraft".to_string()],
            Step { app: "photocraft".into(), to: "plugins".into(), files: vec![], open: false, hint: None },
        ),
        "after-effects" if kind == "script" && (url.ends_with(".jsx") || url.ends_with(".js")) => (
            vec!["effectcraft".to_string()],
            Step {
                app: "effectcraft".into(),
                to: "app:config/Scripts".into(),
                files: vec![],
                open: false,
                hint: Some("Made for After Effects; EffectCraft runs After Effects scripts (File › Scripts).".into()),
            },
        ),
        _ => return None,
    };
    let tags = p
        .get("tags")
        .and_then(Value::as_array)
        .map(|t| t.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let homepage = s("sourceUrl").or_else(|| {
        let source = s("source")?;
        let home = store.homepage.clone()?;
        Some(format!("{home}/tree/main/{source}"))
    });
    Some(Addon {
        id,
        name: s("name")?,
        kind: Kind::Plugin,
        version: s("version"),
        author: s("author"),
        license: s("license"),
        description: [
            s("description").unwrap_or_default(),
            s("compatibility").map(|c| format!("({c})")).unwrap_or_default(),
        ]
        .join(" ")
        .trim()
        .to_string(),
        apps,
        tags,
        homepage,
        trust: Trust::Unchecked,
        files: vec![File { platform: None, url, sha256: s("sha256") }],
        install: vec![step],
        source: store.name.clone(),
    })
}

// ---- installing --------------------------------------------------------------------------------

/// `*` matches within one path segment; `**` matches any number of segments.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn seg(p: &str, s: &str) -> bool {
        let (pb, sb) = (p.as_bytes(), s.as_bytes());
        let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
        while si < sb.len() {
            if pi < pb.len() && (pb[pi] == b'?' || pb[pi].eq_ignore_ascii_case(&sb[si])) {
                pi += 1;
                si += 1;
            } else if pi < pb.len() && pb[pi] == b'*' {
                star = Some(pi);
                mark = si;
                pi += 1;
            } else if let Some(st) = star {
                pi = st + 1;
                mark += 1;
                si = mark;
            } else {
                return false;
            }
        }
        pb[pi..].iter().all(|&c| c == b'*')
    }
    fn parts(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => parts(&p[1..], s) || (!s.is_empty() && parts(p, &s[1..])),
            (Some(a), Some(b)) => seg(a, b) && parts(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    parts(&p, &s)
}

/// Files under `root` (relative, `/`-separated) matching `patterns` (all when empty).
pub fn matching_files(root: &Path, patterns: &[String]) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).min_depth(1).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else { continue };
        let rel = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        if patterns.is_empty() || patterns.iter().any(|p| glob_match(p, &rel)) {
            out.push((rel, entry.into_path()));
        }
    }
    out.sort();
    out
}

/// Plug-in bundles in `root` with this extension (`clap`, `vst3`, `component`): files on Linux and
/// Windows, folders on macOS. Not looked for inside another bundle.
pub fn bundles(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut walker = walkdir::WalkDir::new(root).min_depth(1).into_iter();
    while let Some(Ok(entry)) = walker.next() {
        let is_bundle = entry.path().extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
        if is_bundle {
            out.push(entry.path().to_path_buf());
            if entry.file_type().is_dir() {
                walker.skip_current_dir();
            }
        }
    }
    out.sort();
    out
}

/// The per-user folder hosts scan for this kind of audio plug-in, if this system has one.
pub fn audio_plugin_dir(to: &str, os: Os) -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let home = base.home_dir();
    Some(match (to, os) {
        ("clap", Os::Linux) => home.join(".clap"),
        ("vst3", Os::Linux) => home.join(".vst3"),
        ("clap", Os::Windows) => base.data_local_dir().join("Programs").join("Common").join("CLAP"),
        ("vst3", Os::Windows) => base.data_local_dir().join("Programs").join("Common").join("VST3"),
        ("clap", Os::Macos) => home.join("Library/Audio/Plug-Ins/CLAP"),
        ("vst3", Os::Macos) => home.join("Library/Audio/Plug-Ins/VST3"),
        ("au", Os::Macos) => home.join("Library/Audio/Plug-Ins/Components"),
        _ => return None,
    })
}

/// Copy a file or a folder (a bundle) to `dest`, replacing what's there.
pub fn copy_any(src: &Path, dest: &Path) -> anyhow::Result<()> {
    if dest.is_dir() {
        std::fs::remove_dir_all(dest)?;
    } else if dest.exists() {
        std::fs::remove_file(dest)?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if src.is_dir() {
        for entry in walkdir::WalkDir::new(src).into_iter().filter_map(Result::ok) {
            let rel = entry.path().strip_prefix(src)?;
            let target = dest.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target)?;
            } else if entry.file_type().is_symlink() {
                #[cfg(unix)]
                {
                    let link = std::fs::read_link(entry.path())?;
                    let _ = std::fs::remove_file(&target);
                    std::os::unix::fs::symlink(link, &target)?;
                }
            } else {
                std::fs::copy(entry.path(), &target)?;
            }
        }
    } else {
        std::fs::copy(src, dest)?;
    }
    Ok(())
}

/// `id` as a file name.
pub fn slug(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '-' }).collect()
}

/// A display name as a folder name (no path separators or characters Windows refuses).
pub fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '-' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "Add-on".into()
    } else {
        s
    }
}

/// What CraftSpace installed for an add-on, for removing it later.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub source: String,
    /// Files and bundle folders written.
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    /// Unix seconds.
    #[serde(default)]
    pub installed_at: u64,
}

/// What happened, for people.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub written: Vec<PathBuf>,
    /// Per app: where things went and how to use them.
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_registry_is_complete() {
        let reg = Registry::builtin();
        assert!(!reg.addons.is_empty());
        let catalog = crate::catalog::Catalog::builtin();
        for a in &reg.addons {
            assert!(!a.files.is_empty(), "{}", a.id);
            for f in &a.files {
                assert!(f.url.starts_with("https://"), "{}", a.id);
                let sha = f.sha256.as_deref().unwrap_or_default();
                assert!(
                    sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{} needs a pinned SHA-256",
                    a.id
                );
            }
            for step in &a.install {
                assert!(catalog.app(&step.app).is_some(), "{}: unknown app {}", a.id, step.app);
                let known = step.to == "library"
                    || step.to == "plugins"
                    || ["clap", "vst3", "au"].contains(&step.to.as_str())
                    || step.to.strip_prefix("app:").is_some_and(|t| {
                        let root = t.split('/').next().unwrap_or_default();
                        crate::profiles::spec(&step.app).is_some_and(|s| s.roots.iter().any(|r| r.key == root))
                    });
                assert!(known, "{}: unknown target {}", a.id, step.to);
            }
        }
        // Only CraftSpace's own packs are checked.
        assert!(reg.addons.iter().filter(|a| a.checked()).all(|a| a.author.as_deref() == Some("CraftSpace")));
    }

    #[test]
    fn the_packs_in_the_repository_match_the_registry() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../addons/packs");
        for a in Registry::builtin().addons.iter().filter(|a| a.checked()) {
            for f in &a.files {
                let name = f.url.rsplit('/').next().unwrap();
                let sha = crate::download::sha256_file(&dir.join(name)).unwrap();
                assert_eq!(Some(sha), f.sha256, "{name}");
            }
        }
    }

    #[test]
    fn artcraft_store_listings_become_unchecked_plugins() {
        let store = Store {
            id: "artcraft-store".into(),
            name: "ArtCraft Store".into(),
            url: "https://akkk09.github.io/artcraft-store/catalog.json".into(),
            homepage: Some("https://github.com/akkk09/artcraft-store".into()),
            description: String::new(),
        };
        let text = r#"{"schemaVersion":1,"apps":[],"plugins":[
            {"id":"org.photocraft.community.vignette","name":"Vignette","version":"0.1.0","author":"x","kind":"filter","abi":1,
             "source":"plugins/vignette","artifact":"photocraft_plugin_vignette.wasm","releaseAsset":"photocraft_plugin_vignette.wasm","app":"photocraft","compatibility":"PhotoCraft ABI v1","tags":["vignette"]},
            {"id":"x.ae","name":"AE thing","kind":"plugin","app":"after-effects","downloadUrl":"https://example.com/a.aex"},
            {"id":"x.script","name":"Rename layers","kind":"script","app":"after-effects","downloadUrl":"scripts/rename.jsx"}]}"#;
        let addons = parse_store(&store, text).unwrap();
        assert_eq!(addons.len(), 2);
        let v = &addons[0];
        assert_eq!(v.id, "artcraft-store/org.photocraft.community.vignette");
        assert_eq!(v.files[0].url, "https://akkk09.github.io/artcraft-store/downloads/photocraft_plugin_vignette.wasm");
        assert_eq!((v.trust, v.kind, v.install[0].to.as_str()), (Trust::Unchecked, Kind::Plugin, "plugins"));
        assert_eq!(v.homepage.as_deref(), Some("https://github.com/akkk09/artcraft-store/tree/main/plugins/vignette"));
        assert_eq!(addons[1].files[0].url, "https://akkk09.github.io/artcraft-store/scripts/rename.jsx");
        assert_eq!(addons[1].apps, ["effectcraft"]);
        // A store can't vouch for itself.
        let fake = r#"{"format":"craftspace-addons","version":1,"addons":[{"id":"a","name":"A","trust":"checked","files":[{"url":"https://x/a.zip"}]}]}"#;
        assert_eq!(parse_store(&store, fake).unwrap()[0].trust, Trust::Unchecked);
    }

    #[test]
    fn globs() {
        assert!(glob_match("GIMP palettes/*.gpl", "GIMP palettes/Earth.gpl"));
        assert!(!glob_match("GIMP palettes/*.gpl", "GIMP palettes/sub/Earth.gpl"));
        assert!(glob_match("**/*.cube", "a/b/Vivid.cube"));
        assert!(glob_match("*.lcpreset", "CraftSpace Presets.lcpreset"));
        assert!(!glob_match("*.cube", "a/Vivid.cube"));
    }

    #[test]
    fn files_for_each_computer() {
        let reg = Registry::builtin();
        let dexed = reg.addons.iter().find(|a| a.id == "dexed").unwrap();
        let mac = Platform { os: Os::Macos, arch: crate::platform::Arch::Arm64 };
        assert!(dexed.file_for(mac).unwrap().url.ends_with("macOS.zip"));
        let linux = Platform { os: Os::Linux, arch: crate::platform::Arch::X64 };
        assert!(dexed.file_for(linux).unwrap().url.ends_with("lnx.zip"));
        let arm = Platform { os: Os::Linux, arch: crate::platform::Arch::Arm64 };
        assert!(dexed.file_for(arm).is_none());
    }

    #[test]
    fn bundles_are_found_without_looking_inside_them() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("x/Synth.vst3/Contents/x86_64-linux")).unwrap();
        std::fs::write(root.join("x/Synth.vst3/Contents/x86_64-linux/Synth.so"), b"so").unwrap();
        std::fs::write(root.join("x/Synth.clap"), b"clap").unwrap();
        std::fs::write(root.join("x/Synth-vst.so"), b"vst2").unwrap();
        assert_eq!(bundles(root, "vst3"), [root.join("x/Synth.vst3")]);
        assert_eq!(bundles(root, "clap"), [root.join("x/Synth.clap")]);
        let dest = tmp.path().join("out/Synth.vst3");
        copy_any(&root.join("x/Synth.vst3"), &dest).unwrap();
        assert!(dest.join("Contents/x86_64-linux/Synth.so").is_file());
    }
}
