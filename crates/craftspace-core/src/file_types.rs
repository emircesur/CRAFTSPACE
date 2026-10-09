//! CraftSpace as the app that opens ArtCraft file types: double-clicking a `.psd` runs
//! `craftspace open <file>`, which opens it in the installed app that handles it, or offers to
//! install that app.
//!
//! - Windows: a `CraftSpace.Document` program ID, listed under each extension's "Open with"
//!   (`OpenWithProgids`) and in Settings › Default apps (Capabilities). Windows only lets the
//!   user pick the default, so CraftSpace opens that page.
//! - Linux: a hidden `craftspace-open.desktop` entry with the MIME types, made the default for
//!   them with `xdg-mime` (and removed from `mimeapps.list` again when turned off).
//! - macOS hands files to apps through Apple events, which CraftSpace doesn't handle yet.

use std::path::{Path, PathBuf};

use crate::catalog::Catalog;

/// The argument CraftSpace takes for opening a file: `craftspace open <file>`.
pub const OPEN_COMMAND: &str = "open";

pub fn supported() -> bool {
    cfg!(windows) || cfg!(all(unix, not(target_os = "macos")))
}

/// Formats many other programs open too (pictures, PDFs, office documents, media). They're
/// left unticked by default, so CraftSpace doesn't take over the picture viewer.
const COMMON: &[&str] = &[
    "png", "jpg", "jpeg", "tif", "tiff", "webp", "gif", "bmp", "avif", "pdf", "mp4", "mov", "mkv", "webm", "wav",
    "mp3", "ogg", "flac", "aif", "aiff", "docx", "doc", "odt", "rtf", "pptx", "ppt", "odp", "xlsx", "xls", "csv",
    "ods", "svg",
];

/// Whether `ext` is ticked by default: the ArtCraft apps' own and professional formats (not
/// sidecar files).
pub fn default_on(ext: &str) -> bool {
    !COMMON.contains(&ext) && ext != "xmp"
}

/// A MIME type for every extension: the standard one, or `application/x-<ext>` for project
/// formats that have none (CraftSpace defines those on Linux). The flag says it's ours.
pub fn mime_type_or_own(ext: &str) -> (String, bool) {
    match mime_type(ext) {
        Some(m) => (m.to_string(), false),
        None => (format!("application/x-{ext}"), true),
    }
}

/// The MIME type for an extension the apps open (`None` for formats without a standard one).
pub fn mime_type(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "psd" => "image/vnd.adobe.photoshop",
        "psb" => "image/x-psb",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tga" => "image/x-tga",
        "exr" => "image/x-exr",
        "hdr" => "image/vnd.radiance",
        "avif" => "image/avif",
        "qoi" => "image/qoi",
        "dng" => "image/x-adobe-dng",
        "cr2" => "image/x-canon-cr2",
        "cr3" => "image/x-canon-cr3",
        "nef" => "image/x-nikon-nef",
        "arw" => "image/x-sony-arw",
        "raf" => "image/x-fuji-raf",
        "orf" => "image/x-olympus-orf",
        "rw2" => "image/x-panasonic-rw2",
        "ai" => "application/illustrator",
        "svg" => "image/svg+xml",
        "svgz" => "image/svg+xml-compressed",
        "eps" => "image/x-eps",
        "idml" => "application/vnd.adobe.indesign-idml-package",
        "indd" => "application/x-adobe-indesign",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "wav" => "audio/x-wav",
        "aif" | "aiff" => "audio/x-aiff",
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "doc" => "application/msword",
        "odt" => "application/vnd.oasis.opendocument.text",
        "rtf" => "application/rtf",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "ppt" => "application/vnd.ms-powerpoint",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "csv" => "text/csv",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "dwg" => "image/vnd.dwg",
        "dxf" => "image/vnd.dxf",
        _ => return None,
    })
}

/// Which app opens `path`: the first installed app that handles its extension, else the first
/// app that does (to offer installing it). `None` when no ArtCraft app opens it.
pub fn choose_app(catalog: &Catalog, path: &Path, is_installed: impl Fn(&str) -> bool) -> Option<(String, bool)> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let apps = catalog.apps_for_extension(&ext);
    apps.iter()
        .find(|a| is_installed(&a.id))
        .map(|a| (a.id.clone(), true))
        .or_else(|| apps.first().map(|a| (a.id.clone(), false)))
}

/// What registering did, for the message the user sees.
#[derive(Debug, Default)]
pub struct Registered {
    pub types: usize,
    /// Windows: the user has to confirm the default in Settings (this is the page to open).
    pub confirm_url: Option<String>,
}

/// Register `exe` as the opener for `extensions`, replacing an earlier registration.
pub fn register(exe: &Path, extensions: &[String]) -> anyhow::Result<Registered> {
    unregister_all()?;
    if extensions.is_empty() {
        return Ok(Registered::default());
    }
    imp::register(exe, extensions)
}

/// Undo [`register`].
pub fn unregister_all() -> anyhow::Result<()> {
    imp::unregister()
}

/// The program to register: the installed copy when there is one (a portable or downloaded
/// copy may be deleted), else the CraftSpace app next to the running program (which may be
/// `craftspace-cli`).
pub fn program(installed: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(p) = installed.filter(|p| p.is_file()) {
        return Ok(p);
    }
    // From an AppImage: the AppImage itself (it passes `open <file>` on to CraftSpace).
    if let Some(appimage) = crate::selfupdate::running_appimage() {
        return Ok(appimage);
    }
    let current = std::env::current_exe()?;
    let app = current.with_file_name(format!("craftspace{}", std::env::consts::EXE_SUFFIX));
    anyhow::ensure!(app.is_file(), "can't find the CraftSpace app next to {}", current.display());
    Ok(app)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const PROG_ID: &str = "CraftSpace.Document";
    const CAPABILITIES: &str = r"Software\CraftSpace\Capabilities";

    pub fn register(exe: &Path, extensions: &[String]) -> anyhow::Result<Registered> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let exe = exe.display().to_string();
        let (prog, _) = hkcu.create_subkey(format!(r"Software\Classes\{PROG_ID}"))?;
        prog.set_value("", &"ArtCraft document")?;
        prog.create_subkey("DefaultIcon")?.0.set_value("", &format!("\"{exe}\",0"))?;
        prog.create_subkey(r"shell\open\command")?.0.set_value("", &format!("\"{exe}\" {OPEN_COMMAND} \"%1\""))?;

        let (caps, _) = hkcu.create_subkey(CAPABILITIES)?;
        caps.set_value("ApplicationName", &"CraftSpace")?;
        caps.set_value(
            "ApplicationDescription",
            &"Opens ArtCraft documents in the right app, and offers to install it if it's missing.",
        )?;
        let (assoc, _) = caps.create_subkey("FileAssociations")?;
        for ext in extensions {
            assoc.set_value(format!(".{ext}"), &PROG_ID)?;
            let (with, _) = hkcu.create_subkey(format!(r"Software\Classes\.{ext}\OpenWithProgids"))?;
            with.set_value(PROG_ID, &"")?;
        }
        hkcu.create_subkey(r"Software\RegisteredApplications")?.0.set_value("CraftSpace", &CAPABILITIES)?;
        notify_shell();
        Ok(Registered {
            types: extensions.len(),
            confirm_url: Some("ms-settings:defaultapps?registeredAppUser=CraftSpace".into()),
        })
    }

    pub fn unregister() -> anyhow::Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(assoc) = hkcu.open_subkey(format!(r"{CAPABILITIES}\FileAssociations")) {
            for (ext, _) in assoc.enum_values().flatten() {
                if let Ok(with) = hkcu.open_subkey_with_flags(
                    format!(r"Software\Classes\{ext}\OpenWithProgids"),
                    winreg::enums::KEY_SET_VALUE,
                ) {
                    let _ = with.delete_value(PROG_ID);
                }
            }
        }
        let _ = hkcu.delete_subkey_all(r"Software\CraftSpace\Capabilities");
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{PROG_ID}"));
        if let Ok(reg) = hkcu.open_subkey_with_flags(r"Software\RegisteredApplications", winreg::enums::KEY_SET_VALUE) {
            let _ = reg.delete_value("CraftSpace");
        }
        notify_shell();
        Ok(())
    }

    /// Tell Explorer the file associations changed.
    fn notify_shell() {
        use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::*;
    use std::process::{Command, Stdio};

    pub const DESKTOP_FILE: &str = "craftspace-open.desktop";

    fn applications_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| b.data_dir().join("applications"))
    }

    fn mimeapps_list() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| b.config_dir().join("mimeapps.list"))
    }

    fn mime_package() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| b.data_dir().join("mime/packages/craftspace.xml"))
    }

    pub fn register(exe: &Path, extensions: &[String]) -> anyhow::Result<Registered> {
        let types: Vec<(String, bool, &String)> = extensions
            .iter()
            .map(|e| {
                let (m, own) = mime_type_or_own(e);
                (m, own, e)
            })
            .collect();
        // Project formats without a standard type get one, so the desktop can tell them apart.
        let own: Vec<&(String, bool, &String)> = types.iter().filter(|t| t.1).collect();
        if !own.is_empty() {
            if let Some(pkg) = mime_package() {
                let mut xml = String::from(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n",
                );
                for (mime, _, ext) in &own {
                    xml.push_str(&format!(
                        "  <mime-type type=\"{mime}\">\n    <comment>{} document</comment>\n    <glob pattern=\"*.{ext}\"/>\n  </mime-type>\n",
                        ext.to_uppercase()
                    ));
                }
                xml.push_str("</mime-info>\n");
                if let Some(dir) = pkg.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                crate::paths::write_atomic(&pkg, xml.as_bytes())?;
                update_mime_database();
            }
        }
        let mut mimes: Vec<&str> = types.iter().map(|t| t.0.as_str()).collect();
        mimes.sort_unstable();
        mimes.dedup();
        let dir = applications_dir().ok_or_else(|| anyhow::anyhow!("no home folder"))?;
        std::fs::create_dir_all(&dir)?;
        let exec = exe.display().to_string().replace('"', "\\\"");
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=CraftSpace\nComment=Open in the right ArtCraft app\n\
             Exec=\"{exec}\" {OPEN_COMMAND} %f\nIcon=craftspace\nNoDisplay=true\nTerminal=false\nMimeType={};\n",
            mimes.join(";")
        );
        crate::paths::write_atomic(&dir.join(DESKTOP_FILE), entry.as_bytes())?;
        let _ = Command::new("update-desktop-database").arg(&dir).stdout(Stdio::null()).stderr(Stdio::null()).status();
        let status = Command::new("xdg-mime").arg("default").arg(DESKTOP_FILE).args(&mimes).status();
        if !status.is_ok_and(|s| s.success()) {
            // Without xdg-utils, write the defaults ourselves.
            set_defaults(&mimes)?;
        }
        Ok(Registered { types: extensions.len(), confirm_url: None })
    }

    fn set_defaults(mimes: &[&str]) -> anyhow::Result<()> {
        let path = mimeapps_list().ok_or_else(|| anyhow::anyhow!("no home folder"))?;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut out = String::new();
        let mut in_defaults = false;
        let mut wrote = false;
        for line in text.lines() {
            if line.starts_with('[') {
                if in_defaults && !wrote {
                    mimes.iter().for_each(|m| out.push_str(&format!("{m}={DESKTOP_FILE}\n")));
                    wrote = true;
                }
                in_defaults = line.trim() == "[Default Applications]";
            } else if in_defaults && mimes.iter().any(|m| line.starts_with(&format!("{m}="))) {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        if !wrote {
            if !in_defaults {
                out.push_str("[Default Applications]\n");
            }
            mimes.iter().for_each(|m| out.push_str(&format!("{m}={DESKTOP_FILE}\n")));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::paths::write_atomic(&path, out.as_bytes())?;
        Ok(())
    }

    fn update_mime_database() {
        if let Some(mime) = mime_package().and_then(|p| p.parent().and_then(Path::parent).map(Path::to_path_buf)) {
            let _ =
                Command::new("update-mime-database").arg(&mime).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }

    pub fn unregister() -> anyhow::Result<()> {
        if let Some(pkg) = mime_package().filter(|p| p.exists()) {
            std::fs::remove_file(&pkg)?;
            update_mime_database();
        }
        if let Some(dir) = applications_dir() {
            let file = dir.join(DESKTOP_FILE);
            if file.exists() {
                std::fs::remove_file(&file)?;
                let _ = Command::new("update-desktop-database")
                    .arg(&dir)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        if let Some(path) = mimeapps_list() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let cleaned = without_entry(&text, DESKTOP_FILE);
                if cleaned != text {
                    crate::paths::write_atomic(&path, cleaned.as_bytes())?;
                }
            }
        }
        Ok(())
    }

    /// `mimeapps.list` with `desktop` taken out of every association.
    pub fn without_entry(text: &str, desktop: &str) -> String {
        let mut out = String::new();
        for line in text.lines() {
            if let Some((mime, apps)) = line.split_once('=').filter(|_| !line.starts_with('[')) {
                let kept: Vec<&str> = apps.split(';').filter(|a| !a.is_empty() && *a != desktop).collect();
                if kept.is_empty() {
                    continue;
                }
                out.push_str(&format!("{mime}={};\n", kept.join(";")));
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    pub fn register(_exe: &Path, _extensions: &[String]) -> anyhow::Result<Registered> {
        anyhow::bail!("opening files through CraftSpace isn't available on macOS yet")
    }

    pub fn unregister() -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extension_the_apps_open_is_known() {
        let catalog = Catalog::builtin();
        for ext in catalog.all_extensions() {
            // Project formats without a registered MIME type are fine to skip.
            if ["xmp", "prproj", "ecproj", "lottie", "ptx"].contains(&ext.as_str()) {
                continue;
            }
            assert!(mime_type(&ext).is_some(), "{ext} has no MIME type");
        }
        assert!(
            default_on("psd") && default_on("dwg") && !default_on("png") && !default_on("pdf") && !default_on("xmp")
        );
        assert_eq!(mime_type_or_own("ecproj"), ("application/x-ecproj".to_string(), true));
    }

    #[test]
    fn picks_an_installed_app_first() {
        let catalog = Catalog::builtin();
        let psd = Path::new("/x/poster.PSD");
        assert_eq!(choose_app(&catalog, psd, |_| false), Some(("photocraft".into(), false)));
        assert_eq!(choose_app(&catalog, psd, |id| id == "photocraft"), Some(("photocraft".into(), true)));
        assert_eq!(choose_app(&catalog, Path::new("/x/notes.txt"), |_| true), None);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn mimeapps_entries_are_removed() {
        let text = "[Default Applications]\nimage/vnd.adobe.photoshop=craftspace-open.desktop;\napplication/pdf=evince.desktop;craftspace-open.desktop;\n[Added Associations]\nimage/png=eog.desktop;\n";
        assert_eq!(
            imp::without_entry(text, imp::DESKTOP_FILE),
            "[Default Applications]\napplication/pdf=evince.desktop;\n[Added Associations]\nimage/png=eog.desktop;\n"
        );
    }
}
