//! Windows integration, all per-user (no admin rights needed):
//!
//! * a Start menu shortcut in `Programs\ArtCraft`, and optionally one on the desktop;
//! * an entry in Settings › Apps (`HKCU\…\Uninstall\CraftSpace.<app>`) whose Uninstall button
//!   calls back into CraftSpace;
//! * "Open with" entries for the file types the app handles, without taking over defaults.

use std::path::{Path, PathBuf};

use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
use winreg::RegKey;

use super::{Integration, Request};

const UNINSTALL_ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const CLASSES_ROOT: &str = r"Software\Classes";

/// Registry records are `key:<path>` (delete the whole key) or `value:<path>|<name>`.
fn key_record(path: &str) -> String {
    format!("key:{path}")
}

fn value_record(path: &str, name: &str) -> String {
    format!("value:{path}|{name}")
}

pub fn start_menu_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(base.config_dir().join(r"Microsoft\Windows\Start Menu\Programs\ArtCraft"))
}

pub fn integrate(req: &Request) -> anyhow::Result<Integration> {
    let mut out = Integration::default();
    let exe = req.executable.to_string_lossy().into_owned();
    let workdir = req.dir.to_string_lossy().into_owned();

    let mut shortcut_dirs = Vec::new();
    if let Some(dir) = start_menu_dir() {
        shortcut_dirs.push(dir);
    }
    if req.desktop_shortcut {
        if let Some(desktop) = directories::UserDirs::new().and_then(|u| u.desktop_dir().map(Path::to_path_buf)) {
            shortcut_dirs.push(desktop);
        }
    }
    for dir in shortcut_dirs {
        std::fs::create_dir_all(&dir)?;
        let lnk_path = dir.join(format!("{}.lnk", req.app.name));
        let mut lnk = mslnk::ShellLink::new(req.executable).map_err(|e| anyhow::anyhow!("shortcut: {e:?}"))?;
        lnk.set_working_dir(Some(workdir.clone()));
        lnk.set_icon_location(Some(exe.clone()));
        lnk.set_name(Some(req.app.tagline.clone()));
        lnk.create_lnk(&lnk_path).map_err(|e| anyhow::anyhow!("shortcut {}: {e:?}", lnk_path.display()))?;
        out.files.push(lnk_path);
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // Settings › Apps entry.
    let uninstall_key = format!(r"{UNINSTALL_ROOT}\CraftSpace.{}", req.app.id);
    let (key, _) = hkcu.create_subkey(&uninstall_key)?;
    key.set_value("DisplayName", &req.app.name)?;
    key.set_value("DisplayVersion", &req.version.to_string())?;
    key.set_value("Publisher", &"ArtCraft (installed by CraftSpace)".to_string())?;
    key.set_value("InstallLocation", &workdir)?;
    key.set_value("DisplayIcon", &format!("{exe},0"))?;
    key.set_value("URLInfoAbout", &req.app.github_url())?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    key.set_value("EstimatedSize", &((req.size_bytes / 1024).min(u32::MAX as u64) as u32))?;
    if let Some(cmd) = &req.uninstall_command {
        let line = cmd.iter().map(|a| quote_arg(a)).collect::<Vec<_>>().join(" ");
        key.set_value("UninstallString", &line)?;
        key.set_value("QuietUninstallString", &format!("{line} --yes"))?;
    }
    out.registry_keys.push(key_record(&uninstall_key));

    // "Open with" registrations.
    if !req.app.extensions.is_empty() {
        let prog_id = format!("CraftSpace.{}.Document", req.app.id);
        let prog_key_path = format!(r"{CLASSES_ROOT}\{prog_id}");
        let (prog, _) = hkcu.create_subkey(&prog_key_path)?;
        prog.set_value("", &format!("{} document", req.app.name))?;
        let (icon, _) = prog.create_subkey("DefaultIcon")?;
        icon.set_value("", &format!("{exe},0"))?;
        let (cmd, _) = prog.create_subkey(r"shell\open\command")?;
        cmd.set_value("", &format!("{} \"%1\"", quote_arg(&exe)))?;
        out.registry_keys.push(key_record(&prog_key_path));

        for ext in &req.app.extensions {
            let path = format!(r"{CLASSES_ROOT}\.{}\OpenWithProgids", ext.to_ascii_lowercase());
            let (k, _) = hkcu.create_subkey(&path)?;
            k.set_value(&prog_id, &String::new())?;
            out.registry_keys.push(value_record(&path, &prog_id));
        }

        // The program by its file name, which is what a default picked in "Open with" › "Choose
        // another app" points at. Rewritten on every update, so an entry Windows made for an
        // older path is corrected too.
        if let Some(exe_name) = req.executable.file_name().map(|n| n.to_string_lossy().into_owned()) {
            let app_key_path = format!(r"{CLASSES_ROOT}\Applications\{exe_name}");
            let _ = hkcu.delete_subkey_all(&app_key_path);
            let (app_key, _) = hkcu.create_subkey(&app_key_path)?;
            app_key.set_value("FriendlyAppName", &req.app.name)?;
            let (icon, _) = app_key.create_subkey("DefaultIcon")?;
            icon.set_value("", &format!("{exe},0"))?;
            let (cmd, _) = app_key.create_subkey(r"shell\open\command")?;
            cmd.set_value("", &format!("{} \"%1\"", quote_arg(&exe)))?;
            let (types, _) = app_key.create_subkey("SupportedTypes")?;
            for ext in &req.app.extensions {
                types.set_value(format!(".{}", ext.to_ascii_lowercase()), &String::new())?;
            }
            out.registry_keys.push(key_record(&app_key_path));
        }
    }
    Ok(out)
}

pub fn remove_registry(records: &[String]) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for record in records {
        let result = if let Some(path) = record.strip_prefix("key:") {
            hkcu.delete_subkey_all(path)
        } else if let Some((path, name)) = record.strip_prefix("value:").and_then(|r| r.split_once('|')) {
            hkcu.open_subkey_with_flags(path, KEY_ALL_ACCESS).and_then(|k| k.delete_value(name))
        } else {
            continue;
        };
        if let Err(err) = result {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!("could not remove registry entry {record}: {err}");
            }
        }
    }
}

fn quote_arg(arg: &str) -> String {
    if arg.is_empty() || arg.contains([' ', '\t', '"']) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

/// An entry in Settings › Apps, as installers register them.
#[derive(Debug, Clone, Default)]
pub struct UninstallEntry {
    pub key: String,
    pub display_name: String,
    pub display_version: Option<String>,
    pub install_location: Option<PathBuf>,
    pub display_icon: Option<String>,
    pub uninstall_string: Option<String>,
    pub quiet_uninstall_string: Option<String>,
}

impl UninstallEntry {
    /// The program the entry points at (its icon, or `<binary>.exe` in the install folder).
    pub fn executable(&self, binary: &str) -> Option<PathBuf> {
        let from_icon = self.display_icon.as_ref().map(|icon| {
            let icon = icon.trim().trim_matches('"');
            // "C:\…\App.exe,0"
            let icon = match icon.rsplit_once(',') {
                Some((path, idx)) if idx.trim().parse::<i32>().is_ok() => path,
                _ => icon,
            };
            PathBuf::from(icon.trim_matches('"'))
        });
        if let Some(p) =
            from_icon.filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) && p.is_file())
        {
            return Some(p);
        }
        let dir = self.install_location.as_ref().filter(|d| d.is_dir())?;
        crate::manager::find_executable(dir, binary, crate::platform::Os::Windows)
    }
}

/// Find the Settings › Apps entry an installer created for `display_name` (current user first,
/// then the machine, in both 64- and 32-bit views). CraftSpace's own entries are skipped.
pub fn find_uninstall_entry(display_name: &str) -> Option<UninstallEntry> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    let roots = [
        (RegKey::predef(HKEY_CURRENT_USER), KEY_READ),
        (RegKey::predef(HKEY_LOCAL_MACHINE), KEY_READ | KEY_WOW64_64KEY),
        (RegKey::predef(HKEY_LOCAL_MACHINE), KEY_READ | KEY_WOW64_32KEY),
    ];
    let wanted = display_name.to_lowercase();
    let mut best: Option<UninstallEntry> = None;
    for (root, flags) in roots {
        let Ok(uninstall) = root.open_subkey_with_flags(UNINSTALL_ROOT, flags) else { continue };
        for name in uninstall.enum_keys().filter_map(Result::ok) {
            if name.starts_with("CraftSpace.") {
                continue;
            }
            let Ok(key) = uninstall.open_subkey_with_flags(&name, flags) else { continue };
            let Ok(display) = key.get_value::<String, _>("DisplayName") else { continue };
            let lower = display.to_lowercase();
            let exact = lower == wanted;
            if !(exact || lower.starts_with(&format!("{wanted} "))) {
                continue;
            }
            let entry = UninstallEntry {
                key: name.clone(),
                display_name: display,
                display_version: key.get_value("DisplayVersion").ok(),
                install_location: key
                    .get_value::<String, _>("InstallLocation")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
                display_icon: key.get_value("DisplayIcon").ok(),
                uninstall_string: key.get_value("UninstallString").ok(),
                quiet_uninstall_string: key.get_value("QuietUninstallString").ok(),
            };
            if exact {
                return Some(entry);
            }
            best.get_or_insert(entry);
        }
    }
    best
}

/// Split a registry command line into the program and its raw arguments.
pub fn split_command_line(line: &str) -> (String, String) {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
    }
    let lower = line.to_ascii_lowercase();
    match lower.find(".exe") {
        Some(i) => (line[..i + 4].to_string(), line[i + 4..].trim().to_string()),
        None => match line.split_once(' ') {
            Some((p, a)) => (p.to_string(), a.trim().to_string()),
            None => (line.to_string(), String::new()),
        },
    }
}

/// Run an entry's uninstaller without asking questions where the installer type allows it.
pub fn run_uninstaller(entry: &UninstallEntry, quiet: bool) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    let line = entry
        .quiet_uninstall_string
        .clone()
        .or_else(|| entry.uninstall_string.clone())
        .ok_or_else(|| anyhow::anyhow!("{} has no uninstaller registered", entry.display_name))?;
    let (program, mut args) = split_command_line(&line);
    let is_msi = program.to_ascii_lowercase().ends_with("msiexec.exe") || program.eq_ignore_ascii_case("msiexec");
    if is_msi {
        // "MsiExec.exe /I{GUID}" opens the repair dialog; /X removes.
        args = args.replacen("/I", "/X", 1).replacen("/i", "/X", 1);
        args.push_str(if quiet { " /qn /norestart" } else { " /passive /norestart" });
    } else if entry.quiet_uninstall_string.is_none() {
        // NSIS (and Inno Setup understands /S too): silent.
        args.push_str(" /S");
    }
    let status = std::process::Command::new(&program).raw_arg(&args).status()?;
    anyhow::ensure!(status.success() || status.code() == Some(3010), "the uninstaller exited with {status}");
    Ok(())
}

/// Start CraftSpace when the user logs in (`HKCU\…\Run`).
pub fn set_run_at_login(command: Option<&str>) -> anyhow::Result<()> {
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN)?;
    match command {
        Some(cmd) => key.set_value("CraftSpace", &cmd.to_string())?,
        None => match key.delete_value("CraftSpace") {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
            _ => {}
        },
    }
    Ok(())
}

/// Register fonts for the current user so every app sees them.
pub fn register_font(name: &str, path: &Path) -> anyhow::Result<String> {
    const FONTS: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(FONTS)?;
    key.set_value(name, &path.to_string_lossy().into_owned())?;
    Ok(value_record(FONTS, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_command_lines() {
        assert_eq!(
            split_command_line(r#""C:\Users\me\AppData\Local\ArtCraft\uninstall.exe" /currentuser"#),
            (r"C:\Users\me\AppData\Local\ArtCraft\uninstall.exe".into(), "/currentuser".into())
        );
        assert_eq!(split_command_line("MsiExec.exe /I{1234-5678}"), ("MsiExec.exe".into(), "/I{1234-5678}".into()));
        assert_eq!(
            split_command_line(r"C:\Program Files\App\unins000.exe"),
            (r"C:\Program Files\App\unins000.exe".into(), String::new())
        );
    }

    #[test]
    fn executable_from_icon() {
        let dir = std::env::temp_dir().join(format!("cs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("ArtCraft.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let entry = UninstallEntry { display_icon: Some(format!("\"{}\",0", exe.display())), ..Default::default() };
        assert_eq!(entry.executable("artcraft"), Some(exe.clone()));
        let entry = UninstallEntry { install_location: Some(dir.clone()), ..Default::default() };
        assert_eq!(entry.executable("ArtCraft"), Some(exe));
        let _ = std::fs::remove_dir_all(dir);
    }
}
