//! Unpacking release archives.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use crate::download::Progress;

/// Unpack `archive` (a `.zip` or `.tar.gz`) so its contents end up directly in `dest`.
///
/// Release archives wrap everything in one folder (`photocraft-0.5.0-linux-x86_64/…`); that
/// folder is stripped. `dest` must not exist yet. Unpacking happens in a sibling staging folder
/// that is renamed into place at the end, so a failed or cancelled unpack leaves nothing behind.
pub fn unpack(archive: &Path, dest: &Path, progress: &Progress) -> anyhow::Result<()> {
    anyhow::ensure!(!dest.exists(), "{} already exists", dest.display());
    let parent = dest.parent().ok_or_else(|| anyhow::anyhow!("{} has no parent", dest.display()))?;
    std::fs::create_dir_all(parent)?;
    let staging = parent
        .join(format!(".staging-{}", dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;

    let result = (|| -> anyhow::Result<()> {
        let name = archive.to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".zip") {
            unpack_zip(archive, &staging, progress)?;
        } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            unpack_tar_gz(archive, &staging, progress)?;
        } else {
            anyhow::bail!("don't know how to unpack {}", archive.display());
        }
        let root = single_child_dir(&staging)?.unwrap_or_else(|| staging.clone());
        std::fs::rename(&root, dest)?;
        Ok(())
    })();
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn unpack_zip(archive: &Path, into: &Path, progress: &Progress) -> anyhow::Result<()> {
    let mut zip = zip::ZipArchive::new(BufReader::new(File::open(archive)?))?;
    for i in 0..zip.len() {
        progress.check_cancelled()?;
        let mut entry = zip.by_index(i)?;
        // `enclosed_name` rejects absolute paths and `..` (zip-slip).
        let Some(rel) = entry.enclosed_name() else {
            anyhow::bail!("unsafe path in archive: {}", entry.name());
        };
        let out = into.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&out)?;
        std::io::copy(&mut entry, &mut file)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

fn unpack_tar_gz(archive: &Path, into: &Path, progress: &Progress) -> anyhow::Result<()> {
    let gz = flate2::read::GzDecoder::new(BufReader::new(File::open(archive)?));
    let mut tar = tar::Archive::new(gz);
    tar.set_preserve_permissions(true);
    tar.set_overwrite(true);
    for entry in tar.entries()? {
        progress.check_cancelled()?;
        let mut entry = entry?;
        // `unpack_in` refuses paths that escape `into`.
        if !entry.unpack_in(into)? {
            anyhow::bail!("unsafe path in archive: {}", entry.path()?.display());
        }
    }
    Ok(())
}

/// If `dir` holds exactly one entry and it is a directory, return it.
fn single_child_dir(dir: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut entries = std::fs::read_dir(dir)?;
    let (Some(first), None) = (entries.next(), entries.next()) else { return Ok(None) };
    let first = first?;
    Ok(first.file_type()?.is_dir().then(|| first.path()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;

    pub fn no_progress<R>(f: impl FnOnce(&Progress) -> R) -> R {
        let cancel = AtomicBool::new(false);
        let report = |_| {};
        f(&Progress { report: &report, cancel: &cancel })
    }

    pub fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        for (name, data) in files {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    pub fn make_tar_gz(path: &Path, files: &[(&str, &[u8])]) {
        let gz = flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, name, *data).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn unpacks_zip_and_strips_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("a-portable.zip");
        make_zip(&zip, &[("app-1.0/app.exe", b"exe"), ("app-1.0/sub/readme.md", b"hi")]);
        let dest = dir.path().join("out/1.0.0");
        no_progress(|p| unpack(&zip, &dest, p)).unwrap();
        assert_eq!(std::fs::read(dest.join("app.exe")).unwrap(), b"exe");
        assert_eq!(std::fs::read(dest.join("sub/readme.md")).unwrap(), b"hi");
        assert!(!dir.path().join("out/.staging-1.0.0").exists());
    }

    #[test]
    fn unpacks_tarball_keeping_flat_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let tgz = dir.path().join("a.tar.gz");
        make_tar_gz(&tgz, &[("bin/app", b"elf"), ("share/doc/x", b"doc")]);
        let dest = dir.path().join("out");
        no_progress(|p| unpack(&tgz, &dest, p)).unwrap();
        assert_eq!(std::fs::read(dest.join("bin/app")).unwrap(), b"elf");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dest.join("bin/app")).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn refuses_existing_destination_and_cleans_up_on_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("a.zip");
        make_zip(&zip, &[("x/y", b"1")]);
        let dest = dir.path().join("out");
        std::fs::create_dir(&dest).unwrap();
        assert!(no_progress(|p| unpack(&zip, &dest, p)).is_err());

        let dest2 = dir.path().join("out2");
        let cancel = AtomicBool::new(true);
        let report = |_| {};
        let err = unpack(&zip, &dest2, &Progress { report: &report, cancel: &cancel }).unwrap_err();
        assert!(crate::download::is_cancelled(&err));
        assert!(!dest2.exists());
        assert!(!dir.path().join(".staging-out2").exists());
    }
}
