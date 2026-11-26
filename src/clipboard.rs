//! 系统剪贴板读写与图片粘贴落盘（基于 arboard）。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 从 Windows CF_HTML 数据中截取 `StartFragment` 到 `EndFragment` 之间的 HTML 片段。
///
/// 若输入没有 CF_HTML 头或偏移无效，则依次回退到 `<!--StartFragment-->` 标记和原始字符串。
pub fn extract_cf_html_fragment(raw: &str) -> &str {
    let head = raw.get(..raw.len().min(512)).unwrap_or(raw);
    if head.contains("StartFragment:") {
        let start = parse_cf_offset(head, "StartFragment:");
        let end = parse_cf_offset(head, "EndFragment:");
        if let (Some(s), Some(e)) = (start, end)
            && s <= e
            && e <= raw.len()
            && raw.is_char_boundary(s)
            && raw.is_char_boundary(e)
        {
            return &raw[s..e];
        }
    }

    const START_MARK: &str = "<!--StartFragment-->";
    const END_MARK: &str = "<!--EndFragment-->";
    if let Some(s) = raw.find(START_MARK) {
        let frag_start = s + START_MARK.len();
        if let Some(rel_e) = raw[frag_start..].find(END_MARK) {
            return &raw[frag_start..frag_start + rel_e];
        }
    }

    raw
}

fn parse_cf_offset(head: &str, key: &str) -> Option<usize> {
    let pos = head.find(key)? + key.len();
    let digits: String = head[pos..].chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

pub fn get_html() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let raw = cb.get().html().ok()?;
    let frag = extract_cf_html_fragment(&raw).trim();
    if frag.is_empty() {
        None
    } else {
        Some(frag.to_string())
    }
}

pub fn get_text() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let text = cb.get().text().ok()?;
    if text.is_empty() { None } else { Some(text) }
}

pub fn set_text(text: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败：{e}"))?;
    cb.set_text(text.to_string()).map_err(|e| format!("写入剪贴板失败：{e}"))
}

pub fn set_html(html: &str, alt_text: &str) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败：{e}"))?;
    cb.set()
        .html(html, Some(alt_text))
        .map_err(|e| format!("写入 HTML 剪贴板失败：{e}"))
}

/// 将 RGBA 像素缓冲区保存为 PNG 文件，返回 `(完整路径, 用于 Markdown 的图片链接)`。
pub fn save_rgba_image(
    width: usize,
    height: usize,
    rgba: &[u8],
    base_dir: Option<&Path>,
) -> Result<(PathBuf, String), String> {
    let assets_dir = match base_dir {
        Some(dir) => dir.join("assets"),
        None => std::env::temp_dir().join("mdpad_assets"),
    };
    std::fs::create_dir_all(&assets_dir).map_err(|e| format!("创建图片目录失败：{e}"))?;

    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut idx = 0u32;
    let (file_name, full_path) = loop {
        let name = if idx == 0 {
            format!("paste_{ms}.png")
        } else {
            format!("paste_{ms}_{idx}.png")
        };
        let p = assets_dir.join(&name);
        if !p.exists() {
            break (name, p);
        }
        idx += 1;
    };

    let w = u32::try_from(width).map_err(|_| "图片尺寸超出范围".to_string())?;
    let h = u32::try_from(height).map_err(|_| "图片尺寸超出范围".to_string())?;
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| "剪贴板图像字节数与尺寸不匹配".to_string())?;
    img.save(&full_path).map_err(|e| format!("保存粘贴图片失败：{e}"))?;

    let md_url = if base_dir.is_some() {
        format!("assets/{file_name}")
    } else {
        full_path.to_string_lossy().replace('\\', "/")
    };
    Ok((full_path, md_url))
}

pub fn get_and_save_image(base_dir: Option<&Path>) -> Option<(PathBuf, String)> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let img = cb.get().image().ok()?;
    save_rgba_image(img.width, img.height, &img.bytes, base_dir).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_cf_html_fragment_header_markers_and_bare() {
        let prefix = "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n<html><body><!--StartFragment-->";
        let frag = "<p>你好 <b>世界</b></p>";
        let suffix = "<!--EndFragment--></body></html>";
        let start = prefix.len();
        let end = start + frag.len();
        let real = format!(
            "Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{start:010}\r\nEndFragment:{end:010}\r\n<html><body><!--StartFragment-->{frag}{suffix}",
            0,
            start + frag.len() + suffix.len(),
        );
        assert_eq!(extract_cf_html_fragment(&real), frag);

        // 回退到注释标记
        let markers_only = "<html><body><!--StartFragment--><span>片段</span><!--EndFragment--></body></html>";
        assert_eq!(extract_cf_html_fragment(markers_only), "<span>片段</span>");

        // 裸 HTML 直接原样返回
        let bare = "<p>plain</p>";
        assert_eq!(extract_cf_html_fragment(bare), bare);
    }

    #[test]
    fn saves_rgba_image_to_assets_and_returns_relative_url() {
        let tmp = std::env::temp_dir().join(format!("mdpad_clip_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let rgba = [255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let (path, url) = save_rgba_image(2, 2, &rgba, Some(&tmp)).unwrap();
        assert!(url.starts_with("assets/paste_") && url.ends_with(".png"));
        assert!(path.is_file());
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
