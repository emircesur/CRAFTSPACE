//! Streaming downloads with progress, cancellation and SHA-256 verification.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use ureq::Agent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Resolving,
    Downloading,
    Verifying,
    Installing,
    Integrating,
    Cleaning,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Resolving => "Preparing",
            Stage::Downloading => "Downloading",
            Stage::Verifying => "Verifying",
            Stage::Installing => "Installing",
            Stage::Integrating => "Adding shortcuts",
            Stage::Cleaning => "Cleaning up",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgressEvent {
    Stage(Stage),
    Bytes { done: u64, total: Option<u64> },
}

/// Progress reporting and cancellation for long operations.
pub struct Progress<'a> {
    pub report: &'a (dyn Fn(ProgressEvent) + Sync),
    pub cancel: &'a AtomicBool,
}

impl Progress<'_> {
    pub fn stage(&self, stage: Stage) {
        (self.report)(ProgressEvent::Stage(stage));
    }

    pub fn check_cancelled(&self) -> anyhow::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            anyhow::bail!(Cancelled);
        }
        Ok(())
    }
}

/// Returned (inside `anyhow::Error`) when the user cancels.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancelled(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Cancelled>().is_some()
}

/// Download `url` to `dest` and return its SHA-256. If `expected_sha256` is given and does not
/// match, the file is deleted and an error returned.
///
/// Interrupted downloads continue where they stopped: the partial file (`<dest>.part`) is kept
/// on network errors and cancellation, and the next attempt asks the server for the rest with an
/// HTTP range request. Network errors are retried a few times with back-off.
pub fn download(
    agent: &Agent,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    progress: &Progress,
) -> anyhow::Result<String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    progress.stage(Stage::Downloading);
    let part = dest.with_extension("part");
    let mut attempt = 0u32;
    let mut restarted = false;
    loop {
        let resumed_from = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let actual = match download_once(agent, url, &part, progress) {
            Ok(sha) => sha,
            Err(err) if is_cancelled(&err) => return Err(err),
            Err(err) if attempt < 3 && is_transient(&err) => {
                attempt += 1;
                log::warn!("download of {url} interrupted ({err:#}); retrying ({attempt}/3)");
                std::thread::sleep(Duration::from_secs(1 << attempt));
                continue;
            }
            Err(err) => return Err(err),
        };
        progress.stage(Stage::Verifying);
        if let Some(expected) = expected_sha256 {
            if !expected.eq_ignore_ascii_case(&actual) {
                let _ = std::fs::remove_file(&part);
                if resumed_from > 0 && !restarted {
                    // The kept partial file may belong to a different upload; start over once.
                    restarted = true;
                    progress.stage(Stage::Downloading);
                    continue;
                }
                anyhow::bail!("checksum mismatch for {url}: expected {expected}, got {actual}");
            }
        }
        std::fs::rename(&part, dest)?;
        return Ok(actual);
    }
}

fn download_once(agent: &Agent, url: &str, part: &Path, progress: &Progress) -> anyhow::Result<String> {
    let existing = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let mut req = agent.get(url);
    if existing > 0 {
        req = req.header("Range", &format!("bytes={existing}-"));
    }
    let mut resp = req.call()?;
    let status = resp.status().as_u16();
    let content_length =
        resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());

    let mut hasher = Sha256::new();
    let (mut file, mut done, total) = match status {
        206 if existing > 0 => {
            // Hash what we already have, then append.
            let mut f = File::open(part)?;
            std::io::copy(&mut f, &mut hasher)?;
            let total = resp
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|v| v.parse::<u64>().ok())
                .or(content_length.map(|n| n + existing));
            (std::fs::OpenOptions::new().append(true).open(part)?, existing, total)
        }
        // The partial file is already complete.
        416 if existing > 0 => {
            return sha256_file(part).map_err(Into::into);
        }
        _ => {
            if !(200..300).contains(&status) {
                return Err(HttpStatus { url: url.to_string(), status }.into());
            }
            (File::create(part)?, 0, content_length)
        }
    };

    let mut reader = resp.body_mut().as_reader();
    let mut out = BufWriter::new(&mut file);
    let mut buf = vec![0u8; 128 * 1024];
    let mut last_report = Instant::now() - Duration::from_secs(1);
    (progress.report)(ProgressEvent::Bytes { done, total });
    loop {
        progress.check_cancelled()?;
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        throttle(n);
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        done += n as u64;
        if last_report.elapsed() >= Duration::from_millis(100) {
            (progress.report)(ProgressEvent::Bytes { done, total });
            last_report = Instant::now();
        }
    }
    out.flush()?;
    (progress.report)(ProgressEvent::Bytes { done, total: Some(total.unwrap_or(done)) });
    if let Some(total) = total {
        anyhow::ensure!(done == total, "download ended early ({done} of {total} bytes)");
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Bytes `start..=end` of `url` (for delta updates).
pub fn fetch_range(agent: &Agent, url: &str, start: u64, end: u64, progress: &Progress) -> anyhow::Result<Vec<u8>> {
    let mut attempt = 0u32;
    loop {
        progress.check_cancelled()?;
        let result = (|| -> anyhow::Result<Vec<u8>> {
            let mut resp = agent.get(url).header("Range", &format!("bytes={start}-{end}")).call()?;
            let status = resp.status().as_u16();
            anyhow::ensure!(status == 206, "{url} doesn't support range requests (HTTP {status})");
            let mut out = Vec::with_capacity((end - start + 1) as usize);
            let mut reader = resp.body_mut().as_reader();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                throttle(n);
                out.extend_from_slice(&buf[..n]);
            }
            anyhow::ensure!(out.len() as u64 == end - start + 1, "short range response from {url}");
            Ok(out)
        })();
        match result {
            Ok(v) => return Ok(v),
            Err(err) if attempt < 3 && is_transient(&err) => {
                attempt += 1;
                std::thread::sleep(Duration::from_secs(1 << attempt));
            }
            Err(err) => return Err(err),
        }
    }
}

/// A non-success HTTP status.
#[derive(Debug)]
pub struct HttpStatus {
    pub url: String,
    pub status: u16,
}

impl std::fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} returned HTTP {}", self.url, self.status)
    }
}

impl std::error::Error for HttpStatus {}

/// Network hiccups and server errors are worth retrying; 404s and checksum mismatches are not.
fn is_transient(err: &anyhow::Error) -> bool {
    if let Some(status) = err.downcast_ref::<HttpStatus>() {
        return status.status >= 500 || status.status == 429;
    }
    err.downcast_ref::<ureq::Error>().is_some() || err.downcast_ref::<std::io::Error>().is_some()
}

// ---- speed limit ------------------------------------------------------------------------------

/// Bytes per second shared by every download in the process; 0 is unlimited.
static LIMIT: AtomicU64 = AtomicU64::new(0);
static BUCKET: Mutex<Option<Bucket>> = Mutex::new(None);

/// Cap the combined download speed (`None` for no cap).
pub fn set_speed_limit(bytes_per_sec: Option<u64>) {
    LIMIT.store(bytes_per_sec.unwrap_or(0), Ordering::Relaxed);
}

/// A token bucket allowing up to one second of burst.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    last: Instant,
    tokens: f64,
}

impl Bucket {
    /// Take `n` bytes at `now`; returns how long to wait before continuing.
    fn take(&mut self, n: usize, limit: u64, now: Instant) -> Duration {
        let limit = limit as f64;
        self.tokens = (self.tokens + now.saturating_duration_since(self.last).as_secs_f64() * limit).min(limit);
        self.last = now;
        self.tokens -= n as f64;
        if self.tokens >= 0.0 {
            return Duration::ZERO;
        }
        let wait = Duration::from_secs_f64(-self.tokens / limit);
        self.tokens = 0.0;
        self.last = now + wait;
        wait
    }
}

/// Wait as long as needed to keep under the speed limit after receiving `n` bytes.
fn throttle(n: usize) {
    let limit = LIMIT.load(Ordering::Relaxed);
    if limit == 0 {
        return;
    }
    let mut bucket = BUCKET.lock().unwrap();
    let now = Instant::now();
    let wait = bucket.get_or_insert(Bucket { last: now, tokens: limit as f64 }).take(n, limit, now);
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
}

/// SHA-256 of a file on disk.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

/// Human-readable size: `61.3 MB`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny HTTP server for `body`, honouring `Range: bytes=a-` and `bytes=a-b`.
    pub(crate) fn serve(body: Vec<u8>) -> String {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        let (a, b) = v.trim().split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = if b.is_empty() { body.len() - 1 } else { b.parse().unwrap() };
                        range = Some((a, b));
                    }
                }
                let (status, slice, extra) = match range {
                    Some((a, _)) if a >= body.len() => ("416 Range Not Satisfiable", &body[0..0], String::new()),
                    Some((a, b)) => (
                        "206 Partial Content",
                        &body[a..=b],
                        format!("Content-Range: bytes {a}-{b}/{}\r\n", body.len()),
                    ),
                    None => ("200 OK", &body[..], String::new()),
                };
                let head =
                    format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n", slice.len());
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(slice);
            }
        });
        format!("http://{addr}/file")
    }

    #[test]
    fn resumes_partial_downloads() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let url = serve(body.clone());
        let expected = hex::encode(Sha256::digest(&body));
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.bin");
        // Pretend an earlier attempt got the first 100 000 bytes.
        std::fs::write(dest.with_extension("part"), &body[..100_000]).unwrap();
        let agent = crate::http::agent();
        let seen = Mutex::new(Vec::new());
        let report = |e: ProgressEvent| seen.lock().unwrap().push(e);
        let cancel = AtomicBool::new(false);
        let sha =
            download(&agent, &url, &dest, Some(&expected), &Progress { report: &report, cancel: &cancel }).unwrap();
        assert_eq!(sha, expected);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        // Progress started from the resumed position, not zero.
        let first = seen.lock().unwrap().iter().find_map(|e| match e {
            ProgressEvent::Bytes { done, total } => Some((*done, *total)),
            _ => None,
        });
        assert_eq!(first, Some((100_000, Some(300_000))));

        // A stale partial file of the wrong content is detected and the download restarted.
        let dest2 = dir.path().join("again.bin");
        std::fs::write(dest2.with_extension("part"), vec![9u8; 50_000]).unwrap();
        let sha =
            download(&agent, &url, &dest2, Some(&expected), &Progress { report: &report, cancel: &cancel }).unwrap();
        assert_eq!(sha, expected);

        let range = fetch_range(&agent, &url, 10, 19, &Progress { report: &report, cancel: &cancel }).unwrap();
        assert_eq!(range, body[10..20]);
    }

    #[test]
    fn token_bucket_paces_to_the_limit() {
        let start = Instant::now();
        let mut bucket = Bucket { last: start, tokens: 1_000_000.0 };
        let mut clock = start;
        let mut waited = Duration::ZERO;
        // 3 MB at 1 MB/s with a 1 MB burst: about 2 s of waiting.
        for _ in 0..30 {
            let w = bucket.take(100_000, 1_000_000, clock);
            clock += w;
            waited += w;
        }
        assert!((waited.as_secs_f64() - 2.0).abs() < 0.01, "{waited:?}");
        // After a long pause the burst is capped at one second's worth.
        clock += Duration::from_secs(60);
        assert_eq!(bucket.take(1_000_000, 1_000_000, clock), Duration::ZERO);
        assert!(bucket.take(500_000, 1_000_000, clock) >= Duration::from_millis(499));
    }

    #[test]
    fn classifies_errors() {
        assert!(is_transient(&anyhow::Error::from(HttpStatus { url: "u".into(), status: 503 })));
        assert!(!is_transient(&anyhow::Error::from(HttpStatus { url: "u".into(), status: 404 })));
        assert!(is_transient(&anyhow::Error::from(std::io::Error::other("reset"))));
        assert!(!is_transient(&anyhow::anyhow!("checksum mismatch")));
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(64_281_804), "61.3 MB");
    }
}
