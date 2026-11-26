#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // release 版不弹出控制台窗口

mod app;
mod commands;
mod document;
mod editor;
mod find;
mod fonts;
mod highlight;
mod keymap;
mod markdown;
mod preview;
mod settings;
mod sidebar;
mod stats;
mod tabs;
mod winutil;

use app::{APP_NAME, MdApp};
use eframe::egui;

fn main() -> eframe::Result {
    let mut size = [1200.0, 800.0];
    if let Ok(h) = std::env::var("MDPAD_HEIGHT") {
        size[1] = h.parse().unwrap_or(800.0);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size(size)
            .with_min_inner_size([480.0, 320.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(MdApp::new(cc)))))
}
