//! Fonts and add-on packs.
//!
//! **Fonts** come from a manifest like craft-fonts' `fonts/manifest.txt`
//! (`family | style | file | scripts | licence | licence file | sha256 | source`). Each file is
//! downloaded, checked against its SHA-256 and installed for the current user, so every app
//! (ArtCraft or not) can use it:
//!
//! | | Folder | Registration |
//! |---|---|---|
//! | Windows | `%LOCALAPPDATA%\Microsoft\Windows\Fonts` | `HKCU\…\Fonts` + `AddFontResource` |
//! | Linux | `~/.local/share/fonts/craftspace` | `fc-cache` |
//! | macOS | `~/Library/Fonts` | none needed |
//!
//! **Packs** are zips (with a SHA-256 in the catalog) unpacked into an app's preferences folder,
//! e.g. brush presets into PhotoCraft's `Presets` folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::Addon;
use crate::download::{self, Progress, Stage};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontFile {
    pub family: String,
    pub style: String,
    /// Path relative to the manifest's `base_url`.
    pub file: String,
    pub scripts: Vec<String>,
    pub license: String,
    pub sha256: String,
}

impl FontFile {
    pub fn file_name(&self) -> &str {
        self.file.rsplit('/').next().unwrap_or(&self.file)
    }
}

/// An installed font file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstalledFont {
    pub family: String,
    pub style: String,
    pub path: PathBuf,
    pub sha256: String,
    #[serde(default)]
    pub registry: Option<String>,
}

pub fn parse_manifest(text: &str) -> Vec<FontFile> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
            (f.len() >= 7 && f[6].len() == 64).then(|| FontFile {
                family: f[0].into(),
                style: f[1].into(),
                file: f[2].into(),
                scripts: f[3].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                license: f[4].into(),
                sha256: f[6].to_ascii_lowercase(),
            })
        })
        .collect()
}

/// Human names for ISO 15924 script codes in the manifest.
pub fn script_name(code: &str) -> &str {
    match code {
        "Jpan" => "Japanese",
        "Hans" => "Simplified Chinese",
        "Hant" => "Traditional Chinese",
        "Kore" => "Korean",
        "Arab" => "Arabic",
        "Latn" => "Latin",
        "Cyrl" => "Cyrillic",
        "Grek" => "Greek",
        "Hebr" => "Hebrew",
        "Deva" => "Devanagari",
        "Thai" => "Thai",
        other => other,
    }
}

/// The per-user font folder.
pub fn fonts_dir() -> anyhow::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CRAFTSPACE_FONTS_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let base = directories::BaseDirs::new().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    Ok(if cfg!(windows) {
        base.data_local_dir().join(r"Microsoft\Windows\Fonts")
    } else if cfg!(target_os = "macos") {
        base.home_dir().join("Library/Fonts")
    } else {
        base.data_local_dir().join("fonts/craftspace")
    })
}

pub fn fetch_manifest(agent: &ureq::Agent, addon: &Addon) -> anyhow::Result<Vec<FontFile>> {
    let mut resp = agent.get(&addon.source).call()?;
    crate::http::check_status(&addon.source, resp.status().as_u16())?;
    let fonts = parse_manifest(&resp.body_mut().read_to_string()?);
    anyhow::ensure!(!fonts.is_empty(), "{} lists no fonts", addon.source);
    Ok(fonts)
}

/// Download, verify and install `font`. `staging` holds the download.
pub fn install_font(
    agent: &ureq::Agent,
    addon: &Addon,
    font: &FontFile,
    staging: &Path,
    progress: &Progress,
) -> anyhow::Result<InstalledFont> {
    let base = addon.base_url.as_deref().unwrap_or_else(|| addon.source.rsplit_once('/').map(|(b, _)| b).unwrap_or(""));
    let url = format!("{}/{}", base.trim_end_matches('/'), font.file);
    let tmp = staging.join(font.file_name());
    download::download(agent, &url, &tmp, Some(&font.sha256), progress)?;
    progress.stage(Stage::Installing);
    let dir = fonts_dir()?;
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(font.file_name());
    std::fs::copy(&tmp, &target)?;
    let _ = std::fs::remove_file(&tmp);
    let registry = register(font, &target)?;
    Ok(InstalledFont {
        family: font.family.clone(),
        style: font.style.clone(),
        path: target,
        sha256: font.sha256.clone(),
        registry,
    })
}

pub fn uninstall_font(font: &InstalledFont) {
    #[cfg(windows)]
    {
        unregister_windows(&font.path);
        if let Some(record) = &font.registry {
            crate::integrate::windows::remove_registry(std::slice::from_ref(record));
        }
    }
    if let Err(err) = std::fs::remove_file(&font.path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            log::warn!("couldn't remove {}: {err}", font.path.display());
        }
    }
}

/// Tell the system about new or removed fonts.
pub fn refresh_font_cache() {
    if cfg!(all(unix, not(target_os = "macos"))) {
        if let Ok(dir) = fonts_dir() {
            let _ = std::process::Command::new("fc-cache")
                .arg("-f")
                .arg(dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

#[cfg(windows)]
fn register(font: &FontFile, path: &Path) -> anyhow::Result<Option<String>> {
    use std::os::windows::ffi::OsStrExt;
    let kind = if font.file.to_ascii_lowercase().ends_with(".otf") { "OpenType" } else { "TrueType" };
    let name = format!("{} {} ({kind})", font.family, font.style);
    let record = crate::integrate::windows::register_font(&name, path)?;
    // Make it usable now, not only after the next sign-in.
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        windows_sys::Win32::Graphics::Gdi::AddFontResourceW(wide.as_ptr());
        windows_sys::Win32::UI::WindowsAndMessaging::SendNotifyMessageW(
            windows_sys::Win32::UI::WindowsAndMessaging::HWND_BROADCAST,
            windows_sys::Win32::UI::WindowsAndMessaging::WM_FONTCHANGE,
            0,
            0,
        );
    }
    Ok(Some(record))
}

#[cfg(windows)]
fn unregister_windows(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        windows_sys::Win32::Graphics::Gdi::RemoveFontResourceW(wide.as_ptr());
    }
}

#[cfg(not(windows))]
fn register(_font: &FontFile, _path: &Path) -> anyhow::Result<Option<String>> {
    Ok(None)
}

/// Unpack a pack's zip into `target`; returns the files written, for uninstalling.
pub fn install_pack(
    agent: &ureq::Agent,
    addon: &Addon,
    target: &Path,
    staging: &Path,
    progress: &Progress,
) -> anyhow::Result<Vec<PathBuf>> {
    let sha =
        addon.sha256.as_deref().ok_or_else(|| anyhow::anyhow!("{} has no checksum in the catalog", addon.name))?;
    let zip = staging.join(format!("{}.zip", addon.id));
    download::download(agent, &addon.source, &zip, Some(sha), progress)?;
    progress.stage(Stage::Installing);
    let unpacked = staging.join(format!("{}-unpacked", addon.id));
    if unpacked.exists() {
        std::fs::remove_dir_all(&unpacked)?;
    }
    crate::archive::unpack(&zip, &unpacked, progress)?;
    let mut written = Vec::new();
    for entry in walkdir::WalkDir::new(&unpacked).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(&unpacked)?;
        let dest = target.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(entry.path(), &dest)?;
        written.push(dest);
    }
    let _ = std::fs::remove_dir_all(&unpacked);
    let _ = std::fs::remove_file(&zip);
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_craft_fonts_manifest() {
        // Two real lines from storytold/craft-fonts fonts/manifest.txt.
        let text = "# comment\n\
Shippori Mincho | Regular | fonts/shippori-mincho/ShipporiMincho-Regular.ttf | Jpan,Latn | OFL-1.1 | fonts/shippori-mincho/OFL.txt | 769b5269f0f9bc6534b352c0e6bd856a566e03ff788f107191c2d835863570b2 | https://github.com/google/fonts\n\
BIZ UDPGothic | Bold | fonts/biz-ud-pgothic/BIZUDPGothic-Bold.ttf | Jpan,Latn | OFL-1.1 | fonts/biz-ud-pgothic/OFL.txt | 30eba52fc837e8b62c97d4b82e6706583149fb7294e3712dd71a655eaea80a90 | https://github.com/googlefonts\n\
broken | line\n";
        let fonts = parse_manifest(text);
        assert_eq!(fonts.len(), 2);
        assert_eq!(fonts[0].family, "Shippori Mincho");
        assert_eq!(fonts[0].file_name(), "ShipporiMincho-Regular.ttf");
        assert_eq!(fonts[1].scripts, ["Jpan", "Latn"]);
        assert_eq!(script_name("Jpan"), "Japanese");
    }

    #[test]
    fn installs_fonts_and_packs_from_a_server() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("CRAFTSPACE_FONTS_DIR", tmp.path().join("fonts"));
        let font_bytes = b"fake font".to_vec();
        let sha = crate::download::sha256_file(&{
            let p = tmp.path().join("f");
            std::fs::write(&p, &font_bytes).unwrap();
            p
        })
        .unwrap();
        let url = crate::download::tests::serve(font_bytes.clone());
        let base = url.trim_end_matches("/file").to_string();
        let addon = Addon {
            id: "fonts".into(),
            name: "Fonts".into(),
            kind: crate::catalog::AddonKind::Fonts,
            description: String::new(),
            source: format!("{base}/manifest.txt"),
            base_url: Some(base),
            sha256: None,
            app: None,
            target: None,
            homepage: None,
        };
        let font = FontFile {
            family: "Test Sans".into(),
            style: "Regular".into(),
            file: "file".into(),
            scripts: vec![],
            license: "OFL-1.1".into(),
            sha256: sha,
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let report = |_| {};
        let progress = Progress { report: &report, cancel: &cancel };
        let installed =
            install_font(&crate::http::agent(), &addon, &font, &tmp.path().join("staging"), &progress).unwrap();
        assert_eq!(std::fs::read(&installed.path).unwrap(), font_bytes);
        uninstall_font(&installed);
        assert!(!installed.path.exists());

        // A pack: a zip unpacked into the target folder.
        let zip = tmp.path().join("pack.zip");
        crate::archive::tests::make_zip(&zip, &[("pack/Brushes/soft.pcbrushes", b"{}"), ("pack/tips/a.pctip", b"t")]);
        let zip_sha = crate::download::sha256_file(&zip).unwrap();
        let pack = Addon {
            kind: crate::catalog::AddonKind::PresetPack,
            source: crate::download::tests::serve(std::fs::read(&zip).unwrap()),
            sha256: Some(zip_sha),
            ..addon
        };
        let target = tmp.path().join("Photocraft/Presets");
        let files =
            install_pack(&crate::http::agent(), &pack, &target, &tmp.path().join("staging"), &progress).unwrap();
        assert_eq!(files.len(), 2);
        assert!(target.join("Brushes/soft.pcbrushes").is_file());
        std::env::remove_var("CRAFTSPACE_FONTS_DIR");
    }
}
