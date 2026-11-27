//! 多标签页栏 UI、重名消歧、拖拽重排、右键菜单（5 套主题统一像素骨架）。

use crate::document::Document;
use crate::markdown::bold_family;
use crate::theme::{self, ActiveTabStyle, CHROME};
use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, CursorIcon, FontId, Id, Rect, Sense, Stroke, StrokeKind,
};
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

pub fn show(
    ui: &mut egui::Ui,
    top_bar_bottom: f32,
    pill_top: f32,
    pill_bot: f32,
    docs: &[Document],
    active: usize,
) -> (Option<TabAction>, f32) {
    let mut action: Option<TabAction> = None;
    let titles = tab_titles(docs);
    let spec = theme::current(ui.ctx());
    let tc = &spec.tabs;
    let ppp = ui.ctx().pixels_per_point().max(1.0);
    let pill_h = (pill_bot - pill_top).max(1.0);
    let pill_top_px = (pill_top * ppp).round();
    let pill_bot_px = (pill_bot * ppp).round();
    let pill_center_y = theme::snap_px((pill_top + pill_bot) * 0.5, ppp);
    let slot_h = ui.available_height().max(pill_h);

    let drag_id = Id::unique("tabs_drag_state");
    let mut drag = ui
        .ctx()
        .data_mut(|d| d.get_temp::<DragState>(drag_id).map(|s| s.dragging_uid).unwrap_or(None));
    let mut rects: Vec<Rect> = Vec::with_capacity(docs.len());
    let avail_w = ui.available_width();
    let min_title_w = if avail_w < CHROME.tab_compact_avail_threshold {
        CHROME.tab_title_min_w_compact
    } else {
        CHROME.tab_title_min_w
    };
    let mut occupied_right_x = ui.max_rect().left();

    egui::ScrollArea::horizontal()
        .id_salt("tabs_scroll")
        .scroll_source(egui::scroll_area::ScrollSource {
            scroll_bar: false,
            drag: egui::scroll_area::DragScroll::Never,
            mouse_wheel: true,
        })
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CHROME.tab_gap;
                let font = FontId::proportional(CHROME.tab_font_size);
                let active_font = FontId::new(CHROME.tab_font_size, bold_family());

                for (i, doc) in docs.iter().enumerate() {
                    let is_active = i == active;
                    let text_font = if is_active { active_font.clone() } else { font.clone() };
                    let fg = if is_active { tc.active_fg } else { tc.idle_fg };
                    let galley = ui.fonts_mut(|f| f.layout_no_wrap(titles[i].clone(), text_font, fg));
                    let dot_extra = if doc.dirty {
                        CHROME.tab_dirty_dot_radius * 2.0 + CHROME.tab_dirty_dot_gap
                    } else {
                        0.0
                    };
                    let title_w = (galley.size().x + dot_extra).clamp(min_title_w, CHROME.tab_title_max_w);
                    let close_w = CHROME.tab_close_width;
                    let total_w = CHROME.tab_pad_left + title_w + CHROME.tab_text_close_gap + close_w + CHROME.tab_pad_right;

                    let (slot_rect, _) = ui.allocate_exact_size(vec2(total_w, slot_h), Sense::hover());
                    let pill_rect =
                        Rect::from_min_max(pos2(slot_rect.left(), pill_top), pos2(slot_rect.right(), pill_bot));
                    rects.push(pill_rect);

                    let close_left = pill_rect.right() - close_w - CHROME.tab_pad_right;
                    let title_rect = Rect::from_min_max(pill_rect.min, pos2(close_left, pill_bot));
                    let close_rect = Rect::from_min_max(
                        pos2(close_left, pill_top),
                        pos2(pill_rect.right() - CHROME.tab_pad_right, pill_bot),
                    );

                    let title_resp = ui.interact(title_rect, Id::unique(("tab_title", doc.uid)), Sense::click_and_drag());
                    let close_resp = ui.interact(close_rect, Id::unique(("tab_close", doc.uid)), Sense::click());
                    let hovered = title_resp.hovered() || close_resp.hovered();

                    // 按主题材质策略绘制标签底板（保证 pill_rect 与文字坐标 1:1 零跳动）
                    if is_active {
                        paint_active_tab(ui.painter(), pill_rect, top_bar_bottom, spec);
                    } else if hovered {
                        ui.painter().rect_filled(pill_rect, CHROME.tab_idle_radius, tc.hover_bg);
                    } else if tc.idle_bg != Color32::TRANSPARENT {
                        ui.painter().rect_filled(pill_rect, CHROME.tab_idle_radius, tc.idle_bg);
                    }

                    let clip_r = Rect::from_min_max(
                        pos2(pill_rect.left() + CHROME.tab_text_clip_pad_x, pill_rect.top()),
                        pos2(close_left - 2.0, pill_rect.bottom()),
                    );
                    let clipped = ui.painter().with_clip_rect(clip_r);
                    let mut text_x = pill_rect.left() + CHROME.tab_pad_left;
                    if doc.dirty {
                        let dot_col = if is_active { tc.dirty_active } else { tc.dirty_idle };
                        let dot_cx = text_x + CHROME.tab_dirty_dot_radius;
                        clipped.circle_filled(pos2(dot_cx, pill_center_y), CHROME.tab_dirty_dot_radius, dot_col);
                        text_x = dot_cx + CHROME.tab_dirty_dot_radius + CHROME.tab_dirty_dot_gap;
                    }
                    let text_y = theme::galley_ink_centered_pos_y(pill_top_px, pill_bot_px, &galley, ppp);
                    clipped.galley(pos2(theme::snap_px(text_x, ppp), text_y), galley, fg);

                    if close_resp.hovered() {
                        ui.painter().rect_filled(
                            close_rect.shrink(CHROME.tab_close_hover_inset),
                            CHROME.tab_close_radius,
                            tc.close_hover_bg,
                        );
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let close_fg = if is_active {
                        if close_resp.hovered() {
                            tc.active_fg
                        } else {
                            tc.active_fg.gamma_multiply(0.82)
                        }
                    } else if close_resp.hovered() {
                        spec.text_fg
                    } else {
                        tc.idle_fg
                    };
                    theme::paint_close_x_icon(
                        ui.painter(),
                        close_rect.center(),
                        CHROME.tab_close_icon_half,
                        CHROME.tab_close_stroke_w,
                        close_fg,
                    );

                    if let Some(p) = &doc.path {
                        title_resp.clone().on_hover_text(p.display().to_string());
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
                        rects[target].left() - CHROME.tab_drag_line_offset
                    } else if let Some(last) = rects.last() {
                        last.right() + CHROME.tab_drag_line_offset
                    } else {
                        0.0
                    };
                    if let Some(first_r) = rects.first() {
                        ui.painter().vline(line_x, first_r.y_range(), Stroke::new(CHROME.tab_drag_line_w, tc.accent));
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

                // 极简「+」新建标签按钮（矢量像素对齐 + 替代字形基线漂移）
                let (plus_slot, _) = ui.allocate_exact_size(vec2(CHROME.new_tab_btn_width, slot_h), Sense::hover());
                let plus_rect =
                    Rect::from_min_max(pos2(plus_slot.left(), pill_top), pos2(plus_slot.right(), pill_bot));
                let plus_resp = ui.interact(plus_rect, Id::unique("tabs_new_btn"), Sense::click());
                occupied_right_x = plus_rect.right();
                if plus_resp.hovered() {
                    ui.painter().rect_filled(
                        plus_rect.shrink(CHROME.new_tab_hover_inset),
                        CHROME.new_tab_radius,
                        tc.hover_bg,
                    );
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                theme::paint_plus_icon(
                    ui.painter(),
                    plus_rect.center(),
                    if plus_resp.hovered() { spec.text_fg } else { tc.idle_fg },
                );
                if plus_resp.on_hover_text("新建标签页").clicked() && action.is_none() {
                    action = Some(TabAction::NewTab);
                }
            });
        });

    ui.ctx().data_mut(|d| d.insert_temp(drag_id, DragState { dragging_uid: drag }));
    (action, occupied_right_x)
}

fn paint_active_tab(painter: &egui::Painter, r: Rect, top_bar_bottom: f32, spec: &theme::ThemeSpec) {
    let tc = &spec.tabs;
    match spec.active_tab_style {
        ActiveTabStyle::SoftShadowPill => {
            painter.add(tc.shadow.as_shape(r, CHROME.tab_active_pill_radius));
            painter.rect_filled(r, CHROME.tab_active_pill_radius, tc.active_bg);
        }
        ActiveTabStyle::CrimsonGoldBolt => {
            painter.add(tc.shadow.as_shape(r, CHROME.tab_active_pill_radius));
            painter.rect_filled(r, CHROME.tab_active_pill_radius, tc.active_bg);
            painter.hline(
                (r.left() + 8.0)..=(r.right() - 8.0),
                r.bottom() - 1.2,
                Stroke::new(CHROME.tab_accent_line_w, tc.secondary_accent),
            );
            let spark = Rect::from_center_size(
                pos2(r.center().x, r.bottom() - 1.5),
                vec2(CHROME.tab_spark_size[0], CHROME.tab_spark_size[1]),
            );
            theme::paint_bolt(painter, spark, tc.secondary_accent);
        }
        ActiveTabStyle::ConnectedPageTopBar => {
            let rad = CornerRadius {
                nw: CHROME.tab_active_box_radius,
                ne: CHROME.tab_active_box_radius,
                sw: 0,
                se: 0,
            };
            let page_r = Rect::from_min_max(r.min, pos2(r.right(), top_bar_bottom));
            painter.rect_filled(page_r, rad, tc.active_bg);
            let st = Stroke::new(CHROME.tab_rim_line_w, tc.border);
            painter.vline(page_r.left() + 0.5, (page_r.top() + 5.0)..=page_r.bottom(), st);
            painter.vline(page_r.right() - 0.5, (page_r.top() + 5.0)..=page_r.bottom(), st);
            painter.hline(
                (page_r.left() + 4.0)..=(page_r.right() - 4.0),
                page_r.top() + 1.0,
                Stroke::new(CHROME.tab_accent_line_w, tc.accent),
            );
        }
        ActiveTabStyle::CarbonGoldUnderline => {
            painter.rect_filled(r, CHROME.tab_active_box_radius, tc.active_bg);
            painter.rect_stroke(
                r,
                CHROME.tab_active_box_radius,
                Stroke::new(CHROME.tab_rim_line_w, tc.border),
                StrokeKind::Inside,
            );
            painter.hline(
                (r.left() + 4.0)..=(r.right() - 4.0),
                r.bottom() - 1.0,
                Stroke::new(CHROME.tab_accent_line_w, tc.accent),
            );
        }
        ActiveTabStyle::SmokedGlassRim => {
            painter.rect_filled(r, CHROME.tab_active_pill_radius, tc.active_bg);
            painter.rect_stroke(
                r,
                CHROME.tab_active_pill_radius,
                Stroke::new(CHROME.tab_rim_line_w, tc.border),
                StrokeKind::Inside,
            );
            painter.hline(
                (r.left() + 7.0)..=(r.right() - 7.0),
                r.top() + 0.8,
                Stroke::new(CHROME.tab_rim_line_w, tc.secondary_accent),
            );
        }
    }
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
