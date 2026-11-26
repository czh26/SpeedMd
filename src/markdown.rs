//! Markdown 解析：pulldown-cmark 事件流 → 预先排好版的文档结构（Doc）。
//!
//! 只在文本 / 主题 / 设置变化时重新解析，渲染时直接复用这里生成的 LayoutJob。

use crate::html_inline::{
    detect_media_kind, parse_css_color, parse_dim, parse_html_tag, parse_inline_style, MediaKind,
};
use crate::media::MediaCard;
use eframe::egui::{text::LayoutJob, Align, Color32, FontFamily, FontId, Stroke, TextFormat};
use htmd::node::{Node, NodeType};
use pulldown_cmark::{Alignment, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

pub fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

// ---------------------------------------------------------------- 主题

#[derive(Clone)]
pub struct Theme {
    pub dark: bool,
    pub size: f32,
    pub line_height_factor: f32,
    pub para_spacing: f32,
    pub text: Color32,
    pub weak: Color32,
    pub heading: Color32,
    pub link: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub mark_bg: Color32,
    pub mark_fg: Color32,
    pub quote: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub panel_bg: Color32,
}

impl Theme {
    #[allow(dead_code)]
    pub fn new(dark: bool, size: f32) -> Self {
        Self::with_spacing(dark, size, 1.6, 0.75)
    }

    pub fn with_spacing(dark: bool, size: f32, line_height_factor: f32, para_spacing: f32) -> Self {
        let c = Color32::from_rgb;
        if dark {
            Self {
                dark,
                size,
                line_height_factor,
                para_spacing,
                text: c(0xd8, 0xdb, 0xdf),
                weak: c(0x8b, 0x94, 0x9e),
                heading: c(0xf0, 0xf2, 0xf4),
                link: c(0x58, 0xa6, 0xff),
                code: c(0xff, 0x9e, 0x80),
                code_bg: c(0x2b, 0x2f, 0x36),
                mark_bg: c(0x8a, 0x72, 0x10),
                mark_fg: c(0xff, 0xff, 0xff),
                quote: c(0x9a, 0xa2, 0xab),
                border: c(0x44, 0x4c, 0x56),
                accent: c(0x38, 0x8b, 0xfd),
                panel_bg: c(0x22, 0x26, 0x2c),
            }
        } else {
            Self {
                dark,
                size,
                line_height_factor,
                para_spacing,
                text: c(0x24, 0x29, 0x2f),
                weak: c(0x65, 0x6d, 0x76),
                heading: c(0x11, 0x14, 0x18),
                link: c(0x09, 0x69, 0xda),
                code: c(0xc7, 0x25, 0x4e),
                code_bg: c(0xf0, 0xf2, 0xf4),
                mark_bg: c(0xff, 0xec, 0x80),
                mark_fg: c(0x1f, 0x23, 0x28),
                quote: c(0x65, 0x6d, 0x76),
                border: c(0xd0, 0xd7, 0xde),
                accent: c(0x09, 0x69, 0xda),
                panel_bg: c(0xf6, 0xf8, 0xfa),
            }
        }
    }

    pub fn line_height(&self) -> f32 {
        self.size * self.line_height_factor
    }
}

// ---------------------------------------------------------------- 文档结构

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Url(String),
    Anchor(String),
}

/// 一段排好版的富文本，附带链接所在的字符区间（用于点击检测）
pub struct Rich {
    pub job: Arc<LayoutJob>,
    pub links: Vec<(Range<usize>, Target)>,
}

pub enum Chunk {
    Text(Rich),
    Image { url: String, alt: String, width: Option<f32>, height: Option<f32> },
    Media(MediaCard),
}

pub struct Inline {
    pub chunks: Vec<Chunk>,
    pub plain: String,
    /// 段落内的锚点（脚注引用的返回位置）
    pub anchors: Vec<String>,
}

pub struct Item {
    /// 任务列表：(是否勾选, 源码中 '[' 的字节偏移)
    pub task: Option<(bool, usize)>,
    pub blocks: Vec<Block>,
}

pub enum Block {
    Heading { level: u8, inline: Inline, anchor: String },
    Paragraph(Inline),
    Code { lang: String, code: String },
    Quote { kind: Option<BlockQuoteKind>, blocks: Vec<Block> },
    List { start: Option<u64>, items: Vec<Item> },
    Rule,
    Table { aligns: Vec<Alignment>, head: Vec<Inline>, rows: Vec<Vec<Inline>> },
    FrontMatter(Arc<LayoutJob>),
    Toc,
    Details { id: usize, summary: String, default_open: bool, blocks: Vec<Block> },
    Aligned { align: Align, blocks: Vec<Block> },
    Media(MediaCard),
}

#[derive(Clone)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub anchor: String,
    pub byte_offset: usize,
}

pub struct Footnote {
    pub label: String,
    pub number: usize,
    pub blocks: Vec<Block>,
}

pub struct Doc {
    pub blocks: Vec<Block>,
    pub block_ranges: Vec<Range<usize>>,
    pub headings: Vec<Heading>,
    pub footnotes: Vec<Footnote>,
}

pub struct ParseOptions {
    pub sup_sub: bool,
    pub front_matter: bool,
}

pub fn parse(text: &str, th: &Theme, opts: &ParseOptions) -> Doc {
    let mut o = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;
    if opts.sup_sub {
        o |= Options::ENABLE_SUPERSCRIPT | Options::ENABLE_SUBSCRIPT;
    }
    let mut p = DocParser::new(th, opts.front_matter, opts.sup_sub);
    for (ev, range) in Parser::new_ext(text, o).into_offset_iter() {
        p.event(ev, range, text);
    }
    p.finish()
}

/// GitHub 风格的标题锚点
pub fn slugify(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

// ---------------------------------------------------------------- 行内构建

#[derive(Clone, Copy, PartialEq)]
struct Style {
    strong: bool,
    emph: bool,
    strike: bool,
    underline: bool,
    highlight: bool,
    sup: bool,
    sub: bool,
    code: bool,
    kbd: bool,
    color: Option<Color32>,
    bg: Option<Color32>,
    size_scale: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            strong: false,
            emph: false,
            strike: false,
            underline: false,
            highlight: false,
            sup: false,
            sub: false,
            code: false,
            kbd: false,
            color: None,
            bg: None,
            size_scale: 1.0,
        }
    }
}

enum Atom {
    Text { s: String, st: Style, link: Option<Target> },
    /// `==` 高亮标记，结束时再两两配对
    Mark,
    Image { url: String, alt: String, width: Option<f32>, height: Option<f32> },
    Media(MediaCard),
}

#[derive(Clone, Copy)]
struct Base {
    size: f32,
    bold: bool,
    color: Color32,
}

#[derive(Clone)]
struct TagSnapshot {
    tag: String,
    style: Style,
    link: Option<Target>,
}

struct InlineBuilder {
    base: Base,
    sup_sub: bool,
    atoms: Vec<Atom>,
    plain: String,
    anchors: Vec<String>,
    style: Style,
    style_stack: Vec<TagSnapshot>,
    link: Option<Target>,
    image: Option<(String, String)>,
    pending_media: Option<MediaCard>,
    pending: String,
}

fn target_from_url(url: &str) -> Target {
    match url.strip_prefix('#') {
        Some(frag) => Target::Anchor(percent_decode(frag)),
        None => Target::Url(url.to_string()),
    }
}

impl InlineBuilder {
    fn new(base: Base, sup_sub: bool) -> Self {
        Self {
            base,
            sup_sub,
            atoms: Vec::new(),
            plain: String::new(),
            anchors: Vec::new(),
            style: Style::default(),
            style_stack: Vec::new(),
            link: None,
            image: None,
            pending_media: None,
            pending: String::new(),
        }
    }

    fn text(&mut self, s: &str) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(s);
        } else if let Some(m) = &mut self.pending_media {
            m.title.push_str(s);
        } else {
            self.pending.push_str(s);
        }
    }

    /// 连续的 Text 事件先合并再处理，避免 `:smile_cat:` 之类被拆开
    fn flush(&mut self) {
        if !self.pending.is_empty() {
            let t = std::mem::take(&mut self.pending);
            self.process(&t);
        }
    }

    fn push_raw(&mut self, s: &str, st: Style, link: Option<Target>) {
        if s.is_empty() {
            return;
        }
        self.plain.push_str(s);
        if let Some(Atom::Text { s: prev, st: pst, link: plink }) = self.atoms.last_mut()
            && *pst == st
            && *plink == link
        {
            prev.push_str(s);
            return;
        }
        self.atoms.push(Atom::Text { s: s.to_string(), st, link });
    }

    /// 处理普通文本：==高亮==、:emoji: 短码、GFM 裸链接、词内 ^上标^/~下标~
    fn process(&mut self, t: &str) {
        let link = self.link.clone();
        for seg in crate::textproc::tokenize(t, self.sup_sub, link.is_none()) {
            match seg {
                crate::textproc::Segment::Plain(s) => self.push_raw(s, self.style, link.clone()),
                crate::textproc::Segment::MarkToggle => self.atoms.push(Atom::Mark),
                crate::textproc::Segment::Sup(s) => {
                    let st = Style { sup: true, ..self.style };
                    self.push_raw(s, st, link.clone());
                }
                crate::textproc::Segment::Sub(s) => {
                    let st = Style { sub: true, ..self.style };
                    self.push_raw(s, st, link.clone());
                }
                crate::textproc::Segment::Emoji(e) => self.push_raw(e, self.style, link.clone()),
                crate::textproc::Segment::Autolink { text, url } => {
                    self.push_raw(text, self.style, Some(Target::Url(url)));
                }
            }
        }
    }

    fn set(&mut self, f: impl FnOnce(&mut Style)) {
        self.flush();
        f(&mut self.style);
    }

    fn code(&mut self, s: &str) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(s);
            return;
        }
        self.flush();
        let st = Style { code: true, ..self.style };
        self.push_raw(s, st, self.link.clone());
    }

    fn soft_break(&mut self) {
        // 中文之间的软换行不插入空格
        let last = self.pending.chars().last().or_else(|| self.plain.chars().last());
        if !last.is_some_and(is_cjk) {
            self.pending.push(' ');
        }
    }

    fn hard_break(&mut self) {
        self.pending.push('\n');
    }

    fn push_tag(&mut self, tag: &str) {
        self.flush();
        self.style_stack.push(TagSnapshot {
            tag: tag.to_string(),
            style: self.style,
            link: self.link.clone(),
        });
    }

    fn pop_tag(&mut self, tag: &str) {
        self.flush();
        if let Some(pos) = self.style_stack.iter().rposition(|s| s.tag == tag) {
            let snap = self.style_stack.remove(pos);
            self.style_stack.truncate(pos);
            self.style = snap.style;
            self.link = snap.link;
        }
    }

    fn html(&mut self, raw: &str) {
        let Some(tag) = parse_html_tag(raw) else { return };
        let name = tag.name.as_str();
        if name == "br" {
            self.hard_break();
            return;
        }
        if tag.closing {
            if matches!(name, "video" | "audio")
                && let Some(mut card) = self.pending_media.take()
            {
                card.title = card.title.trim().to_string();
                if !card.url.is_empty() {
                    self.atoms.push(Atom::Media(card));
                }
            } else {
                self.pop_tag(name);
            }
            return;
        }

        match name {
            "img" => {
                if let Some(src) = tag.attr("src") {
                    self.flush();
                    let alt = tag.attr("alt").unwrap_or("").to_string();
                    if let Some(kind) = detect_media_kind(src) {
                        self.atoms.push(Atom::Media(MediaCard { kind, url: src.to_string(), title: alt, poster: None }));
                    } else {
                        let width = tag.attr("width").and_then(parse_dim);
                        let height = tag.attr("height").and_then(parse_dim);
                        self.atoms.push(Atom::Image { url: src.to_string(), alt, width, height });
                    }
                }
            }
            "video" | "audio" => {
                self.flush();
                let kind = if name == "video" { MediaKind::Video } else { MediaKind::Audio };
                let card = MediaCard {
                    kind,
                    url: tag.attr("src").unwrap_or("").to_string(),
                    title: tag.attr("title").unwrap_or("").to_string(),
                    poster: tag.attr("poster").map(str::to_string),
                };
                if tag.self_closing {
                    if !card.url.is_empty() {
                        self.atoms.push(Atom::Media(card));
                    }
                } else {
                    self.pending_media = Some(card);
                }
            }
            "source" => {
                if let Some(m) = &mut self.pending_media
                    && m.url.is_empty()
                    && let Some(src) = tag.attr("src")
                {
                    m.url = src.to_string();
                }
            }
            "a" => {
                self.push_tag(name);
                if let Some(href) = tag.attr("href") {
                    self.link = Some(target_from_url(href));
                }
            }
            "span" => {
                self.push_tag(name);
                if let Some(st) = tag.attr("style") {
                    let css = parse_inline_style(st);
                    if let Some(c) = css.color {
                        self.style.color = Some(c);
                    }
                    if let Some(bg) = css.bg {
                        self.style.bg = Some(bg);
                    }
                    if let Some(b) = css.bold {
                        self.style.strong = b;
                    }
                    if let Some(it) = css.italic {
                        self.style.emph = it;
                    }
                    if let Some(u) = css.underline {
                        self.style.underline = u;
                    }
                    if let Some(s) = css.strike {
                        self.style.strike = s;
                    }
                    if let Some(sc) = css.size_scale {
                        self.style.size_scale = sc;
                    }
                }
            }
            "font" => {
                self.push_tag(name);
                if let Some(c) = tag.attr("color").and_then(parse_css_color) {
                    self.style.color = Some(c);
                }
                if let Some(sz) = tag.attr("size").and_then(|v| v.trim().parse::<u8>().ok()) {
                    let scale = match sz {
                        1 => 0.75,
                        2 => 0.88,
                        3 => 1.0,
                        4 => 1.15,
                        5 => 1.35,
                        6 => 1.6,
                        _ => 1.9,
                    };
                    self.style.size_scale = scale;
                }
            }
            "u" | "ins" => {
                self.push_tag(name);
                self.style.underline = true;
            }
            "mark" => {
                self.push_tag(name);
                self.style.highlight = true;
            }
            "sup" => {
                self.push_tag(name);
                self.style.sup = true;
            }
            "sub" => {
                self.push_tag(name);
                self.style.sub = true;
            }
            "s" | "del" | "strike" => {
                self.push_tag(name);
                self.style.strike = true;
            }
            "b" | "strong" => {
                self.push_tag(name);
                self.style.strong = true;
            }
            "i" | "em" => {
                self.push_tag(name);
                self.style.emph = true;
            }
            "kbd" => {
                self.push_tag(name);
                self.style.kbd = true;
            }
            "code" => {
                self.push_tag(name);
                self.style.code = true;
            }
            _ => {}
        }
    }

    fn footnote_ref(&mut self, label: &str, n: usize) {
        self.flush();
        let st = Style { sup: true, ..self.style };
        self.push_raw(&format!("[{n}]"), st, Some(Target::Anchor(format!("fn-{label}"))));
        self.anchors.push(format!("fnref-{label}"));
    }

    fn start_link(&mut self, url: &str) {
        self.flush();
        self.link = Some(target_from_url(url));
    }

    fn end_link(&mut self) {
        self.flush();
        self.link = None;
    }

    fn start_image(&mut self, url: &str) {
        self.flush();
        self.image = Some((url.to_string(), String::new()));
    }

    fn end_image(&mut self) {
        if let Some((url, alt)) = self.image.take() {
            if let Some(kind) = detect_media_kind(&url) {
                self.atoms.push(Atom::Media(MediaCard { kind, url, title: alt, poster: None }));
            } else {
                self.atoms.push(Atom::Image { url, alt, width: None, height: None });
            }
        }
    }

    fn format(&self, th: &Theme, st: &Style, link: bool) -> TextFormat {
        let base = self.base;
        let mut size = base.size * st.size_scale;
        let family = if st.code || st.kbd {
            FontFamily::Name("code".into())
        } else if st.strong || base.bold {
            bold_family()
        } else {
            FontFamily::Proportional
        };
        if st.code || st.kbd {
            size *= 0.9;
        }
        let mut valign = Align::BOTTOM;
        let mut line_height = Some(base.size * th.line_height_factor);
        if st.code || st.kbd {
            valign = Align::Center;
            line_height = None;
        }
        if st.sup || st.sub {
            size *= 0.68;
            valign = if st.sup { Align::TOP } else { Align::BOTTOM };
            line_height = None;
        }
        let mut color = if let Some(c) = st.color {
            c
        } else if link {
            th.link
        } else if st.code {
            th.code
        } else {
            base.color
        };
        let mut background = st.bg.unwrap_or(Color32::TRANSPARENT);
        if st.code || st.kbd {
            background = th.code_bg;
        }
        if st.highlight {
            background = th.mark_bg;
            if !link && st.color.is_none() {
                color = th.mark_fg;
            }
        }
        TextFormat {
            font_id: FontId::new(size, family),
            color,
            background,
            expand_bg: if background == Color32::TRANSPARENT { 0.0 } else { 1.5 },
            italics: st.emph,
            underline: if st.underline { Stroke::new(1.0, color) } else { Stroke::NONE },
            strikethrough: if st.strike { Stroke::new(1.2, color) } else { Stroke::NONE },
            valign,
            line_height,
            ..Default::default()
        }
    }

    fn finish(mut self, th: &Theme) -> Inline {
        self.flush();
        self.end_image();
        if let Some(mut card) = self.pending_media.take() {
            card.title = card.title.trim().to_string();
            if !card.url.is_empty() {
                self.atoms.push(Atom::Media(card));
            }
        }

        // `==` 两两配对，中间的文本加高亮；落单的还原成字面量
        let marks: Vec<usize> = self
            .atoms
            .iter()
            .enumerate()
            .filter_map(|(i, a)| matches!(a, Atom::Mark).then_some(i))
            .collect();
        let paired = marks.len() / 2 * 2;
        for k in (0..paired).step_by(2) {
            for atom in &mut self.atoms[marks[k] + 1..marks[k + 1]] {
                if let Atom::Text { st, .. } = atom {
                    st.highlight = true;
                }
            }
        }
        let lone_mark = (marks.len() % 2 == 1).then(|| marks[marks.len() - 1]);

        let mut chunks = Vec::new();
        let mut job = LayoutJob::default();
        let mut links: Vec<(Range<usize>, Target)> = Vec::new();
        let mut chars = 0usize;

        let flush_job = |job: &mut LayoutJob, links: &mut Vec<(Range<usize>, Target)>, chars: &mut usize, chunks: &mut Vec<Chunk>| {
            if !job.text.trim().is_empty() {
                chunks.push(Chunk::Text(Rich { job: Arc::new(std::mem::take(job)), links: std::mem::take(links) }));
            } else {
                *job = LayoutJob::default();
                links.clear();
            }
            *chars = 0;
        };

        let atoms = std::mem::take(&mut self.atoms);
        for (idx, atom) in atoms.into_iter().enumerate() {
            match atom {
                Atom::Mark => {
                    if Some(idx) == lone_mark {
                        let f = self.format(th, &Style::default(), false);
                        job.append("==", 0.0, f);
                        chars += 2;
                    }
                }
                Atom::Text { s, st, link } => {
                    // 图片之后的段首空白去掉
                    let s = if job.text.is_empty() { s.trim_start().to_string() } else { s };
                    if s.is_empty() {
                        continue;
                    }
                    let n = s.chars().count();
                    let f = self.format(th, &st, link.is_some());
                    job.append(&s, 0.0, f);
                    if let Some(t) = link {
                        links.push((chars..chars + n, t));
                    }
                    chars += n;
                }
                Atom::Image { url, alt, width, height } => {
                    flush_job(&mut job, &mut links, &mut chars, &mut chunks);
                    chunks.push(Chunk::Image { url, alt, width, height });
                }
                Atom::Media(card) => {
                    flush_job(&mut job, &mut links, &mut chars, &mut chunks);
                    chunks.push(Chunk::Media(card));
                }
            }
        }
        flush_job(&mut job, &mut links, &mut chars, &mut chunks);

        Inline { chunks, plain: self.plain, anchors: self.anchors }
    }
}

fn front_matter_job(src: &str, th: &Theme) -> LayoutJob {
    let font = FontId::monospace(th.size * 0.88);
    let fmt = |color| TextFormat { font_id: font.clone(), color, line_height: Some(th.size * 1.45), ..Default::default() };
    let mut job = LayoutJob::default();
    let lines: Vec<&str> = src.trim_end().lines().collect();
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            job.append(line, 0.0, fmt(th.weak));
        } else if let Some(colon) = line.find(':')
            && !line.starts_with(' ')
            && !line.starts_with('-')
        {
            job.append(&line[..colon], 0.0, fmt(th.link));
            job.append(&line[colon..], 0.0, fmt(th.text));
        } else {
            job.append(line, 0.0, fmt(th.text));
        }
        if n + 1 < lines.len() {
            job.append("\n", 0.0, fmt(th.text));
        }
    }
    job
}

// ---------------------------------------------------------------- 块级解析

enum Frame {
    Root(Vec<Block>),
    Quote(Option<BlockQuoteKind>, Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Option<(bool, usize)>, Vec<Block>),
    Footnote(String, Vec<Block>),
    Details { id: usize, summary: String, default_open: bool, blocks: Vec<Block>, start_byte: usize },
    Aligned { align: Align, close_tag: &'static str, blocks: Vec<Block>, start_byte: usize },
}

enum Leaf {
    None,
    Para,
    Heading { level: u8, id: Option<String>, offset: usize },
    Cell,
    Code { lang: String, buf: String },
    Html(String),
    Meta(String),
}

struct TableState {
    aligns: Vec<Alignment>,
    head: Vec<Inline>,
    rows: Vec<Vec<Inline>>,
    row: Vec<Inline>,
    in_head: bool,
}

struct DocParser<'t> {
    th: &'t Theme,
    sup_sub: bool,
    show_meta: bool,
    stack: Vec<Frame>,
    root_ranges: Vec<Range<usize>>,
    cur_top_range: Option<Range<usize>>,
    last_range_end: usize,
    next_details_id: usize,
    leaf: Leaf,
    inline: Option<InlineBuilder>,
    implicit: bool,
    tables: Vec<TableState>,
    headings: Vec<Heading>,
    anchors_used: HashMap<String, usize>,
    fn_numbers: HashMap<String, usize>,
    fn_defs: Vec<(String, Vec<Block>)>,
}

impl<'t> DocParser<'t> {
    fn new(th: &'t Theme, show_meta: bool, sup_sub: bool) -> Self {
        Self {
            th,
            sup_sub,
            show_meta,
            stack: vec![Frame::Root(Vec::new())],
            root_ranges: Vec::new(),
            cur_top_range: None,
            last_range_end: 0,
            next_details_id: 1,
            leaf: Leaf::None,
            inline: None,
            implicit: false,
            tables: Vec::new(),
            headings: Vec::new(),
            anchors_used: HashMap::new(),
            fn_numbers: HashMap::new(),
            fn_defs: Vec::new(),
        }
    }

    fn in_plain_quote(&self) -> bool {
        self.stack.iter().any(|f| matches!(f, Frame::Quote(None, _)))
    }

    fn para_base(&self) -> Base {
        Base { size: self.th.size, bold: false, color: if self.in_plain_quote() { self.th.quote } else { self.th.text } }
    }

    fn push_block(&mut self, b: Block) {
        match self.stack.last_mut() {
            Some(Frame::Root(v)) => {
                v.push(b);
                let fallback = self.last_range_end..self.last_range_end;
                self.root_ranges.push(self.cur_top_range.take().unwrap_or(fallback));
            }
            Some(
                Frame::Quote(_, v)
                | Frame::Item(_, v)
                | Frame::Footnote(_, v)
                | Frame::Details { blocks: v, .. }
                | Frame::Aligned { blocks: v, .. },
            ) => v.push(b),
            Some(Frame::List(_, items)) => {
                // 理论上不会出现：列表里直接出现块，挂到最后一个条目上
                if let Some(it) = items.last_mut() {
                    it.blocks.push(b);
                }
            }
            None => {}
        }
    }

    fn ib(&mut self) -> &mut InlineBuilder {
        if self.inline.is_none() {
            self.inline = Some(InlineBuilder::new(self.para_base(), self.sup_sub));
            self.implicit = true;
        }
        self.inline.as_mut().unwrap()
    }

    fn finish_paragraph(&mut self, ib: InlineBuilder) {
        let mut inl = ib.finish(self.th);
        let t = inl.plain.trim().to_ascii_uppercase();
        if matches!(t.as_str(), "[TOC]" | "[[TOC]]" | "[[_TOC_]]") {
            self.push_block(Block::Toc);
        } else if inl.chunks.len() == 1 && inl.anchors.is_empty() && matches!(inl.chunks[0], Chunk::Media(_)) {
            if let Some(Chunk::Media(card)) = inl.chunks.pop() {
                self.push_block(Block::Media(card));
            }
        } else if !inl.chunks.is_empty() || !inl.anchors.is_empty() {
            self.push_block(Block::Paragraph(inl));
        }
    }

    fn flush_implicit(&mut self) {
        if self.implicit {
            self.implicit = false;
            if let Some(ib) = self.inline.take() {
                self.finish_paragraph(ib);
            }
        }
    }

    fn unique_anchor(&mut self, base: String) -> String {
        let n = self.anchors_used.entry(base.clone()).or_insert(0);
        let a = if *n == 0 { base } else { format!("{base}-{n}") };
        *n += 1;
        a
    }

    fn footnote_number(&mut self, label: &str) -> usize {
        let next = self.fn_numbers.len() + 1;
        *self.fn_numbers.entry(label.to_string()).or_insert(next)
    }

    fn has_open_details(&self) -> bool {
        self.stack.iter().any(|f| matches!(f, Frame::Details { .. }))
    }

    fn has_open_aligned(&self, tag: &str) -> bool {
        self.stack.iter().any(|f| matches!(f, Frame::Aligned { close_tag, .. } if *close_tag == tag))
    }

    fn unwind_frame(&mut self, frame: Frame) {
        match frame {
            Frame::Details { id, summary, default_open, blocks, start_byte } => {
                if self.stack.len() == 1 {
                    self.cur_top_range = Some(start_byte..self.last_range_end);
                }
                self.push_block(Block::Details { id, summary, default_open, blocks });
            }
            Frame::Aligned { align, blocks, start_byte, .. } => {
                if !blocks.is_empty() {
                    if self.stack.len() == 1 {
                        self.cur_top_range = Some(start_byte..self.last_range_end);
                    }
                    self.push_block(Block::Aligned { align, blocks });
                }
            }
            Frame::Quote(kind, blocks) => self.push_block(Block::Quote { kind, blocks }),
            Frame::List(start, items) => self.push_block(Block::List { start, items }),
            Frame::Item(task, blocks) => {
                if let Some(Frame::List(_, items)) = self.stack.last_mut() {
                    items.push(Item { task, blocks });
                }
            }
            Frame::Footnote(label, blocks) => self.fn_defs.push((label, blocks)),
            Frame::Root(v) => self.stack.push(Frame::Root(v)),
        }
    }

    fn pop_details_frame(&mut self) {
        self.flush_implicit();
        if self.has_open_details() {
            while let Some(top) = self.stack.pop() {
                let is_target = matches!(top, Frame::Details { .. });
                let is_root = matches!(top, Frame::Root(_));
                self.unwind_frame(top);
                if is_target || is_root {
                    break;
                }
            }
        }
    }

    fn pop_aligned_frame(&mut self, tag: &str) {
        self.flush_implicit();
        if self.has_open_aligned(tag) {
            while let Some(top) = self.stack.pop() {
                let is_target = matches!(top, Frame::Aligned { close_tag, .. } if close_tag == tag);
                let is_root = matches!(top, Frame::Root(_));
                self.unwind_frame(top);
                if is_target || is_root {
                    break;
                }
            }
        }
    }

    fn handle_html_block(&mut self, html: &str) {
        let lower = html.to_ascii_lowercase();
        if let Some(open_pos) = find_tag_pos(&lower, "<details")
            && find_tag_pos(&lower, "</details>").is_none()
        {
            let before = html[..open_pos].trim();
            if !before.is_empty() {
                self.push_dom_html(before);
            }
            let after_open_start = open_pos;
            let tag_end = html[after_open_start..].find('>').map(|i| after_open_start + i + 1).unwrap_or(html.len());
            let open_tag_raw = &html[after_open_start..tag_end];
            let default_open = parse_html_tag(open_tag_raw).is_some_and(|t| t.attr("open").is_some());
            let mut rest = html[tag_end..].trim();

            let mut summary = "详细信息".to_string();
            let lower_rest = rest.to_ascii_lowercase();
            if let Some(s_start) = find_tag_pos(&lower_rest, "<summary")
                && let Some(s_gt) = rest[s_start..].find('>')
                && let Some(s_end) = lower_rest[s_start + s_gt + 1..].find("</summary>")
            {
                let inner = &rest[s_start + s_gt + 1..s_start + s_gt + 1 + s_end];
                let clean = strip_html_tags(inner).trim().to_string();
                if !clean.is_empty() {
                    summary = clean;
                }
                rest = rest[s_start + s_gt + 1 + s_end + "</summary>".len()..].trim();
            }

            let id = self.next_details_id;
            self.next_details_id += 1;
            let start_byte = self.cur_top_range.as_ref().map(|r| r.start).unwrap_or(self.last_range_end);
            self.cur_top_range = None;
            self.stack.push(Frame::Details {
                id,
                summary,
                default_open,
                blocks: Vec::new(),
                start_byte,
            });
            if !rest.is_empty() {
                self.push_dom_html(rest);
            }
            return;
        }

        if let Some(close_pos) = find_tag_pos(&lower, "</details>")
            && self.has_open_details()
        {
            let before = html[..close_pos].trim();
            if !before.is_empty() {
                self.push_dom_html(before);
            }
            self.pop_details_frame();
            let after = html[close_pos + "</details>".len()..].trim();
            if !after.is_empty() {
                self.push_dom_html(after);
            }
            return;
        }

        if let Some(Frame::Details { summary, .. }) = self.stack.last_mut()
            && let Some(s_start) = find_tag_pos(&lower, "<summary")
            && let Some(s_gt) = html[s_start..].find('>')
            && let Some(s_end) = lower[s_start + s_gt + 1..].find("</summary>")
        {
            let inner = &html[s_start + s_gt + 1..s_start + s_gt + 1 + s_end];
            let clean = strip_html_tags(inner).trim().to_string();
            if !clean.is_empty() {
                *summary = clean;
            }
            let after = html[s_start + s_gt + 1 + s_end + "</summary>".len()..].trim();
            if !after.is_empty() {
                self.push_dom_html(after);
            }
            return;
        }

        if let Some((tag, open_pos, align)) = opening_aligned_tag(html, &lower)
            && find_tag_pos(&lower[open_pos..], if tag == "div" { "</div>" } else { "</center>" }).is_none()
        {
            let before = html[..open_pos].trim();
            if !before.is_empty() {
                self.push_dom_html(before);
            }
            let tag_end = html[open_pos..].find('>').map(|i| open_pos + i + 1).unwrap_or(html.len());
            let rest = html[tag_end..].trim();
            let start_byte = self.cur_top_range.take().map(|r| r.start).unwrap_or(self.last_range_end);
            self.stack.push(Frame::Aligned {
                align,
                close_tag: tag,
                blocks: Vec::new(),
                start_byte,
            });
            if !rest.is_empty() {
                self.push_dom_html(rest);
            }
            return;
        }

        for tag in ["div", "center"] {
            let close_pat = if tag == "div" { "</div>" } else { "</center>" };
            if let Some(close_pos) = find_tag_pos(&lower, close_pat)
                && self.has_open_aligned(tag)
            {
                let before = html[..close_pos].trim();
                if !before.is_empty() {
                    self.push_dom_html(before);
                }
                self.pop_aligned_frame(tag);
                let after = html[close_pos + close_pat.len()..].trim();
                if !after.is_empty() {
                    self.push_dom_html(after);
                }
                return;
            }
        }

        self.push_dom_html(html);
    }

    fn push_dom_html(&mut self, raw: &str) {
        let top_range = self.cur_top_range.take();
        let blocks = html_block_to_blocks(raw, self.th, self.sup_sub, &mut self.next_details_id);
        for (i, b) in blocks.into_iter().enumerate() {
            if self.stack.len() == 1 && i == 0 {
                self.cur_top_range = top_range.clone();
            }
            self.push_block(b);
        }
    }

    fn event(&mut self, ev: Event, range: Range<usize>, src: &str) {
        self.last_range_end = range.end;
        match ev {
            Event::Start(tag) => {
                if self.stack.len() == 1
                    && self.tables.is_empty()
                    && matches!(self.leaf, Leaf::None)
                    && !matches!(tag, Tag::FootnoteDefinition(_))
                {
                    self.cur_top_range = Some(range.clone());
                }
                self.start(tag, range.start);
            }
            Event::End(end) => self.end(end),
            Event::Text(t) => match &mut self.leaf {
                Leaf::Code { buf, .. } | Leaf::Html(buf) | Leaf::Meta(buf) => buf.push_str(&t),
                _ => self.ib().text(&t),
            },
            Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => self.ib().code(&t),
            Event::Html(t) => match &mut self.leaf {
                Leaf::Html(buf) => buf.push_str(&t),
                _ => {
                    let tr = t.trim();
                    if tr.eq_ignore_ascii_case("</details>") && self.has_open_details() {
                        self.pop_details_frame();
                    } else if tr.eq_ignore_ascii_case("</div>") && self.has_open_aligned("div") {
                        self.pop_aligned_frame("div");
                    } else if tr.eq_ignore_ascii_case("</center>") && self.has_open_aligned("center") {
                        self.pop_aligned_frame("center");
                    } else {
                        self.ib().html(&t);
                    }
                }
            },
            Event::InlineHtml(t) => {
                let tr = t.trim();
                if tr.eq_ignore_ascii_case("</details>") && self.has_open_details() {
                    self.pop_details_frame();
                } else if tr.eq_ignore_ascii_case("</div>") && self.has_open_aligned("div") {
                    self.pop_aligned_frame("div");
                } else if tr.eq_ignore_ascii_case("</center>") && self.has_open_aligned("center") {
                    self.pop_aligned_frame("center");
                } else {
                    self.ib().html(&t);
                }
            }
            Event::FootnoteReference(label) => {
                let n = self.footnote_number(&label);
                self.ib().footnote_ref(&label, n);
            }
            Event::SoftBreak => self.ib().soft_break(),
            Event::HardBreak => self.ib().hard_break(),
            Event::Rule => {
                self.flush_implicit();
                if self.stack.len() == 1 && self.tables.is_empty() {
                    self.cur_top_range = Some(range);
                }
                self.push_block(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                let off = src.get(range.clone()).and_then(|s| s.find('[')).map(|i| range.start + i).unwrap_or(range.start);
                if let Some(Frame::Item(task, _)) = self.stack.last_mut() {
                    *task = Some((checked, off));
                }
            }
        }
    }

    fn start(&mut self, tag: Tag, start_offset: usize) {
        match tag {
            Tag::Paragraph => {
                self.flush_implicit();
                self.inline = Some(InlineBuilder::new(self.para_base(), self.sup_sub));
                self.leaf = Leaf::Para;
            }
            Tag::Heading { level, id, .. } => {
                self.flush_implicit();
                let lv = level as u8;
                let scale = [2.0, 1.6, 1.32, 1.14, 1.0, 0.92][(lv as usize - 1).min(5)];
                let color = if lv == 6 { self.th.weak } else { self.th.heading };
                self.inline = Some(InlineBuilder::new(Base { size: self.th.size * scale, bold: true, color }, self.sup_sub));
                self.leaf = Leaf::Heading { level: lv, id: id.map(|s| s.to_string()), offset: start_offset };
            }
            Tag::BlockQuote(kind) => {
                self.flush_implicit();
                self.stack.push(Frame::Quote(kind, Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.flush_implicit();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.leaf = Leaf::Code { lang, buf: String::new() };
            }
            Tag::HtmlBlock => {
                self.flush_implicit();
                self.leaf = Leaf::Html(String::new());
            }
            Tag::List(start) => {
                self.flush_implicit();
                self.stack.push(Frame::List(start, Vec::new()));
            }
            Tag::Item => {
                self.flush_implicit();
                self.stack.push(Frame::Item(None, Vec::new()));
            }
            Tag::FootnoteDefinition(label) => {
                self.flush_implicit();
                self.stack.push(Frame::Footnote(label.to_string(), Vec::new()));
            }
            Tag::Table(aligns) => {
                self.flush_implicit();
                self.tables.push(TableState { aligns, head: Vec::new(), rows: Vec::new(), row: Vec::new(), in_head: false });
            }
            Tag::TableHead => {
                if let Some(t) = self.tables.last_mut() {
                    t.in_head = true;
                    t.row.clear();
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.tables.last_mut() {
                    t.row.clear();
                }
            }
            Tag::TableCell => {
                let bold = self.tables.last().is_some_and(|t| t.in_head);
                let mut base = self.para_base();
                base.bold = bold;
                self.inline = Some(InlineBuilder::new(base, self.sup_sub));
                self.leaf = Leaf::Cell;
            }
            Tag::Emphasis => self.ib().set(|s| s.emph = true),
            Tag::Strong => self.ib().set(|s| s.strong = true),
            Tag::Strikethrough => self.ib().set(|s| s.strike = true),
            Tag::Superscript => self.ib().set(|s| s.sup = true),
            Tag::Subscript => self.ib().set(|s| s.sub = true),
            Tag::Link { dest_url, .. } => self.ib().start_link(&dest_url),
            Tag::Image { dest_url, .. } => self.ib().start_image(&dest_url),
            Tag::MetadataBlock(_) => self.leaf = Leaf::Meta(String::new()),
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph => {
                if let Some(ib) = self.inline.take() {
                    self.finish_paragraph(ib);
                }
                self.leaf = Leaf::None;
                self.implicit = false;
            }
            TagEnd::Heading(_) => {
                let leaf = std::mem::replace(&mut self.leaf, Leaf::None);
                if let (Some(ib), Leaf::Heading { level, id, offset }) = (self.inline.take(), leaf) {
                    let inline = ib.finish(self.th);
                    let text = inline.plain.trim().to_string();
                    let anchor = self.unique_anchor(id.unwrap_or_else(|| slugify(&text)));
                    self.headings.push(Heading { level, text, anchor: anchor.clone(), byte_offset: offset });
                    self.push_block(Block::Heading { level, inline, anchor });
                }
                self.implicit = false;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_implicit();
                if let Some(Frame::Quote(kind, blocks)) = self.stack.pop() {
                    self.push_block(Block::Quote { kind, blocks });
                }
            }
            TagEnd::CodeBlock => {
                if let Leaf::Code { lang, buf } = std::mem::replace(&mut self.leaf, Leaf::None) {
                    let code = buf.strip_suffix('\n').unwrap_or(&buf).to_string();
                    self.push_block(Block::Code { lang, code });
                }
            }
            TagEnd::HtmlBlock => {
                if let Leaf::Html(buf) = std::mem::replace(&mut self.leaf, Leaf::None) {
                    let html = buf.trim().to_string();
                    if !html.is_empty() {
                        self.handle_html_block(&html);
                    } else {
                        self.cur_top_range = None;
                    }
                }
            }
            TagEnd::List(_) => {
                self.flush_implicit();
                if let Some(Frame::List(start, items)) = self.stack.pop() {
                    self.push_block(Block::List { start, items });
                }
            }
            TagEnd::Item => {
                self.flush_implicit();
                if let Some(Frame::Item(task, blocks)) = self.stack.pop()
                    && let Some(Frame::List(_, items)) = self.stack.last_mut()
                {
                    items.push(Item { task, blocks });
                }
            }
            TagEnd::FootnoteDefinition => {
                self.flush_implicit();
                if let Some(Frame::Footnote(label, blocks)) = self.stack.pop() {
                    self.fn_defs.push((label, blocks));
                }
            }
            TagEnd::TableCell => {
                if let Some(ib) = self.inline.take() {
                    let inl = ib.finish(self.th);
                    if let Some(t) = self.tables.last_mut() {
                        t.row.push(inl);
                    }
                }
                self.leaf = Leaf::None;
                self.implicit = false;
            }
            TagEnd::TableHead => {
                if let Some(t) = self.tables.last_mut() {
                    t.head = std::mem::take(&mut t.row);
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.tables.last_mut() {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.tables.pop() {
                    self.push_block(Block::Table { aligns: t.aligns, head: t.head, rows: t.rows });
                }
            }
            TagEnd::Emphasis => self.ib().set(|s| s.emph = false),
            TagEnd::Strong => self.ib().set(|s| s.strong = false),
            TagEnd::Strikethrough => self.ib().set(|s| s.strike = false),
            TagEnd::Superscript => self.ib().set(|s| s.sup = false),
            TagEnd::Subscript => self.ib().set(|s| s.sub = false),
            TagEnd::Link => self.ib().end_link(),
            TagEnd::Image => self.ib().end_image(),
            TagEnd::MetadataBlock(_) => {
                if let Leaf::Meta(buf) = std::mem::replace(&mut self.leaf, Leaf::None)
                    && self.show_meta
                    && !buf.trim().is_empty()
                {
                    let job = front_matter_job(&buf, self.th);
                    self.push_block(Block::FrontMatter(Arc::new(job)));
                }
            }
            TagEnd::DefinitionList | TagEnd::DefinitionListTitle | TagEnd::DefinitionListDefinition => {}
        }
    }

    fn finish(mut self) -> Doc {
        self.flush_implicit();
        while self.stack.len() > 1 {
            if let Some(top) = self.stack.pop() {
                self.unwind_frame(top);
            }
        }
        let blocks = match self.stack.into_iter().next() {
            Some(Frame::Root(v)) => v,
            _ => Vec::new(),
        };
        let mut next = self.fn_numbers.len() + 1;
        let mut footnotes: Vec<Footnote> = self
            .fn_defs
            .into_iter()
            .map(|(label, blocks)| {
                let number = *self.fn_numbers.entry(label.clone()).or_insert_with(|| {
                    next += 1;
                    next - 1
                });
                Footnote { label, number, blocks }
            })
            .collect();
        footnotes.sort_by_key(|f| f.number);
        Doc { blocks, block_ranges: self.root_ranges, headings: self.headings, footnotes }
    }
}

// ---------------------------------------------------------------- DOM HTML 块转换

fn find_tag_pos(lower: &str, needle: &str) -> Option<usize> {
    let idx = lower.find(needle)?;
    let next = lower.as_bytes().get(idx + needle.len()).copied();
    if needle.ends_with('>') || matches!(next, None | Some(b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/')) {
        Some(idx)
    } else {
        None
    }
}

fn opening_aligned_tag(raw: &str, lower: &str) -> Option<(&'static str, usize, Align)> {
    if let Some(pos) = find_tag_pos(lower, "<center") {
        return Some(("center", pos, Align::Center));
    }
    if let Some(pos) = find_tag_pos(lower, "<div") {
        let end = raw[pos..].find('>').map(|i| pos + i + 1).unwrap_or(raw.len());
        if let Some(tag) = parse_html_tag(&raw[pos..end])
            && !tag.closing
            && !tag.self_closing
        {
            if let Some(a) = tag.attr("align") {
                match a.trim().to_ascii_lowercase().as_str() {
                    "center" => return Some(("div", pos, Align::Center)),
                    "right" => return Some(("div", pos, Align::Max)),
                    "left" => return Some(("div", pos, Align::Min)),
                    _ => {}
                }
            }
            if let Some(st) = tag.attr("style")
                && let Some(a) = parse_inline_style(st).align
            {
                return Some(("div", pos, a));
            }
        }
    }
    None
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    crate::html_inline::decode_basic_entities(&out)
}

fn html_block_to_blocks(raw: &str, th: &Theme, sup_sub: bool, next_details_id: &mut usize) -> Vec<Block> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || (trimmed.starts_with("<!--") && trimmed.ends_with("-->")) {
        return Vec::new();
    }
    let Ok(tree) = htmd::HtmlToMarkdown::new().html_to_tree(trimmed) else {
        return Vec::new();
    };
    let root = tree.root();
    let mut out = Vec::new();
    if !find_and_convert_body(&root, th, sup_sub, next_details_id, &mut out) {
        convert_dom_children(&root, th, sup_sub, next_details_id, &mut out);
    }
    out
}

fn find_and_convert_body(
    node: &Node<'_>,
    th: &Theme,
    sup_sub: bool,
    next_details_id: &mut usize,
    out: &mut Vec<Block>,
) -> bool {
    for ch in node.children().iter() {
        if ch.node_type() == NodeType::Element {
            if ch.tag_name() == Some("body") {
                convert_dom_children(&ch, th, sup_sub, next_details_id, out);
                return true;
            }
            if find_and_convert_body(&ch, th, sup_sub, next_details_id, out) {
                return true;
            }
        }
    }
    false
}

fn is_block_tag(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "section"
            | "article"
            | "main"
            | "center"
            | "figure"
            | "figcaption"
            | "blockquote"
            | "pre"
            | "ul"
            | "ol"
            | "table"
            | "hr"
            | "details"
            | "video"
            | "audio"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    )
}

fn collect_dom_text(node: &Node<'_>, out: &mut String) {
    match node.node_type() {
        NodeType::Text => {
            if let Some(t) = node.text() {
                out.push_str(&t);
            }
        }
        NodeType::Element => {
            if matches!(node.tag_name(), Some("script" | "style" | "noscript")) {
                return;
            }
            if node.tag_name() == Some("br") {
                out.push('\n');
                return;
            }
            for ch in node.children().iter() {
                collect_dom_text(&ch, out);
            }
        }
        _ => {}
    }
}

fn feed_dom_inline(node: &Node<'_>, ib: &mut InlineBuilder) {
    match node.node_type() {
        NodeType::Text => {
            if let Some(t) = node.text() {
                ib.text(&t);
            }
        }
        NodeType::Element => {
            let tag = node.tag_name().unwrap_or("");
            if matches!(tag, "script" | "style" | "noscript" | "template") {
                return;
            }
            if tag == "br" {
                ib.hard_break();
                return;
            }
            if tag == "img" {
                if let Some(src) = node.attrs().get("src") {
                    ib.flush();
                    let alt = node.attrs().get("alt").unwrap_or("").to_string();
                    if let Some(kind) = detect_media_kind(src) {
                        ib.atoms.push(Atom::Media(MediaCard { kind, url: src.to_string(), title: alt, poster: None }));
                    } else {
                        let width = node.attrs().get("width").and_then(parse_dim);
                        let height = node.attrs().get("height").and_then(parse_dim);
                        ib.atoms.push(Atom::Image { url: src.to_string(), alt, width, height });
                    }
                }
                return;
            }
            if matches!(tag, "video" | "audio") {
                if let Some(card) = extract_media_node(node, tag) {
                    ib.flush();
                    ib.atoms.push(Atom::Media(card));
                }
                return;
            }
            ib.push_tag(tag);
            match tag {
                "a" => {
                    if let Some(href) = node.attrs().get("href") {
                        ib.link = Some(target_from_url(href));
                    }
                }
                "span" | "p" | "div" => {
                    if let Some(st) = node.attrs().get("style") {
                        let css = parse_inline_style(st);
                        if let Some(c) = css.color {
                            ib.style.color = Some(c);
                        }
                        if let Some(bg) = css.bg {
                            ib.style.bg = Some(bg);
                        }
                        if let Some(b) = css.bold {
                            ib.style.strong = b;
                        }
                        if let Some(it) = css.italic {
                            ib.style.emph = it;
                        }
                        if let Some(u) = css.underline {
                            ib.style.underline = u;
                        }
                        if let Some(s) = css.strike {
                            ib.style.strike = s;
                        }
                        if let Some(sc) = css.size_scale {
                            ib.style.size_scale = sc;
                        }
                    }
                }
                "font" => {
                    if let Some(c) = node.attrs().get("color").and_then(parse_css_color) {
                        ib.style.color = Some(c);
                    }
                }
                "b" | "strong" => ib.style.strong = true,
                "i" | "em" => ib.style.emph = true,
                "u" | "ins" => ib.style.underline = true,
                "mark" => ib.style.highlight = true,
                "s" | "del" | "strike" => ib.style.strike = true,
                "sup" => ib.style.sup = true,
                "sub" => ib.style.sub = true,
                "kbd" => ib.style.kbd = true,
                "code" => ib.style.code = true,
                _ => {}
            }
            for ch in node.children().iter() {
                feed_dom_inline(&ch, ib);
            }
            ib.pop_tag(tag);
        }
        _ => {}
    }
}

fn extract_media_node(node: &Node<'_>, tag: &str) -> Option<MediaCard> {
    let kind = if tag == "video" { MediaKind::Video } else { MediaKind::Audio };
    let mut url = node.attrs().get("src").unwrap_or("").to_string();
    if url.is_empty() {
        for ch in node.children().iter() {
            if ch.tag_name() == Some("source")
                && let Some(src) = ch.attrs().get("src")
            {
                url = src.to_string();
                break;
            }
        }
    }
    if url.is_empty() {
        return None;
    }
    let mut title = node.attrs().get("title").unwrap_or("").trim().to_string();
    if title.is_empty() {
        let mut fallback = String::new();
        for ch in node.children().iter() {
            if ch.tag_name() != Some("source") {
                collect_dom_text(&ch, &mut fallback);
            }
        }
        title = fallback.trim().to_string();
    }
    let poster = node.attrs().get("poster").map(str::to_string);
    Some(MediaCard { kind, url, title, poster })
}

fn element_align(node: &Node<'_>) -> Option<Align> {
    if node.tag_name() == Some("center") {
        return Some(Align::Center);
    }
    if let Some(a) = node.attrs().get("align") {
        match a.trim().to_ascii_lowercase().as_str() {
            "center" => return Some(Align::Center),
            "right" => return Some(Align::Max),
            "left" => return Some(Align::Min),
            _ => {}
        }
    }
    if let Some(st) = node.attrs().get("style") {
        return parse_inline_style(st).align;
    }
    None
}

fn flush_inline_dom(ib: &mut Option<InlineBuilder>, th: &Theme, out: &mut Vec<Block>) {
    if let Some(builder) = ib.take() {
        let mut inl = builder.finish(th);
        if inl.chunks.len() == 1 && inl.anchors.is_empty() && matches!(inl.chunks[0], Chunk::Media(_)) {
            if let Some(Chunk::Media(card)) = inl.chunks.pop() {
                out.push(Block::Media(card));
            }
        } else if !inl.chunks.is_empty() {
            out.push(Block::Paragraph(inl));
        }
    }
}

fn convert_dom_children(
    parent: &Node<'_>,
    th: &Theme,
    sup_sub: bool,
    next_details_id: &mut usize,
    out: &mut Vec<Block>,
) {
    let base = Base { size: th.size, bold: false, color: th.text };
    let mut cur_inline: Option<InlineBuilder> = None;

    for ch in parent.children().iter() {
        match ch.node_type() {
            NodeType::Comment | NodeType::Doctype | NodeType::ProcessingInstruction => continue,
            NodeType::Text => {
                let t = ch.text().map(|s| s.to_string()).unwrap_or_default();
                if cur_inline.is_none() && t.trim().is_empty() {
                    continue;
                }
                cur_inline.get_or_insert_with(|| InlineBuilder::new(base, sup_sub)).text(&t);
            }
            NodeType::Element => {
                let tag = ch.tag_name().unwrap_or("");
                if matches!(tag, "head" | "script" | "style" | "noscript" | "template") {
                    continue;
                }
                if is_block_tag(tag) {
                    flush_inline_dom(&mut cur_inline, th, out);
                    convert_dom_block(&ch, tag, th, sup_sub, next_details_id, out);
                } else {
                    let ib = cur_inline.get_or_insert_with(|| InlineBuilder::new(base, sup_sub));
                    feed_dom_inline(&ch, ib);
                }
            }
            NodeType::Document => {}
        }
    }

    flush_inline_dom(&mut cur_inline, th, out);
}

fn convert_dom_block(
    node: &Node<'_>,
    tag: &str,
    th: &Theme,
    sup_sub: bool,
    next_details_id: &mut usize,
    out: &mut Vec<Block>,
) {
    match tag {
        "hr" => out.push(Block::Rule),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let lv = tag.as_bytes()[1] - b'0';
            let scale = [2.0, 1.6, 1.32, 1.14, 1.0, 0.92][(lv as usize - 1).min(5)];
            let color = if lv == 6 { th.weak } else { th.heading };
            let mut ib = InlineBuilder::new(Base { size: th.size * scale, bold: true, color }, sup_sub);
            for ch in node.children().iter() {
                feed_dom_inline(&ch, &mut ib);
            }
            let inline = ib.finish(th);
            let anchor = slugify(&inline.plain);
            out.push(Block::Heading { level: lv, inline, anchor });
        }
        "pre" => {
            let mut lang = String::new();
            for ch in node.children().iter() {
                if ch.tag_name() == Some("code")
                    && let Some(cls) = ch.attrs().get("class")
                    && let Some(l) = cls.split_whitespace().find_map(|c| c.strip_prefix("language-"))
                {
                    lang = l.to_string();
                }
            }
            let mut code = String::new();
            collect_dom_text(node, &mut code);
            let code = code.trim_matches('\n').to_string();
            out.push(Block::Code { lang, code });
        }
        "blockquote" => {
            let mut blocks = Vec::new();
            convert_dom_children(node, th, sup_sub, next_details_id, &mut blocks);
            if !blocks.is_empty() {
                out.push(Block::Quote { kind: None, blocks });
            }
        }
        "ul" | "ol" => {
            let start = if tag == "ol" {
                Some(node.attrs().get("start").and_then(|s| s.parse().ok()).unwrap_or(1))
            } else {
                None
            };
            let mut items = Vec::new();
            for ch in node.children().iter() {
                if ch.tag_name() == Some("li") {
                    let mut blocks = Vec::new();
                    convert_dom_children(&ch, th, sup_sub, next_details_id, &mut blocks);
                    items.push(Item { task: None, blocks });
                }
            }
            if !items.is_empty() {
                out.push(Block::List { start, items });
            }
        }
        "video" | "audio" => {
            if let Some(card) = extract_media_node(node, tag) {
                out.push(Block::Media(card));
            }
        }
        "details" => {
            let default_open = node.attrs().get("open").is_some();
            let mut summary = "详细信息".to_string();
            let mut blocks = Vec::new();
            for ch in node.children().iter() {
                if ch.tag_name() == Some("summary") {
                    let mut s = String::new();
                    collect_dom_text(&ch, &mut s);
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        summary = trimmed.to_string();
                    }
                } else if ch.node_type() == NodeType::Element {
                    let ctag = ch.tag_name().unwrap_or("");
                    if is_block_tag(ctag) {
                        convert_dom_block(&ch, ctag, th, sup_sub, next_details_id, &mut blocks);
                    } else {
                        let mut ib = InlineBuilder::new(Base { size: th.size, bold: false, color: th.text }, sup_sub);
                        feed_dom_inline(&ch, &mut ib);
                        let inl = ib.finish(th);
                        if !inl.chunks.is_empty() {
                            blocks.push(Block::Paragraph(inl));
                        }
                    }
                } else if ch.node_type() == NodeType::Text
                    && let Some(t) = ch.text()
                    && !t.trim().is_empty()
                {
                    let mut ib = InlineBuilder::new(Base { size: th.size, bold: false, color: th.text }, sup_sub);
                    ib.text(&t);
                    blocks.push(Block::Paragraph(ib.finish(th)));
                }
            }
            let id = *next_details_id;
            *next_details_id += 1;
            out.push(Block::Details { id, summary, default_open, blocks });
        }
        "table" => {
            let mut head = Vec::new();
            let mut rows = Vec::new();
            collect_dom_table_rows(node, th, sup_sub, &mut head, &mut rows);
            let ncols = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
            if ncols > 0 {
                out.push(Block::Table { aligns: vec![Alignment::None; ncols], head, rows });
            }
        }
        _ => {
            let align = element_align(node);
            let mut inner = Vec::new();
            convert_dom_children(node, th, sup_sub, next_details_id, &mut inner);
            if let Some(a) = align.filter(|a| *a != Align::Min) {
                if !inner.is_empty() {
                    out.push(Block::Aligned { align: a, blocks: inner });
                }
            } else {
                out.extend(inner);
            }
        }
    }
}

fn collect_dom_table_rows(
    node: &Node<'_>,
    th: &Theme,
    sup_sub: bool,
    head: &mut Vec<Inline>,
    rows: &mut Vec<Vec<Inline>>,
) {
    for ch in node.children().iter() {
        let Some(tag) = ch.tag_name() else { continue };
        match tag {
            "thead" | "tbody" | "tfoot" => collect_dom_table_rows(&ch, th, sup_sub, head, rows),
            "tr" => {
                let mut cells = Vec::new();
                let mut is_head_row = false;
                for cell in ch.children().iter() {
                    let Some(ct) = cell.tag_name() else { continue };
                    if ct == "th" || ct == "td" {
                        if ct == "th" {
                            is_head_row = true;
                        }
                        let bold = ct == "th";
                        let mut ib = InlineBuilder::new(Base { size: th.size, bold, color: th.text }, sup_sub);
                        for inner in cell.children().iter() {
                            feed_dom_inline(&inner, &mut ib);
                        }
                        cells.push(ib.finish(th));
                    }
                }
                if !cells.is_empty() {
                    if is_head_row && head.is_empty() {
                        *head = cells;
                    } else {
                        rows.push(cells);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_ranges_and_heading_offsets_match_source() {
        let src = "# 标题一\n\n第一段\n\n```rust\nlet x = 1;\n```\n\n## 标题二\n";
        let th = Theme::new(false, 16.0);
        let doc = parse(src, &th, &ParseOptions { sup_sub: false, front_matter: true });
        assert_eq!(doc.blocks.len(), 4);
        assert_eq!(doc.block_ranges.len(), 4);
        assert_eq!(&src[doc.block_ranges[0].clone()], "# 标题一\n");
        assert_eq!(&src[doc.block_ranges[1].clone()], "第一段\n");
        assert!(src[doc.block_ranges[2].clone()].starts_with("```rust"));
        assert_eq!(doc.headings.len(), 2);
        assert_eq!(doc.headings[0].byte_offset, 0);
        assert_eq!(doc.headings[1].byte_offset, src.find("## 标题二").unwrap());
    }

    #[test]
    fn details_container_and_dom_html_blocks_parse_cleanly() {
        let md = "<details open>\n<summary>展开示例</summary>\n\n内部 **Markdown** 段落\n\n- 列表项\n\n</details>\n\n<!-- 隐藏注释 -->\n\n<div align=\"center\">\n\n居中内容\n\n</div>\n\n<video src=\"demo.mp4\" title=\"演示\"></video>\n";
        let th = Theme::new(false, 16.0);
        let doc = parse(md, &th, &ParseOptions { sup_sub: false, front_matter: true });
        assert_eq!(doc.blocks.len(), doc.block_ranges.len());
        assert!(matches!(doc.blocks[0], Block::Details { default_open: true, .. }));
        if let Block::Details { summary, blocks, .. } = &doc.blocks[0] {
            assert_eq!(summary, "展开示例");
            assert_eq!(blocks.len(), 2);
        }
        assert!(matches!(doc.blocks[1], Block::Aligned { align: Align::Center, .. }));
        assert!(matches!(doc.blocks[2], Block::Media(_)));
    }
}
