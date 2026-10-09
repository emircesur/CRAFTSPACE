//! The ArtCraft apps' own data: their recent-files lists, and settings left behind by portable
//! copies.
//!
//! Every app keeps its settings in one folder: the platform's per-user config folder under the
//! app's name (`%APPDATA%\PhotoCraft`, `~/Library/Application Support/PhotoCraft`,
//! `~/.config/photocraft`), or, in portable mode, `<Name>Data` beside the program
//! (`PhotoCraftData`). The recent files are a JSON array in one of the files there
//! (`fileHandling.recentFiles` in PhotoCraft's `preferences.json`, `recent_files` in
//! VectorCraft's `ui.json`, …), newest first. Rather than knowing every app's layout, this reads
//! the JSON files in the folder and takes arrays whose key names recent files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalog::{AppEntry, Catalog};

/// Settings files bigger than this aren't preferences.
const MAX_JSON: u64 = 8 << 20;

/// The platform's per-user config folder (`%APPDATA%`, `~/Library/Application Support`,
/// `$XDG_CONFIG_HOME` or `~/.config`).
fn config_base() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.config_dir().to_path_buf())
}

/// Where the installed app keeps its settings (it may not exist yet).
pub fn config_dir(app: &AppEntry) -> Option<PathBuf> {
    if let Some(dir) = app.config_dir() {
        return Some(dir);
    }
    let base = config_base()?;
    Some(if cfg!(windows) || cfg!(target_os = "macos") { base.join(&app.name) } else { base.join(&app.id) })
}

/// Every folder the app may keep settings in: the config folder (under the spellings the apps
/// use) and the portable data folders beside `programs`.
pub fn data_dirs(app: &AppEntry, programs: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut add = |d: PathBuf| {
        if d.is_dir() && !dirs.iter().any(|x| same_dir(x, &d)) {
            dirs.push(d);
        }
    };
    if let Some(d) = config_dir(app) {
        add(d);
    }
    if let Some(base) = config_base() {
        let capitalized = capitalize(&app.id);
        for name in [app.name.as_str(), app.id.as_str(), capitalized.as_str()] {
            add(base.join(name));
        }
    }
    for program in programs {
        if let Some(dir) = program.parent() {
            add(dir.join(portable_dir_name(app)));
        }
    }
    dirs
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `PhotoCraftData`.
pub fn portable_dir_name(app: &AppEntry) -> String {
    format!("{}Data", app.name)
}

// ---- recent files ------------------------------------------------------------------------------

/// A file an app lists under File › Open Recent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: PathBuf,
    pub app_id: String,
    /// Position in the app's list (0 = most recent).
    pub rank: usize,
    /// When it was opened, when that's known: the time the list was saved for the newest entry,
    /// else when CraftSpace first saw it at the top.
    pub opened: Option<SystemTime>,
}

/// When each file was first seen at the top of an app's list (`<app>|<path>` → time), so
/// entries further down keep a time once they've been seen.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OpenedTimes(BTreeMap<String, SystemTime>);

impl OpenedTimes {
    pub fn load(path: &Path) -> OpenedTimes {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        Ok(crate::paths::write_atomic(path, &serde_json::to_vec(self)?)?)
    }
}

/// The recent files of `app` that still exist, newest first.
pub fn recent_files(app: &AppEntry, programs: &[PathBuf], times: &mut OpenedTimes) -> Vec<RecentFile> {
    let mut found: Vec<(Vec<String>, Option<SystemTime>)> = Vec::new();
    for dir in data_dirs(app, programs) {
        for (list, saved) in recent_lists_in(&dir) {
            found.push((list, saved));
        }
    }
    // The list saved most recently wins (a portable copy and the installed app can both have one).
    found.sort_by_key(|(_, saved)| std::cmp::Reverse(*saved));
    let mut out: Vec<RecentFile> = Vec::new();
    for (list, saved) in found {
        for (rank, entry) in list.iter().enumerate() {
            let path = PathBuf::from(entry);
            if !path.is_absolute() || !path.is_file() || out.iter().any(|r| r.path == path) {
                continue;
            }
            let key = format!("{}|{}", app.id, path.display());
            if rank == 0 {
                if let Some(t) = saved {
                    let newer = times.0.get(&key).is_none_or(|old| *old < t);
                    if newer {
                        times.0.insert(key.clone(), t);
                    }
                }
            }
            let opened = times.0.get(&key).copied();
            out.push(RecentFile { path, app_id: app.id.clone(), rank: out.len(), opened });
        }
    }
    out
}

/// Recent-file lists in the JSON files directly inside `dir`, with when each file was saved.
fn recent_lists_in(dir: &Path) -> Vec<(Vec<String>, Option<SystemTime>)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_json = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let Ok(meta) = entry.metadata() else { continue };
        if !is_json || !meta.is_file() || meta.len() > MAX_JSON {
            continue;
        }
        let Some(value) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
            continue;
        };
        let mut lists = Vec::new();
        collect_recents(&value, &mut lists);
        for list in lists.into_iter().filter(|l| !l.is_empty()) {
            out.push((list, meta.modified().ok()));
        }
    }
    out
}

/// Whether a JSON key names a list of recently opened documents (not recent fonts, colors…).
fn is_recent_files_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase().replace(['_', '-', ' '], "");
    if !k.contains("recent") {
        return false;
    }
    const OTHER: &[&str] = &[
        "font", "color", "colour", "keyword", "url", "search", "brush", "swatch", "tool", "glyph", "count", "max",
        "command",
    ];
    if OTHER.iter().any(|o| k.contains(o)) {
        return false;
    }
    k == "recent" || ["file", "doc", "project", "path", "open"].iter().any(|w| k.contains(w))
}

fn collect_recents(value: &Value, out: &mut Vec<Vec<String>>) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if is_recent_files_key(key) {
                    if let Value::Array(items) = v {
                        let paths: Vec<String> = items.iter().filter_map(item_path).collect();
                        if !paths.is_empty() {
                            out.push(paths);
                            continue;
                        }
                    }
                }
                collect_recents(v, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_recents(v, out)),
        _ => {}
    }
}

/// A list entry: a path, or an object with one (`{"path": …}`).
fn item_path(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(m) => ["path", "file", "filename", "location"]
            .iter()
            .find_map(|k| m.get(*k).and_then(Value::as_str))
            .map(str::to_string),
        _ => None,
    }
}

// ---- portable copies ---------------------------------------------------------------------------

/// Settings left by a portable copy of an app (`PhotoCraftData` beside its program).
#[derive(Debug, Clone, PartialEq)]
pub struct PortableData {
    pub app_id: String,
    pub dir: PathBuf,
    /// What's in it, for the offer: "preferences.json", "Presets", …
    pub items: Vec<String>,
}

/// Look for `<Name>Data` folders of apps in `catalog` under `roots` (a few levels down).
/// Folders in `skip` (already imported or dismissed) and the apps' own config folders are left
/// out.
pub fn find_portable_data(catalog: &Catalog, roots: &[PathBuf], skip: &[PathBuf]) -> Vec<PortableData> {
    let names: Vec<(String, &AppEntry)> =
        catalog.apps.iter().map(|a| (portable_dir_name(a).to_ascii_lowercase(), a)).collect();
    let started = std::time::Instant::now();
    let mut out: Vec<PortableData> = Vec::new();
    for root in roots {
        let walker = walkdir::WalkDir::new(root).max_depth(4).follow_links(false).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            e.depth() == 0 || (e.file_type().is_dir() && !name.starts_with('.') && name != "node_modules")
        });
        for entry in walker.filter_map(Result::ok) {
            if started.elapsed() > std::time::Duration::from_secs(5) {
                return out;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let Some((_, app)) = names.iter().find(|(n, _)| *n == name) else { continue };
            let dir = entry.path().to_path_buf();
            if skip.iter().any(|s| same_dir(s, &dir)) || out.iter().any(|p| same_dir(&p.dir, &dir)) {
                continue;
            }
            if config_dir(app).is_some_and(|c| same_dir(&c, &dir)) {
                continue;
            }
            let mut items: Vec<String> = std::fs::read_dir(&dir)
                .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            items.retain(|i| !i.starts_with('.'));
            items.sort();
            if !items.is_empty() {
                out.push(PortableData { app_id: app.id.clone(), dir, items });
            }
        }
    }
    out
}

/// What an import did.
#[derive(Debug, Default)]
pub struct ImportReport {
    pub copied: usize,
    /// Files that were already there, kept beside the new ones as `<name>.before-import`.
    pub replaced: Vec<PathBuf>,
    /// Recent files added to the installed app's list.
    pub recents_merged: usize,
    /// The portable folder, renamed so it isn't offered again (`None` if it couldn't be).
    pub moved_to: Option<PathBuf>,
}

/// Copy a portable copy's settings, presets and recent files into the installed app's settings
/// folder `dest`, then rename the portable folder to `<Name>Data.imported`.
///
/// Files the installed app already has are replaced, keeping the old one as
/// `<name>.before-import`; for settings files with a recent-files list, the two lists are merged
/// (the portable copy's first).
pub fn import_portable(src: &Path, dest: &Path) -> anyhow::Result<ImportReport> {
    anyhow::ensure!(src.is_dir(), "{} isn't there anymore", src.display());
    anyhow::ensure!(!same_dir(src, dest), "that's already the app's settings folder");
    std::fs::create_dir_all(dest)?;
    let mut report = ImportReport::default();
    for entry in walkdir::WalkDir::new(src).min_depth(1).into_iter().filter_map(Result::ok) {
        let rel = entry.path().strip_prefix(src)?;
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if target.exists() {
            let backup = backup_name(&target);
            std::fs::copy(&target, &backup)?;
            report.replaced.push(target.clone());
            if let Some(merged) = merge_json_recents(entry.path(), &target, &mut report.recents_merged) {
                crate::paths::write_atomic(&target, merged.as_bytes())?;
                report.copied += 1;
                continue;
            }
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(entry.path(), &target)?;
        report.copied += 1;
    }
    let moved = src.with_file_name(format!("{}.imported", src.file_name().unwrap_or_default().to_string_lossy()));
    if !moved.exists() && std::fs::rename(src, &moved).is_ok() {
        report.moved_to = Some(moved);
    }
    Ok(report)
}

fn backup_name(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut n = 0;
    loop {
        let candidate = path.with_file_name(if n == 0 {
            format!("{name}.before-import")
        } else {
            format!("{name}.before-import-{n}")
        });
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// When both files are JSON with recent-files lists: the portable copy, with the installed app's
/// recent files added after its own.
fn merge_json_recents(portable: &Path, installed: &Path, merged_count: &mut usize) -> Option<String> {
    let read = |p: &Path| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let (mut new, old) = (read(portable)?, read(installed)?);
    let mut old_lists = Vec::new();
    collect_recents(&old, &mut old_lists);
    let extra: Vec<String> = old_lists.into_iter().flatten().collect();
    if extra.is_empty() {
        return None;
    }
    let mut changed = false;
    merge_into(&mut new, &extra, merged_count, &mut changed);
    changed.then(|| serde_json::to_string_pretty(&new).ok()).flatten()
}

fn merge_into(value: &mut Value, extra: &[String], count: &mut usize, changed: &mut bool) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if is_recent_files_key(key) {
                    if let Value::Array(items) = v {
                        if items.iter().all(|i| i.is_string()) {
                            for path in extra {
                                if !items.iter().any(|i| i.as_str() == Some(path)) {
                                    items.push(Value::String(path.clone()));
                                    *count += 1;
                                }
                            }
                            *changed = true;
                            continue;
                        }
                    }
                }
                merge_into(v, extra, count, changed);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| merge_into(v, extra, count, changed)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_file_keys() {
        for k in
            ["recentFiles", "recent_files", "fileHandling.recentFiles", "recentDocuments", "recent", "RecentProjects"]
        {
            assert!(is_recent_files_key(k), "{k}");
        }
        for k in ["recentFonts", "recent_colors", "recentFileCount", "recent_keywords", "recentUrls", "files"] {
            assert!(!is_recent_files_key(k), "{k}");
        }
    }

    #[test]
    fn reads_the_layouts_the_apps_use() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        let (a, b) = (docs.join("a.psd"), docs.join("b.svg"));
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        let cfg = tmp.path().join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        // PhotoCraft: nested camelCase; VectorCraft: flat snake_case; missing files are skipped.
        let photo = serde_json::json!({"fileHandling": {"recentFileCount": 20, "recentFiles": [a, docs.join("gone.psd")]}, "type": {"recentFonts": ["Inter"]}});
        std::fs::write(cfg.join("preferences.json"), photo.to_string()).unwrap();
        let vector = serde_json::json!({"recent_files": [b], "recent_fonts": ["Inter"]});
        std::fs::write(cfg.join("ui.json"), vector.to_string()).unwrap();
        let mut lists: Vec<Vec<String>> = recent_lists_in(&cfg).into_iter().map(|(l, _)| l).collect();
        lists.sort();
        assert_eq!(lists.len(), 2);
        assert!(lists.iter().any(|l| l.len() == 2 && l[0] == a.to_string_lossy()));
        assert!(lists.iter().any(|l| l == &[b.to_string_lossy().into_owned()]));
    }

    #[test]
    fn finds_and_imports_a_portable_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = Catalog::builtin();
        let usb = tmp.path().join("usb/PhotoCraft");
        let data = usb.join("PhotoCraftData");
        std::fs::create_dir_all(data.join("Presets")).unwrap();
        std::fs::write(data.join("Presets/brushes.json"), b"{}").unwrap();
        std::fs::write(
            data.join("preferences.json"),
            r#"{"fileHandling":{"recentFiles":["/p/new.psd"]},"theme":"light"}"#,
        )
        .unwrap();

        let found = find_portable_data(&catalog, &[tmp.path().to_path_buf()], &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].app_id, "photocraft");
        assert_eq!(found[0].items, ["Presets", "preferences.json"]);
        assert!(find_portable_data(&catalog, &[tmp.path().to_path_buf()], std::slice::from_ref(&data)).is_empty());

        let dest = tmp.path().join("config/Photocraft");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(
            dest.join("preferences.json"),
            r#"{"fileHandling":{"recentFiles":["/p/old.psd","/p/new.psd"]}}"#,
        )
        .unwrap();
        let report = import_portable(&data, &dest).unwrap();
        assert_eq!(report.copied, 2);
        assert_eq!(report.recents_merged, 1);
        assert!(dest.join("Presets/brushes.json").is_file());
        assert!(dest.join("preferences.json.before-import").is_file());
        let prefs: Value = serde_json::from_slice(&std::fs::read(dest.join("preferences.json")).unwrap()).unwrap();
        assert_eq!(prefs["theme"], "light");
        assert_eq!(prefs["fileHandling"]["recentFiles"], serde_json::json!(["/p/new.psd", "/p/old.psd"]));
        assert_eq!(report.moved_to.as_deref(), Some(usb.join("PhotoCraftData.imported").as_path()));
        assert!(!data.exists());
    }
}
