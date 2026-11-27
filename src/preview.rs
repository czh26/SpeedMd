//! 预览渲染：把 markdown::Doc 画到 egui 上。
//!
//! - 只渲染视口附近的顶层块（其余用上一帧测得的高度占位），大文档也能流畅滚动
//! - 链接 / 脚注 / 目录可点击，任务列表复选框可直接勾选（会回写源码）

use crate::markdown::{self, bold_family, Block, Chunk, Doc, Inline, Rich, Target, Theme};
use crate::media;
use crate::theme::{self, ThemeId};
use eframe::egui::{
    self, pos2, text::CCursor, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Galley, Rect, RichText,
    Sense, Stroke, StrokeKind, UiBuilder,
};
use pulldown_cmark::{Alignment, BlockQuoteKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    pub allow_local_open: bool,
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
    block_idx: usize,
    scroll_sub_idx: usize,
    scroll_to: &'a mut Option<String>,
    nav: &'a mut Option<String>,
    actions: &'a mut Actions,
    base_dir: Option<&'a Path>,
    list_depth: usize,
    code_line_numbers: bool,
    align: Option<Align>,
    allow_local_open: bool,
}

impl Rcx<'_> {
    fn next_scroll_id(&mut self, kind: &'static str) -> (&'static str, u64, usize, usize) {
        let sub = self.scroll_sub_idx;
        self.scroll_sub_idx += 1;
        (kind, self.doc_uid, self.block_idx, sub)
    }

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

    fn is_external_url(u: &str) -> bool {
        u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")
    }

    fn follow(&mut self, ui: &egui::Ui, target: &Target) {
        match target {
            Target::Anchor(a) => *self.nav = Some(self.resolve_anchor(a)),
            Target::Url(u) => {
                if Self::is_external_url(u) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(u));
                } else if u.contains("://") {
                    if self.allow_local_open {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(u));
                    }
                } else if self.allow_local_open {
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
                    .inner_margin(egui::Margin { left: 22, right: 22, top: 12, bottom: 54 })
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
                                    block_idx: 0,
                                    scroll_sub_idx: 0,
                                    scroll_to,
                                    nav: &mut nav,
                                    actions: &mut actions,
                                    base_dir: opts.base_dir,
                                    list_depth: 0,
                                    code_line_numbers: opts.code_line_numbers,
                                    align: None,
                                    allow_local_open: opts.allow_local_open,
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
                                    rcx.block_idx = i;
                                    rcx.scroll_sub_idx = 0;
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
                                rcx.block_idx = doc.blocks.len();
                                rcx.scroll_sub_idx = 0;
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

/// 在现有容器中直接渲染一份已解析的 `Doc`（不创建独立 ScrollArea），供语法说明书卡片复用主预览渲染逻辑。
pub fn render_doc_inline(
    ui: &mut egui::Ui,
    uid: u64,
    doc: &Doc,
    th: &Theme,
    opts: PreviewOptions<'_>,
    scroll_to: &mut Option<String>,
) {
    let mut actions = Actions::default();
    let mut nav: Option<String> = None;
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let mut rcx = Rcx {
        th,
        doc,
        doc_uid: uid,
        block_idx: 0,
        scroll_sub_idx: 0,
        scroll_to,
        nav: &mut nav,
        actions: &mut actions,
        base_dir: opts.base_dir,
        list_depth: 0,
        code_line_numbers: opts.code_line_numbers,
        align: None,
        allow_local_open: opts.allow_local_open,
    };
    for (i, b) in doc.blocks.iter().enumerate() {
        rcx.block_idx = i;
        rcx.scroll_sub_idx = 0;
        render_block(ui, b, &mut rcx, i == 0);
    }
    rcx.block_idx = doc.blocks.len();
    rcx.scroll_sub_idx = 0;
    render_footnotes(ui, &mut rcx);
    *rcx.scroll_to = None;
    if let Some(a) = nav {
        *scroll_to = Some(a);
        ui.ctx().request_repaint();
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
        Block::Code { lang, code } => {
            let scroll_id = rcx.next_scroll_id("code_scroll");
            render_code(ui, lang, code, rcx.th, rcx.code_line_numbers, scroll_id);
        }
        Block::Quote { kind, blocks } => {
            let (bar, title_col, title) = match kind {
                Some(k) => {
                    let c = th.alerts.color_of(*k);
                    let (icon, name) = alert_meta(*k);
                    let tcol = match (th.theme_id, *k) {
                        (ThemeId::ZenHorizon, BlockQuoteKind::Tip) => th.theme_id.spec().secondary_accent,
                        (ThemeId::ScarletStreak, BlockQuoteKind::Tip) => th.alerts.warning,
                        _ => c,
                    };
                    (c, tcol, Some((icon, name)))
                }
                None => (th.border, th.border, None),
            };
            let fill = if kind.is_some() { th.callout_bg } else { Color32::TRANSPARENT };
            let stroke = if kind.is_some() && matches!(th.theme_id, ThemeId::CarbonGold | ThemeId::TitaniumGlass) {
                Stroke::new(1.0, th.border)
            } else {
                Stroke::NONE
            };
            let resp = egui::Frame::new()
                .fill(fill)
                .stroke(stroke)
                .corner_radius(if kind.is_some() { 8 } else { 0 })
                .inner_margin(egui::Margin { left: 16, right: 12, top: 8, bottom: 8 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some((icon, name)) = title {
                        ui.label(
                            RichText::new(format!("{icon}  {name}"))
                                .font(FontId::new(size, bold_family()))
                                .color(title_col),
                        );
                        ui.add_space(size * 0.4);
                    }
                    render_blocks(ui, blocks, rcx);
                })
                .response;
            let r = resp.rect;
            let bar_rad = if kind.is_some() {
                CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }
            } else {
                CornerRadius::same(2)
            };
            ui.painter()
                .rect_filled(Rect::from_min_max(r.left_top(), pos2(r.left() + 4.0, r.bottom())), bar_rad, bar);
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
            media::render_card_opts(ui, card, th, rcx.base_dir, rcx.allow_local_open);
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

pub fn alert_meta(k: BlockQuoteKind) -> (&'static str, &'static str) {
    match k {
        BlockQuoteKind::Note => ("ℹ", "备注 Note"),
        BlockQuoteKind::Tip => ("💡", "提示 Tip"),
        BlockQuoteKind::Important => ("❗", "重要 Important"),
        BlockQuoteKind::Warning => ("⚠", "警告 Warning"),
        BlockQuoteKind::Caution => ("⛔", "注意 Caution"),
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

fn render_code(
    ui: &mut egui::Ui,
    lang: &str,
    code: &str,
    th: &Theme,
    line_numbers: bool,
    scroll_id: (&'static str, u64, usize, usize),
) {
    let spec = th.theme_id.spec();
    let code_size = th.size * 0.88;
    let line_h = th.size * 1.45;
    let font_id = FontId::monospace(code_size);
    let mut job = if is_supported_highlight_lang(lang) {
        let theme = spec.code.to_code_theme(code_size);
        egui_extras::syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, code, lang)
    } else {
        egui::text::LayoutJob::simple(code.to_string(), font_id.clone(), th.text, f32::INFINITY)
    };
    for s in &mut job.sections {
        s.format.line_height = Some(line_h);
    }
    job.wrap.max_width = f32::INFINITY;
    job.halign = Align::LEFT;
    let code_galley = ui.fonts_mut(|f| f.layout_job(job));

    let resp = egui::Frame::new()
        .fill(th.code_bg)
        .stroke(Stroke::new(1.0, th.code_border))
        .corner_radius(8)
        .inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let show_code_scroll = |ui: &mut egui::Ui| {
                ui.scope(|ui| {
                    let needs_scroll = code_galley.size().x > ui.available_width() + 0.5;
                    let vis = theme::configure_horizontal_scroll(ui, needs_scroll);
                    egui::ScrollArea::horizontal()
                        .id_salt(scroll_id)
                        .scroll_bar_visibility(vis)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(code_galley).selectable(true));
                        });
                });
            };
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
                    show_code_scroll(ui);
                });
            } else {
                show_code_scroll(ui);
            }
        })
        .response;

    let r = resp.rect;
    let btn = Rect::from_min_size(pos2(r.right() - 30.0, r.top() + 5.0), vec2(24.0, 22.0));
    let id = ui.scope_id().with(("copy", scroll_id));
    let b = ui.interact(btn, id, Sense::click());
    if b.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let copied_at: Option<Instant> = ui.ctx().data(|d| d.get_temp(id));
    let copied = copied_at.is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
    if copied {
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    }

    if copied {
        ui.painter().text(
            btn.center(),
            Align2::CENTER_CENTER,
            "✓",
            FontId::proportional(th.size * 0.9),
            spec.semantic.success,
        );
    } else {
        let hovered = b.hovered() || resp.hovered() || ui.rect_contains_pointer(r);
        let color = if b.hovered() {
            th.text
        } else if hovered {
            th.weak
        } else {
            th.weak.gamma_multiply(0.70)
        };
        theme::paint_copy_icon(ui.painter(), btn.center(), 13.0, color);
    }
    if !lang.is_empty() && (b.hovered() || resp.hovered() || ui.rect_contains_pointer(r)) {
        ui.painter().text(
            pos2(btn.left() - 5.0, r.top() + 8.0),
            Align2::RIGHT_TOP,
            lang,
            FontId::proportional(th.size * 0.72),
            th.weak,
        );
    }
    if b.on_hover_text(if copied { "已复制" } else { "复制代码" }).clicked() {
        ui.ctx().copy_text(code.to_string());
        ui.ctx().data_mut(|d| d.insert_temp(id, Instant::now()));
        ui.ctx().request_repaint_after(Duration::from_millis(1550));
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
            let sense = if item.task.is_some() && rcx.allow_local_open { Sense::click() } else { Sense::hover() };
            let (rect, resp) = ui.allocate_exact_size(vec2(marker_w, line_h), sense);
            let painter = ui.painter().clone();
            let cy = rect.center().y;
            match (item.task, start) {
                (Some((checked, off)), _) => {
                    let bx = Rect::from_center_size(pos2(rect.left() + size * 0.55, cy), vec2(size * 0.9, size * 0.9));
                    if checked {
                        painter.rect_filled(bx, 3, th.accent);
                        let p = |x: f32, y: f32| pos2(bx.left() + bx.width() * x, bx.top() + bx.height() * y);
                        painter.line(vec![p(0.22, 0.52), p(0.42, 0.72), p(0.78, 0.30)], Stroke::new(2.0, th.on_accent));
                    } else {
                        let stroke = if resp.hovered() && rcx.allow_local_open { th.accent } else { th.weak };
                        painter.rect_stroke(bx, 3, Stroke::new(1.5, stroke), StrokeKind::Inside);
                    }
                    if rcx.allow_local_open {
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            rcx.actions.toggle_task = Some(off);
                        }
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
        if over_char && let Some(ls) = rich.links.iter().find(|l| l.range.contains(&idx)) {
            let can_follow = match &ls.target {
                Target::Anchor(_) => true,
                Target::Url(u) => Rcx::is_external_url(u) || rcx.allow_local_open,
            };
            if can_follow {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let base_tip = match &ls.target {
                Target::Url(u) if !can_follow => format!("{u}（本地路径示例，说明书预览中不打开）"),
                Target::Url(u) => u.clone(),
                Target::Anchor(a) => format!("#{a}"),
            };
            let tip = match &ls.title {
                Some(title) if !title.is_empty() => format!("{title}\n{base_tip}"),
                _ => base_tip,
            };
            let clicked = resp.clicked();
            resp.on_hover_text_at_pointer(tip);
            if clicked && can_follow {
                rcx.follow(ui, &ls.target);
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

struct ImageRef<'a> {
    url: &'a str,
    alt: &'a str,
    title: Option<&'a str>,
    width: Option<f32>,
    height: Option<f32>,
    link: Option<&'a Target>,
}

fn render_image(ui: &mut egui::Ui, im: ImageRef<'_>, rcx: &mut Rcx) -> Rect {
    let ImageRef { url, alt, title, width, height, link } = im;
    let mut local_fallback = false;
    let uri = if url.starts_with("bytes://") {
        if url.starts_with("bytes://thundermd/") {
            crate::manual::ensure_demo_images(ui.ctx());
        }
        url.to_string()
    } else if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
        url.to_string()
    } else {
        let p = resolve_path(&markdown::percent_decode(url), rcx.base_dir);
        if !rcx.allow_local_open && !p.is_file() {
            crate::manual::ensure_demo_images(ui.ctx());
            local_fallback = true;
            "bytes://thundermd/local-demo.png".to_string()
        } else {
            file_uri(&p)
        }
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
            img.fit_to_exact_size(vec2(w, 10_000.0)).maintain_aspect_ratio(true)
        }
        (None, Some(h)) if h > 0.0 => img.fit_to_exact_size(vec2(avail, h)).maintain_aspect_ratio(true),
        _ => img.fit_to_original_size(1.0).max_width(avail),
    };
    let can_click_link = link.is_some_and(|t| match t {
        Target::Anchor(_) => true,
        Target::Url(u) => Rcx::is_external_url(u) || rcx.allow_local_open,
    });
    if can_click_link {
        img = img.sense(Sense::click());
    }
    let resp = ui.add(img);
    let r = resp.rect;
    if can_click_link && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let clicked = can_click_link && resp.clicked();
    let mut tip_parts: Vec<String> = Vec::new();
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        tip_parts.push(t.to_string());
    } else if !alt.is_empty() {
        tip_parts.push(alt.to_string());
    }
    if local_fallback {
        tip_parts.push(format!("本地相对路径示例：{url}（请替换成你自己的文件路径）"));
    }
    if let Some(t) = link {
        let dst = match t {
            Target::Url(u) => u.clone(),
            Target::Anchor(a) => format!("#{a}"),
        };
        tip_parts.push(format!("点击跳转：{dst}"));
    }
    if !tip_parts.is_empty() {
        resp.on_hover_text(tip_parts.join("\n"));
    }
    if clicked && let Some(t) = link {
        rcx.follow(ui, t);
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
            Chunk::Image { url, alt, title, width, height, link } => render_image(
                ui,
                ImageRef {
                    url,
                    alt,
                    title: title.as_deref(),
                    width: *width,
                    height: *height,
                    link: link.as_ref(),
                },
                rcx,
            ),
            Chunk::Media(card) => {
                let top = ui.cursor().top();
                media::render_card_opts(ui, card, rcx.th, rcx.base_dir, rcx.allow_local_open);
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

    let scroll_id = rcx.next_scroll_id("table_scroll");
    ui.scope(|ui| {
        let needs_scroll = total_w > ui.available_width() + 0.5;
        let vis = theme::configure_horizontal_scroll(ui, needs_scroll);
        egui::ScrollArea::horizontal()
            .id_salt(scroll_id)
            .scroll_bar_visibility(vis)
            .show(ui, |ui| {
                let mut table_rect = Rect::NOTHING;
                for (ri, row) in galleys.iter().enumerate() {
                    let row_h = row.iter().flatten().map(|(g, _)| g.size().y).fold(th.line_height(), f32::max) + pad.y * 2.0;
                    let (row_rect, _) = ui.allocate_exact_size(vec2(total_w, row_h), Sense::hover());
                    table_rect = table_rect.union(row_rect);
                    if ri == 0 {
                        ui.painter().rect_filled(row_rect, 0, th.table_head_bg);
                    } else if ri % 2 == 0 {
                        ui.painter().rect_filled(row_rect, 0, th.table_stripe_bg);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::ParseOptions;

    fn collect_clipped_rects_and_texts<'a>(
        shape: &'a egui::Shape,
        clip_rect: Rect,
        rects: &mut Vec<(Rect, &'a egui::epaint::RectShape)>,
        texts: &mut Vec<&'a egui::epaint::TextShape>,
    ) {
        match shape {
            egui::Shape::Rect(r) => rects.push((clip_rect, r)),
            egui::Shape::Text(t) => texts.push(t),
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_clipped_rects_and_texts(s, clip_rect, rects, texts);
                }
            }
            _ => {}
        }
    }

    fn visible_h_scroll_handles(shapes: &[egui::epaint::ClippedShape]) -> Vec<Rect> {
        let mut rects = Vec::new();
        let mut texts = Vec::new();
        for cs in shapes {
            collect_clipped_rects_and_texts(&cs.shape, cs.clip_rect, &mut rects, &mut texts);
        }
        rects
            .into_iter()
            .filter_map(|(clip, r)| {
                let is_h_handle = r.rect.width() >= 20.0
                    && r.rect.height() >= 1.5
                    && r.rect.height() <= 8.0
                    && r.fill.a() > 0;
                (is_h_handle && clip.is_positive() && clip.intersect(r.rect).is_positive()).then_some(r.rect)
            })
            .collect()
    }

    #[test]
    fn offscreen_wide_blocks_never_leak_horizontal_scrollbar_while_typing() {
        let ctx = egui::Context::default();
        crate::fonts::setup(&ctx);
        crate::theme::apply(&ctx, ThemeId::ZenHorizon);

        let mut body = String::new();
        for i in 0..18 {
            body.push_str(&format!(
                "这是第 {i} 段普通正文，用来把后续较宽的表格与代码块完全推到首屏视口下方之外。\n\n"
            ));
        }
        // 在 +600px 预渲染区及更下方分别放置超宽表格与超宽代码块
        body.push_str("| 列1 very_long_header_alpha | 列2 very_long_header_beta | 列3 very_long_header_gamma |\n|---|---|---|\n| aaa | bbb | ccc |\n\n");
        for i in 18..34 {
            body.push_str(&format!("这是第 {i} 段间隔正文，确保下一处超宽内容远在视口底部之外。\n\n"));
        }
        body.push_str(
            "| 远端列1 super_wide_column_alpha | 远端列2 super_wide_column_beta | 远端列3 super_wide_column_gamma |\n|---|---|---|\n| 111 | 222 | 333 |\n\n\
             ```rust\nlet very_long_offscreen_code_line = \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789\";\n```\n",
        );

        let th = Theme::from_theme(ThemeId::ZenHorizon, 16.0, 1.6, 0.75);
        let opts = ParseOptions { sup_sub: false, front_matter: false };
        let parsed1 = markdown::parse(&body, &th, &opts);
        let mut edited = body;
        let first_para_end = edited.find("\n\n").unwrap();
        edited.insert(first_para_end, 'p');
        let parsed2 = markdown::parse(&edited, &th, &opts);

        let mut preview = PreviewState::default();
        let preview_opts = PreviewOptions {
            base_dir: None,
            code_line_numbers: true,
            max_content_width: 0.0,
            allow_local_open: true,
        };

        let run = |preview: &mut PreviewState, doc: &Doc, doc_key: u64| {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(460.0, 420.0))),
                focused: true,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = preview.ui(ui, 1, doc, &th, doc_key, preview_opts);
                });
            });
            out.textures_delta.clear();
            visible_h_scroll_handles(&out.shapes)
        };

        for (step, (doc, key)) in [(&parsed1, 1), (&parsed1, 1), (&parsed2, 2), (&parsed2, 2)]
            .into_iter()
            .enumerate()
        {
            let handles = run(&mut preview, doc, key);
            assert!(
                handles.is_empty(),
                "frame {step} (doc_key={key}) leaked offscreen horizontal scrollbar handles: {handles:?}"
            );
        }
    }

    #[test]
    fn visible_wide_table_and_code_preserve_horizontal_scroll_across_reparse() {
        let ctx = egui::Context::default();
        crate::fonts::setup(&ctx);
        crate::theme::apply(&ctx, ThemeId::ZenHorizon);

        let base = "| 列一 very_long_header_alpha | 列二 very_long_header_beta | 列三 very_long_header_gamma | 列四 very_long_header_delta |\n\
                    |---|---|---|---|\n\
                    | 单元格ALPHA_UNIQUE | 单元格BETA | 单元格GAMMA | 单元格DELTA |\n\n\
                    ```rust\n\
                    let CODE_UNIQUE_TOKEN = \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_0123456789\";\n\
                    ```\n\n\
                    底部普通段落。\n";
        let th = Theme::from_theme(ThemeId::ZenHorizon, 16.0, 1.6, 0.75);
        let opts = ParseOptions { sup_sub: false, front_matter: false };
        let parsed1 = markdown::parse(base, &th, &opts);
        let edited = format!("{base}p");
        let parsed2 = markdown::parse(&edited, &th, &opts);

        let mut preview = PreviewState::default();
        let preview_opts = PreviewOptions {
            base_dir: None,
            code_line_numbers: true,
            max_content_width: 0.0,
            allow_local_open: true,
        };

        let run = |preview: &mut PreviewState, doc: &Doc, doc_key: u64, events: Vec<egui::Event>| {
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 500.0))),
                focused: true,
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = preview.ui(ui, 7, doc, &th, doc_key, preview_opts);
                });
            });
            out.textures_delta.clear();
            let handles = visible_h_scroll_handles(&out.shapes);
            let mut rects = Vec::new();
            let mut texts = Vec::new();
            for cs in &out.shapes {
                collect_clipped_rects_and_texts(&cs.shape, cs.clip_rect, &mut rects, &mut texts);
            }
            let cell_pos = texts
                .iter()
                .find(|t| t.galley.text().contains("单元格ALPHA_UNIQUE"))
                .expect("table cell should be painted")
                .pos;
            let code_pos = texts
                .iter()
                .find(|t| t.galley.text().contains("CODE_UNIQUE_TOKEN"))
                .expect("code line should be painted")
                .pos;
            (handles, cell_pos, code_pos)
        };

        // 第 1 帧：确认可见的宽表格与宽代码块正常绘制横向滚动条滑块
        let (handles1, cell1, code1) = run(&mut preview, &parsed1, 1, Vec::new());
        assert!(handles1.len() >= 2, "expected visible horizontal scroll handles for table & code, got {handles1:?}");

        let wheel_left = |pos: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(-80.0, 0.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };

        // 第 2-3 帧：分别横向滚动表格和代码块，并等待平滑滚动动画完全收敛
        let _ = run(&mut preview, &parsed1, 1, wheel_left(cell1 + vec2(40.0, 4.0)));
        let _ = run(&mut preview, &parsed1, 1, wheel_left(code1 + vec2(40.0, 4.0)));
        for _ in 0..15 {
            let _ = run(&mut preview, &parsed1, 1, Vec::new());
        }
        let (_, cell_scrolled, code_scrolled) = run(&mut preview, &parsed1, 1, Vec::new());
        assert!(cell_scrolled.x < cell1.x - 5.0, "table did not scroll horizontally: {cell1:?} -> {cell_scrolled:?}");
        assert!(code_scrolled.x < code1.x - 5.0, "code block did not scroll horizontally: {code1:?} -> {code_scrolled:?}");

        // 文档编辑后重新解析（Doc 处于全新堆地址、doc_key=2），横向滚动位置必须保持不重置
        let (handles_after, cell_after, code_after) = run(&mut preview, &parsed2, 2, Vec::new());
        assert!(handles_after.len() >= 2);
        assert!(
            (cell_after.x - cell_scrolled.x).abs() < 1.0,
            "table horizontal scroll reset across reparse: before={cell_scrolled:?}, after={cell_after:?}"
        );
        assert!(
            (code_after.x - code_scrolled.x).abs() < 1.0,
            "code horizontal scroll reset across reparse: before={code_scrolled:?}, after={code_after:?}"
        );
    }
}
