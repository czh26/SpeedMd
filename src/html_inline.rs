//! 行内 HTML 标签/属性解析、CSS 颜色与行内样式解析、媒体链接识别。

use eframe::egui::{self, Color32};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    Video,
    Audio,
}

pub fn detect_media_kind(url: &str) -> Option<MediaKind> {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let ext = clean.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mov" | "webm" | "mkv" => Some(MediaKind::Video),
        "mp3" | "m4a" | "wav" | "flac" | "ogg" => Some(MediaKind::Audio),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlTag {
    pub name: String,
    pub closing: bool,
    pub self_closing: bool,
    pub attrs: Vec<(String, String)>,
}

impl HtmlTag {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

pub fn decode_basic_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", "\u{00a0}")
}

/// 解析单个 HTML 标签（忽略注释与非标签片段）。
pub fn parse_html_tag(raw: &str) -> Option<HtmlTag> {
    let s = raw.trim();
    if !s.starts_with('<') || !s.ends_with('>') || s.starts_with("<!--") || s.starts_with("<!") {
        return None;
    }
    let mut inner = s[1..s.len() - 1].trim();
    let closing = if let Some(rest) = inner.strip_prefix('/') {
        inner = rest.trim_start();
        true
    } else {
        false
    };
    let self_closing = if !closing && inner.ends_with('/') {
        inner = inner[..inner.len() - 1].trim_end();
        true
    } else {
        false
    };

    let bytes = inner.as_bytes();
    let name_len = bytes
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
        .count();
    if name_len == 0 {
        return None;
    }
    let name = inner[..name_len].to_ascii_lowercase();
    let mut rest = inner[name_len..].trim_start();
    let mut attrs = Vec::new();

    if !closing {
        while !rest.is_empty() {
            let key_len = rest
                .as_bytes()
                .iter()
                .take_while(|b| !b.is_ascii_whitespace() && **b != b'=')
                .count();
            if key_len == 0 {
                break;
            }
            let key = rest[..key_len].to_ascii_lowercase();
            rest = rest[key_len..].trim_start();
            let val = if let Some(after_eq) = rest.strip_prefix('=') {
                let after_eq = after_eq.trim_start();
                if let Some(q) = after_eq.chars().next().filter(|&c| c == '"' || c == '\'') {
                    let body = &after_eq[1..];
                    if let Some(end) = body.find(q) {
                        rest = body[end + 1..].trim_start();
                        decode_basic_entities(&body[..end])
                    } else {
                        rest = "";
                        decode_basic_entities(body)
                    }
                } else {
                    let v_len = after_eq.as_bytes().iter().take_while(|b| !b.is_ascii_whitespace()).count();
                    let v = &after_eq[..v_len];
                    rest = after_eq[v_len..].trim_start();
                    decode_basic_entities(v)
                }
            } else {
                String::new()
            };
            if !key.starts_with("on") {
                attrs.push((key, val));
            }
        }
    }

    Some(HtmlTag { name, closing, self_closing, attrs })
}

pub fn parse_css_color(raw: &str) -> Option<Color32> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        let h = |idx| u8::from_str_radix(&hex[idx..idx + 2], 16).ok();
        let nibble = |idx| u8::from_str_radix(&hex[idx..idx + 1], 16).ok().map(|v| v * 17);
        return match hex.len() {
            3 => Some(Color32::from_rgb(nibble(0)?, nibble(1)?, nibble(2)?)),
            4 => Some(Color32::from_rgba_unmultiplied(nibble(0)?, nibble(1)?, nibble(2)?, nibble(3)?)),
            6 => Some(Color32::from_rgb(h(0)?, h(2)?, h(4)?)),
            8 => Some(Color32::from_rgba_unmultiplied(h(0)?, h(2)?, h(4)?, h(6)?)),
            _ => None,
        };
    }

    if let Some(args) = s
        .strip_prefix("rgb(")
        .and_then(|r| r.strip_suffix(')'))
        .or_else(|| s.strip_prefix("rgba(").and_then(|r| r.strip_suffix(')')))
    {
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        if parts.len() == 3 || parts.len() == 4 {
            let parse_chan = |p: &str| -> Option<u8> {
                if let Some(pct) = p.strip_suffix('%') {
                    let f: f32 = pct.trim().parse().ok()?;
                    Some((f.clamp(0.0, 100.0) * 2.55).round() as u8)
                } else {
                    let f: f32 = p.parse().ok()?;
                    Some(f.clamp(0.0, 255.0).round() as u8)
                }
            };
            let (r, g, b) = (parse_chan(parts[0])?, parse_chan(parts[1])?, parse_chan(parts[2])?);
            let a = if parts.len() == 4 {
                let fa: f32 = parts[3].parse().ok()?;
                if fa <= 1.0 {
                    (fa.clamp(0.0, 1.0) * 255.0).round() as u8
                } else {
                    fa.clamp(0.0, 255.0).round() as u8
                }
            } else {
                255
            };
            return Some(Color32::from_rgba_unmultiplied(r, g, b, a));
        }
        return None;
    }

    let c = Color32::from_rgb;
    Some(match s.as_str() {
        "black" => c(0, 0, 0),
        "white" => c(255, 255, 255),
        "gray" | "grey" => c(128, 128, 128),
        "silver" => c(192, 192, 192),
        "lightgray" | "lightgrey" => c(211, 211, 211),
        "darkgray" | "darkgrey" => c(169, 169, 169),
        "red" => c(220, 38, 38),
        "maroon" => c(128, 0, 0),
        "crimson" => c(220, 20, 60),
        "tomato" => c(255, 99, 71),
        "coral" => c(255, 127, 80),
        "orange" => c(234, 88, 12),
        "gold" => c(234, 179, 8),
        "yellow" => c(250, 204, 21),
        "olive" => c(128, 128, 0),
        "lime" => c(0, 255, 0),
        "green" => c(22, 163, 74),
        "forestgreen" => c(34, 139, 34),
        "seagreen" => c(46, 139, 87),
        "teal" => c(13, 148, 136),
        "cyan" | "aqua" => c(6, 182, 212),
        "skyblue" => c(135, 206, 235),
        "dodgerblue" => c(30, 144, 255),
        "steelblue" => c(70, 130, 180),
        "blue" => c(37, 99, 235),
        "royalblue" => c(65, 105, 225),
        "navy" => c(0, 0, 128),
        "indigo" => c(75, 0, 130),
        "purple" => c(147, 51, 234),
        "slateblue" => c(106, 90, 205),
        "violet" => c(238, 130, 238),
        "magenta" | "fuchsia" => c(217, 70, 239),
        "pink" => c(236, 72, 153),
        "brown" => c(165, 42, 42),
        "tan" => c(210, 180, 140),
        _ => return None,
    })
}

#[derive(Clone, Debug, Default)]
pub struct CssInlineStyle {
    pub color: Option<Color32>,
    pub bg: Option<Color32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub size_scale: Option<f32>,
    pub align: Option<egui::Align>,
}

pub fn parse_inline_style(style_attr: &str) -> CssInlineStyle {
    let mut out = CssInlineStyle::default();
    for decl in style_attr.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();
        match key.as_str() {
            "color" => out.color = parse_css_color(val),
            "background-color" | "background" => {
                if let Some(c) = parse_css_color(val) {
                    out.bg = Some(c);
                }
            }
            "font-weight" => {
                let lv = val.to_ascii_lowercase();
                if lv == "bold" || lv == "bolder" || lv.parse::<u16>().is_ok_and(|w| w >= 600) {
                    out.bold = Some(true);
                } else if lv == "normal" {
                    out.bold = Some(false);
                }
            }
            "font-style" => {
                let lv = val.to_ascii_lowercase();
                if lv == "italic" || lv == "oblique" {
                    out.italic = Some(true);
                } else if lv == "normal" {
                    out.italic = Some(false);
                }
            }
            "text-decoration" | "text-decoration-line" => {
                let lv = val.to_ascii_lowercase();
                if lv.contains("underline") {
                    out.underline = Some(true);
                }
                if lv.contains("line-through") {
                    out.strike = Some(true);
                }
            }
            "font-size" => {
                let lv = val.to_ascii_lowercase();
                if let Some(px) = lv.strip_suffix("px").and_then(|n| n.trim().parse::<f32>().ok()) {
                    out.size_scale = Some((px / 16.0).clamp(0.6, 2.5));
                } else if let Some(em) = lv.strip_suffix("em").and_then(|n| n.trim().parse::<f32>().ok()) {
                    out.size_scale = Some(em.clamp(0.6, 2.5));
                } else if let Some(pct) = lv.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()) {
                    out.size_scale = Some((pct / 100.0).clamp(0.6, 2.5));
                }
            }
            "text-align" => match val.to_ascii_lowercase().as_str() {
                "center" => out.align = Some(egui::Align::Center),
                "right" | "end" => out.align = Some(egui::Align::Max),
                "left" | "start" => out.align = Some(egui::Align::Min),
                _ => {}
            },
            _ => {}
        }
    }
    out
}

pub fn parse_dim(val: &str) -> Option<f32> {
    let s = val.trim().to_ascii_lowercase();
    let num = s.strip_suffix("px").unwrap_or(&s).trim();
    num.parse::<f32>().ok().filter(|v| *v > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags_and_strip_event_handlers() {
        let t = parse_html_tag(r#"<span style="color: #f0a; background-color: rgb(10, 20, 30)" onclick="evil()">"#).unwrap();
        assert_eq!(t.name, "span");
        assert!(!t.closing);
        assert!(t.attr("onclick").is_none());
        let st = parse_inline_style(t.attr("style").unwrap());
        assert_eq!(st.color, Some(Color32::from_rgb(0xff, 0x00, 0xaa)));
        assert_eq!(st.bg, Some(Color32::from_rgb(10, 20, 30)));

        let img = parse_html_tag(r#"<img src='a.png' width=240 height="120px" alt="pic &amp; 1"/>"#).unwrap();
        assert!(img.self_closing);
        assert_eq!(img.attr("src"), Some("a.png"));
        assert_eq!(parse_dim(img.attr("width").unwrap()), Some(240.0));
        assert_eq!(parse_dim(img.attr("height").unwrap()), Some(120.0));
        assert_eq!(img.attr("alt"), Some("pic & 1"));
    }

    #[test]
    fn css_color_formats() {
        assert_eq!(parse_css_color("#abc"), Some(Color32::from_rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(parse_css_color("#123456"), Some(Color32::from_rgb(0x12, 0x34, 0x56)));
        assert_eq!(parse_css_color("rgb(12, 34, 56)"), Some(Color32::from_rgb(12, 34, 56)));
        assert_eq!(parse_css_color("dodgerblue"), Some(Color32::from_rgb(30, 144, 255)));
        assert_eq!(parse_css_color("notacolor"), None);
    }

    #[test]
    fn media_detection() {
        assert_eq!(detect_media_kind("clip.MP4?t=10"), Some(MediaKind::Video));
        assert_eq!(detect_media_kind("voice.flac#0"), Some(MediaKind::Audio));
        assert_eq!(detect_media_kind("pic.png"), None);
    }
}
