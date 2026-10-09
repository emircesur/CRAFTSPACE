//! ArtCraft apps installed without CraftSpace: from their own installers, disk images, packages,
//! Flatpak, or a portable copy unpacked somewhere. CraftSpace adopts them, so it shows them as
//! installed, opens them and updates them where they are instead of installing a second copy.

use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::catalog::{AppEntry, Catalog};
use crate::platform::AssetKind;

/// How an adopted install was found, and what that means for updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct External {
    /// For people: "in /Applications", "with its Windows installer", "as an rpm package".
    pub how: String,
    /// Where new versions of a portable copy or AppImage go (next to the one that was found).
    #[serde(default)]
    pub home: Option<PathBuf>,
    /// Flatpak application ID: Flatpak updates it, not CraftSpace.
    #[serde(default)]
    pub flatpak: Option<String>,
}

/// An app found on this computer.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub app_id: String,
    /// `None` when it couldn't be read.
    pub version: Option<Version>,
    pub kind: AssetKind,
    /// The program (a `.app` bundle on macOS).
    pub executable: PathBuf,
    /// The folder the app lives in, for portable copies.
    pub dir: Option<PathBuf>,
    /// The package's name, for rpm/deb installs.
    pub system_package: Option<String>,
    pub external: External,
}

/// The first version number in `text` ("photocraft-0.5.0-windows-x64-portable" → 0.5.0), with
/// an `-rc.1` / `-beta.2` / `-alpha` suffix when there is one.
pub fn version_in(text: &str) -> Option<Version> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let starts_here = chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_alphanumeric());
        if !starts_here {
            i += 1;
            continue;
        }
        // Up to three dot-separated numbers.
        let mut parts: Vec<String> = vec![String::new()];
        let mut j = i;
        while j < chars.len() {
            let c = chars[j];
            if c.is_ascii_digit() {
                parts.last_mut().expect("one part").push(c);
            } else if c == '.' && parts.len() < 3 && j + 1 < chars.len() && chars[j + 1].is_ascii_digit() {
                parts.push(String::new());
            } else {
                break;
            }
            j += 1;
        }
        if parts.len() >= 2 {
            while parts.len() < 3 {
                parts.push("0".into());
            }
            let mut version = parts.join(".");
            // A pre-release tag right after: "-rc.1", "-beta2".
            let rest: String = chars[j..].iter().collect();
            let lower = rest.to_ascii_lowercase();
            for tag in ["-rc", "-beta", "-alpha"] {
                if lower.starts_with(tag) {
                    let tail: String =
                        rest[tag.len()..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                    let tail = tail.trim_end_matches('.');
                    version.push_str(tag);
                    if !tail.is_empty() {
                        version.push('.');
                        version.push_str(tail.trim_start_matches('.'));
                    }
                    break;
                }
            }
            if let Ok(v) = Version::parse(&version) {
                return Some(v);
            }
        }
        i = j.max(i + 1);
    }
    None
}

/// Look for every app in `catalog` that isn't in `known` (already recorded), skipping anything
/// under `own` (CraftSpace's own folders).
pub fn find(catalog: &Catalog, known: &dyn Fn(&str) -> bool, own: &[PathBuf]) -> Vec<Found> {
    let mine = |p: &Path| own.iter().any(|o| p.starts_with(o));
    let mut out = Vec::new();
    for app in catalog.apps.iter().filter(|a| !known(&a.id)) {
        if let Some(found) = find_app(app, &mine) {
            log::info!("found {} {:?} {}", app.name, found.version, found.external.how);
            out.push(found);
        }
    }
    out
}

fn find_app(app: &AppEntry, mine: &dyn Fn(&Path) -> bool) -> Option<Found> {
    #[cfg(target_os = "macos")]
    return mac::find(app, mine);
    #[cfg(windows)]
    return win::find(app, mine);
    #[cfg(all(unix, not(target_os = "macos")))]
    return linux::find(app, mine);
    #[allow(unreachable_code)]
    {
        let _ = (app, mine);
        None
    }
}

/// Portable copies: `<binary>[.exe]` up to a few folders down in `roots`.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn find_portable(
    app: &AppEntry,
    roots: &[PathBuf],
    mine: &dyn Fn(&Path) -> bool,
    exe_name: &str,
) -> Option<(PathBuf, PathBuf)> {
    let started = std::time::Instant::now();
    for root in roots {
        let walker = walkdir::WalkDir::new(root).max_depth(4).follow_links(false).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            e.depth() == 0 || !(name.starts_with('.') || name == "node_modules" || name.ends_with(".imported"))
        });
        for entry in walker.filter_map(Result::ok) {
            if started.elapsed() > std::time::Duration::from_secs(3) {
                return None;
            }
            if !entry.file_type().is_file() || !entry.file_name().to_string_lossy().eq_ignore_ascii_case(exe_name) {
                continue;
            }
            let exe = entry.path().to_path_buf();
            if mine(&exe) {
                continue;
            }
            // The app's folder: the one named after it (`photocraft-0.5.0-…/bin/photocraft`).
            let id = app.id.to_ascii_lowercase();
            let folder = exe
                .ancestors()
                .skip(1)
                .take(3)
                .find(|d| d.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().starts_with(&id)))
                .map(Path::to_path_buf)
                .or_else(|| exe.parent().map(Path::to_path_buf))?;
            return Some((exe, folder));
        }
    }
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;

    /// `CFBundleShortVersionString` from the bundle's Info.plist.
    fn bundle_version(bundle: &Path) -> Option<Version> {
        let plist = bundle.join("Contents/Info.plist");
        let out = std::process::Command::new("plutil")
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(&plist)
            .output()
            .ok()?;
        version_in(String::from_utf8_lossy(&out.stdout).trim())
    }

    pub fn find(app: &AppEntry, mine: &dyn Fn(&Path) -> bool) -> Option<Found> {
        let mut dirs = vec![PathBuf::from("/Applications")];
        if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) {
            dirs.push(home.join("Applications"));
        }
        // Where CraftSpace puts apps (`~/Applications`, or a test folder).
        let ours = crate::integrate::macos::applications_dir();
        if !dirs.contains(&ours) {
            dirs.push(ours);
        }
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                let is_app = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
                if !is_app || stem != app.name.to_ascii_lowercase() || mine(&path) {
                    continue;
                }
                return Some(Found {
                    app_id: app.id.clone(),
                    version: bundle_version(&path),
                    kind: AssetKind::Dmg,
                    executable: path,
                    dir: None,
                    system_package: None,
                    external: External { how: format!("in {}", dir.display()), home: None, flatpak: None },
                });
            }
        }
        None
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::integrate::windows as w;

    pub fn find(app: &AppEntry, mine: &dyn Fn(&Path) -> bool) -> Option<Found> {
        // Installed with its own installer: its Settings › Apps entry.
        if let Some(entry) = w::find_uninstall_entry(&app.name) {
            if let Some(exe) = entry.executable(app.binary()).filter(|e| !mine(e)) {
                let msi = entry.key.starts_with('{') && entry.key.ends_with('}');
                return Some(Found {
                    app_id: app.id.clone(),
                    version: entry.display_version.as_deref().and_then(version_in),
                    kind: if msi { AssetKind::Msi } else { AssetKind::Exe },
                    executable: exe,
                    dir: None,
                    system_package: None,
                    external: External { how: "with its Windows installer".into(), home: None, flatpak: None },
                });
            }
        }
        // A portable copy unpacked somewhere usual.
        let dirs = directories::UserDirs::new();
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(d) = &dirs {
            roots.extend(
                [d.desktop_dir(), d.download_dir(), d.document_dir()].into_iter().flatten().map(Path::to_path_buf),
            );
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("Programs"));
        }
        let (exe, folder) = super::find_portable(app, &roots, mine, &format!("{}.exe", app.binary()))?;
        Some(Found {
            app_id: app.id.clone(),
            version: version_in(&folder.to_string_lossy()),
            kind: AssetKind::PortableZip,
            executable: exe,
            dir: Some(folder.clone()),
            system_package: None,
            external: External {
                how: format!("in {}", folder.display()),
                home: folder.parent().map(Path::to_path_buf),
                flatpak: None,
            },
        })
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod linux {
    use super::*;
    use std::process::{Command, Stdio};

    fn output(cmd: &mut Command) -> Option<String> {
        let out = cmd.stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn find(app: &AppEntry, mine: &dyn Fn(&Path) -> bool) -> Option<Found> {
        use crate::integrate::linux_packages as pkg;
        // A system package named after the app.
        let rpm = output(Command::new("rpm").args(["-q", "--qf", "%{VERSION}", &app.id]))
            .filter(|v| !v.contains("not installed"))
            .map(|v| (AssetKind::Rpm, v));
        let deb = || {
            output(Command::new("dpkg-query").args(["-W", "-f", "${Status}|${Version}", &app.id]))
                .and_then(|s| s.strip_prefix("install ok installed|").map(str::to_string))
                .map(|v| (AssetKind::Deb, v))
        };
        if let Some((kind, version)) = rpm.or_else(deb) {
            if let Some(exe) = pkg::find_program(kind, &app.id, app.binary()) {
                let tool = if kind == AssetKind::Rpm { "an rpm" } else { "a deb" };
                return Some(Found {
                    app_id: app.id.clone(),
                    version: version_in(&version),
                    kind,
                    executable: exe,
                    dir: None,
                    system_package: Some(app.id.clone()),
                    external: External { how: format!("as {tool} package"), home: None, flatpak: None },
                });
            }
        }
        // Flatpak: matched by name or by an ID ending in the app's ID.
        if let Some(list) =
            output(Command::new("flatpak").args(["list", "--app", "--columns=application,version,name"]))
        {
            for line in list.lines() {
                let cols: Vec<&str> = line.split('\t').collect();
                let (fid, version, name) = (
                    cols.first().copied().unwrap_or(""),
                    cols.get(1).copied().unwrap_or(""),
                    cols.get(2).copied().unwrap_or(""),
                );
                let id_matches = fid.to_ascii_lowercase().rsplit('.').next() == Some(app.id.as_str());
                if id_matches || name.eq_ignore_ascii_case(&app.name) {
                    return Some(Found {
                        app_id: app.id.clone(),
                        version: version_in(version),
                        kind: AssetKind::TarGz,
                        executable: PathBuf::from(format!("flatpak:{fid}")),
                        dir: None,
                        system_package: None,
                        external: External { how: "from Flatpak".into(), home: None, flatpak: Some(fid.to_string()) },
                    });
                }
            }
        }
        let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())?;
        // An AppImage in the usual places.
        let name = app.name.to_ascii_lowercase();
        for dir in ["Applications", "Downloads", "Desktop", ".local/bin", "bin"].map(|d| home.join(d)) {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let file = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if file.starts_with(&name) && file.ends_with(".appimage") && path.is_file() && !mine(&path) {
                    return Some(Found {
                        app_id: app.id.clone(),
                        version: version_in(&file),
                        kind: AssetKind::AppImage,
                        executable: path,
                        dir: None,
                        system_package: None,
                        external: External { how: format!("in {}", dir.display()), home: Some(dir), flatpak: None },
                    });
                }
            }
        }
        // An unpacked tarball.
        let roots: Vec<PathBuf> =
            ["Applications", "Downloads", "Desktop", "opt", ".local/opt"].map(|d| home.join(d)).into();
        let (exe, folder) = super::find_portable(app, &roots, mine, app.binary())?;
        Some(Found {
            app_id: app.id.clone(),
            version: version_in(&folder.to_string_lossy()),
            kind: AssetKind::TarGz,
            executable: exe,
            dir: Some(folder.clone()),
            system_package: None,
            external: External {
                how: format!("in {}", folder.display()),
                home: folder.parent().map(Path::to_path_buf),
                flatpak: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_in_names() {
        let v = |s: &str| version_in(s).map(|v| v.to_string());
        assert_eq!(v("photocraft-0.5.0-windows-x64-portable").as_deref(), Some("0.5.0"));
        assert_eq!(v("PhotoCraft-0.4.1-x86_64.AppImage").as_deref(), Some("0.4.1"));
        assert_eq!(v("1.2").as_deref(), Some("1.2.0"));
        assert_eq!(v("0.6.0-rc.1").as_deref(), Some("0.6.0-rc.1"));
        assert_eq!(v("photocraft").as_deref(), None);
        assert_eq!(v("x64-portable").as_deref(), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn finds_a_portable_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let catalog = Catalog::builtin();
        let app = catalog.app("photocraft").unwrap();
        let exe_name = if cfg!(windows) { "photocraft.exe" } else { "photocraft" };
        let folder = tmp.path().join("Downloads/photocraft-0.4.0-linux-x86_64");
        std::fs::create_dir_all(folder.join("bin")).unwrap();
        std::fs::write(folder.join("bin").join(exe_name), b"x").unwrap();
        let (exe, dir) = find_portable(app, &[tmp.path().to_path_buf()], &|_| false, exe_name).unwrap();
        assert_eq!(dir, folder);
        assert!(exe.ends_with(exe_name));
        assert_eq!(version_in(&dir.to_string_lossy()), Some(Version::new(0, 4, 0)));
        // CraftSpace's own copies are skipped.
        assert!(find_portable(app, &[tmp.path().to_path_buf()], &|p| p.starts_with(tmp.path()), exe_name).is_none());
    }
}
