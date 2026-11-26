//! 侧边栏文章列表：后台扫描工作目录（每个 .md 只读前 8 KB），按 (路径, 修改时间) 缓存。

use crate::sidebar::files::{is_markdown_file, natural_cmp, paths_eq};
use crate::stats::strip_front_matter;
use crate::theme;
use crate::winutil;
use eframe::egui::{self, pos2, vec2, CursorIcon, Rect, Sense, Stroke, StrokeKind};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::SystemTime;

const MAX_READ_BYTES: u64 = 8192;

#[derive(Clone, Debug)]
pub struct ArticleItem {
    pub path: PathBuf,
    pub rel_path: String,
    pub title: String,
    pub summary: String,
    pub modified: SystemTime,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    ModifiedDesc,
    TitleAsc,
}

#[derive(Default)]
pub struct ArticlesState {
    pub root: Option<PathBuf>,
    cache: HashMap<PathBuf, (SystemTime, ArticleItem)>,
    items: Vec<ArticleItem>,
    search: String,
    sort: SortMode,
    scan_rx: Option<mpsc::Receiver<ScanOutput>>,
    rescan_pending: bool,
}

struct ScanOutput {
    root: PathBuf,
    cache: HashMap<PathBuf, (SystemTime, ArticleItem)>,
    items: Vec<ArticleItem>,
}

pub enum ArticlesAction {
    OpenFile(PathBuf),
    OpenFolder,
}

fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target")
}

/// 从前 8 KB Markdown 内容提取标题（Front Matter `title` → 首个 H1 → 文件名）及约 80 字摘要。
pub fn extract_title_and_summary(raw: &str, path: &Path) -> (String, String) {
    let mut fm_title: Option<String> = None;
    let body = strip_front_matter(raw);
    if body.len() != raw.len() {
        let fm = &raw[..raw.len() - body.len()];
        for line in fm.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("title:") {
                let val = rest.trim().trim_matches(|c| matches!(c, '"' | '\''));
                if !val.is_empty() {
                    fm_title = Some(val.to_string());
                    break;
                }
            }
        }
    }

    let mut h1_title: Option<String> = None;
    let mut summary = String::new();
    let mut in_fence = false;

    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || t.is_empty() {
            continue;
        }
        if let Some(rest) = t.strip_prefix("# ")
            && h1_title.is_none()
        {
            let h = rest.trim().to_string();
            if !h.is_empty() {
                h1_title = Some(h);
                continue;
            }
        }
        if t.starts_with('#') || t.starts_with("---") || t.starts_with("[TOC]") {
            continue;
        }
        let cleaned = t
            .trim_start_matches(['>', '-', '*', '+'])
            .replace("**", "")
            .replace('`', "");
        let trimmed = cleaned.trim();
        if !trimmed.is_empty() {
            if !summary.is_empty() {
                summary.push(' ');
            }
            summary.push_str(trimmed);
            if summary.chars().count() >= 85 {
                break;
            }
        }
    }

    let fallback = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "未命名".into());
    let title = fm_title.or(h1_title).unwrap_or(fallback);
    if summary.chars().count() > 80 {
        let truncated: String = summary.chars().take(80).collect();
        summary = format!("{truncated}…");
    }
    (title, summary)
}

fn read_article_head(path: &Path, root: &Path, modified: SystemTime) -> Option<ArticleItem> {
    let file = File::open(path).ok()?;
    let mut buf = Vec::with_capacity(MAX_READ_BYTES as usize);
    file.take(MAX_READ_BYTES).read_to_end(&mut buf).ok()?;
    let slice = buf.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&buf);
    let raw = String::from_utf8_lossy(slice);
    let (title, summary) = extract_title_and_summary(&raw, path);
    let rel_path = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    Some(ArticleItem { path: path.to_path_buf(), rel_path, title, summary, modified })
}

fn walk_articles(
    dir: &Path,
    root: &Path,
    prev: &HashMap<PathBuf, (SystemTime, ArticleItem)>,
    out_cache: &mut HashMap<PathBuf, (SystemTime, ArticleItem)>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_s = name.to_string_lossy();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            if !should_skip_dir(&name_s) {
                walk_articles(&path, root, prev, out_cache);
            }
        } else if ft.is_file() && is_markdown_file(&path) {
            let clean = winutil::clean_path(&path);
            let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            if let Some((cached_m, item)) = prev.get(&clean)
                && *cached_m == mtime
            {
                out_cache.insert(clean, (*cached_m, item.clone()));
            } else if let Some(item) = read_article_head(&clean, root, mtime) {
                out_cache.insert(clean, (mtime, item));
            }
        }
    }
}

impl ArticlesState {
    pub fn set_root(&mut self, ctx: &egui::Context, root: Option<&Path>) {
        let clean = root.map(winutil::clean_path);
        if self.root == clean {
            return;
        }
        self.root = clean;
        self.cache.clear();
        self.items.clear();
        self.scan_rx = None;
        self.rescan_pending = false;
        self.trigger_scan(ctx);
    }

    pub fn trigger_scan(&mut self, ctx: &egui::Context) {
        let Some(root) = self.root.clone() else { return };
        if self.scan_rx.is_some() {
            self.rescan_pending = true;
            return;
        }
        let prev = self.cache.clone();
        let ctx2 = ctx.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut next_cache = HashMap::new();
            walk_articles(&root, &root, &prev, &mut next_cache);
            let items: Vec<ArticleItem> = next_cache.values().map(|(_, it)| it.clone()).collect();
            let _ = tx.send(ScanOutput { root, cache: next_cache, items });
            ctx2.request_repaint();
        });
        self.scan_rx = Some(rx);
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.scan_rx
            && let Ok(out) = rx.try_recv()
        {
            self.scan_rx = None;
            if self.root.as_ref() == Some(&out.root) {
                self.cache = out.cache;
                self.items = out.items;
                self.sort_items();
            }
            if self.rescan_pending {
                self.rescan_pending = false;
                self.trigger_scan(ctx);
            }
        }
    }

    fn sort_items(&mut self) {
        match self.sort {
            SortMode::ModifiedDesc => self.items.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| natural_cmp(&a.title, &b.title))),
            SortMode::TitleAsc => self.items.sort_by(|a, b| natural_cmp(&a.title, &b.title)),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, active_path: Option<&Path>) -> Option<ArticlesAction> {
        self.poll(ctx);

        if self.root.is_none() {
            let mut act = None;
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.weak("尚未打开文件夹");
                ui.add_space(8.0);
                if ui.button("打开文件夹…").clicked() {
                    act = Some(ArticlesAction::OpenFolder);
                }
            });
            return act;
        }

        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).desired_width(ui.available_width() - 62.0).hint_text("筛选文章…"));
            let sort_label = match self.sort {
                SortMode::ModifiedDesc => "时间",
                SortMode::TitleAsc => "标题",
            };
            if ui.button(sort_label).on_hover_text("切换排序方式").clicked() {
                self.sort = match self.sort {
                    SortMode::ModifiedDesc => SortMode::TitleAsc,
                    SortMode::TitleAsc => SortMode::ModifiedDesc,
                };
                self.sort_items();
            }
        });
        if self.scan_rx.is_some() {
            ui.weak("正在扫描文章…");
        }
        ui.separator();

        let query = self.search.trim().to_ascii_lowercase();
        let mut open_path: Option<PathBuf> = None;

        let sc = &theme::current(ctx).sidebar;
        egui::ScrollArea::vertical().id_salt("sidebar_articles_scroll").auto_shrink(false).show(ui, |ui| {
            let mut shown = 0usize;
            for item in &self.items {
                if !query.is_empty()
                    && !item.title.to_ascii_lowercase().contains(&query)
                    && !item.summary.to_ascii_lowercase().contains(&query)
                    && !item.rel_path.to_ascii_lowercase().contains(&query)
                {
                    continue;
                }
                shown += 1;
                let selected = active_path.is_some_and(|p| paths_eq(p, &item.path));
                let bg = if selected { sc.active_bg } else { egui::Color32::TRANSPARENT };
                let stroke = if selected { Stroke::new(1.0, sc.border) } else { Stroke::NONE };
                let card_resp = egui::Frame::new()
                    .fill(bg)
                    .stroke(stroke)
                    .corner_radius(5)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.strong(&item.title);
                        if !item.summary.is_empty() {
                            ui.add(egui::Label::new(egui::RichText::new(&item.summary).small().weak()).truncate());
                        }
                        ui.weak(egui::RichText::new(&item.rel_path).small());
                    })
                    .response;
                if selected {
                    let r = card_resp.rect;
                    let bar_x = ui.max_rect().left() - 10.0;
                    ui.painter().rect_filled(
                        Rect::from_min_size(pos2(bar_x, r.top() + 4.0), vec2(2.0, (r.height() - 8.0).max(10.0))),
                        1.0,
                        sc.active_bar,
                    );
                }
                let resp = card_resp.interact(Sense::click());
                if resp.hovered() && !selected {
                    ui.painter()
                        .rect_stroke(resp.rect, 5.0, Stroke::new(1.0, sc.border), StrokeKind::Inside);
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    open_path = Some(item.path.clone());
                }
                ui.add_space(4.0);
            }
            if shown == 0 && self.scan_rx.is_none() {
                ui.weak("没有匹配的 Markdown 文章");
            }
        });

        open_path.map(ArticlesAction::OpenFile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_title_front_matter_h1_and_summary() {
        let md = "---\ntitle: \"Front Title\"\n---\n\n# Ignored H1\n\n第一段**摘要**内容，用于测试自动提取。\n";
        let (title, summary) = extract_title_and_summary(md, Path::new("note.md"));
        assert_eq!(title, "Front Title");
        assert!(summary.contains("第一段摘要内容"));

        let md2 = "# 一级标题\n\n正文开始";
        let (t2, s2) = extract_title_and_summary(md2, Path::new("note.md"));
        assert_eq!(t2, "一级标题");
        assert_eq!(s2, "正文开始");

        let (t3, _) = extract_title_and_summary("只有普通正文", Path::new("fallback_name.md"));
        assert_eq!(t3, "fallback_name");
    }
}
