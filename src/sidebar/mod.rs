//! 侧边栏容器与文件监听协调（文件树 / 文章列表 / 大纲）。

pub mod articles;
pub mod files;
pub mod outline;

use crate::markdown::Heading;
use crate::settings::SidebarTab;
use crate::winutil;
use articles::{ArticlesAction, ArticlesState};
use eframe::egui;
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
    ) -> Option<SidebarAction> {
        let mut action: Option<SidebarAction> = None;

        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(250.0)
            .size_range(170.0..=500.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(tab, SidebarTab::Files, "文件");
                    ui.selectable_value(tab, SidebarTab::Articles, "文章");
                    ui.selectable_value(tab, SidebarTab::Outline, "大纲");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("📂").on_hover_text("打开文件夹…").clicked() {
                            action = Some(SidebarAction::OpenFolder);
                        }
                    });
                });
                ui.separator();

                match tab {
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
                }
            });

        action
    }
}
