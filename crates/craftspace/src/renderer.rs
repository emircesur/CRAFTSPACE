//! Which graphics API draws the window.
//!
//! - macOS: Metal (through wgpu). OpenGL is deprecated there and runs through a translation
//!   layer that drops frames, especially on 120 Hz ProMotion displays.
//! - Windows: OpenGL (glow). Without OpenGL 2 (no GPU driver, virtual machines, some Remote
//!   Desktop sessions) it can't start, so CraftSpace restarts itself with Direct3D through wgpu,
//!   which Windows always provides (in software if need be), and remembers that.
//! - Linux: OpenGL.
//!
//! If the preferred one fails on macOS or Windows, CraftSpace restarts with the other.
//! `CRAFTSPACE_RENDERER=glow|wgpu` forces one.

use craftspace_core::paths::Paths;
use eframe::Renderer;

const ENV: &str = "CRAFTSPACE_RENDERER";

#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn marker(paths: &Paths) -> std::path::PathBuf {
    paths.cache.join("renderer")
}

#[cfg(any(windows, target_os = "macos"))]
fn parse(s: &str) -> Option<Renderer> {
    match s.trim() {
        "wgpu" => Some(Renderer::Wgpu),
        "glow" => Some(Renderer::Glow),
        _ => None,
    }
}

pub fn choose(paths: &Paths) -> Renderer {
    let _ = paths;
    #[cfg(any(windows, target_os = "macos"))]
    {
        if let Some(r) = std::env::var(ENV).ok().as_deref().and_then(parse) {
            return r;
        }
        // A fallback that worked before.
        if let Some(r) = std::fs::read_to_string(marker(paths)).ok().as_deref().and_then(parse) {
            return r;
        }
        if cfg!(target_os = "macos") {
            return Renderer::Wgpu;
        }
    }
    Renderer::Glow
}

/// After the window failed to open: on macOS and Windows, start again with the other renderer.
pub fn fall_back(paths: &Paths, used: Renderer, err: &eframe::Error) {
    log::error!("the window couldn't open with {used}: {err}");
    #[cfg(any(windows, target_os = "macos"))]
    if std::env::var_os(ENV).is_none() {
        let other = if used == Renderer::Glow { "wgpu" } else { "glow" };
        log::warn!("restarting with {other}");
        let _ = std::fs::create_dir_all(&paths.cache);
        let _ = std::fs::write(marker(paths), other);
        if let Ok(exe) = std::env::current_exe() {
            let spawned = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env(ENV, other).spawn();
            if spawned.is_ok() {
                std::process::exit(0);
            }
        }
    }
    let _ = (paths, ENV);
}
