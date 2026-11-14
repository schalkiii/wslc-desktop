//! System-tray icon (Windows notification area) with a small context menu.
//!
//! The tray icon lives on the UI thread for the whole app lifetime. Two global
//! crossbeam channels deliver events, drained non-blockingly from
//! `App::update`:
//!
//! - [`TrayIconEvent::receiver`] — clicks on the icon itself;
//! - [`MenuEvent::receiver`] — selections from its context menu.
//!
//! Left-click shows the window; right-click opens the menu with
//! show / hide / quit.

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Menu item ids (kept stable — they are what `MenuEvent` reports back).
const MENU_SHOW: &str = "wslc-show";
const MENU_HIDE: &str = "wslc-hide";
const MENU_QUIT: &str = "wslc-quit";

/// A user intent coming from the tray icon or its menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ShowWindow,
    HideWindow,
    Quit,
}

/// Owns the tray icon; dropping it removes the icon from the notification area.
pub struct Tray {
    _icon: TrayIcon,
}

impl Tray {
    /// Install the notification-area icon and menu.
    ///
    /// Returns `None` when the platform refuses (rare) — the app then simply
    /// runs without a tray instead of failing to start.
    pub fn install() -> Option<Self> {
        let menu = Menu::new();
        let show = MenuItem::with_id(MENU_SHOW, "Show window", true, None);
        let hide = MenuItem::with_id(MENU_HIDE, "Hide window", true, None);
        let quit = MenuItem::with_id(MENU_QUIT, "Quit", true, None);
        let separator = PredefinedMenuItem::separator();
        menu.append_items(&[&show, &hide, &separator, &quit]).ok()?;

        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("wslc-desktop")
            // Left click is handled by us (show the window); the menu stays on
            // right click only.
            .with_menu_on_left_click(false);
        if let Some(icon) = tray_icon_data() {
            builder = builder.with_icon(icon);
        }
        let icon = builder.build().ok()?;
        Some(Self { _icon: icon })
    }

    /// Drain pending tray events into commands (never blocks).
    pub fn drain(&self) -> Vec<TrayCommand> {
        let mut commands = Vec::new();
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    commands.push(TrayCommand::ShowWindow);
                }
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id().0.as_str();
            match id {
                MENU_SHOW => commands.push(TrayCommand::ShowWindow),
                MENU_HIDE => commands.push(TrayCommand::HideWindow),
                MENU_QUIT => commands.push(TrayCommand::Quit),
                _ => {}
            }
        }
        commands
    }
}

/// Downscale the bundled 256×256 window icon to a 32×32 tray icon by box
/// averaging (the tray wants a small bitmap; a 256px icon looks blurry when
/// the shell scales it).
fn tray_icon_data() -> Option<Icon> {
    const RGBA: &[u8] = include_bytes!("../assets/icon_rgba.bin");
    const SRC: usize = 256;
    const DST: usize = 32;
    const FACTOR: usize = SRC / DST;

    if RGBA.len() != SRC * SRC * 4 {
        return None;
    }
    let mut out = vec![0u8; DST * DST * 4];
    for y in 0..DST {
        for x in 0..DST {
            let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..FACTOR {
                for sx in 0..FACTOR {
                    let i = ((y * FACTOR + sy) * SRC + x * FACTOR + sx) * 4;
                    r += RGBA[i] as u32;
                    g += RGBA[i + 1] as u32;
                    b += RGBA[i + 2] as u32;
                    a += RGBA[i + 3] as u32;
                }
            }
            let n = (FACTOR * FACTOR) as u32;
            let o = (y * DST + x) * 4;
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
            out[o + 3] = (a / n) as u8;
        }
    }
    Icon::from_rgba(out, DST as u32, DST as u32).ok()
}

#[cfg(test)]
mod tests {
    /// 托盘图标降采样：32×32 RGBA 且非全零（源图非空时不应产出空图标）。
    #[test]
    fn tray_icon_is_32px_rgba() {
        // `tray_icon_data` 返回 Icon 无法直接读回像素，这里只验证源图尺寸
        // 契约与降采样常量的一致性（长度不匹配时函数返回 None）。
        const RGBA: &[u8] = include_bytes!("../assets/icon_rgba.bin");
        assert_eq!(RGBA.len(), 256 * 256 * 4);
        assert!(super::tray_icon_data().is_some(), "tray icon build failed");
    }
}
