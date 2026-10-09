//! Which graphics API draws the window.
//!
//! OpenGL (glow) is the default everywhere. On Windows without OpenGL 2 (no GPU driver, virtual
//! machines, some Remote Desktop sessions) it can't start, so CraftSpace restarts itself with
//! Direct3D through wgpu, which Windows always provides (in software if need be), and remembers
//! that for next time. `CRAFTSPACE_RENDERER=glow|wgpu` forces one.

use craftspace_core::paths::Paths;
use eframe::Renderer;

const ENV: &str = "CRAFTSPACE_RENDERER";

#[cfg_attr(not(windows), allow(dead_code))]
fn marker(paths: &Paths) -> std::path::PathBuf {
    paths.cache.join("renderer")
}

pub fn choose(paths: &Paths) -> Renderer {
    #[cfg(windows)]
    {
        let wanted = std::env::var(ENV).ok().or_else(|| std::fs::read_to_string(marker(paths)).ok());
        if wanted.as_deref().map(str::trim) == Some("wgpu") {
            return Renderer::Wgpu;
        }
    }
    let _ = paths;
    Renderer::Glow
}

/// After the window failed to open: on Windows, start again with the other renderer.
pub fn fall_back(paths: &Paths, used: Renderer, err: &eframe::Error) {
    log::error!("the window couldn't open with {used}: {err}");
    #[cfg(windows)]
    if used == Renderer::Glow && std::env::var_os(ENV).is_none() {
        log::warn!("restarting with Direct3D (wgpu)");
        let _ = std::fs::create_dir_all(&paths.cache);
        let _ = std::fs::write(marker(paths), "wgpu");
        if let Ok(exe) = std::env::current_exe() {
            let spawned = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env(ENV, "wgpu").spawn();
            if spawned.is_ok() {
                std::process::exit(0);
            }
        }
    }
    let _ = (paths, ENV);
}
