//! 编辑器里的轻量 Markdown 语法高亮（逐行扫描，O(n)，按文档版本号缓存 + 查找匹配高亮叠加）。

use eframe::egui::{
    text::{ByteIndex, LayoutJob, LayoutSection},
    Color32, FontId, Stroke, TextFormat,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;

const MAX_FIND_HIGHLIGHTS: usize = 10_000;

struct Palette {
    normal: Color32,
    heading: Color32,
    code: Color32,
    code_bg: Color32,
    quote: Color32,
    marker: Color32,
    link: Color32,
    bold: Color32,
    rule: Color32,
    mark_bg: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                normal: Color32::from_rgb(0xd4, 0xd4, 0xd4),
                heading: Color32::from_rgb(0x61, 0xaf, 0xef),
                code: Color32::from_rgb(0xe5, 0xc0, 0x7b),
                code_bg: Color32::from_rgb(0x2a, 0x2d, 0x33),
                quote: Color32::from_rgb(0x8a, 0x93, 0x9e),
                marker: Color32::from_rgb(0xc6, 0x78, 0xdd),
                link: Color32::from_rgb(0x56, 0xb6, 0xc2),
                bold: Color32::from_rgb(0xff, 0xff, 0xff),
                rule: Color32::from_rgb(0x6b, 0x72, 0x80),
                mark_bg: Color32::from_rgb(0x6b, 0x5a, 0x12),
            }
        } else {
            Self {
                normal: Color32::from_rgb(0x24, 0x29, 0x2e),
                heading: Color32::from_rgb(0x00, 0x5c, 0xc5),
                code: Color32::from_rgb(0xb3, 0x1d, 0x28),
                code_bg: Color32::from_rgb(0xf0, 0xf1, 0xf3),
                quote: Color32::from_rgb(0x6a, 0x73, 0x7d),
                marker: Color32::from_rgb(0x8a, 0x3f, 0xc8),
                link: Color32::from_rgb(0x03, 0x66, 0xd6),
                bold: Color32::from_rgb(0x00, 0x00, 0x00),
                rule: Color32::from_rgb(0x95, 0x9d, 0xa5),
                mark_bg: Color32::from_rgb(0xff, 0xef, 0x99),
            }
        }
    }
}

pub struct Highlighter {
    key: u64,
    job: LayoutJob,
    overlay_cache_key: u64,
    overlay_job: LayoutJob,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self {
            key: u64::MAX,
            job: LayoutJob::default(),
            overlay_cache_key: u64::MAX,
            overlay_job: LayoutJob::default(),
        }
    }
}

pub struct FindOverlay<'a> {
    pub matches: &'a [Range<usize>],
    pub current: Option<usize>,
    pub key: u64,
}

impl Highlighter {
    /// 返回高亮后的 LayoutJob；版本号/主题/字号不变时直接复用缓存，并叠加查找匹配背景色。
    pub fn highlight(
        &mut self,
        version: u64,
        text: &str,
        dark: bool,
        font: &FontId,
        overlay: Option<FindOverlay<'_>>,
    ) -> LayoutJob {
        let mut h = DefaultHasher::new();
        version.hash(&mut h);
        dark.hash(&mut h);
        font.size.to_bits().hash(&mut h);
        font.family.hash(&mut h);
        let key = h.finish();
        if key != self.key {
            self.job = build(text, dark, font);
            self.key = key;
            self.overlay_cache_key = u64::MAX;
        }
        let Some(ov) = overlay.filter(|o| !o.matches.is_empty()) else {
            return self.job.clone();
        };
        let mut oh = DefaultHasher::new();
        (key, ov.key, ov.current).hash(&mut oh);
        let ok = oh.finish();
        if ok != self.overlay_cache_key {
            self.overlay_job = apply_find_overlay(&self.job, ov.matches, ov.current, dark);
            self.overlay_cache_key = ok;
        }
        self.overlay_job.clone()
    }
}

/// 以 O(sections + matches) 线性切分 `LayoutSection` 并叠加查找背景色（最多着色前 1 万处匹配）。
pub fn apply_find_overlay(
    base: &LayoutJob,
    matches: &[Range<usize>],
    current_match: Option<usize>,
    dark: bool,
) -> LayoutJob {
    if matches.is_empty() || base.sections.is_empty() {
        return base.clone();
    }
    let limit = matches.len().min(MAX_FIND_HIGHLIGHTS);
    let capped = &matches[..limit];

    let match_bg = if dark {
        Color32::from_rgb(0x5c, 0x4b, 0x16)
    } else {
        Color32::from_rgb(0xff, 0xe8, 0x7a)
    };
    let cur_bg = if dark {
        Color32::from_rgb(0x9a, 0x54, 0x12)
    } else {
        Color32::from_rgb(0xff, 0xa9, 0x40)
    };

    let mut out = Vec::with_capacity(base.sections.len() + capped.len().min(4096) * 2);
    let mut mi = 0usize;

    for sec in &base.sections {
        let s_start = sec.byte_range.start.0;
        let s_end = sec.byte_range.end.0;
        let mut pos = s_start;
        let mut first_piece = true;

        while mi < capped.len() && capped[mi].end <= pos {
            mi += 1;
        }

        let mut k = mi;
        while k < capped.len() && capped[k].start < s_end {
            let m = &capped[k];
            if m.start > pos {
                out.push(LayoutSection {
                    leading_space: if first_piece { sec.leading_space } else { 0.0 },
                    byte_range: ByteIndex(pos)..ByteIndex(m.start),
                    format: sec.format.clone(),
                });
                first_piece = false;
                pos = m.start;
            }
            let overlap_end = m.end.min(s_end);
            if overlap_end > pos {
                let mut fmt = sec.format.clone();
                fmt.background = if current_match == Some(k) { cur_bg } else { match_bg };
                out.push(LayoutSection {
                    leading_space: if first_piece { sec.leading_space } else { 0.0 },
                    byte_range: ByteIndex(pos)..ByteIndex(overlap_end),
                    format: fmt,
                });
                first_piece = false;
                pos = overlap_end;
            }
            if m.end <= s_end {
                k += 1;
                mi = k;
            } else {
                break;
            }
        }

        if pos < s_end {
            out.push(LayoutSection {
                leading_space: if first_piece { sec.leading_space } else { 0.0 },
                byte_range: ByteIndex(pos)..ByteIndex(s_end),
                format: sec.format.clone(),
            });
        }
    }

    let mut job = base.clone();
    job.sections = out;
    job
}

fn fmt(font: &FontId, color: Color32) -> TextFormat {
    TextFormat { font_id: font.clone(), color, ..Default::default() }
}

fn push(job: &mut LayoutJob, s: &str, format: TextFormat) {
    if !s.is_empty() {
        job.append(s, 0.0, format);
    }
}

fn build(text: &str, dark: bool, font: &FontId) -> LayoutJob {
    let p = Palette::new(dark);
    let mut job = LayoutJob::default();
    let code_fmt = TextFormat { background: p.code_bg, ..fmt(font, p.code) };
    let mut in_fence = false;
    // YAML Front Matter：文档开头 --- 到下一个 --- / ...
    let mut in_front = text.starts_with("---\n") || text.starts_with("---\r\n");
    let mut first = true;

    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim_start();

        if in_front {
            let is_end = !first && (content == "---" || content == "...");
            if let (false, false, Some(colon)) = (first, is_end, content.find(':')) {
                push(&mut job, &line[..colon], fmt(font, p.link));
                push(&mut job, &line[colon..], fmt(font, p.quote));
            } else {
                push(&mut job, line, fmt(font, p.rule));
            }
            first = false;
            if is_end {
                in_front = false;
            }
            continue;
        }
        first = false;

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            push(&mut job, line, code_fmt.clone());
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            push(&mut job, line, code_fmt.clone());
            continue;
        }
        if is_heading(trimmed) {
            push(&mut job, line, fmt(font, p.heading));
            continue;
        }
        if trimmed.starts_with('>') {
            push(&mut job, line, TextFormat { italics: true, ..fmt(font, p.quote) });
            continue;
        }
        if is_rule(trimmed) {
            push(&mut job, line, fmt(font, p.rule));
            continue;
        }
        if matches!(trimmed.to_ascii_uppercase().as_str(), "[TOC]" | "[[TOC]]" | "[[_TOC_]]") {
            push(&mut job, line, fmt(font, p.marker));
            continue;
        }
        // 脚注定义  [^label]: ...
        if trimmed.starts_with("[^") && let Some(end) = trimmed.find("]:") {
            let indent = content.len() - trimmed.len();
            let cut = indent + end + 2;
            push(&mut job, &line[..cut], fmt(font, p.link));
            inline(&mut job, &line[cut..], &p, font, &code_fmt);
            continue;
        }

        let indent = content.len() - trimmed.len();
        let marker = list_marker_len(trimmed);
        if marker > 0 {
            push(&mut job, &line[..indent], fmt(font, p.normal));
            push(&mut job, &line[indent..indent + marker], fmt(font, p.marker));
            inline(&mut job, &line[indent + marker..], &p, font, &code_fmt);
        } else {
            inline(&mut job, line, &p, font, &code_fmt);
        }
    }
    job
}

fn is_heading(s: &str) -> bool {
    let n = s.bytes().take_while(|&b| b == b'#').count();
    (1..=6).contains(&n) && matches!(s.as_bytes().get(n), None | Some(b' ') | Some(b'\t'))
}

fn is_rule(s: &str) -> bool {
    let compact: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    compact.len() >= 3
        && matches!(compact[0], b'-' | b'*' | b'_')
        && compact.iter().all(|&b| b == compact[0])
}

fn list_marker_len(s: &str) -> usize {
    let b = s.as_bytes();
    let base = if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        2
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if (1..=9).contains(&digits)
            && matches!(b.get(digits), Some(b'.') | Some(b')'))
            && b.get(digits + 1) == Some(&b' ')
        {
            digits + 2
        } else {
            return 0;
        }
    };
    // 任务列表 "- [ ] " / "- [x] "
    let rest = &b[base..];
    if rest.len() >= 4 && rest[0] == b'[' && matches!(rest[1], b' ' | b'x' | b'X') && rest[2] == b']' && rest[3] == b' ' {
        base + 4
    } else {
        base
    }
}

/// 行内元素：`code`、**粗体** / __粗体__、[链接](url) / ![图片](url)。
/// 只在 ASCII 字节处切片，因此对中文等多字节字符是安全的。
fn inline(job: &mut LayoutJob, s: &str, p: &Palette, font: &FontId, code_fmt: &TextFormat) {
    let b = s.as_bytes();
    let normal = fmt(font, p.normal);
    let (mut i, mut start) = (0usize, 0usize);

    while i < b.len() {
        let mut span: Option<(usize, TextFormat)> = None;
        match b[i] {
            b'`' => {
                if let Some(end) = s[i + 1..].find('`') {
                    span = Some((i + end + 2, code_fmt.clone()));
                }
            }
            b'*' | b'_' if b.get(i + 1) == Some(&b[i]) => {
                let delim = &s[i..i + 2];
                if let Some(end) = s[i + 2..].find(delim)
                    && end > 0
                {
                    span = Some((i + end + 4, fmt(font, p.bold)));
                }
            }
            // 单个 * / _ 斜体（_ 要求左侧不是字母数字，避免 snake_case 误判）
            b'*' | b'_'
                if b.get(i + 1).is_some_and(|c| !c.is_ascii_whitespace())
                    && (b[i] == b'*' || i == 0 || !b[i - 1].is_ascii_alphanumeric()) =>
            {
                let ch = b[i] as char;
                if let Some(end) = s[i + 1..].find(ch) {
                    let close = i + 1 + end;
                    if end > 0 && !b[close - 1].is_ascii_whitespace() && b.get(close + 1) != Some(&b[i]) {
                        span = Some((close + 1, TextFormat { italics: true, ..fmt(font, p.normal) }));
                    }
                }
            }
            b'~' if b.get(i + 1) == Some(&b'~') => {
                if let Some(end) = s[i + 2..].find("~~")
                    && end > 0
                {
                    let f = fmt(font, p.quote);
                    span = Some((i + end + 4, TextFormat { strikethrough: Stroke::new(1.0, p.quote), ..f }));
                }
            }
            b'=' if b.get(i + 1) == Some(&b'=') => {
                if let Some(end) = s[i + 2..].find("==")
                    && end > 0
                {
                    span = Some((i + end + 4, TextFormat { background: p.mark_bg, ..fmt(font, p.normal) }));
                }
            }
            b':' => {
                // :emoji: 短码
                let rest = &b[i + 1..];
                let n = rest.iter().take(40).take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'+' | b'-')).count();
                if n > 0 && rest.get(n) == Some(&b':') && emojis::get_by_shortcode(&s[i + 1..i + 1 + n]).is_some() {
                    span = Some((i + n + 2, fmt(font, p.marker)));
                }
            }
            b'<' => {
                // 常用行内 HTML 标签：<u> <mark> <sup> <sub> <kbd> <br> ...
                if let Some(end) = s[i..].find('>') {
                    let tag = &s[i + 1..i + end];
                    let name = tag.trim_start_matches('/').trim_end_matches('/').trim();
                    if !name.is_empty() && name.len() <= 10 && name.bytes().all(|c| c.is_ascii_alphanumeric()) {
                        span = Some((i + end + 1, fmt(font, p.rule)));
                    }
                }
            }
            b'[' if b.get(i + 1) == Some(&b'^') => {
                if let Some(end) = s[i..].find(']') {
                    span = Some((i + end + 1, fmt(font, p.link)));
                }
            }
            b'[' | b'!' => {
                let j = if b[i] == b'!' { i + 1 } else { i };
                if b.get(j) == Some(&b'[')
                    && let Some(close) = s[j..].find("](")
                {
                    let after = j + close + 2;
                    if let Some(paren) = s[after..].find(')') {
                        span = Some((after + paren + 1, fmt(font, p.link)));
                    }
                }
            }
            _ => {}
        }
        if let Some((end, f)) = span {
            push(job, &s[start..i], normal.clone());
            push(job, &s[i..end], f);
            i = end;
            start = end;
        } else {
            i += 1;
        }
    }
    push(job, &s[start..], normal);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_overlay_keeps_section_invariants() {
        let mut hl = Highlighter::default();
        let text = "# Title\nHello **world** hello\n";
        let matches = vec![8..13, 24..29];
        let job = hl.highlight(
            1,
            text,
            false,
            &FontId::monospace(15.0),
            Some(FindOverlay { matches: &matches, current: Some(1), key: 42 }),
        );
        assert_eq!(job.text, text);
        let mut cursor = 0usize;
        for sec in &job.sections {
            assert_eq!(sec.byte_range.start.0, cursor);
            assert!(sec.byte_range.start.0 <= sec.byte_range.end.0);
            cursor = sec.byte_range.end.0;
        }
        assert_eq!(cursor, text.len());
        let colored: Vec<_> = job
            .sections
            .iter()
            .filter(|s| s.format.background != Color32::TRANSPARENT)
            .map(|s| s.byte_range.start.0..s.byte_range.end.0)
            .collect();
        assert_eq!(colored, vec![8..13, 24..29]);
    }
}
