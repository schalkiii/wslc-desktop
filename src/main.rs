//! wslc-desktop — a native GUI for managing Microsoft WSL Containers (`wslc.exe`).
//!
//! Pure-Rust desktop app built on egui/eframe. All `wslc` calls happen on a
//! background worker thread; the UI stays responsive and never flashes a console.
//!
//! Launch flags (written into the login Run key by the settings menu):
//! - `--autostart` — started at login; the containers checked for autostart
//!   are started too, and the app may exit once they are up.
//! - `--silent` — start with the window hidden (tray icon only).

// Hide the console window on Windows release builds (keep it in debug for logs).
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app;
mod autostart;
mod poller;
mod tray;
mod ui;
mod wslc;

use app::WslcDesktopApp;

/// The window/taskbar icon, decoded from raw 256x256 RGBA bytes baked in at
/// build time (see `assets/make_icon.py`). No image-decoding dependency needed.
fn window_icon() -> egui::IconData {
    const RGBA: &[u8] = include_bytes!("../assets/icon_rgba.bin");
    const SIZE: u32 = 256;
    egui::IconData {
        rgba: RGBA.to_vec(),
        width: SIZE,
        height: SIZE,
    }
}

fn main() -> eframe::Result<()> {
    let flags = autostart::parse_launch_flags(std::env::args().skip(1));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("wslc-desktop")
            .with_inner_size([1160.0, 720.0])
            .with_min_inner_size([880.0, 520.0])
            // Silent login start: never show the window at all (no flash);
            // it stays alive as a tray icon and can be shown from there.
            .with_visible(!flags.silent)
            .with_icon(window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "wslc-desktop",
        native_options,
        Box::new(move |cc| Ok(Box::new(WslcDesktopApp::new(cc, flags)))),
    )
}
