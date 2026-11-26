//! 预览渲染：把 markdown::Doc 画到 egui 上。
//!
//! - 只渲染视口附近的顶层块（其余用上一帧测得的高度占位），大文档也能流畅滚动
//! - 链接 / 脚注 / 目录可点击，任务列表复选框可直接勾选（会回写源码）

use crate::markdown::{self, bold_family, Block, Chunk, Doc, Inline, Rich, Target, Theme};
use crate::media;
use eframe::egui::{
    self, pos2, text::CCursor, vec2, Align, Align2, Color32, CursorIcon, FontId, Galley, Rect, RichText, Sense, Stroke,
    StrokeKind, UiBuilder,
};
use pulldown_cmark::{Alignment, BlockQuoteKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Default)]
pub struct Actions {
    /// 点击了任务复选框：源码中 '[' 的字节偏移
    pub toggle_task: Option<usize>,
    /// 点击了指向本地 .md 文件的链接
    pub open_file: Option<PathBuf>,
}

#[derive(Clone, Copy)]
pub struct PreviewOptions<'a> {
    pub base_dir: Option<&'a Path>,
    pub code_line_numbers: bool,
    pub max_content_width: f32,
}

#[derive(Default)]
pub struct PreviewState {
    doc_key: u64,
    heights: Vec<Option<f32>>,
    heights_width: f32,
    scroll_to: Option<String>,
    scroll_to_block: Option<usize>,
    pub top_block_idx: usize,
}

struct Rcx<'a> {
    th: &'a Theme,
    doc: &'a Doc,
    doc_uid: u64,
    scroll_to: &'a mut Option<String>,
    nav: &'a mut Option<String>,
    actions: &'a mut Actions,
    base_dir: Option<&'a Path>,
    list_depth: usize,
    code_line_numbers: bool,
    align: Option<Align>,
}

impl Rcx<'_> {
    fn check_scroll(&mut self, ui: &egui::Ui, anchor: &str, rect: Rect) {
        if rect.is_positive() && self.scroll_to.as_deref() == Some(anchor) {
            ui.scroll_to_rect(rect, Some(Align::TOP));
            *self.scroll_to = None;
        }
    }

    fn resolve_anchor(&self, a: &str) -> String {
        let slug = markdown::slugify(a);
        self.doc
            .headings
            .iter()
            .find(|h| h.anchor == a || h.anchor == slug || h.text == a)
            .map(|h| h.anchor.clone())
            .unwrap_or_else(|| a.to_string())
    }

    fn follow(&mut self, ui: &egui::Ui, target: &Target) {
        match target {
            Target::Anchor(a) => *self.nav = Some(self.resolve_anchor(a)),
            Target::Url(u) => {
                if u.contains("://") || u.starts_with("mailto:") {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(u));
                } else {
                    let path_part = u.split('#').next().unwrap_or("");
                    let p = resolve_path(&markdown::percent_decode(path_part), self.base_dir);
                    let is_md = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
                    if is_md {
                        self.actions.open_file = Some(p);
                    } else {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(file_uri(&p)));
                    }
                }
            }
        }
    }
}

pub fn resolve_path(p: &str, base: Option<&Path>) -> PathBuf {
    let path = Path::new(p);
    match base {
        Some(b) if path.is_relative() => b.join(path),
        _ => path.to_path_buf(),
    }
}

fn file_uri(p: &Path) -> String {
    format!("file://{}", p.to_string_lossy().replace('\\', "/"))
}

impl PreviewState {
    pub fn scroll_to(&mut self, anchor: String) {
        self.scroll_to = Some(anchor);
    }

    pub fn scroll_to_block(&mut self, idx: usize) {
        self.scroll_to_block = Some(idx);
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        uid: u64,
        doc: &Doc,
        th: &Theme,
        doc_key: u64,
        opts: PreviewOptions<'_>,
    ) -> Actions {
        if self.doc_key != doc_key {
            self.doc_key = doc_key;
            self.heights.clear();
        }

        let Self { heights, heights_width, scroll_to, scroll_to_block, top_block_idx, .. } = self;
        let mut actions = Actions::default();
        let mut nav: Option<String> = None;
        let force = scroll_to.is_some() || scroll_to_block.is_some();
        let mut first_visible: Option<usize> = None;

        egui::ScrollArea::vertical()
            .id_salt(("preview_scroll", uid))
            .auto_shrink(false)
            .show_viewport(ui, |ui, viewport| {
                let origin = ui.max_rect().top();
                egui::Frame::new()
                    .inner_margin(egui::Margin { left: 28, right: 28, top: 18, bottom: 60 })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        let avail_w = ui.available_width();
                        let width = if opts.max_content_width > 0.0 {
                            avail_w.min(opts.max_content_width)
                        } else {
                            avail_w
                        };
                        let side_pad = ((avail_w - width) * 0.5).max(0.0);
                        ui.horizontal(|ui| {
                            if side_pad > 0.0 {
                                ui.add_space(side_pad);
                            }
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                if heights.len() != doc.blocks.len() || (width - *heights_width).abs() > 0.5 {
                                    *heights = vec![None; doc.blocks.len()];
                                    *heights_width = width;
                                }
                                let mut rcx = Rcx {
                                    th,
                                    doc,
                                    doc_uid: uid,
                                    scroll_to,
                                    nav: &mut nav,
                                    actions: &mut actions,
                                    base_dir: opts.base_dir,
                                    list_depth: 0,
                                    code_line_numbers: opts.code_line_numbers,
                                    align: None,
                                };
                                for (i, block) in doc.blocks.iter().enumerate() {
                                    let y = ui.cursor().top() - origin;
                                    if !force
                                        && let Some(h) = heights[i]
                                        && (y + h < viewport.top() - 600.0 || y > viewport.bottom() + 600.0)
                                    {
                                        ui.allocate_space(vec2(width, h));
                                        if first_visible.is_none() && y + h >= viewport.top() {
                                            first_visible = Some(i);
                                        }
                                        continue;
                                    }
                                    let start = ui.cursor().top();
                                    render_block(ui, block, &mut rcx, i == 0);
                                    let end = ui.cursor().top();
                                    heights[i] = Some(end - start);
                                    if first_visible.is_none() && (end - origin) >= viewport.top() {
                                        first_visible = Some(i);
                                    }
                                    if *scroll_to_block == Some(i) {
                                        let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), start..=end.max(start + 1.0));
                                        ui.scroll_to_rect(rect, Some(Align::TOP));
                                        *scroll_to_block = None;
                                    }
                                }
                                render_footnotes(ui, &mut rcx);
                                // 完整渲染过一遍后，找不到的锚点 / 块直接丢弃
                                *rcx.scroll_to = None;
                                *scroll_to_block = None;
                            });
                        });
                    });
            });

        if let Some(idx) = first_visible {
            *top_block_idx = idx;
        }
        if let Some(a) = nav {
            self.scroll_to = Some(a);
            ui.ctx().request_repaint();
        }
        actions
    }
}

// ---------------------------------------------------------------- 块

fn render_blocks(ui: &mut egui::Ui, blocks: &[Block], rcx: &mut Rcx) {
    for (i, b) in blocks.iter().enumerate() {
        render_block(ui, b, rcx, i == 0);
    }
}

fn render_block(ui: &mut egui::Ui, block: &Block, rcx: &mut Rcx, first: bool) {
    let th = rcx.th;
    let size = th.size;
    if !first {
        let gap = match block {
            Block::Heading { level, .. } if *level <= 2 => size * (th.para_spacing + 0.55),
            Block::Heading { .. } => size * (th.para_spacing + 0.25),
            _ if rcx.list_depth > 0 => size * 0.35,
            _ => size * th.para_spacing,
        };
        ui.add_space(gap);
    }

    match block {
        Block::Heading { level, inline, anchor } => {
            let r = render_inline(ui, inline, rcx);
            if *level <= 2 {
                ui.add_space(5.0);
                let full = ui.available_rect_before_wrap();
                ui.painter().hline(full.x_range(), full.top(), Stroke::new(1.0, th.border));
                ui.add_space(3.0);
            }
            rcx.check_scroll(ui, anchor, r);
        }
        Block::Paragraph(inline) => {
            render_inline(ui, inline, rcx);
        }
        Block::Code { lang, code } => render_code(ui, lang, code, th, rcx.code_line_numbers),
        Block::Quote { kind, blocks } => {
            let (bar, title) = match kind {
                Some(k) => {
                    let (c, icon, name) = alert_style(*k);
                    (c, Some((icon, name)))
                }
                None => (th.border, None),
            };
            let fill = if kind.is_some() { bar.gamma_multiply(0.08) } else { Color32::TRANSPARENT };
            let resp = egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin { left: 16, right: 10, top: 6, bottom: 6 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some((icon, name)) = title {
                        ui.label(RichText::new(format!("{icon}  {name}")).font(FontId::new(size, bold_family())).color(bar));
                        ui.add_space(size * 0.4);
                    }
                    render_blocks(ui, blocks, rcx);
                })
                .response;
            let r = resp.rect;
            ui.painter().rect_filled(Rect::from_min_max(r.left_top(), pos2(r.left() + 4.0, r.bottom())), 2, bar);
        }
        Block::List { start, items } => render_list(ui, *start, items, rcx),
        Block::Rule => {
            ui.add_space(size * 0.3);
            let full = ui.available_rect_before_wrap();
            ui.painter().hline(full.x_range(), full.top(), Stroke::new(2.0, th.border));
            ui.add_space(size * 0.3 + 2.0);
        }
        Block::Table { aligns, head, rows } => render_table(ui, aligns, head, rows, rcx),
        Block::FrontMatter(job) => {
            egui::Frame::new()
                .fill(th.panel_bg)
                .stroke(Stroke::new(1.0, th.border))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Front Matter").size(size * 0.75).color(th.weak));
                    ui.add_space(4.0);
                    let mut job = (**job).clone();
                    job.wrap.max_width = ui.available_width();
                    ui.add(egui::Label::new(job).selectable(true));
                });
        }
        Block::Toc => render_toc(ui, rcx),
        Block::Details { id, summary, default_open, blocks } => {
            render_details(ui, *id, summary, *default_open, blocks, rcx);
        }
        Block::Aligned { align, blocks } => {
            let prev = rcx.align;
            rcx.align = Some(*align);
            ui.with_layout(egui::Layout::top_down(*align), |ui| {
                ui.set_width(ui.available_width());
                render_blocks(ui, blocks, rcx);
            });
            rcx.align = prev;
        }
        Block::Media(card) => {
            media::render_card(ui, card, th, rcx.base_dir);
        }
    }
}

fn render_details(
    ui: &mut egui::Ui,
    id: usize,
    summary: &str,
    default_open: bool,
    blocks: &[Block],
    rcx: &mut Rcx,
) {
    let th = rcx.th;
    let size = th.size;
    let state_id = egui::Id::unique(("details", rcx.doc_uid, id, default_open));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), state_id, default_open);
    let summary_text = if summary.trim().is_empty() { "详细信息" } else { summary.trim() };

    egui::Frame::new()
        .fill(th.panel_bg.gamma_multiply(0.55))
        .stroke(Stroke::new(1.0, th.border))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let arrow = if state.is_open() { "▾" } else { "▸" };
            let hdr_resp = ui
                .horizontal(|ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(arrow).size(size * 0.95).color(th.weak));
                    ui.add_space(4.0);
                    ui.label(RichText::new(summary_text).font(FontId::new(size, bold_family())).color(th.heading));
                })
                .response
                .interact(Sense::click());
            if hdr_resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if hdr_resp.clicked() {
                state.toggle(ui);
                state.store(ui.ctx());
            }
            if state.is_open() && !blocks.is_empty() {
                ui.add_space(size * 0.45);
                let full = ui.available_rect_before_wrap();
                ui.painter().hline(full.x_range(), full.top(), Stroke::new(1.0, th.border.gamma_multiply(0.6)));
                ui.add_space(size * 0.45);
                render_blocks(ui, blocks, rcx);
            }
        });
}

pub fn alert_style(k: BlockQuoteKind) -> (Color32, &'static str, &'static str) {
    let c = Color32::from_rgb;
    match k {
        BlockQuoteKind::Note => (c(0x2f, 0x81, 0xf7), "ℹ", "备注 Note"),
        BlockQuoteKind::Tip => (c(0x3f, 0xb9, 0x50), "💡", "提示 Tip"),
        BlockQuoteKind::Important => (c(0xa3, 0x71, 0xf7), "❗", "重要 Important"),
        BlockQuoteKind::Warning => (c(0xd2, 0x99, 0x22), "⚠", "警告 Warning"),
        BlockQuoteKind::Caution => (c(0xf8, 0x51, 0x49), "⛔", "注意 Caution"),
    }
}

pub fn is_supported_highlight_lang(lang: &str) -> bool {
    matches!(
        lang.to_ascii_lowercase().as_str(),
        "c" | "h"
            | "hpp"
            | "cpp"
            | "c++"
            | "js"
            | "javascript"
            | "jsx"
            | "mjs"
            | "ts"
            | "typescript"
            | "tsx"
            | "json"
            | "jsonc"
            | "json5"
            | "py"
            | "python"
            | "rs"
            | "rust"
            | "bash"
            | "sh"
            | "shell"
            | "zsh"
            | "sql"
            | "toml"
            | "yaml"
            | "yml"
    )
}

fn render_code(ui: &mut egui::Ui, lang: &str, code: &str, th: &Theme, line_numbers: bool) {
    let code_size = th.size * 0.88;
    let line_h = th.size * 1.45;
    let font_id = FontId::monospace(code_size);
    let mut job = if is_supported_highlight_lang(lang) {
        let theme = if th.dark {
            egui_extras::syntax_highlighting::CodeTheme::dark(code_size)
        } else {
            egui_extras::syntax_highlighting::CodeTheme::light(code_size)
        };
        egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, code, lang)
    } else {
        egui::text::LayoutJob::simple(code.to_string(), font_id.clone(), th.text, f32::INFINITY)
    };
    for s in &mut job.sections {
        s.format.line_height = Some(line_h);
    }
    job.wrap.max_width = f32::INFINITY;

    let resp = egui::Frame::new()
        .fill(th.code_bg)
        .corner_radius(6)
        .inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if line_numbers {
                let n_lines = code.split('\n').count().max(1);
                let digits = n_lines.to_string().len().max(2);
                let nums: String = (1..=n_lines)
                    .map(|n| format!("{n:>digits$}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut num_job = egui::text::LayoutJob::simple(nums, font_id, th.weak, f32::INFINITY);
                for s in &mut num_job.sections {
                    s.format.line_height = Some(line_h);
                }
                ui.horizontal_top(|ui| {
                    ui.add(egui::Label::new(num_job).selectable(false));
                    ui.add_space(8.0);
                    let x = ui.cursor().left();
                    let top = ui.cursor().top();
                    let h = n_lines as f32 * line_h;
                    ui.painter().vline(x, top..=(top + h), Stroke::new(1.0, th.border));
                    ui.add_space(10.0);
                    egui::ScrollArea::horizontal()
                        .id_salt(("code_scroll", code.as_ptr() as usize))
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(job).selectable(true));
                        });
                });
            } else {
                egui::ScrollArea::horizontal()
                    .id_salt(("code_scroll", code.as_ptr() as usize))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(job).selectable(true));
                    });
            }
        })
        .response;

    let r = resp.rect;
    let btn = Rect::from_min_size(pos2(r.right() - 30.0, r.top() + 4.0), vec2(26.0, 24.0));
    let id = ui.scope_id().with(("copy", code.as_ptr() as usize));
    let b = ui.interact(btn, id, Sense::click());
    let hovered = b.hovered() || resp.hovered() || ui.rect_contains_pointer(r);
    if hovered {
        let color = if b.hovered() { th.text } else { th.weak };
        ui.painter().text(btn.center(), Align2::CENTER_CENTER, "🗐", FontId::proportional(th.size), color);
    }
    if !lang.is_empty() {
        let x = if hovered { btn.left() - 4.0 } else { r.right() - 10.0 };
        ui.painter().text(pos2(x, r.top() + 8.0), Align2::RIGHT_TOP, lang, FontId::proportional(th.size * 0.72), th.weak);
    }
    if b.on_hover_text("复制代码").clicked() {
        ui.ctx().copy_text(code.to_string());
    }
}

fn render_list(ui: &mut egui::Ui, start: Option<u64>, items: &[markdown::Item], rcx: &mut Rcx) {
    let th = rcx.th;
    let size = th.size;
    rcx.list_depth += 1;
    let depth = rcx.list_depth;
    let marker_w = if start.is_some() { size * 2.0 } else { size * 1.5 };
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            ui.add_space(size * 0.3);
        }
        ui.horizontal_top(|ui| {
            let line_h = th.line_height();
            let sense = if item.task.is_some() { Sense::click() } else { Sense::hover() };
            let (rect, resp) = ui.allocate_exact_size(vec2(marker_w, line_h), sense);
            let painter = ui.painter().clone();
            let cy = rect.center().y;
            match (item.task, start) {
                (Some((checked, off)), _) => {
                    let bx = Rect::from_center_size(pos2(rect.left() + size * 0.55, cy), vec2(size * 0.9, size * 0.9));
                    if checked {
                        painter.rect_filled(bx, 3, th.accent);
                        let p = |x: f32, y: f32| pos2(bx.left() + bx.width() * x, bx.top() + bx.height() * y);
                        painter.line(vec![p(0.22, 0.52), p(0.42, 0.72), p(0.78, 0.30)], Stroke::new(2.0, Color32::WHITE));
                    } else {
                        let stroke = if resp.hovered() { th.accent } else { th.weak };
                        painter.rect_stroke(bx, 3, Stroke::new(1.5, stroke), StrokeKind::Inside);
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        rcx.actions.toggle_task = Some(off);
                    }
                }
                (None, Some(s)) => {
                    painter.text(
                        pos2(rect.right() - size * 0.4, cy),
                        Align2::RIGHT_CENTER,
                        format!("{}.", s + i as u64),
                        FontId::proportional(size),
                        th.text,
                    );
                }
                (None, None) => {
                    let c = pos2(rect.left() + size * 0.6, cy);
                    let r = size * 0.18;
                    match depth {
                        1 => {
                            painter.circle_filled(c, r, th.text);
                        }
                        2 => {
                            painter.circle_stroke(c, r, Stroke::new(1.2, th.text));
                        }
                        _ => {
                            painter.rect_filled(Rect::from_center_size(c, vec2(r * 1.8, r * 1.8)), 0, th.text);
                        }
                    }
                }
            }
            ui.vertical(|ui| render_blocks(ui, &item.blocks, rcx));
        });
    }
    rcx.list_depth -= 1;
}

fn layout_rich(ui: &egui::Ui, rich: &Rich, width: f32, halign: Option<Align>) -> Arc<Galley> {
    let mut job = (*rich.job).clone();
    job.wrap.max_width = width;
    if let Some(a) = halign {
        job.halign = a;
    }
    ui.fonts_mut(|f| f.layout_job(job))
}

/// 显示一个已排版的段落，并处理链接悬停 / 点击
fn show_galley(ui: &mut egui::Ui, galley: Arc<Galley>, rich: &Rich, rcx: &mut Rcx) -> Rect {
    let resp = ui.add(egui::Label::new(galley.clone()).selectable(true));
    let rect = resp.rect;
    if !rich.links.is_empty()
        && let Some(pos) = resp.hover_pos()
    {
        let rel = pos - resp.rect.min;
        let c = galley.cursor_from_pos(rel);
        let here = galley.pos_from_cursor(c);
        let idx = if rel.x < here.min.x { c.index.0.saturating_sub(1) } else { c.index.0 };
        let a = galley.pos_from_cursor(CCursor::new(idx));
        let b = galley.pos_from_cursor(CCursor::new(idx + 1));
        let right = if (b.min.y - a.min.y).abs() < 1.0 && b.min.x > a.min.x { b.min.x } else { a.min.x + rcx.th.size };
        let over_char = rel.x >= a.min.x - 1.0 && rel.x <= right + 1.0 && rel.y >= a.min.y - 2.0 && rel.y <= a.max.y + 2.0;
        if over_char && let Some((_, target)) = rich.links.iter().find(|(r, _)| r.contains(&idx)) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            let tip = match target {
                Target::Url(u) => u.clone(),
                Target::Anchor(a) => format!("#{a}"),
            };
            let clicked = resp.clicked();
            resp.on_hover_text_at_pointer(tip);
            if clicked {
                rcx.follow(ui, target);
            }
        }
    }
    rect
}

fn render_rich(ui: &mut egui::Ui, rich: &Rich, rcx: &mut Rcx) -> Rect {
    let galley = layout_rich(ui, rich, ui.available_width(), rcx.align);
    let top = ui.cursor().top();
    show_galley(ui, galley, rich, rcx);
    let bottom = ui.cursor().top();
    let full = ui.max_rect();
    Rect::from_x_y_ranges(full.x_range(), top..=bottom.max(top + 1.0))
}

fn render_image(
    ui: &mut egui::Ui,
    url: &str,
    alt: &str,
    width: Option<f32>,
    height: Option<f32>,
    rcx: &mut Rcx,
) -> Rect {
    let uri = if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
        url.to_string()
    } else {
        file_uri(&resolve_path(&markdown::percent_decode(url), rcx.base_dir))
    };
    let avail = ui.available_width();
    let mut img = egui::Image::new(uri).corner_radius(4);
    img = match (width, height) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => {
            let scale = (avail / w).min(1.0);
            img.fit_to_exact_size(vec2(w * scale, h * scale)).maintain_aspect_ratio(false)
        }
        (Some(w), None) if w > 0.0 => {
            let w = w.min(avail);
            img.max_width(w).fit_to_exact_size(vec2(w, 10_000.0))
        }
        (None, Some(h)) if h > 0.0 => img.max_width(avail).fit_to_exact_size(vec2(avail, h)),
        _ => img.max_width(avail),
    };
    let resp = ui.add(img);
    let r = resp.rect;
    if !alt.is_empty() {
        resp.on_hover_text(alt);
    }
    r
}

fn render_inline(ui: &mut egui::Ui, inl: &Inline, rcx: &mut Rcx) -> Rect {
    let mut rect = Rect::NOTHING;
    for (i, chunk) in inl.chunks.iter().enumerate() {
        if i > 0 {
            ui.add_space(rcx.th.size * 0.3);
        }
        let r = match chunk {
            Chunk::Text(rich) => render_rich(ui, rich, rcx),
            Chunk::Image { url, alt, width, height } => render_image(ui, url, alt, *width, *height, rcx),
            Chunk::Media(card) => {
                let top = ui.cursor().top();
                media::render_card(ui, card, rcx.th, rcx.base_dir);
                let bottom = ui.cursor().top();
                Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=bottom.max(top + 1.0))
            }
        };
        rect = rect.union(r);
    }
    for a in &inl.anchors {
        rcx.check_scroll(ui, a, rect);
    }
    rect
}

fn render_table(ui: &mut egui::Ui, aligns: &[Alignment], head: &[Inline], rows: &[Vec<Inline>], rcx: &mut Rcx) {
    let th = rcx.th;
    let pad = vec2(12.0, 6.0);
    let ncols = aligns.len().max(head.len()).max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if ncols == 0 {
        return;
    }
    let max_cell = (ui.available_width() * 0.6).clamp(160.0, 520.0);

    type CellGalley<'a> = Option<(Arc<Galley>, &'a Rich)>;

    let all: Vec<&[Inline]> = std::iter::once(head).chain(rows.iter().map(Vec::as_slice)).collect();
    let galleys: Vec<Vec<CellGalley<'_>>> = all
        .iter()
        .map(|row| {
            (0..ncols)
                .map(|c| {
                    row.get(c).and_then(|cell| {
                        cell.chunks.iter().find_map(|ch| match ch {
                            Chunk::Text(r) => Some((layout_rich(ui, r, max_cell, None), r)),
                            _ => None,
                        })
                    })
                })
                .collect()
        })
        .collect();

    let mut widths = vec![th.size; ncols];
    for row in &galleys {
        for (c, g) in row.iter().enumerate() {
            if let Some((g, _)) = g {
                widths[c] = widths[c].max(g.size().x);
            }
        }
    }
    let total_w: f32 = widths.iter().map(|w| w + pad.x * 2.0).sum();

    egui::ScrollArea::horizontal().id_salt(("table", head.as_ptr() as usize)).show(ui, |ui| {
        let mut table_rect = Rect::NOTHING;
        for (ri, row) in galleys.iter().enumerate() {
            let row_h = row.iter().flatten().map(|(g, _)| g.size().y).fold(th.line_height(), f32::max) + pad.y * 2.0;
            let (row_rect, _) = ui.allocate_exact_size(vec2(total_w, row_h), Sense::hover());
            table_rect = table_rect.union(row_rect);
            if ri == 0 {
                ui.painter().rect_filled(row_rect, 0, th.panel_bg);
            } else if ri % 2 == 0 {
                ui.painter().rect_filled(row_rect, 0, th.panel_bg.gamma_multiply(0.6));
            }
            let mut x = row_rect.left();
            for c in 0..ncols {
                let cw = widths[c] + pad.x * 2.0;
                if c > 0 {
                    ui.painter().vline(x, row_rect.y_range(), Stroke::new(1.0, th.border));
                }
                if let Some((g, rich)) = &row[c] {
                    let off = match aligns.get(c) {
                        Some(Alignment::Center) => (widths[c] - g.size().x) / 2.0,
                        Some(Alignment::Right) => widths[c] - g.size().x,
                        _ => 0.0,
                    };
                    let pos = pos2(x + pad.x + off, row_rect.top() + pad.y);
                    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(pos, g.size() + vec2(1.0, 1.0))));
                    show_galley(&mut child, g.clone(), rich, rcx);
                }
                x += cw;
            }
            if ri > 0 {
                ui.painter().hline(row_rect.x_range(), row_rect.top(), Stroke::new(1.0, th.border));
            }
            for cell in all[ri] {
                for a in &cell.anchors {
                    rcx.check_scroll(ui, a, row_rect);
                }
            }
        }
        ui.painter().rect_stroke(table_rect, 4, Stroke::new(1.0, th.border), StrokeKind::Inside);
    });
}

fn render_toc(ui: &mut egui::Ui, rcx: &mut Rcx) {
    let th = rcx.th;
    let size = th.size;
    let headings = &rcx.doc.headings;
    egui::Frame::new()
        .fill(th.panel_bg)
        .stroke(Stroke::new(1.0, th.border))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("目录").font(FontId::new(size * 1.05, bold_family())).color(th.heading));
            ui.add_space(6.0);
            if headings.is_empty() {
                ui.label(RichText::new("（文档中还没有标题）").color(th.weak));
            }
            let min_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
            for h in headings {
                ui.horizontal(|ui| {
                    ui.add_space((h.level - min_level) as f32 * size * 1.3);
                    let r = ui.add(
                        egui::Label::new(RichText::new(&h.text).size(size * 0.95).color(th.link))
                            .sense(Sense::click())
                            .selectable(false),
                    );
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if r.clicked() {
                        *rcx.nav = Some(h.anchor.clone());
                    }
                });
                ui.add_space(3.0);
            }
        });
}

fn render_footnotes(ui: &mut egui::Ui, rcx: &mut Rcx) {
    let doc = rcx.doc;
    if doc.footnotes.is_empty() {
        return;
    }
    let th = rcx.th;
    let size = th.size;
    ui.add_space(size * 2.0);
    let full = ui.available_rect_before_wrap();
    ui.painter().hline(full.x_range(), full.top(), Stroke::new(1.0, th.border));
    ui.add_space(size * 0.6);
    ui.label(RichText::new("脚注").size(size * 0.85).color(th.weak));
    ui.add_space(size * 0.4);
    for f in &doc.footnotes {
        let resp = ui
            .horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(size * 2.2, th.line_height()), Sense::hover());
                ui.painter().text(
                    pos2(rect.right() - size * 0.5, rect.center().y),
                    Align2::RIGHT_CENTER,
                    format!("{}.", f.number),
                    FontId::proportional(size * 0.9),
                    th.weak,
                );
                ui.vertical(|ui| {
                    render_blocks(ui, &f.blocks, rcx);
                    let back = ui.add(
                        egui::Label::new(RichText::new("↩ 返回正文").size(size * 0.8).color(th.link))
                            .sense(Sense::click())
                            .selectable(false),
                    );
                    if back.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if back.clicked() {
                        *rcx.nav = Some(format!("fnref-{}", f.label));
                    }
                });
            })
            .response;
        rcx.check_scroll(ui, &format!("fn-{}", f.label), resp.rect);
        ui.add_space(size * 0.5);
    }
}
