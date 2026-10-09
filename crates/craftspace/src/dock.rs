//! The Dock icon on macOS: CraftSpace's full-colour icon while its window is open, and the icon
//! in the app bundle otherwise, which macOS draws in the icon style chosen in System Settings ›
//! Appearance (dark, clear or tinted), like the other ArtCraft apps.

use craftspace_core::settings::DockIcon;

/// The full-colour icon, on Apple's icon grid.
#[cfg(target_os = "macos")]
const COLOR_ICON: &[u8] = include_bytes!("../../../assets/craftspace-macos-512.png");

/// Shows the right icon for `style` and whether the window is open; only acts on changes.
#[derive(Default)]
pub struct Dock {
    shown: Option<bool>,
}

impl Dock {
    pub fn update(&mut self, style: DockIcon, window_open: bool) {
        let color = match style {
            DockIcon::ColorWhenOpen => window_open,
            DockIcon::Color => true,
            DockIcon::System => false,
        };
        if self.shown != Some(color) {
            self.shown = Some(color);
            set_color_icon(color);
        }
    }
}

#[cfg(target_os = "macos")]
fn set_color_icon(color: bool) {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    if color {
        let data = NSData::with_bytes(COLOR_ICON);
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
            unsafe { app.setApplicationIconImage(Some(&image)) };
        }
    } else {
        // Back to the bundle's icon, drawn by macOS.
        unsafe { app.setApplicationIconImage(None) };
    }
}

#[cfg(not(target_os = "macos"))]
fn set_color_icon(_color: bool) {}
