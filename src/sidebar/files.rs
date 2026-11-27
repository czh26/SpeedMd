//! 侧边栏文件树：懒加载目录、自然排序、新建/内联重命名/移入回收站。

use crate::theme;
use crate::winutil;
use eframe::egui::{self, pos2, vec2, CursorIcon, Key, Rect, Sense};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FsEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Default)]
struct DirState {
    children: Option<Vec<FsEntry>>,
    expanded: bool,
}

#[derive(Default)]
pub struct FilesState {
    pub root: Option<PathBuf>,
    dirs: HashMap<PathBuf, DirState>,
    renaming: Option<(PathBuf, String, bool)>,
    creating: Option<(PathBuf, bool, String, bool)>,
    confirm_delete: Option<(PathBuf, bool)>,
    pub error: Option<String>,
}

pub enum FilesAction {
    OpenFile(PathBuf),
    OpenFolder,
    Renamed { old: PathBuf, new: PathBuf },
    Deleted(PathBuf),
}

/// 自然排序（大小写不敏感，连续数字按数值大小比较，如 `2.md < 10.md`、`第2章 < 第10章`）。
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.char_indices().peekable();
    let mut ib = b.char_indices().peekable();

    while let (Some(&(pa, ca)), Some(&(pb, cb))) = (ia.peek(), ib.peek()) {
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let mut ea = pa;
            while let Some(&(idx, ch)) = ia.peek()
                && ch.is_ascii_digit()
            {
                ea = idx + ch.len_utf8();
                ia.next();
            }
            let mut eb = pb;
            while let Some(&(idx, ch)) = ib.peek()
                && ch.is_ascii_digit()
            {
                eb = idx + ch.len_utf8();
                ib.next();
            }
            let sa = &a[pa..ea];
            let sb = &b[pb..eb];
            let ta = sa.trim_start_matches('0');
            let tb = sb.trim_start_matches('0');
            match ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)) {
                Ordering::Equal => match sb.len().cmp(&sa.len()) {
                    Ordering::Equal => continue,
                    ord => return ord,
                },
                ord => return ord,
            }
        } else {
            ia.next();
            ib.next();
            let la = ca.to_lowercase();
            let lb = cb.to_lowercase();
            match la.cmp(lb) {
                Ordering::Equal => {
                    if ca != cb {
                        return ca.cmp(&cb);
                    }
                }
                ord => return ord,
            }
        }
    }

    ia.peek().is_some().cmp(&ib.peek().is_some())
}

fn read_dir_entries(dir: &Path) -> Vec<FsEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<FsEntry> = rd
        .flatten()
        .map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            FsEntry { path, name, is_dir }
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural_cmp(&a.name, &b.name)));
    entries
}

impl FilesState {
    pub fn set_root(&mut self, root: Option<&Path>) {
        let clean = root.map(winutil::clean_path);
        if self.root == clean {
            return;
        }
        self.root = clean.clone();
        self.dirs.clear();
        self.renaming = None;
        self.creating = None;
        self.confirm_delete = None;
        if let Some(r) = clean {
            self.dirs.insert(r, DirState { children: None, expanded: true });
        }
    }

    pub fn mark_dirty(&mut self, path: &Path) {
        let clean = winutil::clean_path(path);
        if let Some(st) = self.dirs.get_mut(&clean) {
            st.children = None;
        }
        if let Some(parent) = clean.parent()
            && let Some(st) = self.dirs.get_mut(parent)
        {
            st.children = None;
        }
    }

    pub fn refresh_expanded(&mut self) {
        for st in self.dirs.values_mut() {
            if st.expanded {
                st.children = None;
            }
        }
    }

    fn ensure_loaded(&mut self, dir: &Path) {
        let st = self.dirs.entry(dir.to_path_buf()).or_default();
        if st.children.is_none() {
            st.children = Some(read_dir_entries(dir));
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, active_path: Option<&Path>) -> Option<FilesAction> {
        let mut action: Option<FilesAction> = None;

        let Some(root) = self.root.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.weak("尚未打开文件夹");
                ui.add_space(8.0);
                if ui.button("打开文件夹…").clicked() {
                    action = Some(FilesAction::OpenFolder);
                }
            });
            return action;
        };

        ui.horizontal(|ui| {
            let root_name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("↻").on_hover_text("刷新").clicked() {
                    self.refresh_expanded();
                }
                if ui.small_button("📁+").on_hover_text("新建文件夹").clicked() {
                    self.creating = Some((root.clone(), true, "新建文件夹".into(), true));
                }
                if ui.small_button("📄+").on_hover_text("新建笔记").clicked() {
                    self.creating = Some((root.clone(), false, "新建笔记.md".into(), true));
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_width(0.0);
                    ui.add(egui::Label::new(egui::RichText::new(root_name).strong()).truncate())
                        .on_hover_text(root.display().to_string());
                });
            });
        });
        ui.separator();

        egui::ScrollArea::vertical().id_salt("sidebar_files_scroll").auto_shrink(false).show(ui, |ui| {
            self.render_dir(ui, &root, 0, active_path, &mut action);
        });

        self.delete_confirm_dialog(ctx, &mut action);
        self.error_dialog(ctx);
        action
    }

    fn render_dir(
        &mut self,
        ui: &mut egui::Ui,
        dir: &Path,
        depth: usize,
        active_path: Option<&Path>,
        action: &mut Option<FilesAction>,
    ) {
        self.ensure_loaded(dir);
        let entries = self.dirs.get(dir).and_then(|s| s.children.clone()).unwrap_or_default();

        // 若正在此目录下新建文件/文件夹，在顶部渲染行内输入框
        let creating_here = self.creating.as_ref().is_some_and(|(p, _, _, _)| p == dir);
        if creating_here {
            self.render_creating_row(ui, depth);
        }

        for entry in entries {
            if entry.is_dir {
                let expanded = self.dirs.get(&entry.path).is_some_and(|s| s.expanded);
                let is_renaming = self.renaming.as_ref().is_some_and(|(p, _, _)| p == &entry.path);
                if is_renaming {
                    self.render_rename_row(ui, depth, action);
                } else {
                    let mut toggle = false;
                    let resp = ui.horizontal(|ui| {
                        ui.add_space(depth as f32 * 14.0);
                        let arrow = if expanded { "▾" } else { "▸" };
                        let r = ui.add(
                            egui::Label::new(format!("{arrow} 📁 {}", entry.name))
                                .sense(Sense::click())
                                .selectable(false),
                        );
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if r.clicked() {
                            toggle = true;
                        }
                        r
                    }).inner;

                    resp.context_menu(|ui| {
                        if ui.button("新建笔记").clicked() {
                            self.dirs.entry(entry.path.clone()).or_default().expanded = true;
                            self.creating = Some((entry.path.clone(), false, "新建笔记.md".into(), true));
                            ui.close();
                        }
                        if ui.button("新建文件夹").clicked() {
                            self.dirs.entry(entry.path.clone()).or_default().expanded = true;
                            self.creating = Some((entry.path.clone(), true, "新建文件夹".into(), true));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("重命名").clicked() {
                            self.renaming = Some((entry.path.clone(), entry.name.clone(), true));
                            ui.close();
                        }
                        if ui.button("移入回收站").clicked() {
                            self.confirm_delete = Some((entry.path.clone(), true));
                            ui.close();
                        }
                        if ui.button("在资源管理器中显示").clicked() {
                            winutil::reveal_in_explorer(&entry.path);
                            ui.close();
                        }
                    });

                    if toggle {
                        let st = self.dirs.entry(entry.path.clone()).or_default();
                        st.expanded = !st.expanded;
                        if st.expanded {
                            st.children = None;
                        }
                    }
                }
                if self.dirs.get(&entry.path).is_some_and(|s| s.expanded) {
                    self.render_dir(ui, &entry.path, depth + 1, active_path, action);
                }
            } else {
                let is_renaming = self.renaming.as_ref().is_some_and(|(p, _, _)| p == &entry.path);
                if is_renaming {
                    self.render_rename_row(ui, depth, action);
                } else {
                    let is_active = active_path.is_some_and(|ap| paths_eq(ap, &entry.path));
                    let sc = &theme::current(ui.ctx()).sidebar;
                    let row_resp = ui.horizontal(|ui| {
                        ui.add_space(depth as f32 * 14.0 + 14.0);
                        let icon = if is_markdown_file(&entry.path) { "📝" } else { "📄" };
                        let r = ui.selectable_label(is_active, format!("{icon} {}", entry.name));
                        if r.clicked() {
                            *action = Some(FilesAction::OpenFile(entry.path.clone()));
                        }
                        r
                    });
                    if is_active {
                        let row_r = row_resp.response.rect;
                        let bar_x = ui.max_rect().left() - 10.0;
                        ui.painter().rect_filled(
                            Rect::from_min_size(pos2(bar_x, row_r.top() + 2.0), vec2(2.0, (row_r.height() - 4.0).max(8.0))),
                            1.0,
                            sc.active_bar,
                        );
                    }
                    let resp = row_resp.inner;

                    resp.context_menu(|ui| {
                        if ui.button("打开").clicked() {
                            *action = Some(FilesAction::OpenFile(entry.path.clone()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("重命名").clicked() {
                            self.renaming = Some((entry.path.clone(), entry.name.clone(), true));
                            ui.close();
                        }
                        if ui.button("移入回收站").clicked() {
                            self.confirm_delete = Some((entry.path.clone(), false));
                            ui.close();
                        }
                        if ui.button("在资源管理器中显示").clicked() {
                            winutil::reveal_in_explorer(&entry.path);
                            ui.close();
                        }
                    });
                }
            }
        }
    }

    fn render_rename_row(&mut self, ui: &mut egui::Ui, depth: usize, action: &mut Option<FilesAction>) {
        let Some((old_path, mut buf, mut need_focus)) = self.renaming.take() else { return };
        let mut commit = false;
        let mut cancel = false;

        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0 + 12.0);
            let resp = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(ui.available_width() - 8.0));
            if need_focus {
                resp.request_focus();
                need_focus = false;
            }
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    commit = true;
                } else {
                    cancel = true;
                }
            }
        });

        if cancel {
            return;
        }
        if commit {
            let trimmed = buf.trim();
            if !trimmed.is_empty()
                && let Some(parent) = old_path.parent()
            {
                let new_path = parent.join(trimmed);
                if new_path != old_path {
                    match std::fs::rename(&old_path, &new_path) {
                        Ok(()) => {
                            self.mark_dirty(&old_path);
                            self.mark_dirty(&new_path);
                            *action = Some(FilesAction::Renamed { old: old_path, new: new_path });
                        }
                        Err(e) => self.error = Some(format!("重命名失败：{e}")),
                    }
                }
            }
            return;
        }
        self.renaming = Some((old_path, buf, need_focus));
    }

    fn render_creating_row(&mut self, ui: &mut egui::Ui, depth: usize) {
        let Some((parent, is_dir, mut buf, mut need_focus)) = self.creating.take() else { return };
        let mut commit = false;
        let mut cancel = false;

        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0 + 12.0);
            let resp = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(ui.available_width() - 8.0));
            if need_focus {
                resp.request_focus();
                need_focus = false;
            }
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    commit = true;
                } else {
                    cancel = true;
                }
            }
        });

        if cancel {
            return;
        }
        if commit {
            let mut name = buf.trim().to_string();
            if !name.is_empty() {
                if !is_dir && !name.contains('.') {
                    name.push_str(".md");
                }
                let target = parent.join(&name);
                let res = if is_dir {
                    std::fs::create_dir(&target)
                } else {
                    std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map(|_| ())
                };
                match res {
                    Ok(()) => self.mark_dirty(&target),
                    Err(e) => self.error = Some(format!("创建失败：{e}")),
                }
            }
            return;
        }
        self.creating = Some((parent, is_dir, buf, need_focus));
    }

    fn delete_confirm_dialog(&mut self, ctx: &egui::Context, action: &mut Option<FilesAction>) {
        let Some((path, is_dir)) = self.confirm_delete.clone() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let kind = if is_dir { "文件夹" } else { "文件" };
        let mut choice = None;

        egui::Window::new("确认移入回收站")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("确定将{kind}「{name}」移入回收站吗？"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("移入回收站").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("取消").clicked() {
                        choice = Some(false);
                    }
                });
            });

        match choice {
            Some(true) => {
                self.confirm_delete = None;
                match winutil::move_to_trash(&path) {
                    Ok(()) => {
                        self.mark_dirty(&path);
                        *action = Some(FilesAction::Deleted(path));
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Some(false) => self.confirm_delete = None,
            None => {}
        }
    }

    fn error_dialog(&mut self, ctx: &egui::Context) {
        let Some(msg) = self.error.clone() else { return };
        egui::Window::new("文件操作错误")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(msg);
                if ui.button("确定").clicked() {
                    self.error = None;
                }
            });
    }
}

pub fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown"))
}

pub fn paths_eq(a: &Path, b: &Path) -> bool {
    winutil::clean_path(a).to_string_lossy().eq_ignore_ascii_case(&winutil::clean_path(b).to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sorting_orders_numbers_and_cjk_correctly() {
        let mut names = ["10.md", "2.md", "1.md", "a20.md", "a3.md", "第10章.md", "第2章.md"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["1.md", "2.md", "10.md", "a3.md", "a20.md", "第2章.md", "第10章.md"]);
    }
}
