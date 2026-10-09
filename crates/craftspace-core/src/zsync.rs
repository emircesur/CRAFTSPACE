//! Delta updates from `.zsync` files.
//!
//! ArtCraft releases publish an `<app>.AppImage.zsync` next to each AppImage: the file split into
//! fixed-size blocks, with a weak rolling checksum and a truncated MD4 per block. The previous
//! AppImage already contains most of those blocks (often at different offsets), so an update
//! slides over the old file to find them and downloads only the rest with HTTP range requests.
//! The result is checked against the SHA-1 in the `.zsync` header (and by the caller against the
//! release's SHA-256).

use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::Path;

use md4::{Digest as _, Md4};
use sha1::Sha1;
use ureq::Agent;

use crate::download::{self, Progress, ProgressEvent, Stage};

#[derive(Debug, Clone)]
pub struct ZsyncFile {
    pub filename: String,
    pub blocksize: usize,
    pub length: u64,
    pub seq_matches: usize,
    pub rsum_bytes: usize,
    pub checksum_bytes: usize,
    /// Where the target file lives, relative to the `.zsync` file's URL.
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub blocks: Vec<BlockSum>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockSum {
    /// The weak checksum, masked to `rsum_bytes`.
    pub rsum: u32,
    /// The leading `checksum_bytes` of the block's MD4.
    pub strong: [u8; 16],
}

/// Below this share of reusable blocks, a plain (resumable) download of the whole file is
/// better than hundreds of small range requests.
pub const MIN_REUSE: f64 = 0.25;

/// Too little of the old file can be reused for a delta update to be worth it.
#[derive(Debug, Clone, Copy)]
pub struct LowReuse {
    pub percent: u8,
}

impl std::fmt::Display for LowReuse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "only {}% of the installed version can be reused", self.percent)
    }
}

impl std::error::Error for LowReuse {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeltaStats {
    pub reused_bytes: u64,
    pub downloaded_bytes: u64,
    pub requests: usize,
}

impl ZsyncFile {
    pub fn parse(bytes: &[u8]) -> anyhow::Result<ZsyncFile> {
        let split = bytes
            .windows(2)
            .position(|w| w == b"\n\n")
            .ok_or_else(|| anyhow::anyhow!("not a zsync file (no header end)"))?;
        let header = std::str::from_utf8(&bytes[..split])?;
        let body = &bytes[split + 2..];
        let mut fields = HashMap::new();
        for line in header.lines() {
            if let Some((k, v)) = line.split_once(':') {
                fields.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        let get = |k: &str| fields.get(k).cloned().ok_or_else(|| anyhow::anyhow!("zsync header has no {k}"));
        let version = get("zsync")?;
        anyhow::ensure!(version.starts_with("0."), "unsupported zsync version {version}");
        let blocksize: usize = get("blocksize")?.parse()?;
        anyhow::ensure!(
            blocksize.is_power_of_two() && (512..=1 << 20).contains(&blocksize),
            "odd blocksize {blocksize}"
        );
        let length: u64 = get("length")?.parse()?;
        let (seq_matches, rsum_bytes, checksum_bytes) = match fields.get("hash-lengths") {
            Some(h) => {
                let parts: Vec<usize> = h.split(',').map(|p| p.trim().parse()).collect::<Result<_, _>>()?;
                anyhow::ensure!(parts.len() == 3, "bad Hash-Lengths {h}");
                (parts[0], parts[1], parts[2])
            }
            None => (1, 4, 16),
        };
        anyhow::ensure!((1..=2).contains(&seq_matches), "unsupported seq_matches {seq_matches}");
        anyhow::ensure!((1..=4).contains(&rsum_bytes) && (3..=16).contains(&checksum_bytes), "bad hash lengths");
        let nblocks = length.div_ceil(blocksize as u64) as usize;
        let per = rsum_bytes + checksum_bytes;
        anyhow::ensure!(body.len() >= nblocks * per, "zsync file is truncated");
        let mask = rsum_mask(rsum_bytes);
        let blocks = body
            .chunks_exact(per)
            .take(nblocks)
            .map(|c| {
                let mut r = [0u8; 4];
                r[4 - rsum_bytes..].copy_from_slice(&c[..rsum_bytes]);
                let mut strong = [0u8; 16];
                strong[..checksum_bytes].copy_from_slice(&c[rsum_bytes..]);
                BlockSum { rsum: u32::from_be_bytes(r) & mask, strong }
            })
            .collect();
        Ok(ZsyncFile {
            filename: fields.get("filename").cloned().unwrap_or_default(),
            blocksize,
            length,
            seq_matches,
            rsum_bytes,
            checksum_bytes,
            url: fields.get("url").cloned(),
            sha1: fields.get("sha-1").map(|s| s.to_ascii_lowercase()),
            blocks,
        })
    }

    /// For each block of the target, the offset in `seed` where an identical block was found.
    pub fn match_seed(&self, seed: &[u8]) -> Vec<Option<usize>> {
        let bs = self.blocksize;
        let n = self.blocks.len();
        let mut found: Vec<Option<usize>> = vec![None; n];
        if seed.len() < bs || n == 0 {
            return found;
        }
        let mask = rsum_mask(self.rsum_bytes);
        let seq = self.seq_matches == 2 && n > 1;
        // Index the target blocks by weak checksum (pairs of consecutive blocks when seq == 2).
        let mut index: HashMap<u64, Vec<usize>> = HashMap::new();
        if seq {
            for i in 0..n - 1 {
                let key = (u64::from(self.blocks[i].rsum) << 32) | u64::from(self.blocks[i + 1].rsum);
                index.entry(key).or_default().push(i);
            }
        } else {
            for (i, b) in self.blocks.iter().enumerate() {
                index.entry(u64::from(b.rsum)).or_default().push(i);
            }
        }
        // A quick filter on the first window's checksum before the hash map.
        let mut first_seen = vec![false; 1 << 16];
        for (i, b) in self.blocks.iter().enumerate() {
            if !seq || i + 1 < n {
                first_seen[(b.rsum & 0xffff) as usize] = true;
            }
        }

        let strong_matches = |block: usize, at: usize| -> bool {
            let mut window = &seed[at..(at + bs).min(seed.len())];
            let padded;
            if window.len() < bs {
                let mut v = window.to_vec();
                v.resize(bs, 0);
                padded = v;
                window = &padded;
            }
            Md4::digest(window)[..self.checksum_bytes] == self.blocks[block].strong[..self.checksum_bytes]
        };

        let last_full = seed.len() - bs;
        let mut p = 0usize;
        let mut w1 = Rsum::of(&seed[p..p + bs]);
        let mut w2 = if seq && p + 2 * bs <= seed.len() { Some(Rsum::of(&seed[p + bs..p + 2 * bs])) } else { None };
        loop {
            let r1 = w1.value() & mask;
            let mut matched = false;
            if first_seen[(r1 & 0xffff) as usize] {
                let key = if seq {
                    w2.map(|w2| (u64::from(r1) << 32) | u64::from(w2.value() & mask))
                } else {
                    Some(u64::from(r1))
                };
                if let Some(candidates) = key.and_then(|k| index.get(&k)) {
                    for &i in candidates {
                        if found[i].is_some() && (!seq || found[i + 1].is_some()) {
                            continue;
                        }
                        if strong_matches(i, p) && (!seq || strong_matches(i + 1, p + bs)) {
                            found[i] = Some(p);
                            if seq {
                                found[i + 1] = Some(p + bs);
                            }
                            matched = true;
                        }
                    }
                }
            }
            if matched {
                // Skip past the matched block(s) and start the windows afresh.
                p += bs;
                if p > last_full {
                    break;
                }
                w1 = Rsum::of(&seed[p..p + bs]);
                w2 = if seq && p + 2 * bs <= seed.len() { Some(Rsum::of(&seed[p + bs..p + 2 * bs])) } else { None };
                continue;
            }
            if p >= last_full {
                break;
            }
            w1.roll(seed[p], seed[p + bs], bs);
            if let Some(w) = w2.as_mut() {
                if p + 2 * bs < seed.len() {
                    w.roll(seed[p + bs], seed[p + 2 * bs], bs);
                } else {
                    w2 = None;
                }
            }
            p += 1;
        }
        found
    }
}

fn rsum_mask(rsum_bytes: usize) -> u32 {
    if rsum_bytes >= 4 {
        u32::MAX
    } else {
        (1u32 << (8 * rsum_bytes)) - 1
    }
}

/// The rsync-style rolling checksum zsync uses: `a` is the byte sum, `b` the position-weighted
/// sum, both modulo 2^16; the 32-bit value is `a << 16 | b`.
#[derive(Debug, Clone, Copy)]
struct Rsum {
    a: u16,
    b: u16,
}

impl Rsum {
    fn of(block: &[u8]) -> Rsum {
        let (mut a, mut b) = (0u16, 0u16);
        let len = block.len();
        for (i, &c) in block.iter().enumerate() {
            a = a.wrapping_add(u16::from(c));
            b = b.wrapping_add(((len - i) as u16).wrapping_mul(u16::from(c)));
        }
        Rsum { a, b }
    }

    fn roll(&mut self, old: u8, new: u8, len: usize) {
        self.a = self.a.wrapping_add(u16::from(new)).wrapping_sub(u16::from(old));
        self.b = self.b.wrapping_add(self.a).wrapping_sub((len as u16).wrapping_mul(u16::from(old)));
    }

    fn value(self) -> u32 {
        (u32::from(self.a) << 16) | u32::from(self.b)
    }
}

/// Merge missing blocks into at most `max_requests` byte ranges (`(first_block, last_block)`).
fn missing_ranges(found: &[Option<usize>], max_requests: usize) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (i, f) in found.iter().enumerate() {
        if f.is_none() {
            match ranges.last_mut() {
                Some((_, end)) if *end + 1 == i => *end = i,
                _ => ranges.push((i, i)),
            }
        }
    }
    // Close the smallest gaps first until few enough requests remain.
    while ranges.len() > max_requests.max(1) {
        let (k, _) = ranges.windows(2).enumerate().min_by_key(|(_, w)| w[1].0 - w[0].1).expect("two ranges");
        ranges[k].1 = ranges[k + 1].1;
        ranges.remove(k + 1);
    }
    ranges
}

/// Build `dest` (the file described by the `.zsync` at `zsync_url`, downloadable from
/// `target_url`) from `seed` plus range requests for what `seed` doesn't have.
pub fn update_from_seed(
    agent: &Agent,
    zsync_url: &str,
    target_url: &str,
    seed: &Path,
    dest: &Path,
    progress: &Progress,
) -> anyhow::Result<DeltaStats> {
    progress.stage(Stage::Resolving);
    let mut resp = agent.get(zsync_url).call()?;
    crate::http::check_status(zsync_url, resp.status().as_u16())?;
    let bytes = resp.body_mut().with_config().limit(64 << 20).read_to_vec()?;
    let zs = ZsyncFile::parse(&bytes)?;

    let seed_bytes = std::fs::read(seed)?;
    let found = zs.match_seed(&seed_bytes);
    progress.check_cancelled()?;
    let reused = found.iter().filter(|f| f.is_some()).count();
    if (reused as f64) < (found.len() as f64) * MIN_REUSE {
        return Err(LowReuse { percent: (reused * 100 / found.len().max(1)) as u8 }.into());
    }

    let bs = zs.blocksize as u64;
    let ranges = missing_ranges(&found, 256);
    let to_download: u64 = ranges.iter().map(|&(a, b)| ((b as u64 + 1) * bs).min(zs.length) - a as u64 * bs).sum();
    progress.stage(Stage::Downloading);
    (progress.report)(ProgressEvent::Bytes { done: 0, total: Some(to_download) });

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest.with_extension("part");
    let mut out = BufWriter::new(std::fs::File::create(&part)?);
    let mut sha1 = Sha1::new();
    let mut stats = DeltaStats { requests: ranges.len(), ..DeltaStats::default() };
    let mut range_iter = ranges.iter().peekable();
    let mut block = 0usize;
    let result = (|| -> anyhow::Result<()> {
        while block < found.len() {
            let start = block as u64 * bs;
            if let Some(&&(first, last)) = range_iter.peek().filter(|(first, _)| *first == block) {
                range_iter.next();
                let end = ((last as u64 + 1) * bs).min(zs.length) - 1;
                let data = download::fetch_range(agent, target_url, start, end, progress)?;
                sha1.update(&data);
                out.write_all(&data)?;
                stats.downloaded_bytes += data.len() as u64;
                (progress.report)(ProgressEvent::Bytes { done: stats.downloaded_bytes, total: Some(to_download) });
                block = last + 1;
                let _ = first;
            } else {
                let at = found[block].expect("blocks outside ranges were found in the seed");
                let len = (bs.min(zs.length - start)) as usize;
                let data = &seed_bytes[at..at + len];
                sha1.update(data);
                out.write_all(data)?;
                stats.reused_bytes += len as u64;
                block += 1;
            }
        }
        out.flush()?;
        Ok(())
    })();
    drop(out);
    if let Err(err) = result {
        let _ = std::fs::remove_file(&part);
        return Err(err);
    }
    progress.stage(Stage::Verifying);
    let got = hex::encode(sha1.finalize());
    if let Some(expected) = &zs.sha1 {
        if *expected != got {
            let _ = std::fs::remove_file(&part);
            anyhow::bail!("delta update produced the wrong file (SHA-1 {got}, expected {expected})");
        }
    }
    std::fs::rename(&part, dest)?;
    Ok(stats)
}

/// Write a `.zsync` file for `data` (used by tests; the format matches `zsyncmake`).
pub fn make(data: &[u8], filename: &str, blocksize: usize, hash_lengths: (usize, usize, usize)) -> Vec<u8> {
    let (seq, rsum_bytes, checksum_bytes) = hash_lengths;
    let mut out = format!(
        "zsync: 0.6.2\nFilename: {filename}\nBlocksize: {blocksize}\nLength: {}\nHash-Lengths: {seq},{rsum_bytes},{checksum_bytes}\nURL: {filename}\nSHA-1: {}\n\n",
        data.len(),
        hex::encode(Sha1::digest(data))
    )
    .into_bytes();
    for chunk in data.chunks(blocksize) {
        let mut block = chunk.to_vec();
        block.resize(blocksize, 0);
        let r = Rsum::of(&block);
        let be = [(r.a >> 8) as u8, r.a as u8, (r.b >> 8) as u8, r.b as u8];
        out.extend_from_slice(&be[4 - rsum_bytes..]);
        out.extend_from_slice(&Md4::digest(&block)[..checksum_bytes]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn pseudo_random(n: usize, seed: u64) -> Vec<u8> {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    #[test]
    fn rolling_checksum_matches_recomputing() {
        let data = pseudo_random(10_000, 7);
        let bs = 2048;
        let mut r = Rsum::of(&data[..bs]);
        for p in 0..500 {
            r.roll(data[p], data[p + bs], bs);
            let fresh = Rsum::of(&data[p + 1..p + 1 + bs]);
            assert_eq!(r.value(), fresh.value(), "at {p}");
        }
    }

    #[test]
    fn parses_what_make_writes() {
        let data = pseudo_random(10_000, 3);
        let zs = ZsyncFile::parse(&make(&data, "x.AppImage", 2048, (2, 2, 5))).unwrap();
        assert_eq!(zs.blocks.len(), 5);
        assert_eq!(zs.length, 10_000);
        assert_eq!((zs.seq_matches, zs.rsum_bytes, zs.checksum_bytes), (2, 2, 5));
        assert_eq!(zs.url.as_deref(), Some("x.AppImage"));
    }

    #[test]
    fn finds_shifted_blocks_and_builds_the_target() {
        // The new version: the old one with an insertion near the start, a changed middle and
        // a deletion, so most blocks move to unaligned offsets.
        let old = pseudo_random(400_000, 11);
        let mut new = Vec::new();
        new.extend_from_slice(&old[..1000]);
        new.extend_from_slice(&pseudo_random(777, 99));
        new.extend_from_slice(&old[1000..150_000]);
        new.extend_from_slice(&pseudo_random(20_000, 5));
        new.extend_from_slice(&old[170_000..300_000]);
        new.extend_from_slice(&old[310_000..]);

        for hash_lengths in [(2, 2, 5), (1, 4, 16), (2, 3, 8)] {
            let zs = ZsyncFile::parse(&make(&new, "new.AppImage", 2048, hash_lengths)).unwrap();
            let found = zs.match_seed(&old);
            let reused = found.iter().filter(|f| f.is_some()).count();
            assert!(reused * 10 >= found.len() * 8, "{hash_lengths:?}: only {reused}/{} blocks reused", found.len());
            for (i, f) in found.iter().enumerate() {
                if let Some(at) = f {
                    let end = ((i + 1) * 2048).min(new.len());
                    assert_eq!(&old[*at..*at + (end - i * 2048)], &new[i * 2048..end], "block {i}");
                }
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let seed = dir.path().join("old.AppImage");
        std::fs::write(&seed, &old).unwrap();
        let zsync_url = crate::download::tests::serve(make(&new, "new.AppImage", 2048, (2, 2, 5)));
        let target_url = crate::download::tests::serve(new.clone());
        let dest = dir.path().join("new.AppImage");
        let cancel = AtomicBool::new(false);
        let report = |_| {};
        let stats = update_from_seed(
            &crate::http::agent(),
            &zsync_url,
            &target_url,
            &seed,
            &dest,
            &Progress { report: &report, cancel: &cancel },
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), new);
        assert!(stats.downloaded_bytes < new.len() as u64 / 5, "{stats:?}");
        assert_eq!(stats.reused_bytes + stats.downloaded_bytes, new.len() as u64);

        // A seed with nothing in common is refused rather than fetched block by block.
        let unrelated = dir.path().join("unrelated.AppImage");
        std::fs::write(&unrelated, pseudo_random(400_000, 777)).unwrap();
        let err = update_from_seed(
            &crate::http::agent(),
            &zsync_url,
            &target_url,
            &unrelated,
            &dir.path().join("x.AppImage"),
            &Progress { report: &report, cancel: &cancel },
        )
        .unwrap_err();
        assert!(err.downcast_ref::<LowReuse>().is_some(), "{err:#}");
    }

    #[test]
    fn merges_ranges_down_to_the_request_cap() {
        let found = [None, Some(0), None, None, Some(1), Some(2), Some(3), None];
        assert_eq!(missing_ranges(&found, 10), vec![(0, 0), (2, 3), (7, 7)]);
        assert_eq!(missing_ranges(&found, 2), vec![(0, 3), (7, 7)]);
        assert_eq!(missing_ranges(&found, 1), vec![(0, 7)]);
    }
}
