//! Screenshots and a feature tour for each app's page.
//!
//! Two sources, both published by the apps themselves:
//! * the AppStream **metainfo** file (`packaging/linux/ai.storyteller.<app>.metainfo.xml.in`):
//!   the official screenshots with captions, the description and its feature list, and links
//!   (homepage, bug tracker, help, contact);
//! * the **README**, whose screenshots carry detailed alt text, grouped under the section they
//!   appear in ("Color", "Effects and motion"…).
//!
//! Images are downloaded on demand and cached on disk.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ureq::Agent;

use crate::catalog::AppEntry;
use crate::paths::write_atomic;

pub const MAX_AGE: Duration = Duration::from_secs(12 * 3600);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tour {
    pub summary: Option<String>,
    pub paragraphs: Vec<String>,
    /// Bullet points from the description.
    pub features: Vec<String>,
    pub slides: Vec<Slide>,
    /// `(kind, url)`: homepage, bugtracker, help, contact…
    pub links: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    pub image: String,
    pub caption: String,
    /// The README section the screenshot belongs to.
    pub section: Option<String>,
}

/// Where an app's metainfo lives: the catalog's `metainfo`, or the ArtCraft convention.
pub fn metainfo_url(app: &AppEntry) -> Option<String> {
    if let Some(url) = &app.metainfo {
        return Some(url.clone());
    }
    app.repo.starts_with("storytold/").then(|| {
        format!(
            "https://raw.githubusercontent.com/{}/HEAD/packaging/linux/ai.storyteller.{}.metainfo.xml.in",
            app.repo, app.id
        )
    })
}

/// The tour for `app`, from the cache when fresh.
pub fn fetch(agent: &Agent, app: &AppEntry, cache_dir: &Path, force: bool) -> anyhow::Result<Tour> {
    let cache = cache_dir.join(format!("{}.json", app.id));
    let fresh =
        std::fs::metadata(&cache).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e < MAX_AGE));
    if fresh && !force {
        if let Some(t) = std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            return Ok(t);
        }
    }
    let raw = format!("https://raw.githubusercontent.com/{}/HEAD/", app.repo);
    let mut tour = match metainfo_url(app).map(|u| get_text(agent, &u)) {
        Some(Ok(xml)) => parse_metainfo(&xml),
        Some(Err(err)) => {
            log::info!("no metainfo for {}: {err:#}", app.id);
            Tour::default()
        }
        None => Tour::default(),
    };
    match get_text(agent, &format!("{raw}README.md")) {
        Ok(md) => {
            for slide in readme_slides(&md, &raw) {
                if !tour.slides.iter().any(|s| same_image(&s.image, &slide.image)) {
                    tour.slides.push(slide);
                }
            }
        }
        Err(err) => log::info!("no README for {}: {err:#}", app.id),
    }
    anyhow::ensure!(
        !tour.slides.is_empty() || !tour.paragraphs.is_empty(),
        "{} publishes no screenshots or description",
        app.name
    );
    let _ = write_atomic(&cache, &serde_json::to_vec(&tour)?);
    Ok(tour)
}

/// `…/main/docs/a.png` and `…/HEAD/docs/a.png` are the same picture.
fn same_image(a: &str, b: &str) -> bool {
    let tail = |s: &str| s.rsplit_once("/docs/").map(|(_, t)| t.to_string()).unwrap_or_else(|| s.to_string());
    a == b || tail(a) == tail(b)
}

fn get_text(agent: &Agent, url: &str) -> anyhow::Result<String> {
    let mut resp = agent.get(url).call()?;
    crate::http::check_status(url, resp.status().as_u16())?;
    Ok(resp.body_mut().with_config().limit(8 << 20).read_to_string()?)
}

/// Pull the parts of an AppStream metainfo file the tour uses.
pub fn parse_metainfo(xml: &str) -> Tour {
    let mut tour = Tour { summary: tag_text(xml, "summary").map(clean), ..Tour::default() };
    if let Some(desc) = section(xml, "description") {
        tour.paragraphs = all_tags(desc, "p").into_iter().map(clean).filter(|p| !p.is_empty()).collect();
        tour.features = all_tags(desc, "li").into_iter().map(clean).filter(|p| !p.is_empty()).collect();
    }
    for shot in all_tags(xml, "screenshot") {
        if let Some(image) = tag_text(shot, "image").map(|s| s.trim().to_string()) {
            tour.slides.push(Slide {
                caption: tag_text(shot, "caption").map(clean).unwrap_or_default(),
                image,
                section: None,
            });
        }
    }
    let mut rest = xml;
    while let Some(i) = rest.find("<url type=\"") {
        let after = &rest[i + "<url type=\"".len()..];
        let Some(q) = after.find('"') else { break };
        let kind = after[..q].to_string();
        if let Some((url, _)) = after[q..].split_once('>').and_then(|(_, r)| r.split_once("</url>")) {
            tour.links.push((kind, url.trim().to_string()));
        }
        rest = &after[q..];
    }
    tour
}

/// Screenshots in a README (`<img src alt>` and `![alt](src)`), with the heading above them.
/// Logos, badges and vector art are skipped.
pub fn readme_slides(md: &str, raw_base: &str) -> Vec<Slide> {
    let mut slides = Vec::new();
    let mut heading: Option<String> = None;
    let mut in_code = false;
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if let Some(h) = t.strip_prefix("## ").or_else(|| t.strip_prefix("### ")) {
            heading = Some(clean(h.trim_matches('#').trim()));
            continue;
        }
        let mut rest = t;
        while let Some(i) = rest.find("<img") {
            let tag_end = rest[i..].find('>').map(|j| i + j + 1).unwrap_or(rest.len());
            let tag = &rest[i..tag_end];
            if let Some(src) = attr(tag, "src") {
                push_slide(&mut slides, src, attr(tag, "alt").unwrap_or(""), &heading, raw_base);
            }
            rest = &rest[tag_end..];
        }
        let mut rest = t;
        while let Some(i) = rest.find("![") {
            let after = &rest[i + 2..];
            let Some((alt, tail)) = after.split_once("](") else { break };
            let Some((src, tail)) = tail.split_once(')') else { break };
            push_slide(&mut slides, src.split_whitespace().next().unwrap_or(src), alt, &heading, raw_base);
            rest = tail;
        }
    }
    slides
}

fn push_slide(slides: &mut Vec<Slide>, src: &str, alt: &str, heading: &Option<String>, raw_base: &str) {
    let lower = src.to_ascii_lowercase();
    let skip = lower.ends_with(".svg")
        || lower.contains("shields.io")
        || lower.contains("badge")
        || lower.contains("logo")
        || lower.contains("/brand/")
        || !(lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".webp"));
    if skip {
        return;
    }
    let image =
        if src.starts_with("http") { src.to_string() } else { format!("{raw_base}{}", src.trim_start_matches("./")) };
    if slides.iter().any(|s: &Slide| s.image == image) {
        return;
    }
    slides.push(Slide { image, caption: clean(alt), section: heading.clone() });
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(&tag[i..j])
}

fn section<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let i = xml.find(&open)?;
    let start = i + xml[i..].find('>')? + 1;
    let end = start + xml[start..].find(&close)?;
    Some(&xml[start..end])
}

fn tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    section(xml, tag)
}

fn all_tags<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = xml;
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    while let Some(i) = rest.find(&open) {
        // Don't mistake <screenshots> for <screenshot>.
        let next = rest.as_bytes().get(i + open.len()).copied();
        if !matches!(next, Some(b'>') | Some(b' ') | Some(b'\n') | Some(b'\t')) {
            rest = &rest[i + open.len()..];
            continue;
        }
        let Some(gt) = rest[i..].find('>') else { break };
        let start = i + gt + 1;
        let Some(len) = rest[start..].find(&close) else { break };
        out.push(&rest[start..start + len]);
        rest = &rest[start + len + close.len()..];
    }
    out
}

/// Collapse whitespace and unescape entities.
fn clean(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    joined
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// A screenshot from the cache, or downloaded into it.
pub fn image(agent: &Agent, url: &str, cache_dir: &Path) -> anyhow::Result<PathBuf> {
    use sha2::Digest;
    let ext = url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("img");
    let name = format!("{}.{ext}", &hex::encode(sha2::Sha256::digest(url.as_bytes()))[..24]);
    let path = cache_dir.join(name);
    if path.is_file() {
        return Ok(path);
    }
    let mut resp = agent.get(url).call()?;
    crate::http::check_status(url, resp.status().as_u16())?;
    let bytes = resp.body_mut().with_config().limit(32 << 20).read_to_vec()?;
    write_atomic(&path, &bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from storytold/photocraft packaging/linux/ai.storyteller.photocraft.metainfo.xml.in.
    const METAINFO: &str = r#"<component type="desktop-application">
  <summary>Edit photos and layered Photoshop documents</summary>
  <description>
    <p>
      PhotoCraft is an open-source, native image editor that works the way Photoshop users
      expect.
    </p>
    <ul>
      <li>Open, edit and save layered PSD and PSB files</li>
      <li>A GPU compositor with multithreaded filters</li>
    </ul>
  </description>
  <url type="homepage">https://getartcraft.com/apps/photocraft</url>
  <url type="bugtracker">https://github.com/storytold/photocraft/issues</url>
  <screenshots>
    <screenshot type="default">
      <caption>Editing The Great Wave off Kanagawa with type layers &amp; adjustment layers</caption>
      <image>https://raw.githubusercontent.com/storytold/photocraft/main/docs/images/photocraft-demo.jpg</image>
    </screenshot>
  </screenshots>
</component>"#;

    #[test]
    fn reads_metainfo() {
        let t = parse_metainfo(METAINFO);
        assert_eq!(t.summary.as_deref(), Some("Edit photos and layered Photoshop documents"));
        assert_eq!(
            t.paragraphs,
            ["PhotoCraft is an open-source, native image editor that works the way Photoshop users expect."]
        );
        assert_eq!(t.features.len(), 2);
        assert_eq!(t.slides.len(), 1);
        assert_eq!(t.slides[0].caption, "Editing The Great Wave off Kanagawa with type layers & adjustment layers");
        assert_eq!(
            t.links[1],
            ("bugtracker".to_string(), "https://github.com/storytold/photocraft/issues".to_string())
        );
    }

    #[test]
    fn finds_readme_screenshots_with_sections() {
        let md = r#"<p align="center"><img alt="ArtCraft" src="docs/brand/artcraft-logo.svg" width="200"></p>
<img alt="Rust" src="https://img.shields.io/badge/100%25-Rust-b7410e">

## Color

<img src="docs/images/filmcraft-color.png" alt="Color workspace on Charade (1963)" width="100%">

```
<img src="docs/images/not-a-real-one.png">
```

### Audio
![Mixer with &quot;ducking&quot;](docs/images/mixer.jpg "title")
![remote](https://example.com/x.webp)
"#;
        let slides = readme_slides(md, "RAW/");
        assert_eq!(slides.len(), 3, "{slides:?}");
        assert_eq!(slides[0].image, "RAW/docs/images/filmcraft-color.png");
        assert_eq!(slides[0].section.as_deref(), Some("Color"));
        assert_eq!(slides[1].caption, "Mixer with \"ducking\"");
        assert_eq!(slides[1].section.as_deref(), Some("Audio"));
        assert_eq!(slides[2].image, "https://example.com/x.webp");
    }

    #[test]
    fn same_picture_on_different_refs() {
        assert!(same_image(
            "https://raw.githubusercontent.com/storytold/photocraft/main/docs/images/photocraft-demo.jpg",
            "https://raw.githubusercontent.com/storytold/photocraft/HEAD/docs/images/photocraft-demo.jpg"
        ));
    }
}
