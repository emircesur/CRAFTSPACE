//! The Files tab: documents in the user's folders that an ArtCraft app can open.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::app_data::RecentFile;
use crate::catalog::Catalog;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    /// Lowercase, without the dot.
    pub ext: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// The app that opens it by default.
    pub app_id: Option<String>,
    /// On an app's own recent-files list.
    #[serde(default)]
    pub opened: Option<Opened>,
}

/// "Opened in PhotoCraft · 2 hours ago".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opened {
    pub app_id: String,
    pub at: Option<SystemTime>,
}

impl FileEntry {
    /// When it was last opened or changed, for sorting.
    pub fn last_used(&self) -> Option<SystemTime> {
        let opened = self.opened.as_ref().and_then(|o| o.at);
        opened.max(self.modified)
    }
}

/// Add the apps' recent files to a folder scan: files already found are marked as opened,
/// others (outside the scanned folders) are added. Newest first.
pub fn merge_recents(files: &mut Vec<FileEntry>, recents: Vec<RecentFile>, catalog: &Catalog) {
    for r in recents {
        let opened = Opened { app_id: r.app_id.clone(), at: r.opened };
        if let Some(f) = files.iter_mut().find(|f| f.path == r.path) {
            let newer = f.opened.as_ref().is_none_or(|o| o.at < opened.at);
            if newer {
                f.opened = Some(opened);
            }
        } else if let Some(mut f) = describe(&r.path, catalog) {
            f.opened = Some(opened);
            files.push(f);
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.last_used()));
}

/// The last scan, saved so the tab shows something the moment it opens.
pub fn load_cache(path: &Path) -> Option<Vec<FileEntry>> {
    let bytes = std::fs::read(path).ok()?;
    let files: Vec<FileEntry> = serde_json::from_slice(&bytes).ok()?;
    // Drop files that are gone since.
    Some(files.into_iter().filter(|f| f.path.exists()).collect())
}

pub fn save_cache(path: &Path, files: &[FileEntry]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(crate::paths::write_atomic(path, &serde_json::to_vec(files)?)?)
}

pub struct ScanOptions {
    pub max_depth: usize,
    pub max_results: usize,
    /// Give up on the walk after this long; huge home folders shouldn't hang the tab.
    pub time_budget: Duration,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions { max_depth: 4, max_results: 2000, time_budget: Duration::from_secs(4) }
    }
}

const SKIP_DIRS: &[&str] =
    &["node_modules", "target", "__pycache__", "venv", ".venv", "AppData", "Library", "$RECYCLE.BIN"];

/// Walk `roots` for files any app in `catalog` opens, newest first.
pub fn scan(roots: &[PathBuf], catalog: &Catalog, opts: &ScanOptions, cancel: &AtomicBool) -> Vec<FileEntry> {
    let extensions = catalog.all_extensions();
    let started = Instant::now();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    'roots: for root in roots {
        let walker = walkdir::WalkDir::new(root)
            .max_depth(opts.max_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !is_skipped(e.path(), e.file_type().is_dir()));
        for entry in walker.filter_map(Result::ok) {
            if cancel.load(Ordering::Relaxed) || started.elapsed() > opts.time_budget || out.len() >= opts.max_results {
                break 'roots;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(ext) = entry.path().extension().map(|e| e.to_string_lossy().to_ascii_lowercase()) else {
                continue;
            };
            if extensions.binary_search(&ext).is_err() || !seen.insert(entry.path().to_path_buf()) {
                continue;
            }
            if let Some(file) = describe(entry.path(), catalog) {
                out.push(file);
            }
        }
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.modified));
    out
}

/// Build a [`FileEntry`] for one path (pinned files are described this way too).
pub fn describe(path: &Path, catalog: &Catalog) -> Option<FileEntry> {
    let meta = std::fs::metadata(path).ok()?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    Some(FileEntry {
        name: path.file_name()?.to_string_lossy().into_owned(),
        app_id: catalog.apps_for_extension(&ext).first().map(|a| a.id.clone()),
        ext,
        size: meta.len(),
        modified: meta.modified().ok(),
        path: path.to_path_buf(),
        opened: None,
    })
}

fn is_skipped(path: &Path, is_dir: bool) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    name.starts_with('.') || (is_dir && SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s)))
}

/// "5 minutes ago", "Yesterday", "3 days ago", or a date.
pub fn relative_time(t: SystemTime) -> String {
    let secs = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match secs {
        0..=59 => "Just now".into(),
        60..=3599 => plural(secs / 60, "minute"),
        3600..=86_399 => plural(secs / 3600, "hour"),
        86_400..=172_799 => "Yesterday".into(),
        172_800..=2_591_999 => plural(secs / 86_400, "day"),
        _ => {
            let unix = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            time::OffsetDateTime::from_unix_timestamp(unix)
                .ok()
                .and_then(|d| d.format(time::macros::format_description!("[day] [month repr:short] [year]")).ok())
                .unwrap_or_default()
        }
    }
}

fn plural(n: u64, unit: &str) -> String {
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_openable_files_and_skips_noise() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("work/.hidden")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("work/poster.psd"), b"x").unwrap();
        std::fs::write(root.join("work/budget.XLSX"), b"x").unwrap();
        std::fs::write(root.join("work/notes.txt"), b"x").unwrap();
        std::fs::write(root.join("work/.hidden/secret.psd"), b"x").unwrap();
        std::fs::write(root.join("node_modules/x.pdf"), b"x").unwrap();

        let catalog = Catalog::builtin();
        let files =
            scan(&[root.to_path_buf(), root.join("work")], &catalog, &ScanOptions::default(), &AtomicBool::new(false));
        let mut names: Vec<_> = files.iter().map(|f| f.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["budget.XLSX", "poster.psd"]);
        let psd = files.iter().find(|f| f.ext == "psd").unwrap();
        assert_eq!(psd.app_id.as_deref(), Some("photocraft"));
    }

    #[test]
    fn relative_times() {
        let now = SystemTime::now();
        assert_eq!(relative_time(now), "Just now");
        assert_eq!(relative_time(now - Duration::from_secs(120)), "2 minutes ago");
        assert_eq!(relative_time(now - Duration::from_secs(3600)), "1 hour ago");
        assert_eq!(relative_time(now - Duration::from_secs(90_000)), "Yesterday");
    }
}
