//! Starting CraftSpace (in the background) when the user logs in.
//!
//! * Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! * Linux: `~/.config/autostart/craftspace.desktop`
//! * macOS: `~/Library/LaunchAgents/io.github.emircesur.craftspace.plist`

use std::path::{Path, PathBuf};

pub const BACKGROUND_FLAG: &str = "--background";
const MAC_LABEL: &str = "io.github.emircesur.craftspace";

/// Turn start-at-login on (for `exe`) or off.
pub fn set(enabled: bool, exe: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let cmd = format!("\"{}\" {BACKGROUND_FLAG}", exe.display());
        crate::integrate::windows::set_run_at_login(enabled.then_some(cmd.as_str()))
    }
    #[cfg(not(windows))]
    {
        let file = entry_path()?;
        if !enabled {
            return match std::fs::remove_file(&file) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
                _ => Ok(()),
            };
        }
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file, entry_contents(exe))?;
        Ok(())
    }
}

/// Where the login item lives (not on Windows, which uses the registry).
pub fn entry_path() -> anyhow::Result<PathBuf> {
    let base = directories::BaseDirs::new().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    Ok(if cfg!(target_os = "macos") {
        base.home_dir().join("Library/LaunchAgents").join(format!("{MAC_LABEL}.plist"))
    } else {
        base.config_dir().join("autostart/craftspace.desktop")
    })
}

pub fn entry_contents(exe: &Path) -> String {
    let exe = exe.display();
    if cfg!(target_os = "macos") {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{MAC_LABEL}</string>\n  <key>ProgramArguments</key>\n  <array><string>{exe}</string><string>{BACKGROUND_FLAG}</string></array>\n  <key>RunAtLoad</key><true/>\n  <key>ProcessType</key><string>Interactive</string>\n</dict>\n</plist>\n"
        )
    } else {
        format!(
            "[Desktop Entry]\nType=Application\nName=CraftSpace\nComment=Keeps your ArtCraft apps up to date\nExec=\"{exe}\" {BACKGROUND_FLAG}\nIcon=craftspace\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_login_entry() {
        let text = entry_contents(Path::new("/opt/craftspace/bin/craftspace"));
        assert!(text.contains("/opt/craftspace/bin/craftspace"));
        assert!(text.contains(BACKGROUND_FLAG));
    }
}
