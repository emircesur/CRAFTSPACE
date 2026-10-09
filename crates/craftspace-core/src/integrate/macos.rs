//! macOS: apps come as disk images holding an `.app` bundle.
//!
//! CraftSpace copies the bundle out of the image into the version folder, then moves the
//! current version to `~/Applications/<Name>.app`, where Finder, Launchpad and Spotlight expect
//! it (no admin rights needed). On update the old bundle moves back into its version folder, so
//! it can be rolled back without showing up twice in Launchpad.
//!
//! Bundles are copied with `ditto`, which keeps code signatures and extended attributes. Files
//! CraftSpace downloads itself carry no quarantine flag, so Gatekeeper doesn't stop them.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `~/Applications` (or `CRAFTSPACE_MAC_APPLICATIONS`, for tests).
pub fn applications_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CRAFTSPACE_MAC_APPLICATIONS").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join("Applications"))
        .unwrap_or_else(|| PathBuf::from("/Applications"))
}

/// The first `.app` bundle directly in `dir` (or one level down).
pub fn find_app_bundle(dir: &Path) -> Option<PathBuf> {
    let is_app = |p: &Path| p.is_dir() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    if let Some(app) = entries.iter().find(|p| is_app(p)) {
        return Some(app.clone());
    }
    entries
        .iter()
        .filter(|p| p.is_dir() && !p.is_symlink())
        .find_map(|sub| std::fs::read_dir(sub).ok()?.filter_map(Result::ok).map(|e| e.path()).find(|p| is_app(p)))
}

/// Copy the `.app` out of `dmg` into `dest_dir` (created). Returns the copied bundle.
pub fn install_dmg(dmg: &Path, dest_dir: &Path) -> anyhow::Result<PathBuf> {
    let mount = tempdir_near(dest_dir, "mount")?;
    // `-nobrowse`: don't show it in Finder; piping "Y" accepts any licence agreement.
    let mut attach = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-noverify", "-mountpoint"])
        .arg(&mount)
        .arg(dmg)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = attach.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(b"Y\n");
    }
    let status = attach.wait()?;
    anyhow::ensure!(status.success(), "couldn't open the disk image {} ({status})", dmg.display());

    let result = (|| -> anyhow::Result<PathBuf> {
        let bundle = find_app_bundle(&mount).ok_or_else(|| anyhow::anyhow!("the disk image has no .app inside"))?;
        std::fs::create_dir_all(dest_dir)?;
        let target = dest_dir.join(bundle.file_name().expect("bundle has a name"));
        copy_bundle(&bundle, &target)?;
        Ok(target)
    })();
    let _ = Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&mount).status();
    let _ = std::fs::remove_dir(&mount);
    result
}

/// `ditto src dst`, keeping signatures, resource forks and permissions.
pub fn copy_bundle(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if dst.exists() {
        std::fs::remove_dir_all(dst)?;
    }
    let status = Command::new("ditto").arg(src).arg(dst).status()?;
    anyhow::ensure!(status.success(), "copying {} failed ({status})", src.display());
    Ok(())
}

/// Move a bundle, falling back to copy + delete across volumes.
pub fn move_bundle(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_bundle(src, dst)?;
    std::fs::remove_dir_all(src)?;
    Ok(())
}

/// Put `bundle` (inside its version folder) into `~/Applications`. Anything already there under
/// that name is moved to `backup_dir` first. Returns the bundle's new path.
pub fn activate(bundle: &Path, applications: &Path, backup_dir: &Path) -> anyhow::Result<PathBuf> {
    let name = bundle.file_name().ok_or_else(|| anyhow::anyhow!("{} has no name", bundle.display()))?;
    let active = applications.join(name);
    if active == bundle {
        return Ok(active);
    }
    if active.exists() || active.is_symlink() {
        let stamp = crate::github::now_secs();
        let backup = backup_dir
            .join(format!("{}-{stamp}.app", Path::new(name).file_stem().unwrap_or_default().to_string_lossy()));
        log::warn!("moving the existing {} to {}", active.display(), backup.display());
        move_bundle(&active, &backup)?;
    }
    move_bundle(bundle, &active)?;
    Ok(active)
}

/// Move an active bundle from `~/Applications` back into `version_dir`. Returns its new path.
pub fn deactivate(active: &Path, version_dir: &Path) -> anyhow::Result<PathBuf> {
    let target = version_dir.join(active.file_name().ok_or_else(|| anyhow::anyhow!("no bundle name"))?);
    move_bundle(active, &target)?;
    Ok(target)
}

/// `open -a <bundle> [files…]`.
pub fn launch_command(bundle: &Path, files: &[PathBuf]) -> Command {
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg(bundle).args(files);
    cmd
}

fn tempdir_near(dir: &Path, what: &str) -> std::io::Result<PathBuf> {
    let base = dir.parent().unwrap_or(dir);
    std::fs::create_dir_all(base)?;
    let path = base.join(format!(".{what}-{}-{}", std::process::id(), crate::github::now_secs()));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_bundle(at: &Path) {
        std::fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
        std::fs::write(at.join("Contents/MacOS/photocraft"), b"bin").unwrap();
    }

    #[test]
    fn finds_bundles() {
        let tmp = tempfile::tempdir().unwrap();
        fake_bundle(&tmp.path().join("mnt/PhotoCraft.app"));
        std::fs::create_dir_all(tmp.path().join("mnt/.background")).unwrap();
        assert_eq!(find_app_bundle(&tmp.path().join("mnt")), Some(tmp.path().join("mnt/PhotoCraft.app")));
        fake_bundle(&tmp.path().join("nested/inner/X.app"));
        assert_eq!(find_app_bundle(&tmp.path().join("nested")), Some(tmp.path().join("nested/inner/X.app")));
        assert_eq!(find_app_bundle(tmp.path().join("nested/inner/X.app/Contents").as_path()), None);
    }

    #[test]
    fn activates_and_deactivates() {
        let tmp = tempfile::tempdir().unwrap();
        let apps = tmp.path().join("Applications");
        let v1 = tmp.path().join("versions/0.4.0");
        let v2 = tmp.path().join("versions/0.5.0");
        fake_bundle(&v1.join("PhotoCraft.app"));
        fake_bundle(&v2.join("PhotoCraft.app"));
        // Something the user put there by hand is backed up, not deleted.
        fake_bundle(&apps.join("PhotoCraft.app"));
        std::fs::write(apps.join("PhotoCraft.app/mine"), b"x").unwrap();

        let backup = tmp.path().join("replaced");
        let active = activate(&v1.join("PhotoCraft.app"), &apps, &backup).unwrap();
        assert_eq!(active, apps.join("PhotoCraft.app"));
        assert!(!v1.join("PhotoCraft.app").exists());
        assert_eq!(std::fs::read_dir(&backup).unwrap().count(), 1);

        // Update: old one back into its folder, new one in.
        let back = deactivate(&active, &v1).unwrap();
        assert_eq!(back, v1.join("PhotoCraft.app"));
        let active = activate(&v2.join("PhotoCraft.app"), &apps, &backup).unwrap();
        assert!(active.join("Contents/MacOS/photocraft").is_file());
        assert!(v1.join("PhotoCraft.app/Contents/MacOS/photocraft").is_file());
        assert_eq!(std::fs::read_dir(&backup).unwrap().count(), 1);
    }
}
