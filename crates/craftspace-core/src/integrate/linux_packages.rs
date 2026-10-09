//! Installing apps as system packages on Linux: `.rpm` through dnf (or zypper) on Fedora, RHEL
//! and openSUSE, `.deb` through apt on Debian and Ubuntu.
//!
//! The package manager then owns the files (menu entries, icons, `/usr/bin` links), and
//! CraftSpace keeps them up to date by installing each new release's package. Installing needs
//! administrator rights: when CraftSpace isn't running as root it asks through `pkexec`, which
//! shows the desktop's password prompt.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::platform::AssetKind;

fn is_root() -> bool {
    Command::new("id").arg("-u").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}

fn have(tool: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {tool}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Run `args` as root: directly when we are root, else through pkexec.
fn run_as_root(args: &[&str], quiet: bool) -> anyhow::Result<()> {
    let mut cmd = if is_root() {
        let mut c = Command::new(args[0]);
        c.args(&args[1..]);
        c
    } else {
        anyhow::ensure!(
            have("pkexec"),
            "installing system packages needs administrator rights; run: sudo {}",
            args.join(" ")
        );
        let mut c = Command::new("pkexec");
        c.args(args);
        c
    };
    if quiet {
        cmd.stdout(Stdio::null());
    }
    let status = cmd.status()?;
    match status.code() {
        Some(0) => Ok(()),
        // pkexec: the user dismissed the password prompt.
        Some(126) | Some(127) if !is_root() => anyhow::bail!("administrator access was not given"),
        _ => anyhow::bail!("`{}` failed ({status})", args.join(" ")),
    }
}

/// The package's name, read from the file.
pub fn package_name(kind: AssetKind, path: &Path) -> anyhow::Result<String> {
    let out = match kind {
        AssetKind::Rpm => Command::new("rpm").args(["-qp", "--qf", "%{NAME}"]).arg(path).output()?,
        AssetKind::Deb => Command::new("dpkg-deb").args(["-f"]).arg(path).arg("Package").output()?,
        other => anyhow::bail!("{other:?} is not a system package"),
    };
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    anyhow::ensure!(out.status.success() && !name.is_empty(), "couldn't read the package name of {}", path.display());
    Ok(name)
}

/// Install (or upgrade, or with `downgrade` go back to) the package at `path`.
pub fn install(kind: AssetKind, path: &Path, downgrade: bool, quiet: bool) -> anyhow::Result<()> {
    let path = path.canonicalize()?;
    let file = path.to_string_lossy().into_owned();
    match kind {
        AssetKind::Rpm if have("dnf") && !downgrade => run_as_root(&["dnf", "install", "-y", &file], quiet),
        AssetKind::Rpm if have("zypper") => {
            let mut args = vec!["zypper", "--non-interactive", "install", "--allow-unsigned-rpm"];
            if downgrade {
                args.push("--oldpackage");
            }
            args.push(&file);
            run_as_root(&args, quiet)
        }
        // Dependencies are already there from the newer version.
        AssetKind::Rpm => run_as_root(&["rpm", "-U", "--oldpackage", "--replacepkgs", &file], quiet),
        AssetKind::Deb => run_as_root(
            &["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y", "--allow-downgrades", &file],
            quiet,
        ),
        other => anyhow::bail!("{other:?} is not a system package"),
    }
}

pub fn remove(kind: AssetKind, name: &str, quiet: bool) -> anyhow::Result<()> {
    match kind {
        AssetKind::Rpm if have("dnf") => run_as_root(&["dnf", "remove", "-y", name], quiet),
        AssetKind::Rpm if have("zypper") => run_as_root(&["zypper", "--non-interactive", "remove", name], quiet),
        AssetKind::Rpm => run_as_root(&["rpm", "-e", name], quiet),
        AssetKind::Deb => {
            run_as_root(&["env", "DEBIAN_FRONTEND=noninteractive", "apt-get", "purge", "-y", name], quiet)
        }
        other => anyhow::bail!("{other:?} is not a system package"),
    }
}

/// Whether the package is installed (it may have been removed outside CraftSpace).
pub fn is_installed(kind: AssetKind, name: &str) -> bool {
    match kind {
        AssetKind::Rpm => {
            Command::new("rpm").args(["-q", name]).stdout(Stdio::null()).status().is_ok_and(|s| s.success())
        }
        AssetKind::Deb => Command::new("dpkg-query")
            .args(["-W", "-f", "${Status}", name])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("install ok installed")),
        _ => false,
    }
}

/// Files of the package that changed or went missing since it was installed.
pub fn verify(kind: AssetKind, name: &str) -> anyhow::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let out = match kind {
        AssetKind::Rpm => Command::new("rpm").args(["-V", name]).output()?,
        AssetKind::Deb => Command::new("dpkg").args(["--verify", name]).output()?,
        other => anyhow::bail!("{other:?} is not a system package"),
    };
    Ok(parse_verify(&String::from_utf8_lossy(&out.stdout)))
}

/// rpm: `missing     /usr/bin/x` or `S.5....T.  c /etc/x`; dpkg: `??5?????? c /etc/x`.
fn parse_verify(text: &str) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut missing = Vec::new();
    let mut changed = Vec::new();
    for line in text.lines() {
        let Some(path) = line.split_whitespace().last().filter(|p| p.starts_with('/')) else { continue };
        // Docs and man pages are often left out on purpose (minimal and container installs).
        if path.starts_with("/usr/share/doc/") || path.starts_with("/usr/share/man/") {
            continue;
        }
        // Config files are expected to change.
        if line.split_whitespace().nth(1) == Some("c") {
            continue;
        }
        if line.starts_with("missing") {
            missing.push(PathBuf::from(path));
        } else {
            changed.push(PathBuf::from(path));
        }
    }
    (missing, changed)
}

/// The app's program: `/usr/bin/<binary>`, or wherever the package put it.
pub fn find_program(kind: AssetKind, name: &str, binary: &str) -> Option<PathBuf> {
    let usr_bin = PathBuf::from("/usr/bin").join(binary);
    if usr_bin.is_file() {
        return Some(usr_bin);
    }
    let out = match kind {
        AssetKind::Rpm => Command::new("rpm").args(["-ql", name]).output().ok()?,
        AssetKind::Deb => Command::new("dpkg").args(["-L", name]).output().ok()?,
        _ => return None,
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.ends_with(&format!("/bin/{binary}")))
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_output_is_parsed() {
        let (m, c) = parse_verify(
            "missing     /usr/bin/photocraft-cli\nS.5....T.    /usr/bin/photocraft\nS.5....T.  c /etc/photocraft.conf\nmissing     /usr/share/doc/photocraft/README.md\n",
        );
        assert_eq!(m, [PathBuf::from("/usr/bin/photocraft-cli")]);
        assert_eq!(c, [PathBuf::from("/usr/bin/photocraft")]);
    }
}
