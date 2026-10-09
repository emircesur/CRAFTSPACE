//! Thumbnails for the Files tab. Pictures are decoded by the app; Photoshop documents (PSD and
//! PSB) carry a small JPEG preview in their image resources, read here without decoding the
//! layers.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

/// Extensions with a picture CraftSpace can decode directly.
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];

/// Whether a thumbnail can be made for files with `ext` (lowercase).
pub fn supported(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext) || ext == "psd" || ext == "psb"
}

/// Where the thumbnail for `path` (as it is now) is cached: the name changes when the file does.
pub fn cache_path(cache_dir: &Path, path: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    let mut h = Sha256::new();
    h.update(path.to_string_lossy().as_bytes());
    h.update(modified.to_le_bytes());
    h.update(meta.len().to_le_bytes());
    Some(cache_dir.join(format!("{}.png", hex::encode(&h.finalize()[..12]))))
}

/// The JPEG preview stored in a PSD/PSB file (image resource 1036, or 1033 in old files).
pub fn psd_preview(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let mut f = std::fs::File::open(path)?;
    let mut header = [0u8; 26];
    f.read_exact(&mut header)?;
    anyhow::ensure!(&header[..4] == b"8BPS", "not a Photoshop document");
    // Color mode data: length, then skip it.
    let color_len = read_u32(&mut f)?;
    f.seek(SeekFrom::Current(i64::from(color_len)))?;
    // Image resources.
    let resources_len = u64::from(read_u32(&mut f)?);
    let start = f.stream_position()?;
    while f.stream_position()? + 12 <= start + resources_len {
        let mut sig = [0u8; 4];
        f.read_exact(&mut sig)?;
        if &sig != b"8BIM" {
            break;
        }
        let mut id = [0u8; 2];
        f.read_exact(&mut id)?;
        let id = u16::from_be_bytes(id);
        // Pascal string name, padded to an even length (length byte included).
        let mut n = [0u8; 1];
        f.read_exact(&mut n)?;
        let name_len = u64::from(n[0]);
        let pad = if (name_len + 1) % 2 == 1 { 1 } else { 0 };
        f.seek(SeekFrom::Current((name_len + pad) as i64))?;
        let size = u64::from(read_u32(&mut f)?);
        let padded = size + size % 2;
        if (id == 1036 || id == 1033) && size > 28 && size < 16 << 20 {
            // 28-byte header (format, width, height, …), then the JFIF data.
            f.seek(SeekFrom::Current(28))?;
            let mut jpeg = vec![0u8; (size - 28) as usize];
            f.read_exact(&mut jpeg)?;
            if jpeg.starts_with(&[0xFF, 0xD8]) {
                return Ok(Some(jpeg));
            }
            f.seek(SeekFrom::Current((padded - size) as i64))?;
            continue;
        }
        f.seek(SeekFrom::Current(padded as i64))?;
    }
    Ok(None)
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal PSD with a thumbnail resource holding `jpeg`.
    pub fn make_psd(path: &Path, jpeg: &[u8]) {
        let mut out = Vec::new();
        out.extend_from_slice(b"8BPS");
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&3u16.to_be_bytes()); // channels
        out.extend_from_slice(&4u32.to_be_bytes()); // height
        out.extend_from_slice(&4u32.to_be_bytes()); // width
        out.extend_from_slice(&8u16.to_be_bytes()); // depth
        out.extend_from_slice(&3u16.to_be_bytes()); // RGB
        out.extend_from_slice(&0u32.to_be_bytes()); // color mode data
        let mut res = Vec::new();
        // An unrelated resource first, with an odd-length name.
        res.extend_from_slice(b"8BIM");
        res.extend_from_slice(&1005u16.to_be_bytes());
        res.extend_from_slice(&[1, b'x']);
        res.extend_from_slice(&3u32.to_be_bytes());
        res.extend_from_slice(&[1, 2, 3, 0]);
        res.extend_from_slice(b"8BIM");
        res.extend_from_slice(&1036u16.to_be_bytes());
        res.extend_from_slice(&[0, 0]);
        let size = 28 + jpeg.len() as u32;
        res.extend_from_slice(&size.to_be_bytes());
        res.extend_from_slice(&[0; 28]);
        res.extend_from_slice(jpeg);
        if size % 2 == 1 {
            res.push(0);
        }
        out.extend_from_slice(&(res.len() as u32).to_be_bytes());
        out.extend_from_slice(&res);
        out.extend_from_slice(&0u32.to_be_bytes()); // layers
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn reads_the_psd_preview() {
        let tmp = tempfile::tempdir().unwrap();
        let psd = tmp.path().join("a.psd");
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 0xFF, 0xD9];
        make_psd(&psd, &jpeg);
        assert_eq!(psd_preview(&psd).unwrap().as_deref(), Some(&jpeg[..]));

        let not = tmp.path().join("b.psd");
        std::fs::write(&not, b"hello world, not a psd at all").unwrap();
        assert!(psd_preview(&not).is_err());
    }

    #[test]
    fn cache_name_follows_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.png");
        std::fs::write(&f, b"1").unwrap();
        let a = cache_path(tmp.path(), &f).unwrap();
        std::fs::write(&f, b"22").unwrap();
        assert_ne!(a, cache_path(tmp.path(), &f).unwrap());
    }
}
