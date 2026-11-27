#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // release 版不弹出控制台窗口

mod app;
mod clipboard;
mod commands;
mod convert;
mod dialog;
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

fn load_app_icon() -> std::sync::Arc<egui::IconData> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("embedded assets/icon.png must be a valid PNG");
    std::sync::Arc::new(icon)
}

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
            .with_icon(load_app_icon())
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

#[cfg(test)]
mod tests {
    use super::load_app_icon;

    #[test]
    fn embedded_png_icon_is_256x256_yellow_bolt_with_exact_symmetry() {
        let icon = load_app_icon();
        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);

        let pixel = |x: usize, y: usize| -> [u8; 4] {
            let off = (y * 256 + x) * 4;
            icon.rgba[off..off + 4].try_into().unwrap()
        };

        assert_eq!(pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(128, 128), [250, 204, 21, 255]);

        for y in 0..256 {
            for x in 0..256 {
                assert_eq!(
                    pixel(x, y),
                    pixel(255 - x, 255 - y),
                    "180-degree rotational symmetry mismatch at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn embedded_ico_contains_all_expected_sizes_and_valid_payloads() {
        let ico = include_bytes!("../assets/icon.ico");
        assert!(ico.len() > 6 + 16 * 7);
        assert_eq!(&ico[0..6], &[0, 0, 1, 0, 7, 0]);

        let expected_sizes = [16usize, 24, 32, 48, 64, 128, 256];
        let mut parsed_sizes = Vec::with_capacity(expected_sizes.len());

        for i in 0..7 {
            let dir = &ico[6 + i * 16..6 + (i + 1) * 16];
            let width = if dir[0] == 0 { 256 } else { dir[0] as usize };
            let height = if dir[1] == 0 { 256 } else { dir[1] as usize };
            assert_eq!(width, height, "entry {i} should be square");
            parsed_sizes.push(width);

            let bytes_in_res = u32::from_le_bytes(dir[8..12].try_into().unwrap()) as usize;
            let image_offset = u32::from_le_bytes(dir[12..16].try_into().unwrap()) as usize;
            let end = image_offset.checked_add(bytes_in_res).expect("entry range overflow");
            assert!(end <= ico.len(), "entry {i} out of bounds");

            let payload = &ico[image_offset..end];
            if width < 256 {
                assert!(payload.len() >= 40, "DIB header too small for {width}x{width}");
                let bi_size = u32::from_le_bytes(payload[0..4].try_into().unwrap());
                let bi_width = i32::from_le_bytes(payload[4..8].try_into().unwrap());
                let bi_height = i32::from_le_bytes(payload[8..12].try_into().unwrap());
                let bi_planes = u16::from_le_bytes(payload[12..14].try_into().unwrap());
                let bi_bit_count = u16::from_le_bytes(payload[14..16].try_into().unwrap());
                assert_eq!(bi_size, 40);
                assert_eq!(bi_width, width as i32);
                assert_eq!(bi_height, (width * 2) as i32);
                assert_eq!(bi_planes, 1);
                assert_eq!(bi_bit_count, 32);
            } else {
                assert!(payload.starts_with(b"\x89PNG\r\n\x1a\n"), "256x256 entry must be PNG");
            }
        }

        assert_eq!(parsed_sizes, expected_sizes);
    }
}
