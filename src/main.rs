#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // release 版不弹出控制台窗口

mod app;
mod clipboard;
mod commands;
mod convert;
mod document;
mod editor;
mod export;
mod find;
mod fonts;
mod highlight;
mod html_inline;
mod import;
mod jobs;
mod keymap;
mod manual;
mod markdown;
mod media;
mod preview;
mod settings;
mod shortcuts_ui;
mod sidebar;
mod stats;
mod tabs;
mod textproc;
mod theme;
mod window_frame;
mod winutil;

use app::{APP_NAME, MdApp};
use eframe::egui;

fn main() -> eframe::Result {
    let mut size = [1200.0, 800.0];
    let env_w = winutil::app_env("WIDTH");
    let env_h = winutil::app_env("HEIGHT");
    let has_w = env_w.is_some();
    let has_h = env_h.is_some();
    if let Some(w) = env_w {
        size[0] = w.parse().unwrap_or(1200.0);
    }
    if let Some(h) = env_h {
        size[1] = h.parse().unwrap_or(800.0);
    }
    let is_automated = winutil::app_env_os("SCREENSHOT").is_some()
        || winutil::app_env("BENCH").as_deref() == Some("typing")
        || winutil::app_env("ZOOM").is_some()
        || has_w
        || has_h;
    if !is_automated {
        window_frame::migrate_legacy_storage_if_needed(APP_NAME, "MdPad");
    }
    let use_system_titlebar = window_frame::load_startup_use_system_titlebar(APP_NAME);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size(size)
            .with_min_inner_size([480.0, 320.0])
            .with_decorations(use_system_titlebar)
            .with_resizable(true)
            .with_drag_and_drop(true),
        persist_window: !is_automated,
        persistence_path: is_automated.then(|| std::env::temp_dir().join("thundermd_test_isolated.ron")),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(MdApp::new(cc, use_system_titlebar)))),
    )
}
