//! 多标签页栏 UI、重名消歧、拖拽重排、右键菜单。

use crate::document::Document;
use eframe::egui::{self, vec2, Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, Sense, Stroke, StrokeKind};
use std::collections::HashMap;
use std::path::PathBuf;

pub enum TabAction {
    Select(usize),
    Close(usize),
    CloseOthers(usize),
    CloseRight(usize),
    NewTab,
    Save(usize),
    CopyPath(PathBuf),
    Reveal(PathBuf),
    Move { from: usize, to: usize },
}

#[derive(Clone, Default)]
struct DragState {
    dragging_uid: Option<u64>,
}

/// 生成每个标签的显示标题；若存在同名文件则附加 `— 父目录` 消歧。
pub fn tab_titles(docs: &[Document]) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for doc in docs {
        if doc.path.is_some() {
            *counts.entry(doc.display_name().to_lowercase()).or_insert(0) += 1;
        }
    }
    docs.iter()
        .map(|doc| {
            let name = doc.display_name();
            if let Some(path) = &doc.path
                && counts.get(&name.to_lowercase()).copied().unwrap_or(0) > 1
                && let Some(parent) = path.parent().and_then(|p| p.file_name())
            {
                return format!("{name} — {}", parent.to_string_lossy());
            }
            name
        })
        .collect()
}

pub fn show(ui: &mut egui::Ui, docs: &[Document], active: usize) -> Option<TabAction> {
    let mut action: Option<TabAction> = None;
    let titles = tab_titles(docs);
    let dark = ui.visuals().dark_mode;
    let active_bg = if dark { Color32::from_rgb(0x28, 0x2c, 0x34) } else { Color32::WHITE };
    let idle_bg = if dark { Color32::from_rgb(0x1d, 0x20, 0x26) } else { Color32::from_rgb(0xed, 0xef, 0xf2) };
    let hover_bg = if dark { Color32::from_rgb(0x23, 0x27, 0x2e) } else { Color32::from_rgb(0xe2, 0xe6, 0xec) };
    let border_col = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let accent = ui.visuals().selection.bg_fill;
    let text_col = ui.visuals().text_color();
    let weak_col = ui.visuals().weak_text_color();

    let drag_id = Id::unique("tabs_drag_state");
    let mut drag = ui.ctx().data_mut(|d| d.get_temp::<DragState>(drag_id).map(|s| s.dragging_uid).unwrap_or(None));
    let mut rects: Vec<Rect> = Vec::with_capacity(docs.len());

    egui::ScrollArea::horizontal()
        .id_salt("tabs_scroll")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let tab_h = 26.0;
                let font = FontId::proportional(13.0);

                for (i, doc) in docs.iter().enumerate() {
                    let is_active = i == active;
                    let prefix = if doc.dirty { "● " } else { "" };
                    let label = format!("{prefix}{}", titles[i]);
                    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.clone(), font.clone(), text_col));
                    let title_w = galley.size().x.clamp(48.0, 220.0);
                    let close_w = 18.0;
                    let total_w = title_w + close_w + 18.0;

                    let (pill_rect, _) = ui.allocate_exact_size(vec2(total_w, tab_h), Sense::hover());
                    rects.push(pill_rect);

                    let title_rect = Rect::from_min_size(pill_rect.min, vec2(total_w - close_w - 4.0, tab_h));
                    let close_rect = Rect::from_min_size(
                        Pos2::new(pill_rect.right() - close_w - 4.0, pill_rect.top()),
                        vec2(close_w, tab_h),
                    );

                    let title_resp = ui.interact(title_rect, Id::unique(("tab_title", doc.uid)), Sense::click_and_drag());
                    let close_resp = ui.interact(close_rect, Id::unique(("tab_close", doc.uid)), Sense::click());

                    let hovered = title_resp.hovered() || close_resp.hovered();
                    let bg = if is_active {
                        active_bg
                    } else if hovered {
                        hover_bg
                    } else {
                        idle_bg
                    };
                    ui.painter().rect_filled(pill_rect, 4.0, bg);
                    if is_active {
                        ui.painter().rect_stroke(pill_rect, 4.0, Stroke::new(1.0, border_col), StrokeKind::Inside);
                        ui.painter().hline(
                            (pill_rect.left() + 4.0)..=(pill_rect.right() - 4.0),
                            pill_rect.top() + 1.0,
                            Stroke::new(2.0, accent),
                        );
                    }

                    let text_pos = Pos2::new(pill_rect.left() + 8.0, pill_rect.center().y - galley.size().y * 0.5);
                    let clipped = ui.painter().with_clip_rect(title_rect.shrink2(vec2(4.0, 0.0)));
                    if doc.dirty {
                        clipped.text(
                            Pos2::new(pill_rect.left() + 8.0, pill_rect.center().y),
                            Align2::LEFT_CENTER,
                            "●",
                            FontId::proportional(11.0),
                            Color32::from_rgb(0xe0, 0x9a, 0x3e),
                        );
                        clipped.text(
                            Pos2::new(pill_rect.left() + 20.0, pill_rect.center().y),
                            Align2::LEFT_CENTER,
                            &titles[i],
                            font.clone(),
                            if is_active { text_col } else { weak_col },
                        );
                    } else {
                        let _ = text_pos;
                        clipped.text(
                            Pos2::new(pill_rect.left() + 8.0, pill_rect.center().y),
                            Align2::LEFT_CENTER,
                            &titles[i],
                            font.clone(),
                            if is_active { text_col } else { weak_col },
                        );
                    }

                    if close_resp.hovered() {
                        ui.painter().rect_filled(close_rect.shrink(3.0), 3.0, hover_bg);
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    ui.painter().text(
                        close_rect.center(),
                        Align2::CENTER_CENTER,
                        "×",
                        FontId::proportional(14.0),
                        if close_resp.hovered() { text_col } else { weak_col },
                    );

                    if let Some(p) = &doc.path {
                        let tip = p.display().to_string();
                        title_resp.clone().on_hover_text(tip);
                    }

                    if close_resp.clicked() || close_resp.middle_clicked() || title_resp.middle_clicked() {
                        action = Some(TabAction::Close(i));
                    } else if title_resp.clicked() {
                        action = Some(TabAction::Select(i));
                    }

                    if title_resp.drag_started() {
                        drag = Some(doc.uid);
                    }
                    if drag == Some(doc.uid) {
                        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    }

                    title_resp.context_menu(|ui| {
                        if ui.button("关闭").clicked() {
                            action = Some(TabAction::Close(i));
                            ui.close();
                        }
                        if docs.len() > 1 && ui.button("关闭其他标签").clicked() {
                            action = Some(TabAction::CloseOthers(i));
                            ui.close();
                        }
                        if i + 1 < docs.len() && ui.button("关闭右侧标签").clicked() {
                            action = Some(TabAction::CloseRight(i));
                            ui.close();
                        }
                        if !doc.is_manual {
                            ui.separator();
                            if ui.button("保存").clicked() {
                                action = Some(TabAction::Save(i));
                                ui.close();
                            }
                        }
                        if let Some(path) = &doc.path {
                            if ui.button("复制文件路径").clicked() {
                                action = Some(TabAction::CopyPath(path.clone()));
                                ui.close();
                            }
                            if ui.button("在资源管理器中显示").clicked() {
                                action = Some(TabAction::Reveal(path.clone()));
                                ui.close();
                            }
                        }
                    });
                }

                // 拖拽插入位置指示与释放重排
                if let Some(uid) = drag
                    && let Some(from) = docs.iter().position(|d| d.uid == uid)
                    && let Some(ptr) = ui.ctx().pointer_latest_pos()
                {
                    let mut target = rects.len();
                    for (idx, r) in rects.iter().enumerate() {
                        if ptr.x < r.center().x {
                            target = idx;
                            break;
                        }
                    }
                    let line_x = if target < rects.len() {
                        rects[target].left() - 2.0
                    } else if let Some(last) = rects.last() {
                        last.right() + 2.0
                    } else {
                        0.0
                    };
                    if let Some(first_r) = rects.first() {
                        ui.painter().vline(line_x, first_r.y_range(), Stroke::new(2.0, accent));
                    }
                    if ui.input(|i| !i.pointer.primary_down()) {
                        let to = if target > from { target - 1 } else { target };
                        if to != from {
                            action = Some(TabAction::Move { from, to });
                        }
                        drag = None;
                    }
                } else if ui.input(|i| !i.pointer.primary_down()) {
                    drag = None;
                }

                let plus_resp = ui.add_sized(vec2(26.0, tab_h), egui::Button::new("+").corner_radius(4));
                if plus_resp.on_hover_text("新建标签页").clicked() && action.is_none() {
                    action = Some(TabAction::NewTab);
                }
            });
        });

    ui.ctx().data_mut(|d| d.insert_temp(drag_id, DragState { dragging_uid: drag }));
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_filenames_are_disambiguated() {
        let d1 = Document::from_file(PathBuf::from(r"C:\docs\a\README.md"), String::new());
        let d2 = Document::from_file(PathBuf::from(r"C:\docs\b\README.md"), String::new());
        let d3 = Document::from_file(PathBuf::from(r"C:\docs\notes.md"), String::new());
        let d4 = Document::new_empty();
        let titles = tab_titles(&[d1, d2, d3, d4]);
        assert_eq!(titles, vec!["README.md — a", "README.md — b", "notes.md", "未命名"]);
    }
}
