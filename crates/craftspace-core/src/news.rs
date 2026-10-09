//! News and tutorials from the ArtCraft website, plus each app's README.
//!
//! The site lists its pages in `sitemap.xml`; each article page carries a `<title>`, a
//! `description` and (for news) an `article:published_time` meta tag. Results are cached for a
//! few hours.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ureq::Agent;

use crate::paths::write_atomic;

pub const MAX_AGE: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArticleKind {
    News,
    Tutorial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Article {
    pub kind: ArticleKind,
    pub url: String,
    pub title: String,
    pub description: String,
    /// `YYYY-MM-DD`, when the page says.
    pub date: Option<String>,
}

/// Articles from `site` (e.g. `https://getartcraft.com`), cached in `cache_file`.
pub fn fetch(agent: &Agent, site: &str, cache_file: &Path, force: bool) -> anyhow::Result<Vec<Article>> {
    let fresh =
        std::fs::metadata(cache_file).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e < MAX_AGE));
    if fresh && !force {
        if let Some(cached) = cached(cache_file) {
            return Ok(cached);
        }
    }
    let site = site.trim_end_matches('/');
    let sitemap = get_text(agent, &format!("{site}/sitemap.xml"))?;
    let urls: Vec<(ArticleKind, String)> = locs(&sitemap)
        .into_iter()
        .filter_map(|url| {
            let path = url.strip_prefix(site)?;
            let kind = if path.starts_with("/news/") {
                ArticleKind::News
            } else if path.starts_with("/tutorials/") {
                ArticleKind::Tutorial
            } else {
                return None;
            };
            Some((kind, url))
        })
        .take(30)
        .collect();

    let mut articles: Vec<Article> = std::thread::scope(|scope| {
        let handles: Vec<_> = urls
            .iter()
            .map(|(kind, url)| {
                scope.spawn(move || get_text(agent, url).ok().map(|html| parse_article(*kind, url, &html)))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    // Newest news first; tutorials keep the site's order.
    articles.sort_by(|a, b| match (a.kind, b.kind) {
        (ArticleKind::News, ArticleKind::News) => b.date.cmp(&a.date),
        (ArticleKind::News, ArticleKind::Tutorial) => std::cmp::Ordering::Less,
        (ArticleKind::Tutorial, ArticleKind::News) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });
    if let Ok(json) = serde_json::to_vec(&articles) {
        let _ = write_atomic(cache_file, &json);
    }
    Ok(articles)
}

pub fn cached(cache_file: &Path) -> Option<Vec<Article>> {
    serde_json::from_slice(&std::fs::read(cache_file).ok()?).ok()
}

fn get_text(agent: &Agent, url: &str) -> anyhow::Result<String> {
    let mut resp = agent.get(url).call()?;
    crate::http::check_status(url, resp.status().as_u16())?;
    Ok(resp.body_mut().with_config().limit(8 << 20).read_to_string()?)
}

fn locs(sitemap: &str) -> Vec<String> {
    sitemap.split("<loc>").skip(1).filter_map(|s| s.split_once("</loc>").map(|(url, _)| unescape(url.trim()))).collect()
}

pub fn parse_article(kind: ArticleKind, url: &str, html: &str) -> Article {
    let title = between(html, "<title>", "</title>").map(unescape).unwrap_or_default();
    // "Some Title — ArtCraft" → "Some Title".
    let title = match title.rsplit_once(" — ") {
        Some((t, _)) if !t.is_empty() => t.to_string(),
        _ => title,
    };
    Article {
        kind,
        url: url.to_string(),
        title,
        description: meta(html, "name=\"description\"").unwrap_or_default(),
        date: meta(html, "property=\"article:published_time\"").map(|d| d.chars().take(10).collect()),
    }
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

/// The `content` of the `<meta …>` tag containing `attr`.
fn meta(html: &str, attr: &str) -> Option<String> {
    let tag_start = html.find(attr).map(|i| html[..i].rfind("<meta").unwrap_or(i))?;
    let tag = &html[tag_start..tag_start + html[tag_start..].find('>')?];
    between(tag, "content=\"", "\"").map(unescape)
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// An app's README from its repository, with relative links and images made absolute.
pub fn fetch_readme(agent: &Agent, repo: &str, cache_file: &Path) -> anyhow::Result<String> {
    let fresh =
        std::fs::metadata(cache_file).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e < MAX_AGE));
    if fresh {
        if let Ok(text) = std::fs::read_to_string(cache_file) {
            return Ok(text);
        }
    }
    let raw = format!("https://raw.githubusercontent.com/{repo}/HEAD/");
    let text = get_text(agent, &format!("{raw}README.md"))?;
    let text = absolutize_links(&html_to_markdown(&text), &raw, &format!("https://github.com/{repo}/blob/HEAD/"));
    let _ = write_atomic(cache_file, text.as_bytes());
    Ok(text)
}

/// READMEs on GitHub often use HTML for centred headers and badges, which a Markdown view shows
/// as raw tags. Turn the common ones into Markdown and drop the rest (outside code blocks).
pub fn html_to_markdown(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut in_code = false;
    // Inside an HTML block (until the next blank line) indentation means nothing, so it's
    // dropped rather than turning text into code blocks.
    let mut in_html = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if line.trim().is_empty() {
            in_html = false;
        } else if !in_code && line.trim_start().starts_with('<') {
            in_html = true;
        }
        if in_code {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if line.trim().is_empty() {
            if !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        if !line.contains('<') {
            if in_html {
                out.push_str(&unescape(line.trim_start()));
            } else {
                out.push_str(line);
            }
            out.push('\n');
            continue;
        }
        let mut l = line.to_string();
        for level in 1..=6 {
            let open = format!("<h{level}");
            if let Some(start) = l.find(&open) {
                if let Some(gt) = l[start..].find('>') {
                    let close = format!("</h{level}>");
                    let inner_start = start + gt + 1;
                    let inner_end = l[inner_start..].find(&close).map(|i| inner_start + i).unwrap_or(l.len());
                    let inner = l[inner_start..inner_end].to_string();
                    l = format!("{} {}", "#".repeat(level), inner);
                }
            }
        }
        // Links keep their text; images, badges and other tags go.
        let mut text = String::new();
        let mut rest = l.as_str();
        while let Some(i) = rest.find('<') {
            text.push_str(&rest[..i]);
            match rest[i..].find('>') {
                Some(j) => {
                    let tag = &rest[i..i + j + 1];
                    if tag.starts_with("<br") {
                        text.push_str("  \n");
                    }
                    rest = &rest[i + j + 1..];
                }
                None => {
                    text.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        text.push_str(rest);
        let text = unescape(text.trim_end());
        // Lines that held only tags become nothing; avoid piling up blank lines.
        if text.trim().is_empty() {
            if !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
        } else {
            out.push_str(text.trim_start());
            out.push('\n');
        }
    }
    out
}

/// `](docs/a.png)` → `](<raw>docs/a.png)` for images, `](<blob>docs/x.md)` for links.
pub fn absolutize_links(md: &str, raw_base: &str, blob_base: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut rest = md;
    while let Some(i) = rest.find("](") {
        let (before, after) = rest.split_at(i + 2);
        out.push_str(before);
        let is_image = before[..before.len() - 2].rfind('[').is_some_and(|j| j > 0 && before.as_bytes()[j - 1] == b'!');
        let relative = !(after.starts_with("http")
            || after.starts_with('#')
            || after.starts_with("mailto:")
            || after.starts_with('/'));
        if relative {
            out.push_str(if is_image { raw_base } else { blob_base });
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// A "new issue" link for `repo`, pre-filled with what a maintainer needs to know.
pub fn issue_url(repo: &str, app_name: &str, version: Option<&str>, package: Option<&str>, platform: &str) -> String {
    let body = format!(
        "**What happened?**\n\n\n**What did you expect?**\n\n\n**Steps to reproduce**\n1. \n\n---\n- {app_name}: {}\n- Package: {}\n- OS: {platform}\n- Installed with CraftSpace {}\n",
        version.unwrap_or("not installed"),
        package.unwrap_or("-"),
        env!("CARGO_PKG_VERSION"),
    );
    format!("https://github.com/{repo}/issues/new?body={}", percent_encode(&body))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_article_pages() {
        // Trimmed from https://getartcraft.com/news/welcome.
        let html = r#"<html><head><title>Welcome to the ArtCraft Blog — ArtCraft</title>
<meta name="description" content="We are excited to launch our new blog where we&#x27;ll share updates."/>
<meta property="og:title" content="Welcome to the ArtCraft Blog — ArtCraft"/>
<meta property="article:published_time" content="2026-01-16"/></head></html>"#;
        let a = parse_article(ArticleKind::News, "https://getartcraft.com/news/welcome", html);
        assert_eq!(a.title, "Welcome to the ArtCraft Blog");
        assert_eq!(a.description, "We are excited to launch our new blog where we'll share updates.");
        assert_eq!(a.date.as_deref(), Some("2026-01-16"));

        let t = parse_article(ArticleKind::Tutorial, "u", "<title>2D Editor Basics — ArtCraft</title>");
        assert_eq!(t.title, "2D Editor Basics");
        assert_eq!(t.date, None);
    }

    #[test]
    fn reads_sitemaps() {
        let xml = "<urlset><url><loc>https://getartcraft.com/</loc></url><url><loc>https://getartcraft.com/news/welcome</loc></url></urlset>";
        assert_eq!(locs(xml), ["https://getartcraft.com/", "https://getartcraft.com/news/welcome"]);
    }

    #[test]
    fn turns_readme_html_into_markdown() {
        // Shaped like the top of storytold/photocraft's README.
        let md = "<p align=\"center\">\n  <a href=\"https://getartcraft.com/\"><img alt=\"ArtCraft\" src=\"docs/logo.svg\"></a>\n</p>\n\n<h1 align=\"center\">PhotoCraft</h1>\n\n<p align=\"center\">\n  <b>Image editing; open source.</b><br>\n  Layers &amp; masks.\n</p>\n\n```html\n<keep>this</keep>\n```\n## Plain";
        let out = html_to_markdown(md);
        assert!(out.contains("# PhotoCraft\n"), "{out}");
        assert!(out.contains("Image editing; open source."), "{out}");
        assert!(out.contains("Layers & masks."), "{out}");
        assert!(out.contains("<keep>this</keep>"), "{out}");
        assert!(!out.contains("<img"), "{out}");
        assert!(!out.contains("\n\n\n"), "{out}");
        assert!(out.ends_with("## Plain\n"));
    }

    #[test]
    fn makes_readme_links_absolute() {
        let md = "![shot](docs/a.png) see [guide](docs/guide.md), [site](https://x.y) and [top](#top)";
        let out = absolutize_links(md, "RAW/", "BLOB/");
        assert_eq!(out, "![shot](RAW/docs/a.png) see [guide](BLOB/docs/guide.md), [site](https://x.y) and [top](#top)");
    }

    #[test]
    fn builds_issue_links() {
        let url = issue_url(
            "storytold/photocraft",
            "PhotoCraft",
            Some("0.5.0"),
            Some("photocraft-0.5.0-windows-x64-portable.zip"),
            "Windows x64",
        );
        assert!(url.starts_with("https://github.com/storytold/photocraft/issues/new?body="));
        assert!(url.contains("PhotoCraft%3A%200.5.0"));
        assert!(!url.contains(' '));
    }
}
