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
    let has_w = std::env::var("MDPAD_WIDTH").is_ok();
    let has_h = std::env::var("MDPAD_HEIGHT").is_ok();
    if let Ok(w) = std::env::var("MDPAD_WIDTH") {
        size[0] = w.parse().unwrap_or(1200.0);
    }
    if let Ok(h) = std::env::var("MDPAD_HEIGHT") {
        size[1] = h.parse().unwrap_or(800.0);
    }
    let is_automated = std::env::var_os("MDPAD_SCREENSHOT").is_some()
        || std::env::var("MDPAD_BENCH").as_deref() == Ok("typing")
        || has_w
        || has_h;
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
        persistence_path: is_automated.then(|| std::env::temp_dir().join("mdpad_test_isolated.ron")),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(MdApp::new(cc, use_system_titlebar)))),
    )
}
