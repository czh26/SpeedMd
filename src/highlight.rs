//! 编辑器里的轻量 Markdown 语法高亮（逐行扫描，O(n)，按文档版本号缓存 + 查找匹配高亮叠加）。

use crate::theme::{EditorPalette, ThemeId};
use eframe::egui::{
    text::{ByteIndex, LayoutJob, LayoutSection},
    Color32, FontId, Stroke, TextFormat,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;

const MAX_FIND_HIGHLIGHTS: usize = 10_000;

pub struct Highlighter {
    style_key: u64,
    has_cached: bool,
    text_gen: u64,
    job: LayoutJob,
    overlay_cache_key: u64,
    overlay_job: LayoutJob,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self {
            style_key: u64::MAX,
            has_cached: false,
            text_gen: 0,
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
    /// 返回高亮后的 LayoutJob；文本内容、主题与字体未变化时直接复用缓存，并叠加查找匹配背景色。
    pub fn highlight(
        &mut self,
        _version: u64,
        text: &str,
        theme: ThemeId,
        font: &FontId,
        overlay: Option<FindOverlay<'_>>,
    ) -> LayoutJob {
        let mut h = DefaultHasher::new();
        theme.hash(&mut h);
        font.size.to_bits().hash(&mut h);
        font.family.hash(&mut h);
        let style_key = h.finish();
        let text_changed = !self.has_cached || self.job.text != text;
        if text_changed || style_key != self.style_key {
            self.job = build(text, theme, font);
            self.style_key = style_key;
            self.has_cached = true;
            if text_changed {
                self.text_gen = self.text_gen.wrapping_add(1);
            }
            self.overlay_cache_key = u64::MAX;
        }
        let Some(ov) = overlay.filter(|o| !o.matches.is_empty()) else {
            return self.job.clone();
        };
        let mut oh = DefaultHasher::new();
        (self.style_key, self.text_gen, ov.key, ov.current).hash(&mut oh);
        let ok = oh.finish();
        if ok != self.overlay_cache_key {
            self.overlay_job = apply_find_overlay(&self.job, ov.matches, ov.current, theme);
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
    theme: ThemeId,
) -> LayoutJob {
    if matches.is_empty() || base.sections.is_empty() {
        return base.clone();
    }
    let limit = matches.len().min(MAX_FIND_HIGHLIGHTS);
    let capped = &matches[..limit];

    let text = base.text.as_str();
    let mut prev_end = 0usize;
    for m in capped {
        if m.start < prev_end
            || m.start >= m.end
            || m.end > text.len()
            || !text.is_char_boundary(m.start)
            || !text.is_char_boundary(m.end)
        {
            return base.clone();
        }
        prev_end = m.end;
    }

    let ed = &theme.spec().editor;
    let match_bg = ed.find_match_bg;
    let cur_bg = ed.find_cur_bg;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum FrontKind {
    None,
    Yaml,
    Toml,
}

fn build(text: &str, theme: ThemeId, font: &FontId) -> LayoutJob {
    let p = &theme.spec().editor;
    let mut job = LayoutJob::default();
    let code_fmt = TextFormat { background: p.code_bg, ..fmt(font, p.code) };
    let mut in_fence = false;
    // Front Matter：文档开头的 YAML (---) 或 TOML (+++)
    let mut front = if text.starts_with("---\n") || text.starts_with("---\r\n") {
        FrontKind::Yaml
    } else if text.starts_with("+++\n") || text.starts_with("+++\r\n") {
        FrontKind::Toml
    } else {
        FrontKind::None
    };
    let mut first = true;

    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim_start();

        match front {
            FrontKind::Yaml => {
                let is_end = !first && (content == "---" || content == "...");
                if !first && !is_end && trimmed.starts_with('#') {
                    push(&mut job, line, fmt(font, p.quote));
                } else if !first
                    && !is_end
                    && !trimmed.starts_with('-')
                    && let Some(colon) = content.find(':')
                    && !content[..colon].contains("://")
                {
                    push(&mut job, &line[..colon], fmt(font, p.link));
                    push(&mut job, &line[colon..], fmt(font, p.quote));
                } else {
                    push(&mut job, line, fmt(font, p.rule));
                }
                first = false;
                if is_end {
                    front = FrontKind::None;
                }
                continue;
            }
            FrontKind::Toml => {
                let is_end = !first && content == "+++";
                if !first && !is_end && trimmed.starts_with('#') {
                    push(&mut job, line, fmt(font, p.quote));
                } else if !first && !is_end && trimmed.starts_with('[') && trimmed.ends_with(']') {
                    push(&mut job, line, fmt(font, p.heading));
                } else if !first
                    && !is_end
                    && let Some(eq) = content.find('=')
                {
                    push(&mut job, &line[..eq], fmt(font, p.link));
                    push(&mut job, &line[eq..], fmt(font, p.quote));
                } else {
                    push(&mut job, line, fmt(font, p.rule));
                }
                first = false;
                if is_end {
                    front = FrontKind::None;
                }
                continue;
            }
            FrontKind::None => {}
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
            inline(&mut job, &line[cut..], p, font, &code_fmt);
            continue;
        }

        let indent = content.len() - trimmed.len();
        let marker = list_marker_len(trimmed);
        if marker > 0 {
            push(&mut job, &line[..indent], fmt(font, p.normal));
            push(&mut job, &line[indent..indent + marker], fmt(font, p.marker));
            inline(&mut job, &line[indent + marker..], p, font, &code_fmt);
        } else {
            inline(&mut job, line, p, font, &code_fmt);
        }
    }
    job
}

/// 生成说明书源码框的高亮 LayoutJob：与编辑器同款配色，并将行尾空格渲染为淡色 `·` 符号。
pub fn highlight_manual_source(code: &str, theme: ThemeId, font: &FontId) -> LayoutJob {
    let base = build(code, theme, font);
    if base.sections.is_empty() {
        return base;
    }

    let mut trailing_ranges: Vec<Range<usize>> = Vec::new();
    let mut line_start = 0usize;
    for line in code.split_inclusive('\n') {
        let no_eol = line.trim_end_matches(['\n', '\r']);
        let trimmed = no_eol.trim_end_matches(' ');
        if trimmed.len() < no_eol.len() {
            trailing_ranges.push((line_start + trimmed.len())..(line_start + no_eol.len()));
        }
        line_start += line.len();
    }

    if trailing_ranges.is_empty() {
        return base;
    }

    let ws_color = theme.spec().editor.trailing_ws;

    let mut out = LayoutJob::default();
    let mut ti = 0usize;

    for sec in &base.sections {
        let s_start = sec.byte_range.start.0;
        let s_end = sec.byte_range.end.0;
        let mut pos = s_start;
        let mut first_piece = true;

        while ti < trailing_ranges.len() && trailing_ranges[ti].end <= pos {
            ti += 1;
        }

        let mut k = ti;
        while k < trailing_ranges.len() && trailing_ranges[k].start < s_end {
            let tr = &trailing_ranges[k];
            if tr.start > pos {
                out.append(
                    &code[pos..tr.start],
                    if first_piece { sec.leading_space } else { 0.0 },
                    sec.format.clone(),
                );
                first_piece = false;
                pos = tr.start;
            }
            let ov_end = tr.end.min(s_end);
            if ov_end > pos {
                let dots: String = std::iter::repeat_n('\u{00B7}', ov_end - pos).collect();
                let mut wf = sec.format.clone();
                wf.color = ws_color;
                out.append(&dots, if first_piece { sec.leading_space } else { 0.0 }, wf);
                first_piece = false;
                pos = ov_end;
            }
            if tr.end <= s_end {
                k += 1;
                ti = k;
            } else {
                break;
            }
        }

        if pos < s_end {
            out.append(
                &code[pos..s_end],
                if first_piece { sec.leading_space } else { 0.0 },
                sec.format.clone(),
            );
        }
    }

    out
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

fn is_escaped_at(b: &[u8], i: usize) -> bool {
    let mut cnt = 0usize;
    let mut k = i;
    while k > 0 && b[k - 1] == b'\\' {
        cnt += 1;
        k -= 1;
    }
    cnt % 2 == 1
}

/// 行内元素：`code`、***粗斜体*** / ___粗斜体___、**粗体** / __粗体__、*斜体* / _斜体_、[链接](url) / ![图片](url)。
/// 只在 ASCII 字节处切片，因此对中文等多字节字符是安全的。
fn inline(job: &mut LayoutJob, s: &str, p: &EditorPalette, font: &FontId, code_fmt: &TextFormat) {
    let b = s.as_bytes();
    let normal = fmt(font, p.normal);
    let (mut i, mut start) = (0usize, 0usize);

    while i < b.len() {
        if is_escaped_at(b, i) {
            i += 1;
            continue;
        }
        let mut span: Option<(usize, TextFormat)> = None;
        match b[i] {
            b'`' => {
                if let Some(end) = s[i + 1..].find('`') {
                    span = Some((i + end + 2, code_fmt.clone()));
                }
            }
            b'*' | b'_' => {
                // 1. 三个 * / _ 粗斜体（_ 要求词边界，避免误判）
                if b.get(i + 1) == Some(&b[i])
                    && b.get(i + 2) == Some(&b[i])
                    && b.get(i + 3).is_some_and(|c| *c != b[i] && !c.is_ascii_whitespace())
                    && (b[i] == b'*' || i == 0 || !b[i - 1].is_ascii_alphanumeric())
                    && let Some(end) = s[i + 3..].find(&s[i..i + 3])
                {
                    let close = i + 3 + end;
                    if end > 0
                        && !b[close - 1].is_ascii_whitespace()
                        && b.get(close + 3) != Some(&b[i])
                        && (b[i] == b'*' || !b.get(close + 3).is_some_and(u8::is_ascii_alphanumeric))
                    {
                        span = Some((close + 3, TextFormat { italics: true, ..fmt(font, p.bold) }));
                    }
                }
                // 2. 两个 * / _ 粗体
                if span.is_none() && b.get(i + 1) == Some(&b[i]) {
                    let delim = &s[i..i + 2];
                    if let Some(end) = s[i + 2..].find(delim)
                        && end > 0
                    {
                        span = Some((i + end + 4, fmt(font, p.bold)));
                    }
                }
                // 3. 单个 * / _ 斜体（_ 要求左侧不是字母数字，避免 snake_case 误判）
                if span.is_none()
                    && b.get(i + 1).is_some_and(|c| !c.is_ascii_whitespace())
                    && (b[i] == b'*' || i == 0 || !b[i - 1].is_ascii_alphanumeric())
                {
                    let ch = b[i] as char;
                    if let Some(end) = s[i + 1..].find(ch) {
                        let close = i + 1 + end;
                        if end > 0 && !b[close - 1].is_ascii_whitespace() && b.get(close + 1) != Some(&b[i]) {
                            span = Some((close + 1, TextFormat { italics: true, ..fmt(font, p.normal) }));
                        }
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
            ThemeId::ZenHorizon,
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

    #[test]
    fn manual_source_marks_trailing_spaces_only_and_keeps_invariants() {
        let code = "  缩进保留  \n普通行  \n+++ \ntitle = \"hi\"";
        for id in ThemeId::ALL {
            let job = highlight_manual_source(code, id, &FontId::monospace(14.0));
            assert_eq!(job.text, "  缩进保留··\n普通行··\n+++·\ntitle = \"hi\"");
            let mut cursor = 0usize;
            for sec in &job.sections {
                assert_eq!(sec.byte_range.start.0, cursor);
                assert!(sec.byte_range.start.0 <= sec.byte_range.end.0);
                cursor = sec.byte_range.end.0;
            }
            assert_eq!(cursor, job.text.len());
        }
    }

    #[test]
    fn bold_italic_delimiters_highlight_completely() {
        let code = "这是 ***非常重要*** 的提示，或 ___同样效果___。";
        let job = highlight_manual_source(code, ThemeId::ZenHorizon, &FontId::monospace(14.0));
        let bold_col = ThemeId::ZenHorizon.spec().editor.bold;

        let spans: Vec<(&str, bool, Color32)> = job
            .sections
            .iter()
            .map(|s| (&job.text[s.byte_range.start.0..s.byte_range.end.0], s.format.italics, s.format.color))
            .collect();

        assert!(
            spans.contains(&("***非常重要***", true, bold_col)),
            "missing full *** span: {spans:?}"
        );
        assert!(
            spans.contains(&("___同样效果___", true, bold_col)),
            "missing full ___ span: {spans:?}"
        );
    }

    #[test]
    fn rebuilds_on_mid_frame_text_mutation_and_reuses_after_version_bump() {
        let mut hl = Highlighter::default();
        let font = FontId::monospace(15.0);

        let empty_job = hl.highlight(1, "", ThemeId::ZenHorizon, &font, None);
        assert_eq!(empty_job.text, "");
        assert_eq!(hl.text_gen, 1);

        let before = "ppp\n欢迎通过";
        let j1 = hl.highlight(1, before, ThemeId::ZenHorizon, &font, None);
        assert_eq!(j1.text, before);
        assert_eq!(hl.text_gen, 2);

        // 模拟同一帧内 TextEdit 插入字符后再次调用 layouter，此时 doc.version 尚未自增
        let after = "pppp\n欢迎通过";
        let j2 = hl.highlight(1, after, ThemeId::ZenHorizon, &font, None);
        assert_eq!(j2.text, after);
        assert_eq!(hl.text_gen, 3);

        // 下一帧 doc.version 自增后，文本未再变化，应直接复用缓存不再重建
        let j3 = hl.highlight(2, after, ThemeId::ZenHorizon, &font, None);
        assert_eq!(j3.text, after);
        assert_eq!(hl.text_gen, 3);

        // 同长度字符替换且 version 未变时也必须重建
        let same_len_a = "pppa\n欢迎";
        let _ = hl.highlight(2, same_len_a, ThemeId::ZenHorizon, &font, None);
        assert_eq!(hl.text_gen, 4);
        let same_len_b = "pppb\n欢迎";
        let j4 = hl.highlight(2, same_len_b, ThemeId::ZenHorizon, &font, None);
        assert_eq!(j4.text, same_len_b);
        assert_eq!(hl.text_gen, 5);
    }

    #[test]
    fn find_overlay_ignores_stale_mid_utf8_or_out_of_bounds_ranges() {
        let mut hl = Highlighter::default();
        let text = "pppp\n欢迎通过";
        let base = hl.highlight(1, text, ThemeId::ZenHorizon, &FontId::monospace(15.0), None);

        let cases: [&[Range<usize>]; 4] = [
            std::slice::from_ref(&(6..9)),
            std::slice::from_ref(&(0..999)),
            &[4..8, 6..10],
            std::slice::from_ref(&Range { start: 8, end: 5 }),
        ];
        for invalid in cases {
            let job = apply_find_overlay(&base, invalid, Some(0), ThemeId::ZenHorizon);
            assert_eq!(job.text, text);
            let mut cursor = 0usize;
            for sec in &job.sections {
                assert_eq!(sec.byte_range.start.0, cursor);
                assert!(sec.byte_range.start.0 <= sec.byte_range.end.0);
                assert!(job.text.is_char_boundary(sec.byte_range.start.0));
                assert!(job.text.is_char_boundary(sec.byte_range.end.0));
                cursor = sec.byte_range.end.0;
            }
            assert_eq!(cursor, text.len());
        }
    }
}
