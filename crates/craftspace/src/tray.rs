//! The system tray (Windows), menu bar (macOS) or status notifier (Linux) icon.

use std::sync::mpsc::Sender;
#[cfg(all(unix, not(target_os = "macos")))]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Show,
    CheckUpdates,
    UpdateAll,
    Quit,
}

/// CraftSpace's icon at `size`, as RGBA.
fn icon_rgba(size: u32) -> Vec<u8> {
    let img = image::load_from_memory(craftspace_core::selfupdate::ICON_PNG).expect("bundled icon decodes");
    img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8().into_raw()
}

#[cfg(any(windows, target_os = "macos"))]
pub struct Tray {
    icon: tray_icon::TrayIcon,
    update_all: tray_icon::menu::MenuItem,
}

#[cfg(any(windows, target_os = "macos"))]
impl Tray {
    pub fn new(ctx: &egui::Context, tx: Sender<TrayCommand>) -> Option<Tray> {
        use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};

        let open = MenuItem::new("Open CraftSpace", true, None);
        let check = MenuItem::new("Check for updates", true, None);
        let update_all = MenuItem::new("Update all", false, None);
        let quit = MenuItem::new("Quit CraftSpace", true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &check, &update_all, &PredefinedMenuItem::separator(), &quit]).ok()?;
        let ids = [
            (open.id().clone(), TrayCommand::Show),
            (check.id().clone(), TrayCommand::CheckUpdates),
            (update_all.id().clone(), TrayCommand::UpdateAll),
            (quit.id().clone(), TrayCommand::Quit),
        ];
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some((_, cmd)) = ids.iter().find(|(id, _)| *id == event.id) {
                    let _ = tx.send(*cmd);
                    ctx.request_repaint();
                }
            }));
        }
        {
            let ctx = ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::DoubleClick { .. } = event {
                    let _ = tx.send(TrayCommand::Show);
                    ctx.request_repaint();
                }
            }));
        }
        let icon = Icon::from_rgba(icon_rgba(32), 32, 32).ok()?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("CraftSpace")
            .with_icon(icon)
            .build()
            .map_err(|e| log::warn!("no tray icon: {e}"))
            .ok()?;
        Some(Tray { icon, update_all })
    }

    /// Whether the icon is on screen, so a closed window can be found again.
    pub fn shown(&self) -> bool {
        true
    }

    pub fn set_updates(&self, n: usize) {
        self.update_all.set_enabled(n > 0);
        self.update_all.set_text(if n > 0 { format!("Update all ({n})") } else { "Update all".into() });
        let tip = if n > 0 {
            format!("CraftSpace: {n} update{} available", if n == 1 { "" } else { "s" })
        } else {
            "CraftSpace".into()
        };
        let _ = self.icon.set_tooltip(Some(tip));
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
pub struct Tray {
    handle: ksni::blocking::Handle<LinuxTray>,
    online: Arc<AtomicBool>,
}

#[cfg(all(unix, not(target_os = "macos")))]
pub struct LinuxTray {
    tx: Sender<TrayCommand>,
    ctx: egui::Context,
    updates: usize,
    icon: Vec<u8>,
    /// Whether a panel is showing the icon (it can come and go: started at login before the
    /// panel, or the panel restarts).
    online: Arc<AtomicBool>,
}

#[cfg(all(unix, not(target_os = "macos")))]
impl LinuxTray {
    fn send(&self, cmd: TrayCommand) {
        let _ = self.tx.send(cmd);
        self.ctx.request_repaint();
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "craftspace".into()
    }

    fn title(&self) -> String {
        "CraftSpace".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon { width: 32, height: 32, data: self.icon.clone() }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = match self.updates {
            0 => "Your apps are up to date".to_string(),
            n => format!("{n} update{} available", if n == 1 { "" } else { "s" }),
        };
        ksni::ToolTip { title: "CraftSpace".into(), description, ..Default::default() }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayCommand::Show);
    }

    fn watcher_online(&self) {
        log::info!("tray icon shown by the panel");
        self.online.store(true, Ordering::Relaxed);
        self.ctx.request_repaint();
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        log::info!("no panel is showing the tray icon yet: {reason:?}");
        self.online.store(false, Ordering::Relaxed);
        self.ctx.request_repaint();
        // Keep waiting: the panel may start (or restart) later.
        true
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: "Open CraftSpace".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::Show)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Check for updates".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::CheckUpdates)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if self.updates > 0 { format!("Update all ({})", self.updates) } else { "Update all".into() },
                enabled: self.updates > 0,
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::UpdateAll)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit CraftSpace".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCommand::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
impl Tray {
    pub fn new(ctx: &egui::Context, tx: Sender<TrayCommand>) -> Option<Tray> {
        use ksni::blocking::TrayMethods;
        // ARGB32 in network byte order.
        let icon = icon_rgba(32).chunks_exact(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
        let online = Arc::new(AtomicBool::new(true));
        let tray = LinuxTray { tx, ctx: ctx.clone(), updates: 0, icon, online: online.clone() };
        // Started at login, CraftSpace can be up before the panel is: register once it appears.
        match tray.assume_sni_available(true).spawn() {
            Ok(handle) => Some(Tray { handle, online }),
            Err(err) => {
                log::info!("no status notifier tray available: {err}");
                None
            }
        }
    }

    /// Whether a panel shows the icon, so a closed window can be found again.
    pub fn shown(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }

    pub fn set_updates(&self, n: usize) {
        self.handle.update(|t| t.updates = n);
    }
}
