//! Is an installed app running right now?

use std::path::{Path, PathBuf};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

/// Whether any process's executable lives under one of `roots`.
pub fn any_running_under(roots: &[PathBuf]) -> bool {
    // An empty path would match every program.
    let roots: Vec<PathBuf> = roots.iter().filter(|r| !r.as_os_str().is_empty()).map(|r| normalize(r)).collect();
    if roots.is_empty() {
        return false;
    }
    let mut sys = System::new_with_specifics(RefreshKind::nothing());
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    sys.processes().values().any(|p| p.exe().is_some_and(|exe| under_any(&normalize(exe), &roots)))
}

fn under_any(exe: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| exe.starts_with(r))
}

/// A comparable form of `path`: resolved, and on Windows without the `\\?\` prefix that
/// `canonicalize` adds (process paths don't have it) and lowercased (paths are case-insensitive).
fn normalize(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if cfg!(windows) {
        let s = resolved.to_string_lossy();
        let s = s
            .strip_prefix(r"\\?\UNC\")
            .map(|rest| format!(r"\\{rest}"))
            .unwrap_or_else(|| s.strip_prefix(r"\\?\").unwrap_or(&s).to_string());
        PathBuf::from(s.to_lowercase())
    } else {
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_path_matches_nothing() {
        assert!(!any_running_under(&[PathBuf::new()]));
    }

    #[test]
    fn sees_this_test_running() {
        let me = std::env::current_exe().unwrap();
        assert!(any_running_under(&[me.parent().unwrap().to_path_buf()]));
        assert!(!any_running_under(&[PathBuf::from("/definitely/not/here")]));
        assert!(!any_running_under(&[]));
    }
}
