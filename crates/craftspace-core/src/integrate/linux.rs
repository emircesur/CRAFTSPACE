//! Freedesktop integration: `.desktop` entries, icons, MIME types and `~/.local/bin` links.
//!
//! ArtCraft tarballs use a `/usr/local`-style layout (`bin/`, `share/applications/`,
//! `share/icons/hicolor/`, `share/mime/packages/`). Those files are copied into the user's
//! XDG data directory, with `Exec=` pointing at the installed version.

use std::path::{Path, PathBuf};

use super::{Integration, Request};

pub struct Dirs {
    /// `$XDG_DATA_HOME` (`~/.local/share`).
    pub data: PathBuf,
    /// `$XDG_BIN_HOME` (`~/.local/bin`).
    pub bin: PathBuf,
    /// The desktop folder, for optional desktop shortcuts.
    pub desktop: Option<PathBuf>,
}

impl Dirs {
    pub fn detect() -> anyhow::Result<Dirs> {
        let base = directories::BaseDirs::new().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
        let bin = base.executable_dir().map(Path::to_path_buf).unwrap_or_else(|| base.home_dir().join(".local/bin"));
        let desktop = directories::UserDirs::new().and_then(|u| u.desktop_dir().map(Path::to_path_buf));
        Ok(Dirs { data: base.data_local_dir().to_path_buf(), bin, desktop })
    }

    fn applications(&self) -> PathBuf {
        self.data.join("applications")
    }
}

pub fn integrate(dirs: &Dirs, req: &Request) -> anyhow::Result<Integration> {
    let mut out = Integration::default();
    let share = req.dir.join("share");

    // Icons and MIME packages, copied file by file.
    for (sub, into) in [("icons", dirs.data.join("icons")), ("mime/packages", dirs.data.join("mime/packages"))] {
        let from = share.join(sub);
        if !from.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&from).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(&from)?;
            let target = into.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
            out.files.push(target);
        }
    }

    // Desktop entries: the app's own, rewritten to the installed path, or one we write.
    std::fs::create_dir_all(dirs.applications())?;
    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(share.join("applications")) {
        for entry in read.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "desktop") {
                let text = std::fs::read_to_string(&path)?;
                entries.push((entry.file_name().to_string_lossy().into_owned(), rewrite_desktop_entry(&text, req)));
            }
        }
    }
    if entries.is_empty() {
        entries.push((format!("craftspace-{}.desktop", req.app.id), generate_desktop_entry(req)));
    }
    for (name, text) in &entries {
        let target = dirs.applications().join(name);
        std::fs::write(&target, text)?;
        out.files.push(target);
        if req.desktop_shortcut {
            if let Some(desktop) = dirs.desktop.as_ref().filter(|d| d.is_dir()) {
                let target = desktop.join(name);
                std::fs::write(&target, text)?;
                set_executable(&target);
                out.files.push(target);
            }
        }
    }

    // Command-line links in ~/.local/bin, never clobbering files we didn't make.
    let mut binaries: Vec<PathBuf> = std::fs::read_dir(req.dir.join("bin"))
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    if binaries.is_empty() {
        binaries.push(req.executable.to_path_buf());
    }
    std::fs::create_dir_all(&dirs.bin)?;
    for binary in binaries {
        let name = if binary == req.executable {
            req.app.binary().to_string()
        } else {
            binary.file_name().unwrap_or_default().to_string_lossy().into_owned()
        };
        let link = dirs.bin.join(&name);
        match std::fs::symlink_metadata(&link) {
            Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(&link)?,
            Ok(_) => {
                log::warn!("not linking {}: a file that CraftSpace did not create is already there", link.display());
                continue;
            }
            Err(_) => {}
        }
        std::os::unix::fs::symlink(&binary, &link)?;
        out.files.push(link);
    }

    refresh_caches(dirs);
    Ok(out)
}

fn exec_value(executable: &Path) -> String {
    let path = executable.to_string_lossy();
    if path.contains([' ', '"', '\'', '\\', '$', '`']) {
        format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`").replace('$', "\\$"))
    } else {
        path.into_owned()
    }
}

/// Point `Exec=` and `TryExec=` at the installed executable, keeping field codes like `%F`.
pub fn rewrite_desktop_entry(text: &str, req: &Request) -> String {
    let exe = exec_value(req.executable);
    let mut out = String::with_capacity(text.len() + 64);
    let mut in_main_group = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            if in_main_group {
                out.push_str(&format!("X-CraftSpace-App={}\n", req.app.id));
            }
            in_main_group = trimmed.starts_with("[Desktop Entry]");
        }
        if let Some(value) = trimmed.strip_prefix("Exec=") {
            let args = split_first_token(value).1;
            out.push_str("Exec=");
            out.push_str(&exe);
            if !args.is_empty() {
                out.push(' ');
                out.push_str(args);
            }
        } else if trimmed.starts_with("TryExec=") {
            out.push_str("TryExec=");
            out.push_str(&req.executable.to_string_lossy());
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if in_main_group {
        out.push_str(&format!("X-CraftSpace-App={}\n", req.app.id));
    }
    out
}

/// Split `"quoted path" rest` or `path rest` into the program and the rest.
fn split_first_token(value: &str) -> (&str, &str) {
    let value = value.trim();
    if let Some(stripped) = value.strip_prefix('"') {
        let mut escaped = false;
        for (i, c) in stripped.char_indices() {
            match c {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => return (&stripped[..i], stripped[i + 1..].trim_start()),
                _ => escaped = false,
            }
        }
        (stripped, "")
    } else {
        match value.split_once(char::is_whitespace) {
            Some((prog, rest)) => (prog, rest.trim_start()),
            None => (value, ""),
        }
    }
}

fn generate_desktop_entry(req: &Request) -> String {
    let icon =
        find_icon(req.dir).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "applications-graphics".into());
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment={comment}\nExec={exe} %F\nTryExec={try_exec}\nIcon={icon}\nTerminal=false\nStartupNotify=true\nCategories=Graphics;\nX-CraftSpace-App={id}\n",
        name = req.app.name,
        comment = req.app.tagline,
        exe = exec_value(req.executable),
        try_exec = req.executable.to_string_lossy(),
        id = req.app.id,
    )
}

/// The largest PNG icon shipped in the version folder, if any.
pub fn find_icon(dir: &Path) -> Option<PathBuf> {
    walkdir::WalkDir::new(dir)
        .max_depth(7)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "png") && e.path().to_string_lossy().contains("icons"))
        .max_by_key(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .map(|e| e.into_path())
}

fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        let _ = std::fs::set_permissions(path, perms);
    }
}

/// Ask the desktop to notice new entries, icons and MIME types. Each tool is optional.
pub fn refresh_caches(dirs: &Dirs) {
    let run = |cmd: &str, args: &[&std::ffi::OsStr]| {
        let _ = std::process::Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    };
    run("update-desktop-database", &[dirs.applications().as_os_str()]);
    let mime = dirs.data.join("mime");
    if mime.join("packages").is_dir() {
        run("update-mime-database", &[mime.as_os_str()]);
    }
    let hicolor = dirs.data.join("icons/hicolor");
    if hicolor.is_dir() {
        run("gtk-update-icon-cache", &[std::ffi::OsStr::new("-f"), std::ffi::OsStr::new("-t"), hicolor.as_os_str()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn req<'a>(app: &'a crate::catalog::AppEntry, dir: &'a Path, exe: &'a Path, v: &'a semver::Version) -> Request<'a> {
        Request {
            app,
            version: v,
            dir,
            executable: exe,
            desktop_shortcut: false,
            uninstall_command: None,
            size_bytes: 0,
        }
    }

    #[test]
    fn rewrites_exec_lines() {
        let catalog = Catalog::builtin();
        let app = catalog.app("photocraft").unwrap();
        let v = semver::Version::new(0, 5, 0);
        let exe = PathBuf::from("/home/me/.local/share/craftspace/apps/photocraft/0.5.0/bin/photocraft");
        let dir = PathBuf::from("/x");
        let text = "[Desktop Entry]\nType=Application\nName=PhotoCraft\nExec=photocraft %F\nTryExec=photocraft\n\n[Desktop Action new]\nExec=\"photocraft\" --new\n";
        let out = rewrite_desktop_entry(text, &req(app, &dir, &exe, &v));
        assert!(out.contains(&format!("Exec={} %F\n", exe.display())));
        assert!(out.contains(&format!("TryExec={}\n", exe.display())));
        assert!(out.contains(&format!("Exec={} --new\n", exe.display())));
        assert_eq!(out.matches("X-CraftSpace-App=photocraft").count(), 1);

        let spaced = PathBuf::from("/home/my user/bin/photocraft");
        let out = rewrite_desktop_entry("[Desktop Entry]\nExec=photocraft %F\n", &req(app, &dir, &spaced, &v));
        assert!(out.contains("Exec=\"/home/my user/bin/photocraft\" %F"));
    }

    #[test]
    fn integrates_and_links() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("apps/photocraft/0.5.0");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("share/applications")).unwrap();
        std::fs::create_dir_all(dir.join("share/icons/hicolor/64x64/apps")).unwrap();
        std::fs::write(dir.join("bin/photocraft"), b"x").unwrap();
        std::fs::write(dir.join("bin/photocraft-cli"), b"x").unwrap();
        std::fs::write(
            dir.join("share/applications/ai.storyteller.photocraft.desktop"),
            "[Desktop Entry]\nExec=photocraft %F\n",
        )
        .unwrap();
        std::fs::write(dir.join("share/icons/hicolor/64x64/apps/ai.storyteller.photocraft.png"), b"png").unwrap();

        let dirs = Dirs { data: tmp.path().join("data"), bin: tmp.path().join("bin"), desktop: None };
        // A file the user owns must survive.
        std::fs::create_dir_all(&dirs.bin).unwrap();
        std::fs::write(dirs.bin.join("photocraft-cli"), b"mine").unwrap();

        let catalog = Catalog::builtin();
        let app = catalog.app("photocraft").unwrap();
        let v = semver::Version::new(0, 5, 0);
        let exe = dir.join("bin/photocraft");
        let out = integrate(&dirs, &req(app, &dir, &exe, &v)).unwrap();

        let desktop = dirs.data.join("applications/ai.storyteller.photocraft.desktop");
        assert!(out.files.contains(&desktop));
        assert!(std::fs::read_to_string(&desktop).unwrap().contains(&exe.display().to_string()));
        assert!(dirs.data.join("icons/hicolor/64x64/apps/ai.storyteller.photocraft.png").is_file());
        assert_eq!(std::fs::read_link(dirs.bin.join("photocraft")).unwrap(), exe);
        assert_eq!(std::fs::read(dirs.bin.join("photocraft-cli")).unwrap(), b"mine");

        super::super::remove(&out.files, &[]);
        assert!(!desktop.exists());
        assert!(!dirs.bin.join("photocraft").exists());
        assert!(dirs.bin.join("photocraft-cli").exists());
    }
}
