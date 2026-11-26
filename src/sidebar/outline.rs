//! 侧边栏大纲：复用 Document.parsed 标题列表、支持折叠与当前章节高亮。

use crate::markdown::{bold_family, Heading};
use crate::theme;
use eframe::egui::{self, pos2, vec2, CursorIcon, FontId, Rect, Sense};
use std::collections::HashSet;

#[derive(Default)]
pub struct OutlineState {
    collapsed: HashSet<(u64, usize)>,
}

pub struct OutlineJump {
    pub anchor: String,
    pub byte_offset: usize,
}

impl OutlineState {
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        doc_uid: u64,
        headings: &[Heading],
        top_byte: usize,
    ) -> Option<OutlineJump> {
        if headings.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.weak("当前文档没有标题");
            });
            return None;
        }

        let active_idx = headings.iter().rposition(|h| h.byte_offset <= top_byte).or(Some(0));
        let min_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
        let sc = &theme::current(ui.ctx()).sidebar;
        let mut jump: Option<OutlineJump> = None;

        egui::ScrollArea::vertical().id_salt("sidebar_outline_scroll").auto_shrink(false).show(ui, |ui| {
            let mut i = 0usize;
            while i < headings.len() {
                let h = &headings[i];
                let has_children = headings.get(i + 1).is_some_and(|next| next.level > h.level);
                let is_collapsed = self.collapsed.contains(&(doc_uid, i));
                let is_active = active_idx == Some(i);

                let row_resp = ui.horizontal(|ui| {
                    ui.add_space((h.level.saturating_sub(min_level)) as f32 * 14.0);
                    if has_children {
                        let arrow = if is_collapsed { "▸" } else { "▾" };
                        if ui.small_button(arrow).clicked() {
                            if is_collapsed {
                                self.collapsed.remove(&(doc_uid, i));
                            } else {
                                self.collapsed.insert((doc_uid, i));
                            }
                        }
                    } else {
                        ui.add_space(10.0);
                    }

                    let rt = if is_active {
                        egui::RichText::new(&h.text)
                            .font(FontId::new(13.0, bold_family()))
                            .color(sc.strong_fg)
                    } else {
                        egui::RichText::new(&h.text).color(sc.muted_fg)
                    };
                    let resp = ui.add(egui::Label::new(rt).sense(Sense::click()).truncate().selectable(false));
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        jump = Some(OutlineJump { anchor: h.anchor.clone(), byte_offset: h.byte_offset });
                    }
                });
                if is_active {
                    let row_r = row_resp.response.rect;
                    let bar_x = ui.max_rect().left() - 10.0;
                    ui.painter().rect_filled(
                        Rect::from_min_size(pos2(bar_x, row_r.top() + 2.0), vec2(2.0, (row_r.height() - 4.0).max(12.0))),
                        1.0,
                        sc.active_bar,
                    );
                }
                ui.add_space(2.0);

                if is_collapsed {
                    let level = h.level;
                    i += 1;
                    while i < headings.len() && headings[i].level > level {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        });

        jump
    }
}
