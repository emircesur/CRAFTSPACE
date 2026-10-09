//! The apps' own icons, downloaded from their repositories and cached on disk.

use std::path::PathBuf;
use std::time::Duration;

use ureq::Agent;

use crate::catalog::AppEntry;
use crate::paths::{write_atomic, Paths};

/// Icons are re-fetched after a week, so a new logo shows up eventually.
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

pub fn cache_path(paths: &Paths, app_id: &str) -> PathBuf {
    paths.cache.join("icons").join(format!("{app_id}.png"))
}

/// The cached icon, if any (however old).
pub fn cached(paths: &Paths, app_id: &str) -> Option<Vec<u8>> {
    std::fs::read(cache_path(paths, app_id)).ok().filter(|b| is_png(b))
}

/// Fetch `app`'s icon unless a fresh copy is cached. Returns the PNG bytes.
pub fn fetch(agent: &Agent, paths: &Paths, app: &AppEntry) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(url) = app.icon.as_deref() else { return Ok(None) };
    let path = cache_path(paths, &app.id);
    let fresh =
        std::fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e < MAX_AGE));
    if fresh {
        if let Some(bytes) = cached(paths, &app.id) {
            return Ok(Some(bytes));
        }
    }
    let mut resp = agent.get(url).call()?;
    crate::http::check_status(url, resp.status().as_u16())?;
    let bytes = resp.body_mut().with_config().limit(8 << 20).read_to_vec()?;
    anyhow::ensure!(is_png(&bytes), "{url} is not a PNG");
    write_atomic(&path, &bytes)?;
    Ok(Some(bytes))
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}
