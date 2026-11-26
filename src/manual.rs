//! 内置「语法说明书」：解析 `manual.md`、中英文同义词搜索、视口裁剪卡片渲染、一键复制与块级/行内插入。

use crate::editor::{self, TextPatch};
use crate::highlight;
use crate::markdown::{self, bold_family, Doc, ParseOptions, Theme};
use crate::preview::{self, PreviewOptions};
use eframe::egui::{
    self, vec2, Align, Color32, CursorIcon, FontFamily, FontId, Margin, RichText, Sense, Stroke, StrokeKind,
};
use image::{codecs::png::PngEncoder, ImageEncoder, Rgba, RgbaImage};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const RAW_MANUAL: &str = include_str!("manual.md");

pub const DEMO_LANDSCAPE_URI: &str = "bytes://mdpad/landscape.png";
pub const DEMO_BADGE_URI: &str = "bytes://mdpad/badge.png";
pub const DEMO_LOCAL_URI: &str = "bytes://mdpad/local-demo.png";

struct DemoImages {
    landscape: Vec<u8>,
    badge: Vec<u8>,
    local_demo: Vec<u8>,
}

static DEMO_IMAGES: OnceLock<DemoImages> = OnceLock::new();

fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = PngEncoder::new(&mut buf).write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8);
    buf
}

fn build_demo_images() -> DemoImages {
    // 1. 山川日落风景插画（360x160）
    let (w, h) = (360u32, 160u32);
    let mut land = RgbaImage::new(w, h);
    let sun_x = 265.0f32;
    let sun_y = 46.0f32;
    for y in 0..h {
        let fy = y as f32 / h as f32;
        for x in 0..w {
            let fx = x as f32 / w as f32;
            let mut r = (36.0 + fy * 175.0) as u8;
            let mut g = (72.0 + fy * 105.0) as u8;
            let mut b = (138.0 + (1.0 - fy) * 85.0) as u8;

            let dx = x as f32 - sun_x;
            let dy = y as f32 - sun_y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < 21.0 {
                r = 255;
                g = 232;
                b = 158;
            } else if dist < 44.0 {
                let k = (44.0 - dist) / 23.0 * 0.45;
                r = (r as f32 * (1.0 - k) + 255.0 * k) as u8;
                g = (g as f32 * (1.0 - k) + 210.0 * k) as u8;
                b = (b as f32 * (1.0 - k) + 140.0 * k) as u8;
            }

            // 远山
            let hill1 = 95.0 - 24.0 * (fx * 5.2 + 0.6).sin() - 12.0 * (fx * 11.0).cos();
            if (y as f32) > hill1 {
                r = 74;
                g = 96;
                b = 128;
            }
            // 近山
            let hill2 = 114.0 - 22.0 * (fx * 4.1 + 2.3).sin() - 10.0 * (fx * 9.5).sin();
            if (y as f32) > hill2 {
                r = 42;
                g = 62;
                b = 88;
            }
            // 底部水面
            if y >= 134 {
                let wave = ((x as f32 * 0.12 + y as f32 * 0.7).sin() * 8.0) as i16;
                r = (35 + wave).clamp(20, 70) as u8;
                g = (78 + wave).clamp(50, 110) as u8;
                b = (122 + wave).clamp(90, 155) as u8;
            }
            land.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    // 2. 功能徽章图标（180x110）
    let (bw, bh) = (180u32, 110u32);
    let mut badge = RgbaImage::new(bw, bh);
    for y in 0..bh {
        let fy = y as f32 / bh as f32;
        for x in 0..bw {
            let fx = x as f32 / bw as f32;
            let mut r = (24.0 + fx * 38.0) as u8;
            let mut g = (94.0 + fy * 60.0) as u8;
            let mut b = (196.0 - fy * 32.0) as u8;
            let cx = (x as f32 - bw as f32 * 0.5).abs();
            let cy = (y as f32 - bh as f32 * 0.5).abs();
            // 菱形高光与内框
            if (cx + cy * 1.35 - 32.0).abs() < 3.0 {
                r = 255;
                g = 255;
                b = 255;
            } else if cx < 6.0 && cy < 16.0 {
                r = 240;
                g = 248;
                b = 255;
            }
            if x < 2 || y < 2 || x + 2 >= bw || y + 2 >= bh {
                r = 160;
                g = 205;
                b = 255;
            }
            badge.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    // 3. 本地路径回退示意图（320x130）
    let (lw, lh) = (320u32, 130u32);
    let mut local = RgbaImage::new(lw, lh);
    for y in 0..lh {
        for x in 0..lw {
            let grid = x.is_multiple_of(20) || y.is_multiple_of(20);
            let (mut r, mut g, mut b) = if grid { (222, 230, 240) } else { (240, 244, 250) };
            let dx = x as i32 - (lw as i32 / 2);
            let dy = y as i32 - (lh as i32 / 2);
            if dx.abs() < 42 && dy.abs() < 28 && (dx.abs() >= 38 || dy.abs() >= 24) {
                r = 90;
                g = 128;
                b = 180;
            } else if dx.abs() < 34 && dy > 4 && dy < 20 && (dx + 34) / 2 > (20 - dy) {
                r = 110;
                g = 152;
                b = 205;
            } else if (dx - 16) * (dx - 16) + (dy + 10) * (dy + 10) < 36 {
                r = 235;
                g = 178;
                b = 72;
            }
            local.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    DemoImages {
        landscape: encode_png(&land),
        badge: encode_png(&badge),
        local_demo: encode_png(&local),
    }
}

/// 按需将内置演示 PNG 注册到 egui 图片缓存中（启动阶段零开销）。
pub fn ensure_demo_images(ctx: &egui::Context) {
    let reg_id = egui::Id::unique("mdpad_manual_demo_images_loaded");
    if ctx.data(|d| d.get_temp::<bool>(reg_id)).unwrap_or(false) {
        return;
    }
    let imgs = DEMO_IMAGES.get_or_init(build_demo_images);
    ctx.include_bytes(DEMO_LANDSCAPE_URI, imgs.landscape.clone());
    ctx.include_bytes(DEMO_BADGE_URI, imgs.badge.clone());
    ctx.include_bytes(DEMO_LOCAL_URI, imgs.local_demo.clone());
    ctx.data_mut(|d| d.insert_temp(reg_id, true));
}

// ---------------------------------------------------------------- 数据模型与解析

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingDep {
    SupSub,
    FrontMatter,
}

impl SettingDep {
    pub fn label(self) -> &'static str {
        match self {
            Self::SupSub => "上标 / 下标（^上标^、~下标~）",
            Self::FrontMatter => "预览中显示 YAML Front Matter",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertMode {
    Auto,
    Inline,
    Block,
}

#[derive(Clone, Debug)]
pub struct ManualEntry {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub setting: Option<SettingDep>,
    pub insert_mode: InsertMode,
    pub code: String,
    pub search_blob: String,
}

#[derive(Clone, Debug)]
pub struct ManualCategory {
    pub name: String,
    pub entry_indices: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct ManualData {
    pub categories: Vec<ManualCategory>,
    pub entries: Vec<ManualEntry>,
}

const SYNONYM_GROUPS: &[&[&str]] = &[
    &["粗体", "加粗", "黑体", "bold", "strong"],
    &["斜体", "倾斜", "italic", "emphasis", "em"],
    &["粗斜体", "加粗斜体", "bold italic"],
    &["删除线", "划线", "中划线", "strikethrough", "strike", "del"],
    &["高亮", "荧光笔", "底色", "highlight", "mark"],
    &["上标", "次方", "幂", "superscript", "sup"],
    &["下标", "化学式", "subscript", "sub"],
    &["下划线", "underline", "ins"],
    &["标题", "heading", "title", "h1", "h2", "h3", "h4", "h5", "h6"],
    &["换行", "硬换行", "软换行", "回车", "linebreak", "line break", "br"],
    &["列表", "无序", "有序", "待办", "任务", "list", "bullet", "todo", "checklist"],
    &["引用", "提示块", "警告框", "callout", "admonition", "alert", "blockquote", "quote"],
    &["代码", "代码块", "围栏", "反引号", "code", "codeblock", "fence", "backtick"],
    &["链接", "超链接", "网址", "锚点", "link", "url", "href", "anchor", "mailto"],
    &["图片", "图像", "插图", "image", "img", "picture"],
    &["表格", "table", "grid"],
    &["脚注", "目录", "大纲", "footnote", "toc", "outline"],
    &["分割线", "分隔线", "横线", "hr", "divider", "rule"],
    &["表情", "emoji", "短码", "shortcode"],
    &["元数据", "frontmatter", "front matter", "yaml", "toml"],
    &["折叠", "详情", "展开", "手风琴", "details", "summary", "collapse"],
    &["颜色", "字体", "居中", "对齐", "样式", "color", "font", "center", "align", "span", "style"],
    &["按键", "快捷键", "键盘", "kbd", "keyboard"],
    &["视频", "音频", "媒体", "video", "audio", "media", "poster"],
    &["转义", "实体", "反斜杠", "字面", "escape", "backslash", "entity"],
    &["常见问题", "问答", "faq", "为什么", "怎么"],
];

fn contains_term(haystack: &str, term: &str) -> bool {
    if term.is_ascii() {
        let bytes = haystack.as_bytes();
        let tb = term.as_bytes();
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(term) {
            let abs = start + pos;
            let left_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
            let right_idx = abs + tb.len();
            let right_ok = right_idx >= bytes.len() || !bytes[right_idx].is_ascii_alphanumeric();
            if left_ok && right_ok {
                return true;
            }
            start = abs + 1;
        }
        false
    } else {
        haystack.contains(term)
    }
}

fn expand_synonyms(meta_blob: &str, search_blob: &mut String) {
    let mut extras: Vec<&str> = Vec::new();
    for group in SYNONYM_GROUPS {
        if group.iter().any(|term| contains_term(meta_blob, term)) {
            extras.extend_from_slice(group);
        }
    }
    for term in extras {
        search_blob.push(' ');
        search_blob.push_str(term);
    }
}

struct DraftEntry {
    id: String,
    category_idx: usize,
    category: String,
    title: String,
    summary_lines: Vec<String>,
    note: Option<String>,
    tags: Vec<String>,
    keywords: Vec<String>,
    setting: Option<SettingDep>,
    insert_mode: InsertMode,
    code_lines: Vec<String>,
}

impl DraftEntry {
    fn new(category_idx: usize, category: String, title: String) -> Self {
        Self {
            id: String::new(),
            category_idx,
            category,
            title,
            summary_lines: Vec::new(),
            note: None,
            tags: Vec::new(),
            keywords: Vec::new(),
            setting: None,
            insert_mode: InsertMode::Auto,
            code_lines: Vec::new(),
        }
    }

    fn finish(self, fallback_idx: usize) -> ManualEntry {
        let id = if self.id.trim().is_empty() {
            format!("entry-{fallback_idx}")
        } else {
            self.id.trim().to_string()
        };
        let summary = self.summary_lines.join(" ").trim().to_string();
        let code = self.code_lines.join("\n");

        let mut meta = String::new();
        meta.push_str(&self.category);
        meta.push(' ');
        meta.push_str(&self.title);
        for t in &self.tags {
            meta.push(' ');
            meta.push_str(t);
        }
        for k in &self.keywords {
            meta.push(' ');
            meta.push_str(k);
        }
        let lower_meta = meta.to_lowercase();

        let mut blob = lower_meta.clone();
        blob.push(' ');
        blob.push_str(&summary);
        if let Some(n) = &self.note {
            blob.push(' ');
            blob.push_str(n);
        }
        blob.push(' ');
        blob.push_str(&code);
        let mut lower_blob = blob.to_lowercase();
        expand_synonyms(&lower_meta, &mut lower_blob);

        ManualEntry {
            id,
            title: self.title,
            summary,
            note: self.note,
            tags: self.tags,
            setting: self.setting,
            insert_mode: self.insert_mode,
            code,
            search_blob: lower_blob,
        }
    }
}

impl ManualData {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let norm = raw.replace("\r\n", "\n");
        let mut categories: Vec<ManualCategory> = Vec::new();
        let mut entries: Vec<ManualEntry> = Vec::new();
        let mut current: Option<DraftEntry> = None;
        let mut in_comment = false;
        let mut fence_len: Option<usize> = None;
        let mut seen_code = false;

        let flush_entry = |cur: &mut Option<DraftEntry>,
                           categories: &mut Vec<ManualCategory>,
                           entries: &mut Vec<ManualEntry>| {
            if let Some(d) = cur.take() {
                let idx = entries.len();
                let cat_idx = d.category_idx;
                entries.push(d.finish(idx));
                if let Some(cat) = categories.get_mut(cat_idx) {
                    cat.entry_indices.push(idx);
                }
            }
        };

        for line in norm.split('\n') {
            if let Some(flen) = fence_len {
                let t = line.trim_end_matches([' ', '\t']);
                if t.len() >= flen && t.bytes().all(|b| b == b'~') {
                    fence_len = None;
                    seen_code = true;
                } else if let Some(cur) = &mut current {
                    cur.code_lines.push(line.to_string());
                }
                continue;
            }

            if in_comment {
                if line.contains("-->") {
                    in_comment = false;
                }
                continue;
            }

            let trimmed = line.trim();
            if current.is_none() && trimmed.starts_with("<!--") {
                if !trimmed.contains("-->") {
                    in_comment = true;
                }
                continue;
            }

            if let Some(cat_name) = line.strip_prefix("# ") {
                flush_entry(&mut current, &mut categories, &mut entries);
                let name = cat_name.trim().to_string();
                if !name.is_empty() {
                    categories.push(ManualCategory { name, entry_indices: Vec::new() });
                }
                seen_code = false;
                continue;
            }

            if let Some(title) = line.strip_prefix("## ") {
                flush_entry(&mut current, &mut categories, &mut entries);
                if categories.is_empty() {
                    categories.push(ManualCategory { name: "通用".into(), entry_indices: Vec::new() });
                }
                let cat_idx = categories.len() - 1;
                let cat_name = categories[cat_idx].name.clone();
                current = Some(DraftEntry::new(cat_idx, cat_name, title.trim().to_string()));
                seen_code = false;
                continue;
            }

            let Some(cur) = &mut current else { continue };

            if trimmed.starts_with("~~~~") {
                let n = trimmed.bytes().take_while(|&b| b == b'~').count();
                if n >= 4 && !seen_code {
                    fence_len = Some(n);
                    continue;
                }
            }

            if trimmed.is_empty() || seen_code {
                continue;
            }

            if cur.summary_lines.is_empty() {
                if let Some(v) = line.strip_prefix("id:") {
                    cur.id = v.trim().to_string();
                    continue;
                }
                if let Some(v) = line.strip_prefix("keywords:") {
                    for kw in v.split([',', '，']) {
                        let k = kw.trim();
                        if !k.is_empty() {
                            cur.keywords.push(k.to_string());
                        }
                    }
                    continue;
                }
                if let Some(v) = line.strip_prefix("tags:") {
                    for tg in v.split('|') {
                        let t = tg.trim();
                        if !t.is_empty() {
                            cur.tags.push(t.to_string());
                        }
                    }
                    continue;
                }
                if let Some(v) = line.strip_prefix("setting:") {
                    cur.setting = match v.trim() {
                        "sup_sub" => Some(SettingDep::SupSub),
                        "front_matter" => Some(SettingDep::FrontMatter),
                        other => return Err(format!("条目「{}」存在未知 setting: {other}", cur.title)),
                    };
                    continue;
                }
                if let Some(v) = line.strip_prefix("mode:") {
                    cur.insert_mode = match v.trim() {
                        "inline" => InsertMode::Inline,
                        "block" => InsertMode::Block,
                        "auto" => InsertMode::Auto,
                        other => return Err(format!("条目「{}」存在未知 mode: {other}", cur.title)),
                    };
                    continue;
                }
                if let Some(v) = line.strip_prefix("note:") {
                    let t = v.trim();
                    if !t.is_empty() {
                        cur.note = Some(t.to_string());
                    }
                    continue;
                }
            }

            cur.summary_lines.push(trimmed.to_string());
        }

        flush_entry(&mut current, &mut categories, &mut entries);

        let mut seen_ids = HashSet::new();
        for e in &entries {
            if e.title.trim().is_empty() {
                return Err(format!("存在空标题条目 id={}", e.id));
            }
            if e.code.trim().is_empty() {
                return Err(format!("条目「{}」示例源码为空", e.title));
            }
            if !seen_ids.insert(e.id.as_str()) {
                return Err(format!("存在重复条目 id: {}", e.id));
            }
        }

        Ok(Self { categories, entries })
    }

    pub fn matches_query(&self, entry_idx: usize, query: &str) -> bool {
        let q = query.trim();
        if q.is_empty() {
            return true;
        }
        let Some(entry) = self.entries.get(entry_idx) else { return false };
        let lower = q.to_lowercase();
        lower.split_whitespace().all(|term| entry.search_blob.contains(term))
    }
}

// ---------------------------------------------------------------- 插入补空行规则

pub fn is_block_snippet(code: &str, mode: InsertMode) -> bool {
    match mode {
        InsertMode::Block => true,
        InsertMode::Inline => false,
        InsertMode::Auto => {
            if code.contains('\n') {
                return true;
            }
            let t = code.trim_start();
            t.starts_with('#')
                || t.starts_with('>')
                || t.starts_with("- ")
                || t.starts_with("* ")
                || t.starts_with("+ ")
                || t.starts_with("```")
                || t.starts_with("~~~")
                || t.starts_with("---")
                || t.starts_with("***")
                || t.starts_with("___")
                || t.starts_with("+++")
                || t.starts_with("<!--")
                || t.eq_ignore_ascii_case("[TOC]")
                || code.starts_with("    ")
                || t.starts_with("<details")
                || t.starts_with("<div")
                || t.starts_with("<center")
                || t.starts_with("<video")
                || t.starts_with("<audio")
        }
    }
}

/// 构造将手册示例插入目标文档光标/选区位置的 `TextPatch`（块级语法自动补齐前后空行）。
pub fn build_insert_patch(text: &str, sel_chars: Range<usize>, code: &str, mode: InsertMode) -> TextPatch {
    let max_c = text.chars().count();
    let c0 = sel_chars.start.min(max_c);
    let c1 = sel_chars.end.min(max_c);
    let b0 = editor::char_to_byte(text, c0);
    let b1 = editor::char_to_byte(text, c1);

    let block = is_block_snippet(code, mode);
    let (prefix, suffix) = if block {
        let before = &text[..b0];
        let after = &text[b1..];
        let pre = if before.is_empty() || before.ends_with("\n\n") {
            ""
        } else if before.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let suf = if after.is_empty() || after.starts_with("\n\n") {
            ""
        } else if after.starts_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        (pre, suf)
    } else {
        ("", "")
    };

    let insert = format!("{prefix}{code}{suffix}");
    let cur = c0 + prefix.chars().count() + code.chars().count();
    TextPatch {
        replace_bytes: b0..b1,
        insert,
        new_sel_chars: cur..cur,
    }
}

// ---------------------------------------------------------------- UI 状态与渲染

#[derive(Default)]
struct CardCache {
    source_key: u64,
    source_job: Option<Arc<egui::text::LayoutJob>>,
    preview_key: u64,
    preview_doc: Option<(Arc<Doc>, Theme)>,
}

pub struct ManualViewCfg<'a> {
    pub editor_font_size: f32,
    pub editor_font_family: &'a str,
    pub preview_font_size: f32,
    pub line_height: f32,
    pub para_spacing: f32,
    pub sup_sub: bool,
    pub front_matter: bool,
    pub code_line_numbers: bool,
    pub max_content_width: f32,
    pub allow_esc_clear: bool,
}

pub enum ManualAction {
    Insert { code: String, mode: InsertMode },
    EnableSetting(SettingDep),
}

#[derive(Default)]
pub struct ManualState {
    data: Option<Arc<ManualData>>,
    caches: Vec<CardCache>,
    pub query: String,
    pub focus_search: bool,
    active_category: usize,
    pending_scroll_cat: Option<usize>,
    pending_scroll_entry: Option<String>,
    preview_scroll_to: Option<(usize, String)>,
    copied_idx: Option<(usize, Instant)>,
    card_heights: Vec<Option<f32>>,
    heights_width: f32,
    heights_filter_key: u64,
}

impl ManualState {
    pub fn ensure_loaded(&mut self) {
        if self.data.is_some() {
            return;
        }
        let parsed = ManualData::parse(RAW_MANUAL).unwrap_or_else(|e| ManualData {
            categories: vec![ManualCategory { name: "解析错误".into(), entry_indices: vec![0] }],
            entries: vec![ManualEntry {
                id: "parse-error".into(),
                title: "manual.md 解析失败".into(),
                summary: e,
                note: None,
                tags: Vec::new(),
                setting: None,
                insert_mode: InsertMode::Block,
                code: String::new(),
                search_blob: String::new(),
            }],
        });
        self.caches = (0..parsed.entries.len()).map(|_| CardCache::default()).collect();
        self.card_heights = vec![None; parsed.entries.len()];
        self.data = Some(Arc::new(parsed));
    }

    pub fn entry_count(&self) -> usize {
        self.data.as_ref().map_or(0, |d| d.entries.len())
    }

    pub fn category_count(&self) -> usize {
        self.data.as_ref().map_or(0, |d| d.categories.len())
    }

    pub fn reset_scroll_if_idle(&mut self) {
        if self.pending_scroll_cat.is_none() && self.pending_scroll_entry.is_none() && self.query.trim().is_empty() {
            self.pending_scroll_cat = Some(0);
            self.active_category = 0;
        }
    }

    pub fn scroll_to_category_name(&mut self, name: &str) {
        self.ensure_loaded();
        if let Some(data) = &self.data
            && let Some(idx) = data.categories.iter().position(|c| c.name == name)
        {
            self.pending_scroll_cat = Some(idx);
            self.active_category = idx;
        }
    }

    pub fn scroll_to_entry_id(&mut self, id: &str) {
        self.ensure_loaded();
        if let Some(data) = &self.data
            && let Some((cat_idx, _)) = data
                .categories
                .iter()
                .enumerate()
                .find(|(_, c)| c.entry_indices.iter().any(|&ei| data.entries[ei].id == id))
        {
            self.pending_scroll_entry = Some(id.to_string());
            self.active_category = cat_idx;
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, cfg: &ManualViewCfg<'_>) -> Option<ManualAction> {
        self.ensure_loaded();
        ensure_demo_images(ui.ctx());
        let data = self.data.clone()?;
        let dark = ui.visuals().dark_mode;
        let mut action: Option<ManualAction> = None;

        // 清空过期复制提示
        if let Some((_, at)) = self.copied_idx
            && at.elapsed() >= Duration::from_millis(1500)
        {
            self.copied_idx = None;
        }

        // 筛选匹配条目
        let matching: Vec<bool> = (0..data.entries.len()).map(|i| data.matches_query(i, &self.query)).collect();
        let match_count = matching.iter().filter(|&&m| m).count();

        // 1. 顶部搜索栏
        let search_id = egui::Id::unique("manual_search_input");
        let esc_pressed = cfg.allow_esc_clear && ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc_pressed && !self.query.is_empty() {
            self.query.clear();
            ui.ctx().request_repaint();
        }

        let bar_bg = if dark { Color32::from_rgb(0x20, 0x24, 0x2a) } else { Color32::from_rgb(0xf6, 0xf8, 0xfa) };
        let border_col = ui.visuals().widgets.noninteractive.bg_stroke.color;
        egui::Frame::new()
            .fill(bar_bg)
            .inner_margin(Margin::symmetric(18, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📖 语法说明书").font(FontId::new(15.0, bold_family())));
                    ui.add_space(12.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(search_id)
                            .desired_width(300.0)
                            .hint_text("搜索语法（支持中英文：粗体 / bold / 表格 / details…）"),
                    );
                    if self.focus_search {
                        resp.request_focus();
                        self.focus_search = false;
                    }
                    if !self.query.is_empty() && ui.button("清空 (Esc)").clicked() {
                        self.query.clear();
                        resp.request_focus();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let status = if self.query.trim().is_empty() {
                            format!("共 {} 个分类 · {} 条语法", data.categories.len(), data.entries.len())
                        } else {
                            format!("匹配 {match_count} / {} 条语法", data.entries.len())
                        };
                        ui.weak(status);
                    });
                });
            });
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.cursor().top(),
            Stroke::new(1.0, border_col),
        );

        let avail = ui.available_rect_before_wrap();
        let left_w = 205.0f32.min((avail.width() * 0.28).max(150.0));

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

            // 2. 左侧分类目录
            ui.allocate_ui_with_layout(vec2(left_w, avail.height()), egui::Layout::top_down(Align::Min), |ui| {
                egui::Frame::new()
                    .fill(if dark { Color32::from_rgb(0x1b, 0x1e, 0x24) } else { Color32::from_rgb(0xf9, 0xfa, 0xfb) })
                    .inner_margin(Margin::symmetric(10, 12))
                    .show(ui, |ui| {
                        ui.set_min_height((avail.height() - 24.0).max(0.0));
                        egui::ScrollArea::vertical().id_salt("manual_toc_scroll").auto_shrink(false).show(ui, |ui| {
                            ui.label(RichText::new("语法分类").size(12.5).color(ui.visuals().weak_text_color()));
                            ui.add_space(6.0);
                            for (cat_idx, cat) in data.categories.iter().enumerate() {
                                let count = cat.entry_indices.iter().filter(|&&i| matching[i]).count();
                                if !self.query.trim().is_empty() && count == 0 {
                                    continue;
                                }
                                let active = cat_idx == self.active_category;
                                let (rect, btn) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 26.0),
                                    Sense::click(),
                                );
                                if btn.hovered() {
                                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                }
                                if active {
                                    ui.painter().rect_filled(
                                        rect,
                                        4.0,
                                        if dark { Color32::from_rgb(0x28, 0x35, 0x48) } else { Color32::from_rgb(0xe8, 0xf2, 0xff) },
                                    );
                                    ui.painter().rect_stroke(
                                        rect,
                                        4.0,
                                        Stroke::new(
                                            1.0,
                                            if dark { Color32::from_rgb(0x38, 0x8b, 0xfd) } else { Color32::from_rgb(0x9b, 0xc3, 0xf5) },
                                        ),
                                        StrokeKind::Inside,
                                    );
                                } else if btn.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        4.0,
                                        if dark { Color32::from_rgb(0x26, 0x2b, 0x33) } else { Color32::from_rgb(0xed, 0xf1, 0xf5) },
                                    );
                                }
                                let text_col = if active {
                                    if dark { Color32::WHITE } else { Color32::from_rgb(0x09, 0x69, 0xda) }
                                } else {
                                    ui.visuals().text_color()
                                };
                                let font = if active { FontId::new(13.5, bold_family()) } else { FontId::proportional(13.5) };
                                ui.painter().text(
                                    eframe::egui::pos2(rect.left() + 8.0, rect.center().y),
                                    eframe::egui::Align2::LEFT_CENTER,
                                    &cat.name,
                                    font,
                                    text_col,
                                );
                                ui.painter().text(
                                    eframe::egui::pos2(rect.right() - 8.0, rect.center().y),
                                    eframe::egui::Align2::RIGHT_CENTER,
                                    format!("{count}"),
                                    FontId::proportional(12.0),
                                    ui.visuals().weak_text_color(),
                                );
                                if btn.hovered() {
                                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                }
                                if btn.clicked() {
                                    self.pending_scroll_cat = Some(cat_idx);
                                    self.active_category = cat_idx;
                                }
                            }
                        });
                    });
            });

            let div_x = ui.cursor().left();
            ui.painter().vline(div_x, avail.y_range(), Stroke::new(1.0, border_col));

            // 3. 右侧按分类排列的语法卡片（带高度缓存与视口裁剪）
            let right_w = (ui.available_width() - 1.0).max(200.0);
            ui.allocate_ui_with_layout(vec2(right_w, avail.height()), egui::Layout::top_down(Align::Min), |ui| {
                if match_count == 0 {
                    ui.add_space(48.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("没有找到匹配的语法条目").size(16.0).strong());
                        ui.add_space(6.0);
                        ui.weak("试试搜索关键词：粗体、bold、换行、表格、details、上标、代码…");
                        ui.add_space(10.0);
                        if ui.button("清空搜索条件").clicked() {
                            self.query.clear();
                        }
                    });
                    return;
                }

                let layout_key = hash_layout_key(&self.query, dark, cfg);
                let had_pending_cat = self.pending_scroll_cat.is_some();
                let had_pending_entry = self.pending_scroll_entry.is_some();
                let force = had_pending_cat || had_pending_entry || self.preview_scroll_to.is_some();
                let mut top_cat: Option<usize> = None;

                egui::ScrollArea::vertical()
                    .id_salt("manual_cards_scroll")
                    .auto_shrink(false)
                    .show_viewport(ui, |ui, viewport| {
                        let origin = ui.max_rect().top();
                        egui::Frame::new()
                            .inner_margin(Margin { left: 22, right: 22, top: 16, bottom: 48 })
                            .show(ui, |ui| {
                                let avail_w = ui.available_width();
                                let content_w = if cfg.max_content_width > 0.0 {
                                    avail_w.min(cfg.max_content_width + 60.0)
                                } else {
                                    avail_w
                                };
                                let side_pad = ((avail_w - content_w) * 0.5).max(0.0);

                                if self.heights_filter_key != layout_key || (content_w - self.heights_width).abs() > 0.5 {
                                    self.card_heights.fill(None);
                                    self.heights_width = content_w;
                                    self.heights_filter_key = layout_key;
                                }

                                ui.horizontal(|ui| {
                                    if side_pad > 0.0 {
                                        ui.add_space(side_pad);
                                    }
                                    ui.vertical(|ui| {
                                        ui.set_width(content_w);
                                        for (cat_idx, cat) in data.categories.iter().enumerate() {
                                            let vis_entries: Vec<usize> =
                                                cat.entry_indices.iter().copied().filter(|&i| matching[i]).collect();
                                            if vis_entries.is_empty() {
                                                continue;
                                            }
                                            if cat_idx > 0 {
                                                ui.add_space(10.0);
                                            }
                                            let hdr_resp = ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(&cat.name)
                                                        .font(FontId::new(19.0, bold_family()))
                                                        .color(ui.visuals().strong_text_color()),
                                                );
                                                ui.add_space(8.0);
                                                ui.weak(format!("{} 条", vis_entries.len()));
                                            });
                                            let hdr_rect = hdr_resp.response.rect;
                                            if self.pending_scroll_cat == Some(cat_idx) {
                                                ui.scroll_to_rect(hdr_rect, Some(Align::TOP));
                                                self.pending_scroll_cat = None;
                                            }
                                            if top_cat.is_none() && (hdr_rect.bottom() - origin) >= viewport.top() + 8.0 {
                                                top_cat = Some(cat_idx);
                                            }
                                            ui.add_space(6.0);
                                            ui.painter().hline(
                                                ui.available_rect_before_wrap().x_range(),
                                                ui.cursor().top(),
                                                Stroke::new(1.0, border_col),
                                            );
                                            ui.add_space(12.0);

                                            for &entry_idx in &vis_entries {
                                                let y = ui.cursor().top() - origin;
                                                if !force
                                                    && let Some(h) = self.card_heights[entry_idx]
                                                    && (y + h < viewport.top() - 600.0 || y > viewport.bottom() + 600.0)
                                                {
                                                    ui.allocate_space(vec2(content_w, h));
                                                    if top_cat.is_none() && y + h >= viewport.top() + 24.0 {
                                                        top_cat = Some(cat_idx);
                                                    }
                                                    ui.add_space(12.0);
                                                    continue;
                                                }

                                                let start_y = ui.cursor().top();
                                                let (card_act, card_rect) =
                                                    self.render_card(ui, &data.entries[entry_idx], entry_idx, dark, cfg);
                                                if let Some(act) = card_act {
                                                    action = Some(act);
                                                }
                                                if self.pending_scroll_entry.as_deref() == Some(data.entries[entry_idx].id.as_str()) {
                                                    ui.scroll_to_rect(card_rect, Some(Align::TOP));
                                                    self.pending_scroll_entry = None;
                                                }
                                                let end_y = ui.cursor().top();
                                                self.card_heights[entry_idx] = Some((end_y - start_y).max(1.0));
                                                if top_cat.is_none() && (end_y - origin) >= viewport.top() + 24.0 {
                                                    top_cat = Some(cat_idx);
                                                }
                                                ui.add_space(12.0);
                                            }
                                        }
                                    });
                                });
                            });
                    });

                if !had_pending_cat
                    && !had_pending_entry
                    && let Some(c) = top_cat
                {
                    self.active_category = c;
                }
            });
        });

        action
    }

    fn render_card(
        &mut self,
        ui: &mut egui::Ui,
        entry: &ManualEntry,
        idx: usize,
        dark: bool,
        cfg: &ManualViewCfg<'_>,
    ) -> (Option<ManualAction>, egui::Rect) {
        let mut action: Option<ManualAction> = None;
        let card_bg = if dark { Color32::from_rgb(0x21, 0x25, 0x2b) } else { Color32::WHITE };
        let src_bg = if dark { Color32::from_rgb(0x19, 0x1c, 0x21) } else { Color32::from_rgb(0xf6, 0xf8, 0xfa) };
        let prev_bg = if dark { Color32::from_rgb(0x1d, 0x20, 0x26) } else { Color32::from_rgb(0xfc, 0xfd, 0xfe) };
        let border = if dark { Color32::from_rgb(0x3a, 0x41, 0x4c) } else { Color32::from_rgb(0xd8, 0xde, 0xe4) };
        let accent = if dark { Color32::from_rgb(0x58, 0xa6, 0xff) } else { Color32::from_rgb(0x09, 0x69, 0xda) };

        // 按需更新该卡片源码高亮与效果预览缓存
        let src_font = FontId::new(cfg.editor_font_size.clamp(12.0, 22.0), FontFamily::Name("editor".into()));
        let src_key = hash_pair(&(dark, cfg.editor_font_size.to_bits(), cfg.editor_font_family));
        let prev_key = hash_pair(&(
            dark,
            cfg.preview_font_size.to_bits(),
            cfg.line_height.to_bits(),
            cfg.para_spacing.to_bits(),
            cfg.sup_sub,
            cfg.front_matter,
        ));

        let cache = &mut self.caches[idx];
        if cache.source_job.is_none() || cache.source_key != src_key {
            cache.source_job = Some(Arc::new(highlight::highlight_manual_source(&entry.code, dark, &src_font)));
            cache.source_key = src_key;
        }
        if cache.preview_doc.is_none() || cache.preview_key != prev_key {
            let th = Theme::with_spacing(dark, cfg.preview_font_size, cfg.line_height, cfg.para_spacing);
            let parsed = markdown::parse(
                &entry.code,
                &th,
                &ParseOptions { sup_sub: cfg.sup_sub, front_matter: cfg.front_matter },
            );
            cache.preview_doc = Some((Arc::new(parsed), th));
            cache.preview_key = prev_key;
        }

        let src_job = cache.source_job.as_ref().unwrap().clone();
        let (prev_doc, prev_th) = cache.preview_doc.as_ref().unwrap().clone();

        let resp = egui::Frame::new()
            .fill(card_bg)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                // 1. 名称、标签与操作按钮
                ui.horizontal_top(|ui| {
                    let left_w = (ui.available_width() - 175.0).max(120.0);
                    ui.allocate_ui(vec2(left_w, 0.0), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&entry.title)
                                    .font(FontId::new(15.5, bold_family()))
                                    .color(ui.visuals().strong_text_color()),
                            );
                            for tag in &entry.tags {
                                draw_tag_pill(ui, tag, dark);
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui
                            .button(RichText::new("插入到文档").color(accent))
                            .on_hover_text("插入到最近激活的文档光标处（可按 Ctrl+Z 撤销）")
                            .clicked()
                        {
                            action = Some(ManualAction::Insert {
                                code: entry.code.clone(),
                                mode: entry.insert_mode,
                            });
                        }

                        let copied = self
                            .copied_idx
                            .is_some_and(|(ci, at)| ci == idx && at.elapsed() < Duration::from_millis(1500));
                        let copy_label = if copied { "✓ 已复制" } else { "复制" };
                        let copy_btn = if copied {
                            egui::Button::new(RichText::new(copy_label).color(Color32::from_rgb(0x2d, 0xa4, 0x4e)))
                        } else {
                            egui::Button::new(copy_label)
                        };
                        if ui.add(copy_btn).clicked() {
                            ui.ctx().copy_text(entry.code.clone());
                            self.copied_idx = Some((idx, Instant::now()));
                            ui.ctx().request_repaint_after(Duration::from_millis(1550));
                        }
                        if copied {
                            ui.ctx().request_repaint_after(Duration::from_millis(300));
                        }
                    });
                });

                ui.add_space(6.0);

                // 2. 说明与注意事项
                ui.label(&entry.summary);
                if let Some(note) = &entry.note {
                    ui.add_space(2.0);
                    ui.label(RichText::new(format!("注意：{note}")).size(12.5).color(ui.visuals().weak_text_color()));
                }

                // 3. 依赖设置的状态与一键开启按钮
                if let Some(dep) = entry.setting {
                    ui.add_space(6.0);
                    let enabled = match dep {
                        SettingDep::SupSub => cfg.sup_sub,
                        SettingDep::FrontMatter => cfg.front_matter,
                    };
                    let (bg, col, msg) = if enabled {
                        let c = Color32::from_rgb(0x2d, 0xa4, 0x4e);
                        (c.gamma_multiply(0.10), c, format!("✓ 已开启「{}」", dep.label()))
                    } else {
                        let c = Color32::from_rgb(0xd2, 0x99, 0x22);
                        (
                            c.gamma_multiply(0.12),
                            c,
                            format!("⚠ 当前未开启「{}」（下方展示当前设置下的真实渲染）", dep.label()),
                        )
                    };
                    egui::Frame::new()
                        .fill(bg)
                        .stroke(Stroke::new(1.0, col.gamma_multiply(0.6)))
                        .corner_radius(5)
                        .inner_margin(Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(msg).size(12.5).color(col));
                                if !enabled && ui.small_button("立即开启").clicked() {
                                    action = Some(ManualAction::EnableSetting(dep));
                                }
                            });
                        });
                }

                ui.add_space(8.0);

                // 4. 源码框（编辑器同款高亮 + 横向滚动 + 淡色行尾空格标志）
                egui::Frame::new()
                    .fill(src_bg)
                    .stroke(Stroke::new(1.0, border))
                    .corner_radius(5)
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("源码").size(11.5).color(ui.visuals().weak_text_color()));
                        });
                        ui.add_space(3.0);
                        let mut job = (*src_job).clone();
                        job.wrap.max_width = f32::INFINITY;
                        egui::ScrollArea::horizontal()
                            .id_salt(("manual_src_scroll", idx))
                            .show(ui, |ui| {
                                ui.add(egui::Label::new(job).selectable(true));
                            });
                    });

                ui.add_space(6.0);

                // 5. 效果预览框（复用主预览完全相同的解析和渲染逻辑）
                egui::Frame::new()
                    .fill(prev_bg)
                    .stroke(Stroke::new(1.0, border))
                    .corner_radius(5)
                    .inner_margin(Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("效果预览").size(11.5).color(ui.visuals().weak_text_color()));
                        ui.add_space(4.0);
                        let mut card_scroll = self
                            .preview_scroll_to
                            .as_ref()
                            .and_then(|(ci, a)| (*ci == idx).then(|| a.clone()));
                        ui.push_id(("manual_card_prev", idx), |ui| {
                            preview::render_doc_inline(
                                ui,
                                10_000 + idx as u64,
                                &prev_doc,
                                &prev_th,
                                PreviewOptions {
                                    base_dir: None,
                                    code_line_numbers: cfg.code_line_numbers,
                                    max_content_width: 0.0,
                                    allow_local_open: false,
                                },
                                &mut card_scroll,
                            );
                        });
                        if let Some(a) = card_scroll {
                            self.preview_scroll_to = Some((idx, a));
                        } else if self.preview_scroll_to.as_ref().is_some_and(|(ci, _)| *ci == idx) {
                            self.preview_scroll_to = None;
                        }
                    });
            })
            .response;

        (action, resp.rect)
    }
}

fn draw_tag_pill(ui: &mut egui::Ui, tag: &str, dark: bool) {
    let col = if tag.contains("设置") {
        Color32::from_rgb(0xd2, 0x99, 0x22)
    } else if tag.contains("扩展") {
        if dark { Color32::from_rgb(0xb8, 0x86, 0xff) } else { Color32::from_rgb(0x82, 0x50, 0xdf) }
    } else if tag.contains("HTML") {
        if dark { Color32::from_rgb(0x4b, 0xc2, 0xd6) } else { Color32::from_rgb(0x0b, 0x7a, 0x99) }
    } else {
        ui.visuals().weak_text_color()
    };
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(tag.to_string(), FontId::proportional(11.5), col));
    let sz = galley.size() + vec2(12.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(sz, Sense::hover());
    ui.painter().rect_filled(rect, 4.0, col.gamma_multiply(0.14));
    ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, col.gamma_multiply(0.55)), StrokeKind::Inside);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        tag,
        FontId::proportional(11.5),
        col,
    );
}

fn hash_pair<T: Hash>(val: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    val.hash(&mut h);
    h.finish()
}

fn hash_layout_key(query: &str, dark: bool, cfg: &ManualViewCfg<'_>) -> u64 {
    hash_pair(&(
        query,
        dark,
        cfg.editor_font_size.to_bits(),
        cfg.editor_font_family,
        cfg.preview_font_size.to_bits(),
        cfg.line_height.to_bits(),
        cfg.para_spacing.to_bits(),
        cfg.sup_sub,
        cfg.front_matter,
        cfg.code_line_numbers,
        cfg.max_content_width.to_bits(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_md_parses_completely_with_unique_ids_and_nonempty_fields() {
        let data = ManualData::parse(RAW_MANUAL).expect("manual.md 应能完整解析");
        assert_eq!(data.categories.len(), 16);
        assert_eq!(data.entries.len(), 78);
        let mut ids = HashSet::new();
        for e in &data.entries {
            assert!(!e.title.trim().is_empty());
            assert!(!e.summary.trim().is_empty(), "条目 {} 说明为空", e.id);
            assert!(!e.code.trim().is_empty(), "条目 {} 示例源码为空", e.id);
            assert!(ids.insert(e.id.as_str()), "重复 id: {}", e.id);
        }
    }

    #[test]
    fn all_manual_examples_parse_and_highlight_without_panic() {
        let data = ManualData::parse(RAW_MANUAL).unwrap();
        let font = FontId::monospace(14.0);
        for dark in [false, true] {
            for sup_sub in [false, true] {
                let th = Theme::new(dark, 16.0);
                let opts = ParseOptions { sup_sub, front_matter: true };
                for entry in &data.entries {
                    let doc = markdown::parse(&entry.code, &th, &opts);
                    if entry.id != "html-comment" {
                        assert!(
                            !doc.blocks.is_empty(),
                            "条目 {} 在 sup_sub={} 下解析后块为空",
                            entry.id,
                            sup_sub
                        );
                    }
                    let job = highlight::highlight_manual_source(&entry.code, dark, &font);
                    let mut cursor = 0usize;
                    for sec in &job.sections {
                        assert_eq!(sec.byte_range.start.0, cursor);
                        cursor = sec.byte_range.end.0;
                    }
                    assert_eq!(cursor, job.text.len());
                }
            }
        }
    }

    #[test]
    fn search_matches_chinese_english_and_synonyms() {
        let data = ManualData::parse(RAW_MANUAL).unwrap();
        let hit_ids = |q: &str| -> Vec<&str> {
            (0..data.entries.len())
                .filter(|&i| data.matches_query(i, q))
                .map(|i| data.entries[i].id.as_str())
                .collect()
        };

        for q in ["加粗", "粗体", "bold", "strong"] {
            let hits = hit_ids(q);
            assert!(hits.contains(&"style-bold"), "查询 {q:?} 未命中粗体: {hits:?}");
        }
        assert!(hit_ids("表格").contains(&"table-basic"));
        assert!(hit_ids("table").contains(&"table-basic"));
        assert!(!hit_ids("表格").contains(&"heading-atx"));
        assert!(hit_ids("折叠").contains(&"html-details"));
        assert!(hit_ids("details").contains(&"html-details"));
        assert!(hit_ids("上标").contains(&"style-superscript"));
        assert!(hit_ids("superscript").contains(&"style-superscript"));
        assert!(hit_ids("提示块 warning").contains(&"alert-warning"));
    }

    #[test]
    fn insert_patch_pads_blank_lines_for_blocks_and_preserves_inline() {
        // 1. 空文档插入块级语法，不额外补前导空行
        let p_empty = build_insert_patch("", 0..0, "## 标题", InsertMode::Block);
        let mut s_empty = String::new();
        p_empty.apply(&mut s_empty);
        assert_eq!(s_empty, "## 标题");

        // 2. 紧贴前文行末插入块级语法（例如 --- 分割线），自动补两个换行避免变成标题
        let mut s_mid = "第一段文字".to_string();
        let c = s_mid.chars().count();
        let p_hr = build_insert_patch(&s_mid, c..c, "---", InsertMode::Block);
        p_hr.apply(&mut s_mid);
        assert_eq!(s_mid, "第一段文字\n\n---");

        // 3. 插入两段正文中间（仅单换行分隔）
        let mut s_between = "上文\n下文".to_string();
        let p_mid = build_insert_patch(&s_between, 3..3, "- 列表项", InsertMode::Block);
        p_mid.apply(&mut s_between);
        assert_eq!(s_between, "上文\n\n- 列表项\n\n下文");

        // 4. 行内语法不额外加换行
        let mut s_inl = "重要内容".to_string();
        let p_inl = build_insert_patch(&s_inl, 2..4, "**加粗**", InsertMode::Inline);
        p_inl.apply(&mut s_inl);
        assert_eq!(s_inl, "重要**加粗**");
    }
}
