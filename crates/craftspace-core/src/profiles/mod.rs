//! Workspace sync: an app's layouts, shortcuts, preferences and presets, carried to another
//! computer (or a whole classroom) as one `.craftprofile` file.
//!
//! Each ArtCraft app keeps these in its own files; [`spec`] says which files, which keys in them
//! are which part, and which values belong to the computer they're on (recent files, window
//! positions, GPU and audio devices, folder paths) and so never travel. Account sign-ins, API keys
//! and digital IDs are never in a profile: only the files a spec names are read.
//!
//! A profile is a zip: `profile.json` (what's inside) and `files/<root>/<path>`. Importing merges
//! the chosen parts into the app's files, keeping this computer's own values, after saving the
//! current settings as a profile to go back to.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const EXTENSION: &str = "craftprofile";
const FORMAT: &str = "craftspace-profile";
const MANIFEST: &str = "profile.json";
/// Files bigger than this aren't settings.
const MAX_FILE: u64 = 64 << 20;

/// The parts of an app's setup a profile can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Part {
    /// Workspaces, panel and toolbar layouts.
    Layouts,
    Shortcuts,
    Preferences,
    /// Presets, brushes, swatches, templates, scripts and plug-ins the user added.
    Presets,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Layouts, Part::Shortcuts, Part::Preferences, Part::Presets];

    pub fn label(self) -> &'static str {
        match self {
            Part::Layouts => "Workspaces and layouts",
            Part::Shortcuts => "Keyboard shortcuts",
            Part::Preferences => "Preferences",
            Part::Presets => "Presets, swatches, templates and plug-ins",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Part> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "layouts" | "layout" | "workspaces" | "workspace" => Part::Layouts,
            "shortcuts" | "keys" | "keyboard" => Part::Shortcuts,
            "preferences" | "prefs" | "settings" => Part::Preferences,
            "presets" | "content" => Part::Presets,
            other => anyhow::bail!("unknown part \"{other}\" (layouts, shortcuts, preferences, presets)"),
        })
    }

    /// `layouts,shortcuts` → parts; empty → all.
    pub fn parse_list(s: &str) -> anyhow::Result<Vec<Part>> {
        let parts: Vec<Part> =
            s.split(',').filter(|p| !p.trim().is_empty()).map(Part::parse).collect::<Result<_, _>>()?;
        Ok(if parts.is_empty() { Part::ALL.to_vec() } else { parts })
    }
}

// ---- what each app keeps where ----------------------------------------------------------------

/// The folder a root's name is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// `%APPDATA%`, `~/Library/Application Support`, `$XDG_CONFIG_HOME`.
    Config,
    /// `%APPDATA%`, `~/Library/Application Support`, `$XDG_DATA_HOME`.
    Data,
    Home,
}

/// One folder an app keeps settings in.
#[derive(Debug, Clone, Copy)]
pub struct RootSpec {
    pub key: &'static str,
    pub base: Base,
    /// Its name under `base`: Windows, macOS, Linux.
    pub names: [&'static str; 3],
    /// An environment variable the app reads to move it.
    pub env: Option<&'static str>,
    /// In portable mode (a `portable.txt` or `<Name>.portable` beside the program), this folder
    /// beside the program instead.
    pub portable: Option<&'static str>,
    /// A setting that moves it: (root, file, JSON pointer), e.g. LightCraft's library path.
    pub setting: Option<(&'static str, &'static str, &'static str)>,
}

#[derive(Debug, Clone, Copy)]
pub enum What {
    /// Every file in a folder.
    Dir(Part),
    File(Part),
    /// A JSON file whose keys belong to different parts (`keys`: top-level names, or JSON
    /// pointers for values inside one; the rest are `default`'s). `machine` are JSON pointers
    /// (`*` for every element) that stay on each computer.
    Json {
        default: Part,
        keys: &'static [(Part, &'static [&'static str])],
        machine: &'static [&'static str],
    },
}

#[derive(Debug, Clone, Copy)]
pub struct ItemSpec {
    pub root: &'static str,
    pub path: &'static str,
    pub what: What,
}

impl ItemSpec {
    fn parts(&self) -> Vec<Part> {
        match self.what {
            What::Dir(p) | What::File(p) => vec![p],
            What::Json { default, keys, .. } => {
                let mut parts: BTreeSet<Part> = keys.iter().map(|(p, _)| *p).collect();
                parts.insert(default);
                parts.into_iter().collect()
            }
        }
    }

    fn part_of_key(&self, key: &str) -> Part {
        match self.what {
            What::Json { default, keys, .. } => {
                keys.iter().find(|(_, ks)| ks.contains(&key)).map(|(p, _)| *p).unwrap_or(default)
            }
            What::Dir(p) | What::File(p) => p,
        }
    }

    /// Values inside a top-level key that belong to their own part: (pointer, part).
    fn nested_units(&self, key: &str) -> Vec<(&'static str, Part)> {
        let What::Json { keys, .. } = self.what else { return Vec::new() };
        let prefix = format!("/{}/", escape_pointer(key));
        keys.iter()
            .flat_map(|(part, ks)| ks.iter().filter(|k| k.starts_with(&prefix)).map(move |k| (*k, *part)))
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AppSpec {
    pub app: &'static str,
    pub roots: &'static [RootSpec],
    pub items: &'static [ItemSpec],
    /// Something to know about this app's profiles.
    pub note: Option<&'static str>,
}

impl AppSpec {
    /// The parts this app has.
    pub fn parts(&self) -> Vec<Part> {
        let set: BTreeSet<Part> = self.items.iter().flat_map(ItemSpec::parts).collect();
        set.into_iter().collect()
    }
}

/// Where an app is, for finding its folders.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The app's programs (for portable mode).
    pub programs: Vec<PathBuf>,
    /// The Flatpak id, when installed through Flatpak (settings are under `~/.var/app/<id>`).
    pub flatpak: Option<String>,
    /// The app's display name (for `<Name>.portable`).
    pub name: String,
}

fn os_index() -> usize {
    if cfg!(windows) {
        0
    } else if cfg!(target_os = "macos") {
        1
    } else {
        2
    }
}

/// The folder `root` is in on this computer.
pub fn root_dir(root: &RootSpec, ctx: &Context) -> Option<PathBuf> {
    if let Some(var) = root.env {
        if let Some(dir) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(dir));
        }
    }
    if let Some(folder) = root.portable {
        for program in &ctx.programs {
            let Some(dir) = program.parent() else { continue };
            let marked = dir.join("portable.txt").is_file() || dir.join(format!("{}.portable", ctx.name)).is_file();
            if marked {
                return Some(dir.join(folder));
            }
        }
    }
    let dirs = directories::BaseDirs::new()?;
    let base = match (&ctx.flatpak, root.base) {
        (Some(id), Base::Config) => dirs.home_dir().join(".var/app").join(id).join("config"),
        (Some(id), Base::Data) => dirs.home_dir().join(".var/app").join(id).join("data"),
        (None, Base::Config) => dirs.config_dir().to_path_buf(),
        (None, Base::Data) => dirs.data_dir().to_path_buf(),
        (_, Base::Home) => dirs.home_dir().to_path_buf(),
    };
    Some(base.join(root.names[os_index()]))
}

fn roots(spec: &AppSpec, ctx: &Context) -> Vec<(&'static str, PathBuf)> {
    let mut out: Vec<(&'static str, PathBuf)> = Vec::new();
    for root in spec.roots {
        let env_set = root.env.is_some_and(|v| std::env::var_os(v).is_some_and(|v| !v.is_empty()));
        let from_setting = root.setting.filter(|_| !env_set).and_then(|(key, file, pointer)| {
            let dir = &out.iter().find(|(k, _)| *k == key)?.1;
            let value: Value = serde_json::from_slice(&std::fs::read(dir.join(file)).ok()?).ok()?;
            value.pointer(pointer)?.as_str().filter(|s| !s.is_empty()).map(PathBuf::from)
        });
        if let Some(dir) = from_setting.or_else(|| root_dir(root, ctx)) {
            out.push((root.key, dir));
        }
    }
    out
}

// ---- the profile file -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub app: String,
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub app_version: Option<String>,
    /// Unix seconds.
    pub created: u64,
    #[serde(default)]
    pub os: String,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestFile {
    pub root: String,
    pub path: String,
}

#[derive(Debug, Default)]
pub struct ExportReport {
    pub files: usize,
    pub parts: Vec<Part>,
}

/// Write the chosen parts of an app's setup to `out`.
pub fn export(
    spec: &AppSpec,
    ctx: &Context,
    parts: &[Part],
    app_version: Option<String>,
    out: &Path,
) -> anyhow::Result<ExportReport> {
    let roots = roots(spec, ctx);
    let want: BTreeSet<Part> = parts.iter().copied().collect();
    let mut entries: Vec<(ManifestFile, Vec<u8>)> = Vec::new();
    for item in spec.items {
        let Some(root) = roots.iter().find(|(k, _)| *k == item.root).map(|(_, d)| d) else { continue };
        let path = root.join(item.path);
        match item.what {
            What::Dir(part) if want.contains(&part) && path.is_dir() => {
                for entry in walkdir::WalkDir::new(&path).max_depth(8).into_iter().filter_map(Result::ok) {
                    if !entry.file_type().is_file() || entry.metadata().map(|m| m.len() > MAX_FILE).unwrap_or(true) {
                        continue;
                    }
                    let rel = entry.path().strip_prefix(root)?;
                    entries.push((manifest_file(item.root, rel), std::fs::read(entry.path())?));
                }
            }
            What::File(part) if want.contains(&part) && path.is_file() => {
                entries.push((manifest_file(item.root, Path::new(item.path)), std::fs::read(&path)?));
            }
            What::Json { machine, .. } if path.is_file() => {
                let text = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
                let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&text) else {
                    log::warn!("{} isn't a JSON object; left out", path.display());
                    continue;
                };
                let mut value = Value::Object(map);
                for pointer in machine {
                    remove_pointer(&mut value, pointer);
                }
                keep_parts(item, &mut value, &want);
                if value.as_object().is_some_and(Map::is_empty) {
                    continue;
                }
                entries.push((manifest_file(item.root, Path::new(item.path)), serde_json::to_vec_pretty(&value)?));
            }
            _ => {}
        }
    }
    anyhow::ensure!(
        !entries.is_empty(),
        "there's nothing of {} to save yet on this computer",
        if spec.app.is_empty() { "this app" } else { spec.app }
    );

    let parts_in: Vec<Part> = Part::ALL.into_iter().filter(|p| want.contains(p)).collect();
    let manifest = Manifest {
        format: FORMAT.into(),
        version: 1,
        app: spec.app.into(),
        app_name: ctx.name.clone(),
        app_version,
        created: crate::github::now_secs(),
        os: std::env::consts::OS.into(),
        parts: parts_in.clone(),
        files: entries.iter().map(|(m, _)| m.clone()).collect(),
    };
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = out.with_extension("craftprofile.tmp");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(MANIFEST, opts)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        for (file, bytes) in &entries {
            zip.start_file(format!("files/{}/{}", file.root, file.path), opts)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
    }
    std::fs::rename(&tmp, out)?;
    Ok(ExportReport { files: entries.len(), parts: parts_in })
}

fn manifest_file(root: &str, rel: &Path) -> ManifestFile {
    let path = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
    ManifestFile { root: root.into(), path }
}

/// What a profile holds.
pub fn read_manifest(file: &Path) -> anyhow::Result<Manifest> {
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(file).with_context(|| format!("opening {}", file.display()))?)
            .with_context(|| format!("{} isn't a CraftSpace profile", file.display()))?;
    let mut text = String::new();
    zip.by_name(MANIFEST)
        .with_context(|| format!("{} isn't a CraftSpace profile", file.display()))?
        .read_to_string(&mut text)?;
    let manifest: Manifest = serde_json::from_str(&text).context("the profile's description is damaged")?;
    anyhow::ensure!(manifest.format == FORMAT, "{} isn't a CraftSpace profile", file.display());
    anyhow::ensure!(manifest.version <= 1, "this profile needs a newer CraftSpace");
    Ok(manifest)
}

#[derive(Debug, Default)]
pub struct ImportReport {
    /// Files written or merged.
    pub changed: Vec<PathBuf>,
    pub parts: Vec<Part>,
}

/// Merge the chosen parts of a profile into the app's files on this computer. The app must be
/// closed (it would overwrite them when it quits).
pub fn import(spec: &AppSpec, ctx: &Context, file: &Path, parts: &[Part]) -> anyhow::Result<ImportReport> {
    let manifest = read_manifest(file)?;
    anyhow::ensure!(
        manifest.app == spec.app,
        "this profile is for {}, not {}",
        if manifest.app_name.is_empty() { &manifest.app } else { &manifest.app_name },
        ctx.name
    );
    let want: BTreeSet<Part> = parts.iter().copied().filter(|p| manifest.parts.contains(p)).collect();
    anyhow::ensure!(!want.is_empty(), "the profile has none of the chosen parts");
    let roots = roots(spec, ctx);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file)?)?;
    let mut report = ImportReport { parts: want.iter().copied().collect(), ..Default::default() };
    for entry in &manifest.files {
        let Some(item) = spec.items.iter().find(|i| i.root == entry.root && covers(i, &entry.path)) else {
            log::warn!("{}/{}: not a settings file of this app; skipped", entry.root, entry.path);
            continue;
        };
        let Some(root) = roots.iter().find(|(k, _)| *k == entry.root).map(|(_, d)| d) else { continue };
        let rel = safe_relative(&entry.path)?;
        let target = root.join(&rel);
        let mut bytes = Vec::new();
        zip.by_name(&format!("files/{}/{}", entry.root, entry.path))?.read_to_end(&mut bytes)?;
        let written = match item.what {
            What::Dir(part) | What::File(part) => {
                if !want.contains(&part) {
                    continue;
                }
                bytes
            }
            What::Json { machine, .. } => {
                let incoming: Value =
                    serde_json::from_slice(&bytes).context("a settings file in the profile is damaged")?;
                let existing = std::fs::read(&target).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                let Some(merged) = merge_json(item, machine, &incoming, existing.as_ref(), &want) else { continue };
                serde_json::to_vec_pretty(&merged)?
            }
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::paths::write_atomic(&target, &written)?;
        report.changed.push(target);
    }
    Ok(report)
}

fn covers(item: &ItemSpec, path: &str) -> bool {
    match item.what {
        What::Dir(_) => path.starts_with(&format!("{}/", item.path)),
        _ => path == item.path,
    }
}

/// A profile's path, without `..` or absolute parts.
fn safe_relative(path: &str) -> anyhow::Result<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split('/') {
        anyhow::ensure!(
            !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', ':']),
            "the profile has an unsafe file name: {path}"
        );
        out.push(part);
    }
    Ok(out)
}

/// Only the chosen parts of a settings file.
fn keep_parts(item: &ItemSpec, value: &mut Value, want: &BTreeSet<Part>) {
    let Some(map) = value.as_object_mut() else { return };
    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        let nested = item.nested_units(&key);
        if want.contains(&item.part_of_key(&key)) {
            // Drop the values inside that belong to parts not chosen.
            let mut wrapper = Value::Object(Map::from_iter([(key.clone(), map.remove(&key).unwrap_or(Value::Null))]));
            for (pointer, part) in &nested {
                if !want.contains(part) {
                    remove_pointer(&mut wrapper, pointer);
                }
            }
            if let Some(v) = wrapper.as_object_mut().and_then(|w| w.remove(&key)) {
                map.insert(key, v);
            }
        } else {
            // Only the values inside that belong to chosen parts.
            let mut wrapper = Value::Object(Map::from_iter([(key.clone(), map.remove(&key).unwrap_or(Value::Null))]));
            let mut kept = Value::Object(Map::new());
            for (pointer, part) in &nested {
                if want.contains(part) {
                    if let Some(v) = wrapper.pointer_mut(pointer).map(Value::take) {
                        set_pointer(&mut kept, pointer, v);
                    }
                }
            }
            if let Some(v) = kept.as_object_mut().and_then(|k| k.remove(&key)) {
                map.insert(key, v);
            }
        }
    }
}

/// The app's file with the profile's values for the chosen parts; this computer's `machine`
/// values and the parts not chosen stay as they are. `None` when nothing changes.
fn merge_json(
    item: &ItemSpec,
    machine: &[&str],
    incoming: &Value,
    existing: Option<&Value>,
    want: &BTreeSet<Part>,
) -> Option<Value> {
    let incoming = incoming.as_object()?;
    let before = existing.cloned().filter(Value::is_object).unwrap_or_else(|| Value::Object(Map::new()));
    let mut merged = before.clone();
    for (key, value) in incoming {
        let prefix = format!("/{}", escape_pointer(key));
        if machine.contains(&prefix.as_str()) {
            continue;
        }
        let nested = item.nested_units(key);
        if want.contains(&item.part_of_key(key)) {
            let mut wrapper = Value::Object(Map::from_iter([(key.clone(), value.clone())]));
            // Keep this computer's values, and the values inside of parts not chosen.
            let local: Vec<&str> = machine
                .iter()
                .copied()
                .filter(|p| p.starts_with(&format!("{prefix}/")))
                .chain(nested.iter().filter(|(_, part)| !want.contains(part)).map(|(p, _)| *p))
                .collect();
            for pointer in local {
                remove_pointer(&mut wrapper, pointer);
                if !pointer.contains('*') {
                    if let Some(v) = before.pointer(pointer).cloned() {
                        set_pointer(&mut wrapper, pointer, v);
                    }
                }
            }
            let value = wrapper.as_object_mut()?.remove(key).unwrap_or(Value::Null);
            merged.as_object_mut()?.insert(key.clone(), value);
        } else {
            let wrapper = Value::Object(Map::from_iter([(key.clone(), value.clone())]));
            for (pointer, part) in &nested {
                if want.contains(part) {
                    if let Some(v) = wrapper.pointer(pointer).cloned() {
                        set_pointer(&mut merged, pointer, v);
                    }
                }
            }
        }
    }
    (merged != before).then_some(merged)
}

fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// Remove the value at `pointer`; a `*` token stands for every element of an array or object.
fn remove_pointer(value: &mut Value, pointer: &str) {
    let tokens: Vec<String> = pointer.split('/').skip(1).map(unescape).collect();
    remove_tokens(value, &tokens);
}

fn remove_tokens(value: &mut Value, tokens: &[String]) {
    let Some((first, rest)) = tokens.split_first() else { return };
    if rest.is_empty() {
        match value {
            Value::Object(map) => {
                map.remove(first);
            }
            Value::Array(items) if first == "*" => items.clear(),
            _ => {}
        }
        return;
    }
    match (value, first.as_str()) {
        (Value::Array(items), "*") => items.iter_mut().for_each(|v| remove_tokens(v, rest)),
        (Value::Object(map), "*") => map.values_mut().for_each(|v| remove_tokens(v, rest)),
        (Value::Array(items), index) => {
            if let Some(v) = index.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                remove_tokens(v, rest);
            }
        }
        (Value::Object(map), key) => {
            if let Some(v) = map.get_mut(key) {
                remove_tokens(v, rest);
            }
        }
        _ => {}
    }
}

fn set_pointer(value: &mut Value, pointer: &str, new: Value) {
    let mut current = value;
    let tokens: Vec<String> = pointer.split('/').skip(1).map(unescape).collect();
    for (i, token) in tokens.iter().enumerate() {
        let Value::Object(map) = current else { return };
        if i == tokens.len() - 1 {
            map.insert(token.clone(), new);
            return;
        }
        current = map.entry(token.clone()).or_insert_with(|| Value::Object(Map::new()));
    }
}

// ---- the apps ---------------------------------------------------------------------------------

/// What `app` keeps where, when CraftSpace knows.
pub fn spec(app: &str) -> Option<&'static AppSpec> {
    SPECS.iter().find(|s| s.app == app)
}

/// Why an app has no profiles, for the apps that don't.
pub fn unsupported_reason(app: &str) -> &'static str {
    match app {
        "cadcraft" => "CADCraft doesn't save settings yet, so there is nothing to carry over.",
        "pdfcraft" => "PdfCraft keeps your signatures and digital IDs in the same file as its settings, so CraftSpace leaves it alone.",
        _ => "CraftSpace doesn't know where this app keeps its settings.",
    }
}

mod specs;
use specs::SPECS;

#[cfg(test)]
mod tests;
