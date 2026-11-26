//! 加载 Windows 系统字体。
//!
//! - 正文：Segoe UI + 微软雅黑
//! - 粗体（自定义字体族 "bold"）：Segoe UI Bold + 微软雅黑 Bold
//! - 等宽：Consolas + 微软雅黑
//! - 之后是 egui 自带的 emoji / 图标字体，最后是 Segoe UI Symbol 兜底

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontTweak};
use std::path::PathBuf;
use std::sync::Arc;

fn font_dir() -> PathBuf {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    PathBuf::from(windir).join("Fonts")
}

pub fn setup(ctx: &egui::Context) {
    let dir = font_dir();
    let mut fonts = FontDefinitions::default();

    // 加载第一个存在的字体文件，返回注册名
    let mut load = |name: &str, files: &[&str]| -> Option<String> {
        for f in files {
            if let Ok(bytes) = std::fs::read(dir.join(f)) {
                fonts.font_data.insert(name.into(), Arc::new(FontData::from_owned(bytes)));
                return Some(name.to_string());
            }
        }
        None
    };
    let ui_regular = load("segoe", &["segoeui.ttf"]);
    let ui_bold = load("segoe-bold", &["segoeuib.ttf"]);
    let cjk = load("cjk", &["msyh.ttc", "msyh.ttf", "Deng.ttf", "simhei.ttf", "simsun.ttc"]);
    let cjk_bold = load("cjk-bold", &["msyhbd.ttc", "msyhbd.ttf", "Dengb.ttf", "simhei.ttf"]);
    let mono = load("consolas", &["consola.ttf"]);
    let symbol = load("symbol", &["seguisym.ttf"]);
    // 行内代码专用的 Consolas：字形下移，让基线与正文（Segoe UI）对齐
    let code_mono = std::fs::read(dir.join("consola.ttf")).ok().map(|bytes| {
        let tweak = FontTweak { y_offset_factor: 0.25, ..Default::default() };
        fonts.font_data.insert("consolas-inline".into(), Arc::new(FontData::from_owned(bytes).tweak(tweak)));
        "consolas-inline".to_string()
    });

    let defaults_prop = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let defaults_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    // 顺序：指定字体 → Segoe UI Symbol（线条风格 emoji）→ egui 默认字体（含 emoji / 图标）
    let build = |front: &[&Option<String>], rest: &[String]| -> Vec<String> {
        let mut v: Vec<String> = front.iter().filter_map(|o| (*o).clone()).collect();
        if let Some(s) = &symbol {
            v.push(s.clone());
        }
        for r in rest {
            if !v.contains(r) {
                v.push(r.clone());
            }
        }
        v
    };

    let prop = build(&[&ui_regular, &cjk], &defaults_prop);
    let bold = build(&[&ui_bold, &cjk_bold, &ui_regular, &cjk], &defaults_prop);
    let monospace = build(&[&mono], &{
        // 中文放在 egui 默认等宽字体之后、emoji 之前
        let mut v = defaults_mono.clone();
        if let Some(c) = &cjk {
            let pos = 1.min(v.len());
            v.insert(pos, c.clone());
        }
        v
    });

    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    let code = build(&[&code_mono, &cjk], &defaults_mono);
    fonts.families.insert(FontFamily::Monospace, monospace);
    fonts.families.insert(FontFamily::Name("code".into()), code);

    ctx.set_fonts(fonts);
}
