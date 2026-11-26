//! 字数统计（纯函数 + 防抖与选区缓存 + 千分位格式化）。

use eframe::egui;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct StatsOptions {
    pub exclude_code: bool,
    pub exclude_syntax: bool,
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct DetailedStats {
    pub cjk_chars: usize,
    pub en_words: usize,
    pub words: usize,
    pub chars_with_spaces: usize,
    pub chars_no_spaces: usize,
    pub lines: usize,
    pub paragraphs: usize,
    pub read_minutes: f64,
}

pub fn is_cjk(c: char) -> bool {
    matches!(
        c as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
    )
}

/// 剥离文档开头的 YAML (`---`) 或 TOML (`+++`) Front Matter。
pub fn strip_front_matter(text: &str) -> &str {
    let delim = if text.starts_with("---\n") || text.starts_with("---\r\n") {
        "---"
    } else if text.starts_with("+++\n") || text.starts_with("+++\r\n") {
        "+++"
    } else {
        return text;
    };

    let first_nl = match text.find('\n') {
        Some(i) => i + 1,
        None => return text,
    };
    let mut pos = first_nl;
    for line in text[first_nl..].split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        pos += line.len();
        if trimmed == delim || (delim == "---" && trimmed == "...") {
            return &text[pos..];
        }
    }
    text
}

struct Counter {
    cjk_chars: usize,
    en_words: usize,
    chars_with_spaces: usize,
    chars_no_spaces: usize,
    in_word: bool,
    has_non_ws: bool,
}

impl Counter {
    fn new() -> Self {
        Self {
            cjk_chars: 0,
            en_words: 0,
            chars_with_spaces: 0,
            chars_no_spaces: 0,
            in_word: false,
            has_non_ws: false,
        }
    }

    fn feed(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\r' {
                continue;
            }
            if c != '\n' {
                self.chars_with_spaces += 1;
                if !c.is_whitespace() {
                    self.chars_no_spaces += 1;
                    self.has_non_ws = true;
                }
            }
            if is_cjk(c) {
                self.cjk_chars += 1;
                self.in_word = false;
            } else if c.is_alphanumeric() {
                if !self.in_word {
                    self.en_words += 1;
                    self.in_word = true;
                }
            } else {
                self.in_word = false;
            }
        }
    }

    fn finish_token(&mut self) {
        self.in_word = false;
    }

    fn take_non_ws(&mut self) -> bool {
        let v = self.has_non_ws;
        self.has_non_ws = false;
        v
    }
}

pub fn compute_stats(text: &str, opts: StatsOptions) -> DetailedStats {
    let body = strip_front_matter(text);
    if body.is_empty() {
        return DetailedStats::default();
    }

    let lines = body.split('\n').count();
    let mut counter = Counter::new();
    let mut paragraphs = 0usize;

    if opts.exclude_syntax {
        let mut p_opts = Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS;
        p_opts |= Options::ENABLE_GFM;

        let mut in_code_block = false;
        let mut in_image = false;
        let mut block_depth = 0usize;

        for ev in Parser::new_ext(body, p_opts) {
            match ev {
                Event::Start(tag) => match tag {
                    Tag::CodeBlock(_) => {
                        in_code_block = true;
                        block_depth += 1;
                    }
                    Tag::Image { .. } => in_image = true,
                    Tag::Paragraph | Tag::Heading { .. } | Tag::TableRow => block_depth += 1,
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        block_depth = block_depth.saturating_sub(1);
                        if !opts.exclude_code && counter.take_non_ws() {
                            paragraphs += 1;
                        }
                        counter.finish_token();
                    }
                    TagEnd::Image => in_image = false,
                    TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableRow => {
                        block_depth = block_depth.saturating_sub(1);
                        if counter.take_non_ws() {
                            paragraphs += 1;
                        }
                        counter.finish_token();
                    }
                    TagEnd::Item => {
                        if block_depth == 0 && counter.take_non_ws() {
                            paragraphs += 1;
                        }
                        counter.finish_token();
                    }
                    _ => {}
                },
                Event::Text(t) => {
                    if in_image || (in_code_block && opts.exclude_code) {
                        continue;
                    }
                    counter.feed(&t);
                }
                Event::Code(c) => {
                    if !opts.exclude_code && !in_image {
                        counter.feed(&c);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    counter.finish_token();
                    if !(in_code_block && opts.exclude_code) {
                        counter.chars_with_spaces += 1;
                    }
                }
                _ => {}
            }
        }
        if counter.take_non_ws() {
            paragraphs += 1;
        }
    } else {
        let mut in_fence: Option<(char, usize)> = None;
        let mut in_para = false;
        for line in body.split('\n') {
            let trimmed = line.trim_start();
            let fence_ch = trimmed.chars().next().filter(|&c| c == '`' || c == '~');
            let fence_len = fence_ch.map_or(0, |fc| trimmed.chars().take_while(|&c| c == fc).count());
            if fence_len >= 3 {
                let fc = fence_ch.unwrap();
                match in_fence {
                    None => {
                        in_fence = Some((fc, fence_len));
                        if !opts.exclude_code {
                            counter.feed(line);
                            in_para = true;
                        }
                        counter.finish_token();
                        continue;
                    }
                    Some((open_c, open_n)) if open_c == fc && fence_len >= open_n && trimmed[fence_len..].trim().is_empty() => {
                        in_fence = None;
                        if !opts.exclude_code {
                            counter.feed(line);
                        }
                        counter.finish_token();
                        continue;
                    }
                    _ => {}
                }
            }

            if in_fence.is_some() && opts.exclude_code {
                continue;
            }

            let line_to_count = if opts.exclude_code {
                strip_inline_code(line)
            } else {
                line.to_string()
            };

            if line_to_count.trim().is_empty() {
                if in_para {
                    paragraphs += 1;
                    in_para = false;
                }
            } else {
                in_para = true;
                counter.feed(&line_to_count);
            }
            counter.finish_token();
        }
        if in_para {
            paragraphs += 1;
        }
    }

    let words = counter.cjk_chars + counter.en_words;
    let read_minutes = (counter.cjk_chars as f64 / 300.0) + (counter.en_words as f64 / 200.0);

    DetailedStats {
        cjk_chars: counter.cjk_chars,
        en_words: counter.en_words,
        words,
        chars_with_spaces: counter.chars_with_spaces,
        chars_no_spaces: counter.chars_no_spaces,
        lines,
        paragraphs,
        read_minutes,
    }
}

fn strip_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for part in line.split('`') {
        if !in_code {
            out.push_str(part);
        }
        in_code = !in_code;
    }
    out
}

pub fn format_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn format_read_time(minutes: f64, words: usize) -> String {
    if words == 0 {
        "0 分钟".into()
    } else if minutes < 1.0 {
        "< 1 分钟".into()
    } else {
        format!("约 {} 分钟", minutes.ceil() as usize)
    }
}

#[derive(Default)]
pub struct StatsCache {
    full_key: Option<(u64, StatsOptions)>,
    pub full: DetailedStats,
    sel_key: Option<(u64, Range<usize>, StatsOptions)>,
    pub sel: Option<DetailedStats>,
}

impl StatsCache {
    pub fn ensure_full(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        version: u64,
        opts: StatsOptions,
        last_edit_at: Option<Instant>,
    ) {
        if self.full_key == Some((version, opts)) {
            return;
        }
        if self.full_key.is_some()
            && let Some(t) = last_edit_at
        {
            let delay = Duration::from_millis(300);
            let elapsed = t.elapsed();
            if elapsed < delay {
                ctx.request_repaint_after(delay - elapsed);
                return;
            }
        }
        self.full = compute_stats(text, opts);
        self.full_key = Some((version, opts));
    }

    pub fn update_selection(&mut self, text: &str, version: u64, byte_range: Option<Range<usize>>, opts: StatsOptions) {
        let Some(range) = byte_range.filter(|r| r.start < r.end && r.end <= text.len()) else {
            self.sel_key = None;
            self.sel = None;
            return;
        };
        if self.sel_key.as_ref().is_some_and(|(v, r, o)| *v == version && *r == range && *o == opts) {
            return;
        }
        let s = compute_stats(&text[range.clone()], opts);
        self.sel = Some(s);
        self.sel_key = Some((version, range, opts));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_yaml_and_toml_front_matter() {
        let yaml = "---\ntitle: 测试\n---\n正文 hello";
        assert_eq!(strip_front_matter(yaml), "正文 hello");
        let toml = "+++\ntitle = 'test'\n+++\nBody";
        assert_eq!(strip_front_matter(toml), "Body");
        assert_eq!(strip_front_matter("普通文本\n---\n分隔"), "普通文本\n---\n分隔");
    }

    #[test]
    fn mixed_cjk_english_code_and_syntax_options() {
        let md = "---\ntitle: demo\n---\n# 标题 Hello\n\n这是 **中文** and `inline` words。\n\n```rust\nfn main() {}\n```\n";
        let with_code = compute_stats(md, StatsOptions { exclude_code: false, exclude_syntax: true });
        assert_eq!(with_code.cjk_chars, 6); // 标 题 这 是 中 文
        assert_eq!(with_code.en_words, 6); // Hello and inline words fn main
        assert_eq!(with_code.words, 12);
        assert_eq!(with_code.paragraphs, 3);

        let no_code = compute_stats(md, StatsOptions { exclude_code: true, exclude_syntax: true });
        assert_eq!(no_code.cjk_chars, 6);
        assert_eq!(no_code.en_words, 3); // Hello and words
        assert_eq!(no_code.words, 9);
        assert_eq!(no_code.paragraphs, 2);

        let raw_syntax = compute_stats(md, StatsOptions { exclude_code: true, exclude_syntax: false });
        assert!(raw_syntax.chars_no_spaces > no_code.chars_no_spaces);
    }

    #[test]
    fn reading_time_and_thousands_formatting() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1234), "1,234");
        assert_eq!(format_thousands(1234567), "1,234,567");

        let cn_300: String = std::iter::repeat_n('字', 300).collect();
        let st = compute_stats(&cn_300, StatsOptions::default());
        assert!((st.read_minutes - 1.0).abs() < 1e-6);
        assert_eq!(format_read_time(st.read_minutes, st.words), "约 1 分钟");

        let en_400: String = std::iter::repeat_n("word", 400).collect::<Vec<_>>().join(" ");
        let st2 = compute_stats(&en_400, StatsOptions::default());
        assert!((st2.read_minutes - 2.0).abs() < 1e-6);
        assert_eq!(format_read_time(st2.read_minutes, st2.words), "约 2 分钟");
    }
}
