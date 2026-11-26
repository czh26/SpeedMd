//! Markdown 解析：pulldown-cmark 事件流 → 预先排好版的文档结构（Doc）。
//!
//! 只在文本 / 主题 / 设置变化时重新解析，渲染时直接复用这里生成的 LayoutJob。

use eframe::egui::{text::LayoutJob, Align, Color32, FontFamily, FontId, Stroke, TextFormat};
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
    Image { url: String, alt: String },
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
    Html(String),
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

#[derive(Clone, Copy, Default, PartialEq)]
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
}

enum Atom {
    Text { s: String, st: Style, link: Option<Target> },
    /// `==` 高亮标记，结束时再两两配对
    Mark,
    Image { url: String, alt: String },
}

#[derive(Clone, Copy)]
struct Base {
    size: f32,
    bold: bool,
    color: Color32,
}

struct InlineBuilder {
    base: Base,
    sup_sub: bool,
    atoms: Vec<Atom>,
    plain: String,
    anchors: Vec<String>,
    style: Style,
    link: Option<Target>,
    image: Option<(String, String)>,
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
            link: None,
            image: None,
            pending: String::new(),
        }
    }

    fn text(&mut self, s: &str) {
        if let Some((_, alt)) = &mut self.image {
            alt.push_str(s);
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

    /// 处理普通文本：==高亮==、:emoji: 短码、GFM 裸链接
    fn process(&mut self, t: &str) {
        let b = t.as_bytes();
        let (mut i, mut start) = (0usize, 0usize);
        while i < b.len() {
            let c = b[i];
            if c == b'=' && b.get(i + 1) == Some(&b'=') {
                self.push_raw(&t[start..i], self.style, self.link.clone());
                self.atoms.push(Atom::Mark);
                i += 2;
                start = i;
                continue;
            }
            // 词内的 ^上标^ / ~下标~（pulldown-cmark 按 CommonMark 规则不处理词内的情况，如 H~2~O）
            if self.sup_sub && (c == b'^' || (c == b'~' && b.get(i + 1) != Some(&b'~') && (i == 0 || b[i - 1] != b'~'))) {
                let mut j = i + 1;
                while j < b.len() && b[j] != c && !b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < b.len() && b[j] == c && j > i + 1 && b.get(j + 1) != Some(&c) {
                    self.push_raw(&t[start..i], self.style, self.link.clone());
                    let st = Style { sup: c == b'^', sub: c == b'~', ..self.style };
                    self.push_raw(&t[i + 1..j], st, self.link.clone());
                    i = j + 1;
                    start = i;
                    continue;
                }
            }
            if c == b':'
                && let Some((end, emoji)) = emoji_at(t, i)
            {
                self.push_raw(&t[start..i], self.style, self.link.clone());
                self.push_raw(emoji, self.style, self.link.clone());
                i = end;
                start = i;
                continue;
            }
            if self.link.is_none()
                && (c == b'h' || c == b'w' || c == b'H' || c == b'W')
                && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
                && let Some((end, url)) = url_at(t, i)
            {
                self.push_raw(&t[start..i], self.style, None);
                self.push_raw(&t[i..end], self.style, Some(Target::Url(url)));
                i = end;
                start = i;
                continue;
            }
            i += 1;
        }
        self.push_raw(&t[start..], self.style, self.link.clone());
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

    fn html(&mut self, raw: &str) {
        let tag: String = raw.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        let tag = tag.as_str();
        match tag {
            "<br>" | "<br/>" => self.hard_break(),
            _ => {
                let (closing, name) = match tag.strip_prefix("</") {
                    Some(rest) => (true, rest.trim_end_matches('>')),
                    None => (false, tag.trim_start_matches('<').trim_end_matches('>')),
                };
                let on = !closing;
                match name {
                    "u" | "ins" => self.set(|s| s.underline = on),
                    "mark" => self.set(|s| s.highlight = on),
                    "sup" => self.set(|s| s.sup = on),
                    "sub" => self.set(|s| s.sub = on),
                    "s" | "del" | "strike" => self.set(|s| s.strike = on),
                    "b" | "strong" => self.set(|s| s.strong = on),
                    "i" | "em" => self.set(|s| s.emph = on),
                    "kbd" => self.set(|s| s.kbd = on),
                    "code" => self.set(|s| s.code = on),
                    _ => {} // 其他 HTML 标签忽略
                }
            }
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
            self.atoms.push(Atom::Image { url, alt });
        }
    }

    fn format(&self, th: &Theme, st: &Style, link: bool) -> TextFormat {
        let base = self.base;
        let mut size = base.size;
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
        let mut color = if link {
            th.link
        } else if st.code {
            th.code
        } else {
            base.color
        };
        let mut background = Color32::TRANSPARENT;
        if st.code || st.kbd {
            background = th.code_bg;
        }
        if st.highlight {
            background = th.mark_bg;
            if !link {
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
        if let Some((url, alt)) = self.image.take() {
            self.atoms.push(Atom::Image { url, alt });
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
                Atom::Image { url, alt } => {
                    flush_job(&mut job, &mut links, &mut chars, &mut chunks);
                    chunks.push(Chunk::Image { url, alt });
                }
            }
        }
        flush_job(&mut job, &mut links, &mut chars, &mut chunks);

        Inline { chunks, plain: self.plain, anchors: self.anchors }
    }
}

fn emoji_at(t: &str, i: usize) -> Option<(usize, &'static str)> {
    let b = t.as_bytes();
    let mut j = i + 1;
    while j < b.len() && j - i <= 40 {
        let c = b[j];
        if c == b':' {
            break;
        }
        if !(c.is_ascii_alphanumeric() || c == b'_' || c == b'+' || c == b'-') {
            return None;
        }
        j += 1;
    }
    if j >= b.len() || b[j] != b':' || j == i + 1 {
        return None;
    }
    emojis::get_by_shortcode(&t[i + 1..j]).map(|e| (j + 1, e.as_str()))
}

fn url_at(t: &str, i: usize) -> Option<(usize, String)> {
    let b = &t.as_bytes()[i..];
    let starts = |p: &str| b.len() >= p.len() && b[..p.len()].eq_ignore_ascii_case(p.as_bytes());
    let prefix = if starts("https://") {
        8
    } else if starts("http://") {
        7
    } else if starts("www.") {
        4
    } else {
        return None;
    };
    let mut end = 0;
    while end < b.len() {
        let c = b[end];
        if c.is_ascii_whitespace() || c == b'<' || c >= 0x80 {
            break;
        }
        end += 1;
    }
    while end > prefix && matches!(b[end - 1], b'.' | b',' | b':' | b';' | b'!' | b'?' | b'"' | b'\'' | b'*' | b'_' | b'~') {
        end -= 1;
    }
    if end > prefix && b[end - 1] == b')' {
        let opens = b[..end].iter().filter(|&&c| c == b'(').count();
        let closes = b[..end].iter().filter(|&&c| c == b')').count();
        if closes > opens {
            end -= 1;
        }
    }
    if end <= prefix {
        return None;
    }
    let url = &t[i..i + end];
    if prefix == 4 {
        if !url[4..].contains('.') {
            return None;
        }
        Some((i + end, format!("http://{url}")))
    } else {
        Some((i + end, url.to_string()))
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
                self.root_ranges.push(self.cur_top_range.take().unwrap_or(0..0));
            }
            Some(Frame::Quote(_, v) | Frame::Item(_, v) | Frame::Footnote(_, v)) => v.push(b),
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
        let inl = ib.finish(self.th);
        let t = inl.plain.trim().to_ascii_uppercase();
        if matches!(t.as_str(), "[TOC]" | "[[TOC]]" | "[[_TOC_]]") {
            self.push_block(Block::Toc);
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

    fn event(&mut self, ev: Event, range: Range<usize>, src: &str) {
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
                _ => self.ib().html(&t),
            },
            Event::InlineHtml(t) => self.ib().html(&t),
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
                        self.push_block(Block::Html(html));
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
}
