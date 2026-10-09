//! Platform detection and choosing the right release asset for this machine.
//!
//! ArtCraft apps publish assets named `<app>-<version>-<platform>[-portable].<ext>`, for example
//! `photocraft-0.5.0-windows-x64-portable.zip` or `photocraft-0.5.0-linux-x86_64.tar.gz`.
//! CraftSpace prefers the self-contained archives (portable zip on Windows, tarball on Linux),
//! which it can install per-user without admin rights and update side by side.

use serde::{Deserialize, Serialize};

use crate::version;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    Linux,
    Macos,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X64,
    Arm64,
    X86,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

/// How an asset gets installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetKind {
    /// Windows portable zip, unpacked and managed by CraftSpace.
    PortableZip,
    /// Linux tarball with a `bin/` + `share/` prefix layout, unpacked and managed by CraftSpace.
    TarGz,
    /// A single self-contained Linux executable, managed by CraftSpace.
    AppImage,
    /// Windows Installer package, handed to `msiexec`.
    Msi,
    /// Other system installer (`setup.exe`), run as-is.
    Exe,
    /// macOS disk image, opened for the user.
    Dmg,
    /// Linux package for dnf / zypper (Fedora, RHEL, openSUSE), installed system-wide.
    Rpm,
    /// Linux package for apt (Debian, Ubuntu), installed system-wide.
    Deb,
}

impl AssetKind {
    /// Whether CraftSpace owns the files (and can update/uninstall them itself).
    pub fn is_managed(self) -> bool {
        matches!(self, AssetKind::PortableZip | AssetKind::TarGz | AssetKind::AppImage | AssetKind::Dmg)
    }

    pub fn label(self) -> &'static str {
        match self {
            AssetKind::PortableZip => "Portable (zip)",
            AssetKind::TarGz => "Tarball",
            AssetKind::AppImage => "AppImage",
            AssetKind::Msi => "Windows Installer (MSI)",
            AssetKind::Exe => "Setup program",
            AssetKind::Dmg => "Disk image",
            AssetKind::Rpm => "RPM package (dnf)",
            AssetKind::Deb => "Debian package (apt)",
        }
    }
}

/// Which kind of package to prefer when a release has several for this platform.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssetPrefs {
    /// Windows: the MSI / setup program over the portable zip.
    pub prefer_system_installer: bool,
    /// Windows: a setup program (often per-user) over an MSI (often per-machine).
    pub prefer_exe_installer: bool,
    /// Linux: the AppImage over the tarball (smaller delta updates).
    pub prefer_appimage: bool,
    /// Linux: the distribution's package format, used when `prefer_system_installer` is set.
    pub linux_package: Option<AssetKind>,
}

impl AssetPrefs {
    pub fn system(prefer_system_installer: bool) -> AssetPrefs {
        AssetPrefs { prefer_system_installer, ..AssetPrefs::default() }
    }
}

impl Platform {
    pub fn current() -> Platform {
        let os = match std::env::consts::OS {
            "windows" => Os::Windows,
            "linux" => Os::Linux,
            "macos" => Os::Macos,
            _ => Os::Other,
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => Arch::X64,
            "aarch64" => Arch::Arm64,
            "x86" => Arch::X86,
            _ => Arch::Other,
        };
        Platform { os, arch }
    }

    /// The `<platform>` token used in ArtCraft asset names.
    pub fn token(self) -> Option<&'static str> {
        Some(match (self.os, self.arch) {
            (Os::Windows, Arch::X64) => "windows-x64",
            (Os::Windows, Arch::Arm64) => "windows-arm64",
            (Os::Windows, Arch::X86) => "windows-x86",
            (Os::Linux, Arch::X64) => "linux-x86_64",
            (Os::Linux, Arch::Arm64) => "linux-aarch64",
            (Os::Macos, _) => "macos-universal",
            _ => return None,
        })
    }

    pub fn display(self) -> String {
        let os = match self.os {
            Os::Windows => "Windows",
            Os::Linux => "Linux",
            Os::Macos => "macOS",
            Os::Other => std::env::consts::OS,
        };
        let arch = match self.arch {
            Arch::X64 => "x64",
            Arch::Arm64 => "ARM64",
            Arch::X86 => "x86",
            Arch::Other => std::env::consts::ARCH,
        };
        format!("{os} {arch}")
    }

    /// Asset kinds this platform can install, most preferred first.
    pub fn preferred_kinds(self, prefs: AssetPrefs) -> Vec<AssetKind> {
        let installers = if prefs.prefer_exe_installer {
            [AssetKind::Exe, AssetKind::Msi]
        } else {
            [AssetKind::Msi, AssetKind::Exe]
        };
        match self.os {
            Os::Windows if prefs.prefer_system_installer => {
                vec![installers[0], installers[1], AssetKind::PortableZip]
            }
            Os::Windows => vec![AssetKind::PortableZip, installers[0], installers[1]],
            Os::Linux if prefs.prefer_system_installer && prefs.linux_package.is_some() => {
                vec![prefs.linux_package.expect("checked"), AssetKind::TarGz, AssetKind::AppImage]
            }
            Os::Linux if prefs.prefer_appimage => vec![AssetKind::AppImage, AssetKind::TarGz],
            Os::Linux => vec![AssetKind::TarGz, AssetKind::AppImage],
            Os::Macos => vec![AssetKind::Dmg],
            Os::Other => vec![],
        }
    }

    fn suffix(self, kind: AssetKind) -> Option<&'static str> {
        Some(match (self.os, kind) {
            (Os::Windows, AssetKind::PortableZip) => "-portable.zip",
            (Os::Windows, AssetKind::Msi) => ".msi",
            (Os::Windows, AssetKind::Exe) => ".exe",
            (Os::Linux, AssetKind::TarGz) => ".tar.gz",
            (Os::Linux, AssetKind::AppImage) => ".AppImage",
            (Os::Linux, AssetKind::Rpm) => ".rpm",
            (Os::Linux, AssetKind::Deb) => ".deb",
            (Os::Macos, AssetKind::Dmg) => ".dmg",
            _ => return None,
        })
    }

    /// Pick the best asset for `app_id` among `names`. Returns the index into `names` and its kind.
    pub fn select_asset<S: AsRef<str>>(
        self,
        app_id: &str,
        names: &[S],
        prefs: AssetPrefs,
    ) -> Option<(usize, AssetKind)> {
        let kinds = self.preferred_kinds(prefs);
        // 1. The ArtCraft naming convention, exactly.
        if let Some(token) = self.token() {
            for &kind in &kinds {
                let Some(suffix) = self.suffix(kind) else { continue };
                let wanted_end = format!("-{token}{suffix}");
                let prefix = format!("{app_id}-");
                let found = names.iter().position(|n| {
                    let n = n.as_ref();
                    n.len() > prefix.len() + wanted_end.len()
                        && n[..prefix.len()].eq_ignore_ascii_case(&prefix)
                        && n.ends_with(&wanted_end)
                        && version::is_version(&n[prefix.len()..n.len() - wanted_end.len()])
                });
                if let Some(i) = found {
                    return Some((i, kind));
                }
            }
        }
        // 2. Anything that names our OS and architecture (for apps on another release pipeline).
        let mut best: Option<(i32, usize, AssetKind)> = None;
        for (i, name) in names.iter().enumerate() {
            let lower = name.as_ref().to_ascii_lowercase();
            let Some(kind) = kind_from_name(&lower) else { continue };
            let Some(rank) = kinds.iter().position(|k| *k == kind) else { continue };
            if !self.os_matches(&lower, kind) {
                continue;
            }
            let arch_score = match self.arch_match(&lower) {
                Some(true) => 10,
                None => 5,
                Some(false) => continue,
            };
            let score = arch_score * 10 - rank as i32;
            if best.is_none_or(|(s, _, _)| score > s) {
                best = Some((score, i, kind));
            }
        }
        best.map(|(_, i, kind)| (i, kind))
    }

    fn os_matches(self, lower: &str, kind: AssetKind) -> bool {
        match self.os {
            // MSI and setup exes are Windows-only by nature.
            Os::Windows => {
                matches!(kind, AssetKind::Msi | AssetKind::Exe)
                    || lower.contains("windows")
                    || lower.contains("win64")
                    || lower.contains("win32")
            }
            Os::Linux => kind == AssetKind::AppImage || lower.contains("linux"),
            Os::Macos => true,
            Os::Other => false,
        }
    }

    /// `Some(true)` if the name names our arch, `Some(false)` if it names another, `None` if neither.
    fn arch_match(self, lower: &str) -> Option<bool> {
        const X64: &[&str] = &["x64", "x86_64", "amd64", "win64"];
        const ARM64: &[&str] = &["arm64", "aarch64"];
        const X86: &[&str] = &["x86", "i686", "i386", "win32"];
        let has = |set: &[&str]| set.iter().any(|t| lower.contains(t));
        let (is_x64, is_arm, is_x86) = (has(X64), has(ARM64), has(X86) && !has(&["x86_64"]));
        if !(is_x64 || is_arm || is_x86) {
            return None;
        }
        Some(match self.arch {
            Arch::X64 => is_x64,
            Arch::Arm64 => is_arm,
            Arch::X86 => is_x86,
            Arch::Other => false,
        })
    }
}

/// The package format of this Linux distribution, from `/etc/os-release`.
pub fn linux_package_format() -> Option<AssetKind> {
    static FORMAT: std::sync::OnceLock<Option<AssetKind>> = std::sync::OnceLock::new();
    *FORMAT.get_or_init(|| {
        if !cfg!(target_os = "linux") {
            return None;
        }
        let text = std::fs::read_to_string("/etc/os-release").ok()?;
        package_format_from_os_release(&text)
    })
}

pub fn package_format_from_os_release(text: &str) -> Option<AssetKind> {
    let mut ids = String::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("ID=").or_else(|| line.strip_prefix("ID_LIKE=")) {
            ids.push(' ');
            ids.push_str(&v.trim_matches('"').to_ascii_lowercase());
        }
    }
    let has = |names: &[&str]| ids.split_whitespace().any(|id| names.contains(&id));
    if has(&[
        "fedora",
        "rhel",
        "centos",
        "rocky",
        "almalinux",
        "suse",
        "opensuse",
        "opensuse-tumbleweed",
        "opensuse-leap",
        "mageia",
        "nobara",
        "ultramarine",
    ]) {
        Some(AssetKind::Rpm)
    } else if has(&["debian", "ubuntu", "linuxmint", "pop", "elementary", "zorin", "neon"]) {
        Some(AssetKind::Deb)
    } else {
        None
    }
}

/// Classify an asset by file name.
pub fn kind_from_name(name: &str) -> Option<AssetKind> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with("-portable.zip") || (lower.ends_with(".zip") && lower.contains("windows")) {
        Some(AssetKind::PortableZip)
    } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        Some(AssetKind::TarGz)
    } else if lower.ends_with(".appimage") {
        Some(AssetKind::AppImage)
    } else if lower.ends_with(".msi") {
        Some(AssetKind::Msi)
    } else if lower.ends_with(".exe") {
        Some(AssetKind::Exe)
    } else if lower.ends_with(".rpm") && !lower.ends_with(".src.rpm") {
        Some(AssetKind::Rpm)
    } else if lower.ends_with(".deb") {
        Some(AssetKind::Deb)
    } else if lower.ends_with(".dmg") {
        Some(AssetKind::Dmg)
    } else {
        None
    }
}

/// Whether this process runs as an administrator (root, or an elevated Windows administrator).
pub fn is_elevated() -> bool {
    #[cfg(unix)]
    return unsafe { libc::geteuid() } == 0;
    #[cfg(windows)]
    return unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } != 0;
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHOTOCRAFT: &[&str] = &[
        "photocraft-0.5.0-freebsd-x86_64.tar.gz",
        "photocraft-0.5.0-linux-aarch64.AppImage",
        "photocraft-0.5.0-linux-aarch64.AppImage.zsync",
        "photocraft-0.5.0-linux-aarch64.deb",
        "photocraft-0.5.0-linux-aarch64.tar.gz",
        "photocraft-0.5.0-linux-x86_64.AppImage",
        "photocraft-0.5.0-linux-x86_64.deb",
        "photocraft-0.5.0-linux-x86_64.tar.gz",
        "photocraft-0.5.0-macos-universal.dmg",
        "photocraft-0.5.0-windows-arm64-portable.zip",
        "photocraft-0.5.0-windows-arm64.msi",
        "photocraft-0.5.0-windows-x64-portable.zip",
        "photocraft-0.5.0-windows-x64.msi",
        "photocraft-0.5.0-windows-x86-portable.zip",
        "photocraft-0.5.0-windows-x86.msi",
        "photocraft-cli-0.5.0-macos-universal.zip",
        "photocraft-web-0.5.0.zip",
        "SHA256SUMS.txt",
    ];

    fn pick(os: Os, arch: Arch, system: bool) -> Option<(&'static str, AssetKind)> {
        Platform { os, arch }
            .select_asset("photocraft", PHOTOCRAFT, AssetPrefs::system(system))
            .map(|(i, k)| (PHOTOCRAFT[i], k))
    }

    #[test]
    fn picks_convention_assets() {
        assert_eq!(
            pick(Os::Windows, Arch::X64, false),
            Some(("photocraft-0.5.0-windows-x64-portable.zip", AssetKind::PortableZip))
        );
        assert_eq!(pick(Os::Windows, Arch::X64, true), Some(("photocraft-0.5.0-windows-x64.msi", AssetKind::Msi)));
        assert_eq!(
            pick(Os::Windows, Arch::Arm64, false),
            Some(("photocraft-0.5.0-windows-arm64-portable.zip", AssetKind::PortableZip))
        );
        assert_eq!(
            pick(Os::Windows, Arch::X86, false),
            Some(("photocraft-0.5.0-windows-x86-portable.zip", AssetKind::PortableZip))
        );
        assert_eq!(pick(Os::Linux, Arch::X64, false), Some(("photocraft-0.5.0-linux-x86_64.tar.gz", AssetKind::TarGz)));
        assert_eq!(
            pick(Os::Linux, Arch::Arm64, false),
            Some(("photocraft-0.5.0-linux-aarch64.tar.gz", AssetKind::TarGz))
        );
        assert_eq!(pick(Os::Macos, Arch::Arm64, false), Some(("photocraft-0.5.0-macos-universal.dmg", AssetKind::Dmg)));
    }

    #[test]
    fn falls_back_to_appimage() {
        let names = ["photocraft-0.5.0-linux-x86_64.AppImage", "photocraft-0.5.0-linux-x86_64.deb"];
        let p = Platform { os: Os::Linux, arch: Arch::X64 };
        assert_eq!(p.select_asset("photocraft", &names, AssetPrefs::default()), Some((0, AssetKind::AppImage)));
    }

    #[test]
    fn generic_names_for_other_pipelines() {
        let names = [
            "ArtCraft_0.41.0_aarch64.dmg",
            "ArtCraft_0.41.0_x64-setup.exe",
            "ArtCraft_0.41.0_x64_en-US.msi",
            "artcraft_0.41.0_amd64.AppImage",
        ];
        let win = Platform { os: Os::Windows, arch: Arch::X64 };
        assert_eq!(win.select_asset("artcraft", &names, AssetPrefs::default()), Some((2, AssetKind::Msi)));
        let linux = Platform { os: Os::Linux, arch: Arch::X64 };
        assert_eq!(linux.select_asset("artcraft", &names, AssetPrefs::default()), Some((3, AssetKind::AppImage)));
        let arm_linux = Platform { os: Os::Linux, arch: Arch::Arm64 };
        assert_eq!(arm_linux.select_asset("artcraft", &names, AssetPrefs::default()), None);
    }

    #[test]
    fn artcraft_release_assets() {
        // The real asset list of storytold/artcraft artcraft-v0.41.0 (a Tauri app).
        let names = [
            "ArtCraft_0.41.0_universal.dmg",
            "ArtCraft_0.41.0_x64-setup.exe",
            "ArtCraft_0.41.0_x64_en-US.msi",
            "ArtCraft_universal.app.tar.gz",
        ];
        let exe_first = AssetPrefs { prefer_exe_installer: true, ..AssetPrefs::default() };
        let win = Platform { os: Os::Windows, arch: Arch::X64 };
        assert_eq!(win.select_asset("artcraft", &names, exe_first), Some((1, AssetKind::Exe)));
        assert_eq!(win.select_asset("artcraft", &names, AssetPrefs::default()), Some((2, AssetKind::Msi)));
        let mac = Platform { os: Os::Macos, arch: Arch::Arm64 };
        assert_eq!(mac.select_asset("artcraft", &names, exe_first), Some((0, AssetKind::Dmg)));
        let linux = Platform { os: Os::Linux, arch: Arch::X64 };
        assert_eq!(linux.select_asset("artcraft", &names, exe_first), None);
    }

    #[test]
    fn system_packages_on_linux() {
        let fedora =
            AssetPrefs { prefer_system_installer: true, linux_package: Some(AssetKind::Rpm), ..AssetPrefs::default() };
        let names = [
            "photocraft-0.5.0-linux-x86_64.tar.gz",
            "photocraft-0.5.0-linux-x86_64.rpm",
            "photocraft-0.5.0-linux-x86_64.deb",
        ];
        let p = Platform { os: Os::Linux, arch: Arch::X64 };
        assert_eq!(p.select_asset("photocraft", &names, fedora), Some((1, AssetKind::Rpm)));
        let ubuntu = AssetPrefs { linux_package: Some(AssetKind::Deb), ..fedora };
        assert_eq!(p.select_asset("photocraft", &names, ubuntu), Some((2, AssetKind::Deb)));
        // Only when asked for.
        let off = AssetPrefs { prefer_system_installer: false, ..fedora };
        assert_eq!(p.select_asset("photocraft", &names, off), Some((0, AssetKind::TarGz)));
        assert!(!AssetKind::Rpm.is_managed());
    }

    #[test]
    fn detects_distribution_families() {
        assert_eq!(
            package_format_from_os_release("NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=43\n"),
            Some(AssetKind::Rpm)
        );
        assert_eq!(
            package_format_from_os_release("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n"),
            Some(AssetKind::Rpm)
        );
        assert_eq!(package_format_from_os_release("ID=ubuntu\nID_LIKE=debian\n"), Some(AssetKind::Deb));
        assert_eq!(package_format_from_os_release("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"), Some(AssetKind::Deb));
        assert_eq!(package_format_from_os_release("ID=arch\n"), None);
    }

    #[test]
    fn nothing_for_unknown_platforms() {
        assert_eq!(pick(Os::Other, Arch::X64, false), None);
    }
}
