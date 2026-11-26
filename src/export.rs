//! Markdown → HTML 导出（完整带样式文档 & 正文片段 + Base64 图片/代码高亮/GFM 提示块/TOC）。

use crate::html_inline::detect_media_kind;
use crate::markdown;
use crate::preview::{alert_meta, is_supported_highlight_lang, resolve_path};
use crate::textproc::{self, Segment};
use crate::theme::ThemeId;
use eframe::egui::Color32;
use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ExportOptions {
    pub full_document: bool,
    pub theme: ThemeId,
    pub embed_images_base64: bool,
    pub base_dir: Option<PathBuf>,
    pub sup_sub: bool,
    pub fallback_title: String,
}

pub fn to_html_fragment(md: &str, base_dir: Option<&Path>, sup_sub: bool) -> String {
    export_markdown(
        md,
        &ExportOptions {
            full_document: false,
            theme: ThemeId::ZenHorizon,
            embed_images_base64: false,
            base_dir: base_dir.map(Path::to_path_buf),
            sup_sub,
            fallback_title: "MdPad 导出".into(),
        },
    )
}

pub fn export_markdown(md: &str, opts: &ExportOptions) -> String {
    let mut p_opts = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS;
    if opts.sup_sub {
        p_opts |= Options::ENABLE_SUPERSCRIPT | Options::ENABLE_SUBSCRIPT;
    }

    // 预扫描标题（计算去重后的 slug id 与首个一级标题）+ Front Matter title
    let (headings, fm_title, first_h1) = prescan_doc(md, p_opts);
    let doc_title = fm_title
        .or(first_h1)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| opts.fallback_title.clone());

    let hl_ctx = eframe::egui::Context::default();
    let hl_style = eframe::egui::Style::default();
    let code_theme = opts.theme.spec().code.to_code_theme(14.0);

    let mut out_events: Vec<Event<'static>> = Vec::new();
    let mut heading_idx = 0usize;
    let mut in_meta = false;
    let mut in_code: Option<String> = None;
    let mut code_buf = String::new();
    let mut in_image = false;
    let mut in_media_image = false;
    let mut link_depth = 0usize;
    let mut quote_is_alert: Vec<bool> = Vec::new();

    // 段落/行内缓冲，用于识别 `[TOC]` 与保证 `<mark>` 成对闭合
    let mut top_para_depth = 0usize;
    let mut in_para_capture = false;
    let mut para_buf: Vec<Event<'static>> = Vec::new();
    let mut para_plain = String::new();

    let push_target = |ev: Event<'static>,
                       in_para: bool,
                       para_buf: &mut Vec<Event<'static>>,
                       out: &mut Vec<Event<'static>>| {
        if in_para { para_buf.push(ev) } else { out.push(ev) }
    };

    for ev in Parser::new_ext(md, p_opts) {
        if in_meta {
            if matches!(ev, Event::End(TagEnd::MetadataBlock(_))) {
                in_meta = false;
            }
            continue;
        }
        if let Some(lang) = in_code.as_deref() {
            match ev {
                Event::Text(t) | Event::Code(t) => code_buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let lang_owned = lang.to_string();
                    in_code = None;
                    let raw_code = code_buf.strip_suffix('\n').unwrap_or(&code_buf);
                    let html = render_code_html(&lang_owned, raw_code, &hl_ctx, &hl_style, &code_theme);
                    code_buf.clear();
                    push_target(Event::Html(html.into()), in_para_capture, &mut para_buf, &mut out_events);
                }
                _ => {}
            }
            continue;
        }
        if in_media_image {
            if matches!(ev, Event::End(TagEnd::Image)) {
                in_media_image = false;
            }
            continue;
        }

        match ev {
            Event::Start(Tag::MetadataBlock(_)) => {
                in_meta = true;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code_buf.clear();
                in_code = Some(lang);
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let id = headings.get(heading_idx).map(|h| h.id.as_str()).unwrap_or("");
                heading_idx += 1;
                let html = format!("<h{} id=\"{}\">", level as u8, html_escape(id));
                out_events.push(Event::Html(html.into()));
            }
            Event::End(TagEnd::Heading(level)) => {
                out_events.push(Event::Html(format!("</h{}>\n", level as u8).into()));
            }
            Event::Start(Tag::Paragraph) => {
                top_para_depth += 1;
                if !in_para_capture && top_para_depth == 1 {
                    in_para_capture = true;
                    para_buf.clear();
                    para_plain.clear();
                } else {
                    push_target(Event::Start(Tag::Paragraph), in_para_capture, &mut para_buf, &mut out_events);
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if in_para_capture && top_para_depth == 1 {
                    in_para_capture = false;
                    let upper = para_plain.trim().to_ascii_uppercase();
                    if matches!(upper.as_str(), "[TOC]" | "[[TOC]]" | "[[_TOC_]]") {
                        out_events.push(Event::Html(render_toc_html(&headings).into()));
                    } else {
                        out_events.push(Event::Start(Tag::Paragraph));
                        emit_with_marks(std::mem::take(&mut para_buf), &mut out_events);
                        out_events.push(Event::End(TagEnd::Paragraph));
                    }
                    para_plain.clear();
                } else {
                    push_target(Event::End(TagEnd::Paragraph), in_para_capture, &mut para_buf, &mut out_events);
                }
                top_para_depth = top_para_depth.saturating_sub(1);
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                if let Some(k) = kind {
                    quote_is_alert.push(true);
                    let (icon, label) = alert_meta(k);
                    let cls = alert_class(k);
                    let h = format!(
                        "<div class=\"alert alert-{cls}\">\n<p class=\"alert-title\">{icon} {}</p>\n",
                        html_escape(label)
                    );
                    out_events.push(Event::Html(h.into()));
                } else {
                    quote_is_alert.push(false);
                    out_events.push(Event::Start(Tag::BlockQuote(None)));
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if quote_is_alert.pop() == Some(true) {
                    out_events.push(Event::Html("</div>\n".into()));
                } else {
                    out_events.push(Event::End(TagEnd::BlockQuote(None)));
                }
            }
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
                link_depth += 1;
                push_target(
                    Event::Start(Tag::Link {
                        link_type,
                        dest_url: dest_url.into_string().into(),
                        title: title.into_string().into(),
                        id: id.into_string().into(),
                    }),
                    in_para_capture,
                    &mut para_buf,
                    &mut out_events,
                );
            }
            Event::End(TagEnd::Link) => {
                link_depth = link_depth.saturating_sub(1);
                push_target(Event::End(TagEnd::Link), in_para_capture, &mut para_buf, &mut out_events);
            }
            Event::Start(Tag::Image { link_type, dest_url, title, id }) => {
                if let Some(kind) = detect_media_kind(&dest_url) {
                    in_media_image = true;
                    let tag = match kind {
                        crate::html_inline::MediaKind::Video => "video",
                        crate::html_inline::MediaKind::Audio => "audio",
                    };
                    let src = html_escape(&dest_url);
                    let h = format!("<{tag} controls src=\"{src}\"></{tag}>");
                    push_target(Event::Html(h.into()), in_para_capture, &mut para_buf, &mut out_events);
                } else {
                    in_image = true;
                    let final_url = if opts.embed_images_base64 {
                        embed_image_if_local(&dest_url, opts.base_dir.as_deref())
                    } else {
                        dest_url.into_string()
                    };
                    push_target(
                        Event::Start(Tag::Image {
                            link_type,
                            dest_url: final_url.into(),
                            title: title.into_string().into(),
                            id: id.into_string().into(),
                        }),
                        in_para_capture,
                        &mut para_buf,
                        &mut out_events,
                    );
                }
            }
            Event::End(TagEnd::Image) => {
                in_image = false;
                push_target(Event::End(TagEnd::Image), in_para_capture, &mut para_buf, &mut out_events);
            }
            Event::Text(t) => {
                if in_para_capture {
                    para_plain.push_str(&t);
                }
                if in_image {
                    push_target(Event::Text(t.into_string().into()), in_para_capture, &mut para_buf, &mut out_events);
                } else {
                    let allow_auto = link_depth == 0;
                    for seg in textproc::tokenize(&t, opts.sup_sub, allow_auto) {
                        match seg {
                            Segment::Plain(s) => {
                                push_target(Event::Text(s.to_owned().into()), in_para_capture, &mut para_buf, &mut out_events);
                            }
                            Segment::MarkToggle => {
                                push_target(
                                    Event::InlineHtml("__MDPAD_MARK_TOGGLE__".into()),
                                    in_para_capture,
                                    &mut para_buf,
                                    &mut out_events,
                                );
                            }
                            Segment::Sup(s) => {
                                let h = format!("<sup>{}</sup>", html_escape(s));
                                push_target(Event::InlineHtml(h.into()), in_para_capture, &mut para_buf, &mut out_events);
                            }
                            Segment::Sub(s) => {
                                let h = format!("<sub>{}</sub>", html_escape(s));
                                push_target(Event::InlineHtml(h.into()), in_para_capture, &mut para_buf, &mut out_events);
                            }
                            Segment::Emoji(e) => {
                                push_target(Event::Text(e.into()), in_para_capture, &mut para_buf, &mut out_events);
                            }
                            Segment::Autolink { text, url } => {
                                let h = format!("<a href=\"{}\">{}</a>", html_escape(&url), html_escape(text));
                                push_target(Event::InlineHtml(h.into()), in_para_capture, &mut para_buf, &mut out_events);
                            }
                        }
                    }
                }
            }
            Event::Html(h) => {
                let fixed = normalize_media_html(&h, opts);
                push_target(Event::Html(fixed.into()), in_para_capture, &mut para_buf, &mut out_events);
            }
            Event::InlineHtml(h) => {
                let fixed = normalize_media_html(&h, opts);
                push_target(Event::InlineHtml(fixed.into()), in_para_capture, &mut para_buf, &mut out_events);
            }
            other => {
                push_target(into_owned_event(other), in_para_capture, &mut para_buf, &mut out_events);
            }
        }
    }

    if !para_buf.is_empty() {
        emit_with_marks(para_buf, &mut out_events);
    }

    let mut body = String::new();
    pulldown_cmark::html::push_html(&mut body, out_events.into_iter());

    if !opts.full_document {
        return body;
    }

    wrap_full_document(&doc_title, &body, opts.theme)
}

#[derive(Clone)]
struct HeadingInfo {
    level: u8,
    text: String,
    id: String,
}

fn prescan_doc(md: &str, opts: Options) -> (Vec<HeadingInfo>, Option<String>, Option<String>) {
    let mut headings = Vec::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut fm_title = None;
    let mut first_h1 = None;

    let mut in_meta = false;
    let mut meta_buf = String::new();
    let mut heading_lvl: Option<(u8, Option<String>)> = None;
    let mut h_buf = String::new();

    for ev in Parser::new_ext(md, opts) {
        if in_meta {
            match ev {
                Event::Text(t) => meta_buf.push_str(&t),
                Event::End(TagEnd::MetadataBlock(_)) => {
                    in_meta = false;
                    if fm_title.is_none() {
                        fm_title = extract_front_matter_title(&meta_buf);
                    }
                    meta_buf.clear();
                }
                _ => {}
            }
            continue;
        }
        if let Some((lv, explicit_id)) = heading_lvl.as_ref() {
            match ev {
                Event::Text(t) | Event::Code(t) => h_buf.push_str(&t),
                Event::SoftBreak | Event::HardBreak => h_buf.push(' '),
                Event::End(TagEnd::Heading(_)) => {
                    let clean_text = expand_emojis_in_plain(h_buf.trim());
                    let base = explicit_id
                        .clone()
                        .unwrap_or_else(|| markdown::slugify(&clean_text));
                    let n = used.entry(base.clone()).or_insert(0);
                    let id = if *n == 0 { base } else { format!("{base}-{n}") };
                    *n += 1;
                    if *lv == 1 && first_h1.is_none() {
                        first_h1 = Some(clean_text.clone());
                    }
                    headings.push(HeadingInfo { level: *lv, text: clean_text, id });
                    heading_lvl = None;
                    h_buf.clear();
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::MetadataBlock(_)) => {
                in_meta = true;
                meta_buf.clear();
            }
            Event::Start(Tag::Heading { level, id, .. }) => {
                heading_lvl = Some((level as u8, id.map(|s| s.into_string())));
                h_buf.clear();
            }
            _ => {}
        }
    }
    (headings, fm_title, first_h1)
}

fn expand_emojis_in_plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for seg in textproc::tokenize(s, false, false) {
        match seg {
            Segment::Plain(p) => out.push_str(p),
            Segment::MarkToggle => out.push_str("=="),
            Segment::Emoji(e) => out.push_str(e),
            Segment::Sup(x) | Segment::Sub(x) => out.push_str(x),
            Segment::Autolink { text, .. } => out.push_str(text),
        }
    }
    out
}

fn extract_front_matter_title(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("title:")
            .or_else(|| trimmed.strip_prefix("title ="))
            .map(str::trim);
        if let Some(val) = rest {
            let unquoted = val
                .trim_matches('"')
                .trim_matches('\'')
                .trim();
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

fn emit_with_marks(buf: Vec<Event<'static>>, out: &mut Vec<Event<'static>>) {
    let total_toggles = buf
        .iter()
        .filter(|e| matches!(e, Event::InlineHtml(s) if s.as_ref() == "__MDPAD_MARK_TOGGLE__"))
        .count();
    let pairs = total_toggles / 2;
    let mut emitted_toggles = 0usize;
    let mut open = false;

    for ev in buf {
        if let Event::InlineHtml(ref s) = ev
            && s.as_ref() == "__MDPAD_MARK_TOGGLE__"
        {
            if emitted_toggles < pairs * 2 {
                emitted_toggles += 1;
                open = !open;
                out.push(Event::InlineHtml(if open { "<mark>" } else { "</mark>" }.into()));
            } else {
                out.push(Event::Text("==".into()));
            }
        } else {
            out.push(ev);
        }
    }
}

fn render_toc_html(headings: &[HeadingInfo]) -> String {
    if headings.is_empty() {
        return String::from("<nav class=\"toc\"></nav>\n");
    }
    let min_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
    let mut out = String::from("<nav class=\"toc\">\n<ul>\n");
    for h in headings {
        let indent = h.level.saturating_sub(min_level) as usize;
        out.push_str(&format!(
            "<li style=\"margin-left:{}em\"><a href=\"#{}\">{}</a></li>\n",
            indent * 12 / 10,
            html_escape(&h.id),
            html_escape(&h.text)
        ));
    }
    out.push_str("</ul>\n</nav>\n");
    out
}

fn render_code_html(
    lang: &str,
    code: &str,
    ctx: &eframe::egui::Context,
    style: &eframe::egui::Style,
    theme: &egui_extras::syntax_highlighting::CodeTheme,
) -> String {
    let class_attr = if lang.is_empty() {
        String::new()
    } else {
        format!(" class=\"language-{}\"", html_escape(lang))
    };
    if !is_supported_highlight_lang(lang) {
        return format!("<pre><code{class_attr}>{}</code></pre>\n", html_escape(code));
    }
    let job = egui_extras::syntax_highlighting::highlight(ctx, style, theme, code, lang);
    let mut out = format!("<pre><code{class_attr}>");
    for sec in &job.sections {
        let range = sec.byte_range.start.0..sec.byte_range.end.0;
        let slice = job.text.get(range).unwrap_or("");
        if slice.is_empty() {
            continue;
        }
        let c = sec.format.color;
        let escaped = html_escape(slice);
        if c != eframe::egui::Color32::TRANSPARENT {
            out.push_str(&format!(
                "<span style=\"color:#{:02x}{:02x}{:02x}\">{}</span>",
                c.r(),
                c.g(),
                c.b(),
                escaped
            ));
        } else {
            out.push_str(&escaped);
        }
    }
    out.push_str("</code></pre>\n");
    out
}

fn alert_class(k: BlockQuoteKind) -> &'static str {
    match k {
        BlockQuoteKind::Note => "note",
        BlockQuoteKind::Tip => "tip",
        BlockQuoteKind::Important => "important",
        BlockQuoteKind::Warning => "warning",
        BlockQuoteKind::Caution => "caution",
    }
}

fn normalize_media_html(raw: &str, opts: &ExportOptions) -> String {
    let mut s = raw.to_string();
    let lower = s.to_ascii_lowercase();
    for tag in ["<video", "<audio"] {
        if let Some(pos) = lower.find(tag)
            && !lower[pos..].contains("controls")
            && let Some(gt) = s[pos..].find('>')
        {
            s.insert_str(pos + gt, " controls");
            break;
        }
    }
    if opts.embed_images_base64 && lower.contains("<img") {
        // 尽量把行内 <img src="relative"> 也转换为 data URI
        if let Some(parsed) = crate::html_inline::parse_html_tag(&s)
            && parsed.name == "img"
            && let Some(src) = parsed.attr("src")
        {
            let data_url = embed_image_if_local(src, opts.base_dir.as_deref());
            if data_url != src {
                s = s.replacen(src, &data_url, 1);
            }
        }
    }
    s
}

fn embed_image_if_local(url: &str, base_dir: Option<&Path>) -> String {
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("data:")
        || url.starts_with("file://")
    {
        return url.to_string();
    }
    let decoded = markdown::percent_decode(url);
    let path = resolve_path(&decoded, base_dir);
    let Ok(bytes) = std::fs::read(&path) else {
        return url.to_string();
    };
    let mime = match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    format!("data:{mime};base64,{}", base64_encode(&bytes))
}

pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = *chunk.get(1).unwrap_or(&0) as usize;
        let b2 = *chunk.get(2).unwrap_or(&0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) & 63] as char);
        out.push(TABLE[(n >> 12) & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n & 63] as char } else { '=' });
    }
    out
}

fn into_owned_cow(c: CowStr<'_>) -> CowStr<'static> {
    c.into_string().into()
}

fn into_owned_tag(tag: Tag<'_>) -> Tag<'static> {
    match tag {
        Tag::Paragraph => Tag::Paragraph,
        Tag::Heading { level, id, classes, attrs } => Tag::Heading {
            level,
            id: id.map(into_owned_cow),
            classes: classes.into_iter().map(into_owned_cow).collect(),
            attrs: attrs.into_iter().map(|(k, v)| (into_owned_cow(k), v.map(into_owned_cow))).collect(),
        },
        Tag::BlockQuote(k) => Tag::BlockQuote(k),
        Tag::CodeBlock(kind) => Tag::CodeBlock(match kind {
            CodeBlockKind::Indented => CodeBlockKind::Indented,
            CodeBlockKind::Fenced(info) => CodeBlockKind::Fenced(into_owned_cow(info)),
        }),
        Tag::HtmlBlock => Tag::HtmlBlock,
        Tag::List(s) => Tag::List(s),
        Tag::Item => Tag::Item,
        Tag::FootnoteDefinition(l) => Tag::FootnoteDefinition(into_owned_cow(l)),
        Tag::DefinitionList => Tag::DefinitionList,
        Tag::DefinitionListTitle => Tag::DefinitionListTitle,
        Tag::DefinitionListDefinition => Tag::DefinitionListDefinition,
        Tag::Table(a) => Tag::Table(a),
        Tag::TableHead => Tag::TableHead,
        Tag::TableRow => Tag::TableRow,
        Tag::TableCell => Tag::TableCell,
        Tag::Emphasis => Tag::Emphasis,
        Tag::Strong => Tag::Strong,
        Tag::Strikethrough => Tag::Strikethrough,
        Tag::Superscript => Tag::Superscript,
        Tag::Subscript => Tag::Subscript,
        Tag::Link { link_type, dest_url, title, id } => Tag::Link {
            link_type,
            dest_url: into_owned_cow(dest_url),
            title: into_owned_cow(title),
            id: into_owned_cow(id),
        },
        Tag::Image { link_type, dest_url, title, id } => Tag::Image {
            link_type,
            dest_url: into_owned_cow(dest_url),
            title: into_owned_cow(title),
            id: into_owned_cow(id),
        },
        Tag::MetadataBlock(k) => Tag::MetadataBlock(k),
    }
}

fn into_owned_event(ev: Event<'_>) -> Event<'static> {
    match ev {
        Event::Start(t) => Event::Start(into_owned_tag(t)),
        Event::End(e) => Event::End(e),
        Event::Text(s) => Event::Text(into_owned_cow(s)),
        Event::Code(s) => Event::Code(into_owned_cow(s)),
        Event::InlineMath(s) => Event::InlineMath(into_owned_cow(s)),
        Event::DisplayMath(s) => Event::DisplayMath(into_owned_cow(s)),
        Event::Html(s) => Event::Html(into_owned_cow(s)),
        Event::InlineHtml(s) => Event::InlineHtml(into_owned_cow(s)),
        Event::FootnoteReference(s) => Event::FootnoteReference(into_owned_cow(s)),
        Event::SoftBreak => Event::SoftBreak,
        Event::HardBreak => Event::HardBreak,
        Event::Rule => Event::Rule,
        Event::TaskListMarker(b) => Event::TaskListMarker(b),
    }
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn css_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn wrap_full_document(title: &str, body: &str, theme: ThemeId) -> String {
    let css = &theme.spec().export_css;
    let bg = css_hex(css.bg);
    let fg = css_hex(css.fg);
    let heading = css_hex(css.heading);
    let muted = css_hex(css.muted);
    let border = css_hex(css.border);
    let panel = css_hex(css.panel);
    let code_bg = css_hex(css.code_bg);
    let link = css_hex(css.link);
    let mark_bg = css_hex(css.mark_bg);
    let mark_fg = css_hex(css.mark_fg);
    let callout_bg = css_hex(css.callout_bg);
    let alert_note = css_hex(css.alerts.note);
    let alert_tip = css_hex(css.alerts.tip);
    let alert_important = css_hex(css.alerts.important);
    let alert_warning = css_hex(css.alerts.warning);
    let alert_caution = css_hex(css.alerts.caution);
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<style>
  :root {{ color-scheme: {}; }}
  body {{ margin: 0; padding: 32px 20px 64px; background: {bg}; color: {fg}; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei UI", sans-serif; line-height: 1.7; }}
  .markdown-body {{ max-width: 860px; margin: 0 auto; }}
  h1, h2, h3, h4, h5, h6 {{ color: {heading}; margin: 1.4em 0 0.6em; line-height: 1.3; }}
  h1, h2 {{ border-bottom: 1px solid {border}; padding-bottom: 0.3em; }}
  a {{ color: {link}; text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
  p, ul, ol, blockquote, pre, table, details {{ margin: 0 0 1em; }}
  code {{ background: {code_bg}; padding: 0.15em 0.4em; border-radius: 4px; font-family: Consolas, "Cascadia Mono", monospace; font-size: 0.9em; }}
  pre {{ background: {code_bg}; border: 1px solid {border}; border-radius: 8px; padding: 14px 16px; overflow-x: auto; }}
  pre code {{ background: transparent; padding: 0; font-size: 0.9em; }}
  blockquote {{ border-left: 4px solid {border}; margin-left: 0; padding: 4px 16px; color: {muted}; }}
  mark {{ background: {mark_bg}; color: {mark_fg}; padding: 0 3px; border-radius: 3px; }}
  kbd {{ background: {panel}; border: 1px solid {border}; border-radius: 4px; padding: 1px 6px; font-family: Consolas, monospace; font-size: 0.88em; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th, td {{ border: 1px solid {border}; padding: 8px 12px; }}
  th {{ background: {panel}; }}
  img, video, audio {{ max-width: 100%; border-radius: 6px; }}
  details {{ background: {panel}; border: 1px solid {border}; border-radius: 6px; padding: 10px 14px; }}
  details summary {{ cursor: pointer; font-weight: 600; color: {heading}; }}
  nav.toc {{ background: {panel}; border: 1px solid {border}; border-radius: 6px; padding: 12px 18px; margin-bottom: 1.2em; }}
  nav.toc ul {{ list-style: none; padding: 0; margin: 0; }}
  .alert {{ border-left: 4px solid {alert_note}; background: {callout_bg}; padding: 10px 16px; border-radius: 0 8px 8px 0; margin-bottom: 1em; }}
  .alert-title {{ font-weight: 600; margin-bottom: 4px; }}
  .alert-note {{ border-left-color: {alert_note}; }}
  .alert-tip {{ border-left-color: {alert_tip}; }}
  .alert-important {{ border-left-color: {alert_important}; }}
  .alert-warning {{ border-left-color: {alert_warning}; }}
  .alert-caution {{ border-left-color: {alert_caution}; }}
</style>
</head>
<body>
<article class="markdown-body">
{body}
</article>
</body>
</html>
"#,
        html_escape(title),
        css.color_scheme,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn full_export_handles_front_matter_toc_mark_alerts_and_code() {
        let md = "---\ntitle: 测试导出标题\n---\n\n[TOC]\n\n# 第一节\n\n这是 ==高亮文本== 与 H~2~O。\n\n> [!NOTE]\n> 提示内容\n\n```rust\nlet x = 42;\n```\n";
        let html = export_markdown(
            md,
            &ExportOptions {
                full_document: true,
                theme: ThemeId::ZenHorizon,
                embed_images_base64: false,
                base_dir: None,
                sup_sub: true,
                fallback_title: "默认标题".into(),
            },
        );
        assert!(html.contains("<title>测试导出标题</title>"));
        assert!(!html.contains("title: 测试导出标题"));
        assert!(html.contains("<nav class=\"toc\">"));
        assert!(html.contains("<h1 id=\"第一节\">"));
        assert!(html.contains("<mark>高亮文本</mark>"));
        assert!(html.contains("<sub>2</sub>"));
        assert!(html.contains("alert-note"));
        assert!(html.contains("<span style=\"color:"));
    }

    /// 往返一致性测试：Markdown -> HTML 片段 -> Markdown 后比较 pulldown-cmark 事件骨架。
    fn event_skeleton(md: &str) -> Vec<String> {
        let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        let mut skel = Vec::new();
        let mut text_acc = String::new();
        let flush = |acc: &mut String, out: &mut Vec<String>| {
            let clean = acc.split_whitespace().collect::<Vec<_>>().join(" ");
            if !clean.is_empty() {
                out.push(format!("T:{clean}"));
            }
            acc.clear();
        };
        for ev in Parser::new_ext(md, opts) {
            match ev {
                Event::Text(s) | Event::Code(s) => text_acc.push_str(&s),
                Event::SoftBreak | Event::HardBreak => text_acc.push(' '),
                Event::Start(t) => {
                    flush(&mut text_acc, &mut skel);
                    let tag_name = match t {
                        Tag::Paragraph => "P".into(),
                        Tag::Heading { level, .. } => format!("H{}", level as u8),
                        Tag::List(Some(_)) => "OL".into(),
                        Tag::List(None) => "UL".into(),
                        Tag::Item => "LI".into(),
                        Tag::Table(_) => "TBL".into(),
                        Tag::TableHead => "TH".into(),
                        Tag::TableRow => "TR".into(),
                        Tag::TableCell => "TD".into(),
                        Tag::CodeBlock(_) => "CODE".into(),
                        Tag::Link { dest_url, .. } => format!("A({dest_url})"),
                        Tag::Strong => "B".into(),
                        Tag::Emphasis => "I".into(),
                        other => format!("{other:?}"),
                    };
                    skel.push(format!("+{tag_name}"));
                }
                Event::End(e) => {
                    flush(&mut text_acc, &mut skel);
                    let end_name = match e {
                        TagEnd::Paragraph => "P",
                        TagEnd::Heading(_) => "H",
                        TagEnd::List(_) => "L",
                        TagEnd::Item => "LI",
                        TagEnd::Table => "TBL",
                        TagEnd::TableHead => "TH",
                        TagEnd::TableRow => "TR",
                        TagEnd::TableCell => "TD",
                        TagEnd::CodeBlock => "CODE",
                        TagEnd::Link => "A",
                        TagEnd::Strong => "B",
                        TagEnd::Emphasis => "I",
                        _ => "OTHER",
                    };
                    skel.push(format!("-{end_name}"));
                }
                _ => {}
            }
        }
        flush(&mut text_acc, &mut skel);
        skel
    }

    #[test]
    fn markdown_html_markdown_roundtrip_preserves_structure() {
        let src = "\
# 主标题

包含 **加粗**、*斜体* 与 [示例链接](https://example.com) 的段落。

## 子列表与表格

- 列表项一
- 列表项二

| 列甲 | 列乙 |
| --- | --- |
| A | B |

```rust
fn demo() {}
```
";
        let html = to_html_fragment(src, None, false);
        let back_md = convert::html_to_markdown(&html);
        assert_eq!(event_skeleton(src), event_skeleton(&back_md));
    }
}
