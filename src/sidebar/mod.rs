//! 侧边栏容器与文件监听协调（文件树 / 文章列表 / 大纲）。

pub mod articles;
pub mod files;
pub mod outline;

use crate::markdown::{bold_family, Heading};
use crate::settings::SidebarTab;
use crate::theme::{self, SIDEBAR_DEFAULT_W, SIDEBAR_HEADER_H, SIDEBAR_MAX_W, SIDEBAR_MIN_W};
use crate::winutil;
use articles::{ArticlesAction, ArticlesState};
use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Frame, Margin, Rect, Sense, Stroke};
use files::{FilesAction, FilesState};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use outline::OutlineState;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

pub enum SidebarAction {
    OpenFile(PathBuf),
    OpenFolder,
    Renamed { old: PathBuf, new: PathBuf },
    Deleted(PathBuf),
    JumpToOutline { anchor: String, byte_offset: usize },
}

#[derive(Default)]
pub struct SidebarState {
    pub files: FilesState,
    pub articles: ArticlesState,
    pub outline: OutlineState,
    watch_root: Option<PathBuf>,
    watcher: Option<RecommendedWatcher>,
    watch_rx: Option<mpsc::Receiver<Vec<PathBuf>>>,
    prev_focused: bool,
}

impl SidebarState {
    pub fn sync_root(&mut self, ctx: &egui::Context, root: Option<&Path>) {
        let clean = root.map(winutil::clean_path);
        if self.watch_root != clean {
            self.watch_root = clean.clone();
            self.watcher = None;
            self.watch_rx = None;
            if let Some(dir) = &clean {
                let (tx, rx) = mpsc::channel();
                let ctx2 = ctx.clone();
                if let Ok(mut w) = notify::recommended_watcher(move |res: notify::Result<Event>| {
                    if let Ok(ev) = res
                        && !ev.paths.is_empty()
                    {
                        let _ = tx.send(ev.paths);
                        ctx2.request_repaint();
                    }
                }) && w.watch(dir, RecursiveMode::Recursive).is_ok()
                {
                    self.watcher = Some(w);
                    self.watch_rx = Some(rx);
                }
            }
        }
        self.files.set_root(clean.as_deref());
        self.articles.set_root(ctx, clean.as_deref());
    }

    pub fn poll_watchers(&mut self, ctx: &egui::Context) {
        let mut any_fs_change = false;
        if let Some(rx) = &self.watch_rx {
            while let Ok(paths) = rx.try_recv() {
                any_fs_change = true;
                for p in paths {
                    self.files.mark_dirty(&p);
                }
            }
        }
        if any_fs_change {
            self.articles.trigger_scan(ctx);
        }

        let focused = ctx.input(|i| i.focused);
        if focused && !self.prev_focused && self.watch_root.is_some() {
            self.files.refresh_expanded();
            self.articles.trigger_scan(ctx);
        }
        self.prev_focused = focused;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        tab: &mut SidebarTab,
        active_path: Option<&Path>,
        doc_uid: u64,
        headings: &[Heading],
        top_byte: usize,
    ) -> (Option<SidebarAction>, f32) {
        let mut action: Option<SidebarAction> = None;
        let spec = theme::current(ctx);
        let sc = &spec.sidebar;

        let panel_resp = egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(SIDEBAR_DEFAULT_W)
            .size_range(SIDEBAR_MIN_W..=SIDEBAR_MAX_W)
            .show_separator_line(false)
            .frame(Frame::new().fill(sc.bg).inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                *ui.visuals_mut() = theme::sidebar_visuals(ctx).clone();

                // 顶部紧凑分段条：文件 | 文章 | 大纲 + 右对齐 📂
                let (hdr_rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), SIDEBAR_HEADER_H), Sense::hover());
                let folder_rect = Rect::from_min_size(
                    pos2(hdr_rect.right() - 32.0, hdr_rect.top() + 5.0),
                    vec2(24.0, 24.0),
                );
                let folder_resp = ui.interact(folder_rect, ui.make_persistent_id("sb_open_folder"), Sense::click());
                if folder_resp.hovered() {
                    ui.painter().rect_filled(folder_rect, 5.0, sc.hover_bg);
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                ui.painter().text(
                    folder_rect.center(),
                    Align2::CENTER_CENTER,
                    "📂",
                    FontId::proportional(13.5),
                    if folder_resp.hovered() { sc.strong_fg } else { sc.muted_fg },
                );
                if folder_resp.on_hover_text("打开文件夹…").clicked() {
                    action = Some(SidebarAction::OpenFolder);
                }

                let tabs = [
                    (SidebarTab::Files, "文件"),
                    (SidebarTab::Articles, "文章"),
                    (SidebarTab::Outline, "大纲"),
                ];
                let mut x = hdr_rect.left() + 10.0;
                for (t_val, t_label) in tabs {
                    let is_cur = *tab == t_val;
                    let font = if is_cur {
                        FontId::new(13.0, bold_family())
                    } else {
                        FontId::proportional(13.0)
                    };
                    let w = ui
                        .fonts_mut(|f| f.layout_no_wrap(t_label.into(), font.clone(), sc.fg))
                        .size()
                        .x
                        + 12.0;
                    let r = Rect::from_min_size(pos2(x, hdr_rect.top() + 3.0), vec2(w, SIDEBAR_HEADER_H - 4.0));
                    let resp = ui.interact(r, ui.make_persistent_id(("sb_tab", t_label)), Sense::click());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        if !is_cur {
                            ui.painter().rect_filled(r.shrink(2.0), 4.0, sc.hover_bg);
                        }
                    }
                    ui.painter().text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        t_label,
                        font,
                        if is_cur { sc.strong_fg } else { sc.muted_fg },
                    );
                    if is_cur {
                        ui.painter().hline(
                            (r.left() + 3.0)..=(r.right() - 3.0),
                            hdr_rect.bottom() - 1.0,
                            Stroke::new(2.0, sc.tab_underline),
                        );
                    }
                    if resp.clicked() {
                        *tab = t_val;
                    }
                    x += w + 4.0;
                }

                ui.painter().hline(hdr_rect.x_range(), hdr_rect.bottom() - 0.5, Stroke::new(1.0, sc.border));

                Frame::new()
                    .inner_margin(Margin { left: 10, right: 10, top: 8, bottom: 8 })
                    .show(ui, |ui| match tab {
                        SidebarTab::Files => {
                            if let Some(act) = self.files.ui(ui, ctx, active_path) {
                                action = Some(match act {
                                    FilesAction::OpenFile(p) => SidebarAction::OpenFile(p),
                                    FilesAction::OpenFolder => SidebarAction::OpenFolder,
                                    FilesAction::Renamed { old, new } => SidebarAction::Renamed { old, new },
                                    FilesAction::Deleted(p) => SidebarAction::Deleted(p),
                                });
                            }
                        }
                        SidebarTab::Articles => {
                            if let Some(act) = self.articles.ui(ui, ctx, active_path) {
                                action = Some(match act {
                                    ArticlesAction::OpenFile(p) => SidebarAction::OpenFile(p),
                                    ArticlesAction::OpenFolder => SidebarAction::OpenFolder,
                                });
                            }
                        }
                        SidebarTab::Outline => {
                            if let Some(jump) = self.outline.ui(ui, doc_uid, headings, top_byte) {
                                action = Some(SidebarAction::JumpToOutline {
                                    anchor: jump.anchor,
                                    byte_offset: jump.byte_offset,
                                });
                            }
                        }
                    });
            });

        let panel_rect = panel_resp.response.rect;
        if sc.outer_border != Color32::TRANSPARENT {
            ui.painter()
                .vline(panel_rect.right() - 0.5, panel_rect.y_range(), Stroke::new(1.0, sc.outer_border));
        }
        if let theme::DividerStyle::GlassInset { top_highlight, .. } = spec.divider {
            ui.painter()
                .hline(panel_rect.x_range(), panel_rect.top() + 0.6, Stroke::new(1.0, top_highlight));
        }

        (action, panel_rect.width())
    }
}
