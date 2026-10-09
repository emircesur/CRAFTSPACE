//! Discovering releases on GitHub.
//!
//! The REST API gives the full release history with notes, but unauthenticated clients get
//! only 60 requests an hour. When the API is rate limited or unreachable, CraftSpace falls back
//! to plain `github.com` URLs, which have no such limit: the redirect behind
//! `/releases/latest/download/SHA256SUMS.txt` names the latest tag, and that file lists every
//! asset with its checksum. Results are cached on disk, with ETags, so an unchanged repository
//! costs nothing against the rate limit and the app list still works offline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use semver::Version;
use serde::{Deserialize, Serialize};
use ureq::Agent;

use crate::http::check_status;
use crate::paths::write_atomic;
use crate::version;

pub const SUMS_FILE: &str = "SHA256SUMS.txt";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub version: Option<Version>,
    pub prerelease: bool,
    /// RFC 3339 publish time, when known.
    pub published_at: Option<String>,
    /// Release notes (Markdown).
    pub body: String,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Asset {
    pub name: String,
    pub size: Option<u64>,
    pub url: String,
    /// Expected SHA-256 (lowercase hex), when GitHub or a checksum file told us.
    pub sha256: Option<String>,
}

/// Where a release list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Api,
    /// Latest release only, from the `releases/latest` redirect and `SHA256SUMS.txt`.
    LatestRedirect,
    /// Served from the on-disk cache (offline, or rate limited with no fallback).
    Cache,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseList {
    pub repo: String,
    pub releases: Vec<Release>,
    pub source: Source,
    /// Seconds since the Unix epoch.
    pub fetched_at: u64,
    #[serde(default)]
    pub etag: Option<String>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    pub fn asset_names(&self) -> Vec<&str> {
        self.assets.iter().map(|a| a.name.as_str()).collect()
    }

    /// The publish date as `YYYY-MM-DD`, when known.
    pub fn date(&self) -> Option<&str> {
        self.published_at.as_deref().and_then(|d| d.get(..10))
    }
}

impl ReleaseList {
    /// The newest release, optionally including pre-releases.
    pub fn latest(&self, include_prereleases: bool) -> Option<&Release> {
        self.releases
            .iter()
            .filter(|r| r.version.is_some() && (include_prereleases || !r.prerelease))
            .max_by(|a, b| a.version.cmp(&b.version))
    }

    pub fn find(&self, version: &Version) -> Option<&Release> {
        self.releases.iter().find(|r| r.version.as_ref() == Some(version))
    }
}

/// Fetches and caches release lists.
#[derive(Clone)]
pub struct GitHub {
    agent: Agent,
    token: Option<String>,
    cache_dir: PathBuf,
    api_base: String,
    web_base: String,
}

/// How long a cached list is used without asking GitHub again.
pub const FRESH_FOR: Duration = Duration::from_secs(10 * 60);

impl GitHub {
    pub fn new(agent: Agent, token: Option<String>, cache_dir: PathBuf) -> GitHub {
        GitHub {
            agent,
            token,
            cache_dir,
            api_base: "https://api.github.com".into(),
            web_base: "https://github.com".into(),
        }
    }

    /// Point at a different server (tests, GitHub Enterprise).
    pub fn with_bases(mut self, api_base: &str, web_base: &str) -> GitHub {
        self.api_base = api_base.trim_end_matches('/').into();
        self.web_base = web_base.trim_end_matches('/').into();
        self
    }

    fn cache_path(&self, repo: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.json", repo.replace('/', "__")))
    }

    pub fn cached(&self, repo: &str) -> Option<ReleaseList> {
        let bytes = std::fs::read(self.cache_path(repo)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn store(&self, list: &ReleaseList) {
        if let Ok(json) = serde_json::to_vec(list) {
            if let Err(err) = write_atomic(&self.cache_path(&list.repo), &json) {
                log::warn!("could not cache releases for {}: {err}", list.repo);
            }
        }
    }

    /// Releases for `repo`, newest first. With `force`, a fresh cache entry is not trusted.
    pub fn releases(&self, repo: &str, force: bool) -> anyhow::Result<ReleaseList> {
        let cached = self.cached(repo);
        if let Some(c) = &cached {
            let age = now_secs().saturating_sub(c.fetched_at);
            if !force && age < FRESH_FOR.as_secs() && c.source != Source::Cache {
                return Ok(c.clone());
            }
        }

        let api_err = match self.fetch_api(repo, cached.as_ref()) {
            Ok(list) => {
                self.store(&list);
                return Ok(list);
            }
            Err(err) => err,
        };
        log::info!("GitHub API unavailable for {repo} ({api_err:#}); trying the latest-release redirect");

        match self.fetch_latest_redirect(repo) {
            Ok(mut list) => {
                // Keep history and notes we learned from the API earlier.
                if let Some(c) = &cached {
                    for old in &c.releases {
                        if let Some(new) = list.releases.iter_mut().find(|r| r.tag == old.tag) {
                            if new.body.is_empty() {
                                new.body = old.body.clone();
                                new.published_at = new.published_at.take().or(old.published_at.clone());
                                new.name = old.name.clone();
                            }
                        } else {
                            list.releases.push(old.clone());
                        }
                    }
                    sort_releases(&mut list.releases);
                }
                self.store(&list);
                Ok(list)
            }
            Err(redirect_err) => match cached {
                Some(mut c) => {
                    log::warn!("using cached releases for {repo}: {redirect_err:#}");
                    c.source = Source::Cache;
                    Ok(c)
                }
                None => Err(anyhow::anyhow!("{api_err:#}; the github.com fallback failed too: {redirect_err:#}")),
            },
        }
    }

    fn fetch_api(&self, repo: &str, cached: Option<&ReleaseList>) -> anyhow::Result<ReleaseList> {
        let url = format!("{}/repos/{repo}/releases?per_page=30", self.api_base);
        let mut req = self
            .agent
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let cached_etag = cached.filter(|c| c.source == Source::Api).and_then(|c| c.etag.clone());
        if let Some(etag) = &cached_etag {
            req = req.header("If-None-Match", etag);
        }
        let mut resp = req.call()?;
        let status = resp.status().as_u16();
        if status == 304 {
            let mut list = cached.cloned().expect("304 only with a cached etag");
            list.fetched_at = now_secs();
            return Ok(list);
        }
        if status == 403 || status == 429 {
            let remaining = resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()).unwrap_or("?");
            anyhow::bail!("GitHub API refused the request (HTTP {status}, rate limit remaining: {remaining})");
        }
        check_status(&url, status)?;
        let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
        let api: Vec<ApiRelease> = resp.body_mut().with_config().limit(32 * 1024 * 1024).read_json()?;
        let mut releases: Vec<Release> = api.into_iter().filter(|r| !r.draft).map(Release::from).collect();
        sort_releases(&mut releases);
        Ok(ReleaseList { repo: repo.into(), releases, source: Source::Api, fetched_at: now_secs(), etag })
    }

    /// Find the latest tag from the `releases/latest/download/` redirect, then list its assets
    /// from `SHA256SUMS.txt`.
    fn fetch_latest_redirect(&self, repo: &str) -> anyhow::Result<ReleaseList> {
        let probe = format!("{}/{repo}/releases/latest/download/{SUMS_FILE}", self.web_base);
        let resp = self.agent.get(&probe).config().max_redirects(0).build().call()?;
        let status = resp.status().as_u16();
        anyhow::ensure!((300..400).contains(&status), "{probe} returned HTTP {status}, not a redirect");
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow::anyhow!("{probe} redirected without a Location"))?;
        let tag = tag_from_download_url(location)
            .ok_or_else(|| anyhow::anyhow!("could not find a tag in redirect {location}"))?;

        let release = self.release_by_tag(repo, &tag)?;
        Ok(ReleaseList {
            repo: repo.into(),
            releases: vec![release],
            source: Source::LatestRedirect,
            fetched_at: now_secs(),
            etag: None,
        })
    }

    /// A release from its `SHA256SUMS.txt` alone (no API needed): its files and checksums.
    fn release_from_sums(&self, repo: &str, tag: &str) -> anyhow::Result<Release> {
        let tag = tag.to_string();
        let sums_url = download_url(&self.web_base, repo, &tag, SUMS_FILE);
        let mut resp = self.agent.get(&sums_url).call()?;
        check_status(&sums_url, resp.status().as_u16())?;
        let sums = parse_sums(&resp.body_mut().read_to_string()?);
        anyhow::ensure!(!sums.is_empty(), "{sums_url} lists no files");

        let mut assets: Vec<Asset> = sums
            .into_iter()
            .map(|(name, sha)| Asset {
                url: download_url(&self.web_base, repo, &tag, &name),
                name,
                size: None,
                sha256: Some(sha),
            })
            .collect();
        assets.sort_by(|a, b| a.name.cmp(&b.name));
        let version = version::parse_tag(&tag);
        Ok(Release {
            prerelease: version.as_ref().is_some_and(|v| !v.pre.is_empty()),
            name: tag.clone(),
            html_url: format!("{}/{repo}/releases/tag/{tag}", self.web_base),
            tag,
            version,
            published_at: None,
            body: String::new(),
            assets,
        })
    }

    /// A release from its page on github.com (no API needed), for repositories that publish no
    /// `SHA256SUMS.txt`: its files, with checksums filled in later from sidecar files.
    fn release_from_page(&self, repo: &str, tag: &str) -> anyhow::Result<Release> {
        let page = format!("{}/{repo}/releases/expanded_assets/{tag}", self.web_base);
        let html = self.fetch_text(&page)?;
        let names = asset_links(&html, repo, tag);
        anyhow::ensure!(!names.is_empty(), "{page} lists no files");
        let mut assets: Vec<Asset> = names
            .into_iter()
            .map(|name| Asset { url: download_url(&self.web_base, repo, tag, &name), name, size: None, sha256: None })
            .collect();
        assets.sort_by(|a, b| a.name.cmp(&b.name));
        let version = version::parse_tag(tag);
        Ok(Release {
            prerelease: version.as_ref().is_some_and(|v| !v.pre.is_empty()),
            name: tag.to_string(),
            html_url: format!("{}/{repo}/releases/tag/{tag}", self.web_base),
            tag: tag.to_string(),
            version,
            published_at: None,
            body: String::new(),
            assets,
        })
    }

    /// One release by its tag, found without the API, for installing an older version while the
    /// API is rate limited.
    pub fn release_by_tag(&self, repo: &str, tag: &str) -> anyhow::Result<Release> {
        self.release_from_sums(repo, tag).or_else(|sums_err| {
            self.release_from_page(repo, tag).map_err(|err| anyhow::anyhow!("{sums_err:#}, and {err:#}"))
        })
    }

    /// Fill in missing checksums from the release's `SHA256SUMS.txt`, if it has one.
    pub fn fill_checksums(&self, release: &mut Release) -> anyhow::Result<()> {
        if release.assets.iter().all(|a| a.sha256.is_some()) {
            return Ok(());
        }
        // `SHA256SUMS.txt` (the ArtCraft apps), or another checksum list (other projects).
        let Some(sums_asset) =
            release.asset(SUMS_FILE).or_else(|| release.assets.iter().find(|a| is_checksum_list(&a.name)))
        else {
            return Ok(());
        };
        let sums = parse_sums(&self.fetch_text(&sums_asset.url.clone())?);
        for asset in &mut release.assets {
            if asset.sha256.is_none() {
                asset.sha256 = sums.get(&asset.name).cloned();
            }
        }
        Ok(())
    }

    /// The checksum of one file: from a checksum list, or a `<file>.sha256` next to it (as many
    /// projects outside ArtCraft publish).
    pub fn fill_checksum(&self, release: &mut Release, name: &str) -> anyhow::Result<()> {
        self.fill_checksums(release)?;
        let Some(i) = release.assets.iter().position(|a| a.name == name) else { return Ok(()) };
        if release.assets[i].sha256.is_some() {
            return Ok(());
        }
        let sidecar = [".sha256", ".sha256sum", ".sha256.txt"]
            .iter()
            .find_map(|ext| release.asset(&format!("{name}{ext}")).map(|a| a.url.clone()));
        if let Some(url) = sidecar {
            let text = self.fetch_text(&url)?;
            // "<hash>  <name>", or just the hash.
            let hash = parse_sums(&text).remove(name).or_else(|| {
                let h = text.split_whitespace().next()?.to_ascii_lowercase();
                (h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
            });
            release.assets[i].sha256 = hash;
        }
        Ok(())
    }

    fn fetch_text(&self, url: &str) -> anyhow::Result<String> {
        let mut resp = self.agent.get(url).call()?;
        check_status(url, resp.status().as_u16())?;
        Ok(resp.body_mut().read_to_string()?)
    }
}

/// The names of the files a release page links to (`/<repo>/releases/download/<tag>/<name>`).
fn asset_links(html: &str, repo: &str, tag: &str) -> Vec<String> {
    let prefix = format!("/{repo}/releases/download/{tag}/");
    let mut names: Vec<String> = Vec::new();
    for part in html.split("href=\"").skip(1) {
        let Some(link) = part.split('"').next() else { continue };
        let path = link.strip_prefix("https://github.com").unwrap_or(link);
        // Repository names in links can differ in case from what was typed.
        if path.len() > prefix.len() && path[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            let name = percent_decode(&path[prefix.len()..]);
            if !name.is_empty() && !name.contains('/') && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

pub fn download_url(web_base: &str, repo: &str, tag: &str, name: &str) -> String {
    format!("{web_base}/{repo}/releases/download/{tag}/{name}")
}

/// `https://github.com/o/r/releases/download/v1.2.3/FILE` → `v1.2.3`.
fn tag_from_download_url(url: &str) -> Option<String> {
    let rest = &url[url.find("/releases/download/")? + "/releases/download/".len()..];
    let tag = rest.split('/').next()?;
    (!tag.is_empty()).then(|| percent_decode(tag))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse `sha256sum` output: `<hex>  <name>` or `<hex> *<name>` per line.
/// Checksum lists projects publish under other names: `checksums.txt`, `sha256sums`,
/// `tool_1.2.3_checksums.txt`.
fn is_checksum_list(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "sha256sums" || n.ends_with("sha256sums.txt") || n.ends_with("checksums.txt") || n == "checksums.sha256"
}

pub fn parse_sums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (hash, name) = line.split_once(char::is_whitespace)?;
            let name = name.trim_start().trim_start_matches('*').trim();
            let name = name.rsplit('/').next().unwrap_or(name);
            (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && !name.is_empty())
                .then(|| (name.to_string(), hash.to_ascii_lowercase()))
        })
        .collect()
}

fn sort_releases(releases: &mut [Release]) {
    releases.sort_by(|a, b| b.version.cmp(&a.version).then_with(|| b.published_at.cmp(&a.published_at)));
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    name: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    /// `sha256:<hex>` on releases published since mid-2025.
    digest: Option<String>,
}

impl From<ApiRelease> for Release {
    fn from(r: ApiRelease) -> Release {
        let version = version::parse_tag(&r.tag_name);
        Release {
            name: r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone()),
            prerelease: r.prerelease || version.as_ref().is_some_and(|v| !v.pre.is_empty()),
            version,
            tag: r.tag_name,
            published_at: r.published_at,
            body: r.body.unwrap_or_default(),
            html_url: r.html_url,
            assets: r
                .assets
                .into_iter()
                .map(|a| Asset {
                    sha256: a.digest.and_then(|d| d.strip_prefix("sha256:").map(str::to_ascii_lowercase)),
                    name: a.name,
                    size: Some(a.size),
                    url: a.browser_download_url,
                })
                .collect(),
        }
    }
}

/// Read a cached release list without a client (used by the UI before the first refresh).
pub fn read_cache(cache_dir: &Path, repo: &str) -> Option<ReleaseList> {
    let bytes = std::fs::read(cache_dir.join(format!("{}.json", repo.replace('/', "__")))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha256sums() {
        let text = "\
ff5668990314162ff75bf2a4d01b87ec25fd7d9143ada42fb0fd8c9894a7b9ab  photocraft-0.5.0-freebsd-x86_64.tar.gz
DA0402C19AA1B65F3CDE310461CC6E41D391791E6097659A34006E526FB9D9CD *dist/photocraft-0.5.0-windows-x64-portable.zip
not a line
";
        let sums = parse_sums(text);
        assert_eq!(sums.len(), 2);
        assert_eq!(
            sums["photocraft-0.5.0-windows-x64-portable.zip"],
            "da0402c19aa1b65f3cde310461cc6e41d391791e6097659a34006e526fb9d9cd"
        );
    }

    #[test]
    fn lists_files_from_a_release_page() {
        let html = r#"<li><a href="/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz" rel="nofollow">
            <a href="/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz.sha256">
            <a href="https://github.com/burntsushi/ripgrep/releases/download/15.2.0/ripgrep_15.2.0-1_amd64.deb">
            <a href="/BurntSushi/ripgrep/archive/refs/tags/15.2.0.zip">
            <a href="/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz">"#;
        assert_eq!(
            asset_links(html, "BurntSushi/ripgrep", "15.2.0"),
            [
                "ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz",
                "ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz.sha256",
                "ripgrep_15.2.0-1_amd64.deb"
            ]
        );
    }

    #[test]
    fn finds_tag_in_redirect() {
        assert_eq!(
            tag_from_download_url("https://github.com/storytold/photocraft/releases/download/v0.5.0/SHA256SUMS.txt"),
            Some("v0.5.0".into())
        );
        assert_eq!(
            tag_from_download_url("https://github.com/o/r/releases/download/app%2Fv1.0/SHA256SUMS.txt"),
            Some("app/v1.0".into())
        );
        assert_eq!(tag_from_download_url("https://github.com/o/r/releases"), None);
    }

    #[test]
    fn converts_api_releases() {
        let json = r#"[
          {"tag_name":"v0.5.0","name":"PhotoCraft v0.5.0","draft":false,"prerelease":false,
           "published_at":"2026-10-08T14:52:00Z","body":"Notes","html_url":"https://github.com/x/y/releases/tag/v0.5.0",
           "assets":[{"name":"photocraft-0.5.0-windows-x64-portable.zip","size":10,
                      "browser_download_url":"https://example.com/a.zip",
                      "digest":"sha256:DA0402C19AA1B65F3CDE310461CC6E41D391791E6097659A34006E526FB9D9CD"}]},
          {"tag_name":"v0.6.0-rc.1","name":"","draft":false,"prerelease":false,
           "published_at":"2026-10-09T00:00:00Z","body":null,"html_url":"h","assets":[]},
          {"tag_name":"v0.7.0","name":"draft","draft":true,"prerelease":false,"published_at":null,"body":null,"html_url":"h","assets":[]}
        ]"#;
        let api: Vec<ApiRelease> = serde_json::from_str(json).unwrap();
        let mut releases: Vec<Release> = api.into_iter().filter(|r| !r.draft).map(Release::from).collect();
        sort_releases(&mut releases);
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag, "v0.6.0-rc.1");
        assert!(releases[0].prerelease);
        assert_eq!(releases[0].name, "v0.6.0-rc.1");
        assert_eq!(releases[1].date(), Some("2026-10-08"));
        assert_eq!(
            releases[1].assets[0].sha256.as_deref(),
            Some("da0402c19aa1b65f3cde310461cc6e41d391791e6097659a34006e526fb9d9cd")
        );
        let list = ReleaseList { repo: "x/y".into(), releases, source: Source::Api, fetched_at: 0, etag: None };
        assert_eq!(list.latest(false).unwrap().tag, "v0.5.0");
        assert_eq!(list.latest(true).unwrap().tag, "v0.6.0-rc.1");
    }
}
