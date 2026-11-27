//! 自定义快捷键窗口：搜索过滤、按分类分组、键帽展示、按键录制、冲突检测与 JSON 导入/导出。

use crate::commands::Command;
use crate::dialog;
use crate::keymap::{Keymap, Shortcut};
use crate::markdown::bold_family;
use crate::theme;
use eframe::egui::{self, pos2, vec2, Align, CursorIcon, Event, FontId, Key, Margin, Rect, Sense, Stroke, UiBuilder};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const RESET_CONFIRM_TIMEOUT: Duration = Duration::from_secs(3);

/// 按命令出现顺序归并的分类顺序（避免 `Command::ALL` 穿插导致同名分组重复）。
const CATEGORY_ORDER: &[&str] = &[
    "文件与标签",
    "编辑与剪贴板",
    "视图与字号",
    "侧边栏",
    "外观与主题",
    "设置",
    "帮助",
];

#[derive(Clone, Copy)]
struct PendingConflict {
    target: Command,
    existing: Command,
    shortcut: Shortcut,
}

#[derive(Default)]
pub struct ShortcutsUiState {
    pub open: bool,
    pub search: String,
    capturing: Option<Command>,
    conflict: Option<PendingConflict>,
    reset_confirm_deadline: Option<Instant>,
}

pub fn export_overrides_json(keymap: &Keymap) -> Result<String, String> {
    serde_json::to_string_pretty(&keymap.to_overrides()).map_err(|e| format!("序列化快捷键 JSON 失败：{e}"))
}

pub fn import_overrides_json(json: &str) -> Result<Keymap, String> {
    let map: BTreeMap<String, String> =
        serde_json::from_str(json).map_err(|e| format!("解析快捷键 JSON 失败：{e}"))?;
    Ok(Keymap::from_overrides(&map))
}

/// 若处于录制状态，则吞掉本帧按键/文本事件并尝试捕获新快捷键（返回 true 表示已拦截输入）。
fn apply_capture_key(
    capturing: &mut Option<Command>,
    conflict: &mut Option<PendingConflict>,
    keymap: &mut Keymap,
    key: Key,
    physical_key: Option<Key>,
    modifiers: egui::Modifiers,
) -> bool {
    let Some(target) = *capturing else { return false };
    let no_mods = !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift;
    if key == Key::Escape && no_mods {
        *capturing = None;
        return true;
    }
    if matches!(key, Key::Backspace | Key::Delete) && no_mods {
        keymap.set(target, None);
        *capturing = None;
        return true;
    }
    if let Some(sc) = Shortcut::from_event(key, physical_key, modifiers) {
        if let Some(existing) = keymap.find_conflict(target, sc) {
            *conflict = Some(PendingConflict { target, existing, shortcut: sc });
        } else {
            keymap.set(target, Some(sc));
        }
        *capturing = None;
        return true;
    }
    false
}

fn command_matches_filter(cmd: Command, cur: Option<Shortcut>, filter: &str) -> bool {
    if filter.is_empty() {
        return true;
    }
    let cur_str = cur.map(|s| s.to_string()).unwrap_or_else(|| "未绑定".into());
    cmd.label().to_ascii_lowercase().contains(filter)
        || cmd.category().to_ascii_lowercase().contains(filter)
        || cmd.id().to_ascii_lowercase().contains(filter)
        || cur_str.to_ascii_lowercase().contains(filter)
}

#[derive(Clone, Copy)]
struct TableColumns {
    cmd_w: f32,
    cur_w: f32,
    def_w: f32,
    act_w: f32,
}

impl TableColumns {
    fn from_width(avail_w: f32) -> Self {
        let act_w = 40.0;
        let remain = (avail_w - act_w).max(240.0);
        if avail_w >= 560.0 {
            let cur_w = 176.0;
            let def_w = 154.0;
            let cmd_w = (remain - cur_w - def_w).max(160.0);
            Self { cmd_w, cur_w, def_w, act_w }
        } else {
            let cur_w = (remain * 0.34).clamp(124.0, 165.0);
            let def_w = (remain * 0.26).clamp(88.0, 138.0);
            let cmd_w = (remain - cur_w - def_w).max(100.0);
            Self { cmd_w, cur_w, def_w, act_w }
        }
    }
}

impl ShortcutsUiState {
    pub fn has_conflict(&self) -> bool {
        self.conflict.is_some()
    }

    pub fn dismiss_conflict(&mut self) {
        self.conflict = None;
    }

    pub fn clear_transient_state(&mut self) {
        self.capturing = None;
        self.conflict = None;
        self.reset_confirm_deadline = None;
    }

    /// 供自测/截图验证展示未绑定、已修改、录制态或冲突条用辅助入口。
    pub(crate) fn debug_set_conflict(&mut self, target: Command, existing: Command, shortcut: Shortcut) {
        self.conflict = Some(PendingConflict { target, existing, shortcut });
    }

    pub(crate) fn debug_set_capturing(&mut self, target: Option<Command>) {
        self.capturing = target;
    }

    /// 处理「全部恢复默认」的二次确认状态机：首次点击进入 3 秒待确认态，再次点击返回 true 执行重置。
    fn trigger_reset_all_step(&mut self, now: Instant) -> bool {
        if let Some(deadline) = self.reset_confirm_deadline
            && now <= deadline
        {
            self.reset_confirm_deadline = None;
            return true;
        }
        self.reset_confirm_deadline = Some(now + RESET_CONFIRM_TIMEOUT);
        false
    }

    pub fn capture_input(&mut self, ctx: &egui::Context, keymap: &mut Keymap) -> bool {
        if self.capturing.is_none() {
            return false;
        }
        ctx.input_mut(|i| {
            let mut captured = false;
            for ev in &i.events {
                if !captured
                    && let Event::Key { pressed: true, key, physical_key, modifiers, .. } = ev
                {
                    captured = apply_capture_key(
                        &mut self.capturing,
                        &mut self.conflict,
                        keymap,
                        *key,
                        *physical_key,
                        *modifiers,
                    );
                }
            }
            i.events.retain(|ev| !matches!(ev, Event::Key { .. } | Event::Text(_)));
        });
        true
    }

    pub fn window_ui(&mut self, ctx: &egui::Context, keymap: &mut Keymap, error_out: &mut Option<String>) {
        if !self.open {
            dialog::reset_dialog_drag_offset(ctx, dialog::shortcuts_dialog_id());
            self.clear_transient_state();
            return;
        }

        let now = Instant::now();
        if let Some(deadline) = self.reset_confirm_deadline {
            if now >= deadline {
                self.reset_confirm_deadline = None;
            } else {
                // 仅在 3 秒确认倒计时未结束时安排一次到期唤醒，空闲绝不空转重绘
                ctx.request_repaint_after(deadline - now);
            }
        }

        let mut open = self.open;
        let mut close_clicked = false;
        let spec = theme::current(ctx);
        let customized_count = Command::ALL
            .iter()
            .filter(|&&c| keymap.shortcut(c) != c.default_shortcut())
            .count();
        let filter = self.search.trim().to_ascii_lowercase();

        dialog::dialog_shell(
            ctx,
            dialog::shortcuts_dialog_id(),
            720.0,
            560.0,
            "shortcuts_dialog_scroll",
            "自定义快捷键",
            "点击键位后按下新组合键即可修改",
            &mut open,
            |ui, section| match section {
                dialog::DialogSection::SubHeader => self.render_sub_header(ui, keymap, error_out),
                dialog::DialogSection::Body => self.render_table_body(ui, keymap, &filter),
                dialog::DialogSection::Footer => {
                    let armed = self.reset_confirm_deadline.is_some_and(|d| Instant::now() <= d);
                    let reset_label = if armed { "确认恢复？" } else { "全部恢复默认" };
                    if dialog::danger_button(ui, reset_label, armed).clicked()
                        && self.trigger_reset_all_step(Instant::now())
                    {
                        keymap.reset_all();
                        self.clear_transient_state();
                    }

                    ui.add_space(6.0);
                    let count_txt = format!("已自定义 {customized_count} 项");
                    ui.label(egui::RichText::new(count_txt).size(12.0).color(spec.muted_fg));

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if dialog::primary_button(ui, "完成").clicked() {
                            close_clicked = true;
                        }
                    });
                }
            },
        );

        self.open = open && !close_clicked;
        if !self.open {
            dialog::reset_dialog_drag_offset(ctx, dialog::shortcuts_dialog_id());
            self.clear_transient_state();
        }
    }

    fn render_sub_header(&mut self, ui: &mut egui::Ui, keymap: &mut Keymap, error_out: &mut Option<String>) {
        let spec = theme::current(ui.ctx());
        let w = ui.available_width();

        // 1. 工具栏（内边距 [16, 10]）：左搜索框、右导入/导出分段按钮组
        egui::Frame::new()
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.set_width((w - 32.0).max(200.0));
                ui.horizontal(|ui| {
                    let search_w = 260.0_f32.min((ui.available_width() - 180.0).max(130.0));
                    dialog::search_input(
                        ui,
                        "shortcuts_search_input",
                        &mut self.search,
                        search_w,
                        "搜索命令、分类或快捷键…",
                    );

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let (import_resp, export_resp) =
                            dialog::segmented_secondary_pair(ui, "导入 JSON", "导出 JSON");
                        if export_resp.clicked() {
                            match export_overrides_json(keymap) {
                                Ok(json) => {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter("JSON", &["json"])
                                        .set_file_name("thundermd-shortcuts.json")
                                        .save_file()
                                        && let Err(e) = std::fs::write(&p, json.as_bytes())
                                    {
                                        *error_out = Some(format!("导出快捷键失败：{e}"));
                                    }
                                }
                                Err(e) => *error_out = Some(e),
                            }
                        }
                        if import_resp.clicked()
                            && let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()
                        {
                            match std::fs::read_to_string(&p)
                                .map_err(|e| format!("读取快捷键 JSON 失败：{e}"))
                                .and_then(|s| import_overrides_json(&s))
                            {
                                Ok(new_km) => {
                                    *keymap = new_km;
                                    self.clear_transient_state();
                                }
                                Err(e) => *error_out = Some(e),
                            }
                        }
                    });
                });

                ui.add_space(7.0);
                // 2. 键帽化操作提示语
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 2.0);
                    let tip_col = spec.muted_fg;
                    ui.label(egui::RichText::new("点击键位后按下新组合键即可修改 ·").size(11.5).color(tip_col));
                    dialog::keycap(ui, "Esc", 0.9, true);
                    ui.label(egui::RichText::new("取消录制 ·").size(11.5).color(tip_col));
                    dialog::keycap(ui, "Backspace", 0.9, true);
                    ui.label(egui::RichText::new("/").size(11.5).color(tip_col));
                    dialog::keycap(ui, "Delete", 0.9, true);
                    ui.label(egui::RichText::new("解绑").size(11.5).color(tip_col));
                });

                // 3. 冲突提示条（带淡入动画、3px 左竖条、⚠ 图标、键帽展示、warn 主按钮覆盖）
                let conf_fade = ui
                    .ctx()
                    .animate_bool_with_time(ui.make_persistent_id("shortcuts_conflict_fade"), self.conflict.is_some(), 0.14);
                if let Some(conf) = self.conflict {
                    ui.add_space(8.0);
                    self.render_conflict_banner(ui, keymap, conf, conf_fade);
                }
            });

        // 4. 固定表头：命令 / 当前快捷键 / 默认 / 操作 + 1px border 底线
        let cols = TableColumns::from_width((w - 32.0).max(240.0));
        let (hdr_rect, _) = ui.allocate_exact_size(vec2(w, 26.0), Sense::hover());
        let inner = hdr_rect.shrink2(vec2(16.0, 0.0));
        let header_fg = spec.muted_fg;
        let x_cmd = inner.left() + 8.0;
        let x_cur = inner.left() + cols.cmd_w;
        let x_def = x_cur + cols.cur_w;
        let x_act = x_def + cols.def_w + cols.act_w * 0.5;
        let cy = inner.center().y;
        let font = FontId::proportional(11.5);

        ui.painter()
            .text(pos2(x_cmd, cy), egui::Align2::LEFT_CENTER, "命令", font.clone(), header_fg);
        ui.painter()
            .text(pos2(x_cur, cy), egui::Align2::LEFT_CENTER, "当前快捷键", font.clone(), header_fg);
        ui.painter()
            .text(pos2(x_def, cy), egui::Align2::LEFT_CENTER, "默认", font.clone(), header_fg);
        ui.painter()
            .text(pos2(x_act, cy), egui::Align2::CENTER_CENTER, "操作", font, header_fg);

        let ppp = ui.ctx().pixels_per_point().max(1.0);
        let line_y = theme::snap_line_coord(hdr_rect.bottom() - 0.5, 1.0, ppp);
        ui.painter()
            .hline(hdr_rect.left()..=hdr_rect.right(), line_y, Stroke::new(1.0, spec.border));
    }

    fn render_conflict_banner(
        &mut self,
        ui: &mut egui::Ui,
        keymap: &mut Keymap,
        conf: PendingConflict,
        opacity: f32,
    ) {
        let spec = theme::current(ui.ctx());
        let warn = spec.semantic.warn;
        let op = opacity.clamp(0.05, 1.0);
        let resp = egui::Frame::new()
            .fill(warn.gamma_multiply(0.12 * op))
            .stroke(Stroke::new(1.0, warn.gamma_multiply(0.45 * op)))
            .corner_radius(6)
            .inner_margin(Margin { left: 13, right: 10, top: 8, bottom: 8 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    paint_warn_triangle_icon(ui, warn.gamma_multiply(op));
                    ui.label(egui::RichText::new("快捷键").size(12.5).color(spec.text_fg));
                    dialog::shortcut_keys(ui, conf.shortcut, op, true);
                    ui.label(
                        egui::RichText::new(format!("已被「{}」占用，是否覆盖？", conf.existing.label()))
                            .size(12.5)
                            .color(spec.text_fg),
                    );

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if dialog::secondary_button(ui, "取消").clicked() {
                            self.conflict = None;
                        }
                        if dialog::primary_color_button(ui, "覆盖", warn, spec.dialog.on_warn).clicked() {
                            keymap.set(conf.existing, None);
                            keymap.set(conf.target, Some(conf.shortcut));
                            self.conflict = None;
                        }
                    });
                });
            });

        // 左侧 3px semantic.warn 竖条
        let r = resp.response.rect;
        let bar = Rect::from_min_max(pos2(r.left() + 1.0, r.top() + 3.0), pos2(r.left() + 4.0, r.bottom() - 3.0));
        ui.painter().rect_filled(bar, 1.5, warn.gamma_multiply(op));
    }

    fn render_table_body(&mut self, ui: &mut egui::Ui, keymap: &mut Keymap, filter: &str) {
        let spec = theme::current(ui.ctx());
        let w = ui.available_width();
        let inner_w = (w - 32.0).max(240.0);
        let cols = TableColumns::from_width(inner_w);

        let mut total_matches = 0usize;
        for &cat in CATEGORY_ORDER {
            let cmds: Vec<Command> = Command::ALL
                .iter()
                .copied()
                .filter(|&c| c.category() == cat && command_matches_filter(c, keymap.shortcut(c), filter))
                .collect();
            if cmds.is_empty() {
                continue;
            }
            total_matches += cmds.len();

            egui::Frame::new()
                .inner_margin(Margin { left: 16, right: 16, top: 8, bottom: 4 })
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    // 分类组标题 + 右侧计数
                    let (cat_r, _) = ui.allocate_exact_size(vec2(inner_w, 22.0), Sense::hover());
                    ui.painter().text(
                        pos2(cat_r.left() + 8.0, cat_r.center().y),
                        egui::Align2::LEFT_CENTER,
                        cat,
                        FontId::new(11.5, bold_family()),
                        spec.muted_fg,
                    );
                    ui.painter().text(
                        pos2(cat_r.right() - 8.0, cat_r.center().y),
                        egui::Align2::RIGHT_CENTER,
                        format!("{} 项", cmds.len()),
                        FontId::proportional(11.0),
                        spec.muted_fg.gamma_multiply(0.85),
                    );
                    ui.painter().hline(
                        (cat_r.left() + 6.0)..=(cat_r.right() - 6.0),
                        cat_r.bottom() - 1.0,
                        Stroke::new(1.0, spec.border.gamma_multiply(0.55)),
                    );
                    ui.add_space(2.0);

                    for &cmd in &cmds {
                        self.render_command_row(ui, keymap, cmd, inner_w, cols);
                    }
                });
        }

        if total_matches == 0 {
            let empty_h = ui.available_height().max(180.0);
            let (empty_rect, _) = ui.allocate_exact_size(vec2(w, empty_h), Sense::hover());
            let mut empty_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(empty_rect)
                    .layout(egui::Layout::top_down(Align::Center)),
            );
            empty_ui.add_space((empty_h * 0.36).max(24.0));
            empty_ui.label(egui::RichText::new("没有找到匹配的命令").size(13.5).color(spec.muted_fg));
            empty_ui.add_space(6.0);
            let link_resp = empty_ui.add(
                egui::Label::new(
                    egui::RichText::new("清除搜索")
                        .size(12.5)
                        .color(spec.accent)
                        .underline(),
                )
                .sense(Sense::click()),
            );
            if link_resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if link_resp.clicked() {
                self.search.clear();
            }
        }
    }

    fn render_command_row(
        &mut self,
        ui: &mut egui::Ui,
        keymap: &mut Keymap,
        cmd: Command,
        row_w: f32,
        cols: TableColumns,
    ) {
        let spec = theme::current(ui.ctx());
        let cur = keymap.shortcut(cmd);
        let default_sc = cmd.default_shortcut();
        let modified = cur != default_sc;
        let recording = self.capturing == Some(cmd);

        let row_h = 36.0;
        let (row_rect, _) = ui.allocate_exact_size(vec2(row_w, row_h), Sense::hover());
        let ptr_hovered = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|p| row_rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id()));

        if recording {
            ui.painter().rect_filled(row_rect, 6.0, spec.active_bg);
            let bar = Rect::from_min_max(
                pos2(row_rect.left() + 1.0, row_rect.top() + 5.0),
                pos2(row_rect.left() + 3.0, row_rect.bottom() - 5.0),
            );
            ui.painter().rect_filled(bar, 1.0, spec.accent);
        } else if ptr_hovered {
            ui.painter().rect_filled(row_rect, 6.0, spec.dialog.row_hover_bg);
        }

        let r_cmd = Rect::from_min_size(pos2(row_rect.left() + 8.0, row_rect.top()), vec2(cols.cmd_w - 14.0, row_h));
        let r_cur = Rect::from_min_size(pos2(row_rect.left() + cols.cmd_w, row_rect.top()), vec2(cols.cur_w - 8.0, row_h));
        let r_def = Rect::from_min_size(
            pos2(row_rect.left() + cols.cmd_w + cols.cur_w, row_rect.top()),
            vec2(cols.def_w - 6.0, row_h),
        );
        let r_act = Rect::from_min_size(
            pos2(row_rect.left() + cols.cmd_w + cols.cur_w + cols.def_w, row_rect.top()),
            vec2(cols.act_w, row_h),
        );

        // 1. 命令列 + 已修改标记
        let mut cmd_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(r_cmd)
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        cmd_ui.set_clip_rect(ui.clip_rect().intersect(r_cmd));
        cmd_ui.label(
            egui::RichText::new(cmd.label())
                .size(13.0)
                .color(if recording { spec.strong_fg } else { spec.text_fg }),
        );
        if modified {
            cmd_ui.add_space(6.0);
            if r_cmd.width() >= 140.0 {
                dialog::pill(&mut cmd_ui, "已修改", spec.accent);
            } else {
                let (dot_r, _) = cmd_ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                cmd_ui.painter().circle_filled(dot_r.center(), 3.0, spec.accent);
            }
        }

        // 2. 当前快捷键（录制态或键帽）
        let mut cur_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(r_cur)
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        cur_ui.set_clip_rect(ui.clip_rect().intersect(r_cur.expand(2.0)));
        let rec_w = r_cur.width().clamp(122.0, 164.0);
        if dialog::shortcut_recorder(&mut cur_ui, ("sc_rec", cmd.id()), cur, recording, rec_w).clicked() {
            self.conflict = None;
            self.capturing = if recording { None } else { Some(cmd) };
        }

        // 3. 默认快捷键（或录制操作小字提示）
        let mut def_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(r_def)
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        def_ui.set_clip_rect(ui.clip_rect().intersect(r_def));
        if recording {
            def_ui.label(
                egui::RichText::new("Esc 取消 · Backspace 解绑")
                    .size(11.0)
                    .color(spec.muted_fg),
            );
        } else if let Some(def) = default_sc {
            dialog::shortcut_keys(&mut def_ui, def, 0.60, false);
        } else {
            def_ui.label(egui::RichText::new("—").size(12.5).color(spec.muted_fg));
        }

        // 4. 操作列（仅已修改行或悬停且有改动时显示 ↺ 恢复按钮）
        if modified {
            let mut act_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(r_act)
                    .layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
            );
            act_ui.set_clip_rect(ui.clip_rect().intersect(r_act));
            if dialog::reset_icon_button(&mut act_ui, ("sc_reset", cmd.id()), "恢复默认快捷键").clicked() {
                keymap.reset(cmd);
                if self.capturing == Some(cmd) {
                    self.capturing = None;
                }
            }
        }
    }
}

fn paint_warn_triangle_icon(ui: &mut egui::Ui, color: egui::Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    let c = r.center();
    let pts = vec![pos2(c.x, c.y - 6.2), pos2(c.x + 6.5, c.y + 5.2), pos2(c.x - 6.5, c.y + 5.2)];
    ui.painter()
        .add(egui::epaint::PathShape::closed_line(pts, Stroke::new(1.5, color)));
    ui.painter()
        .line_segment([pos2(c.x, c.y - 2.3), pos2(c.x, c.y + 1.5)], Stroke::new(1.6, color));
    ui.painter().circle_filled(pos2(c.x, c.y + 3.4), 0.95, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_preserves_overrides_and_unbound_and_ignores_invalid() {
        let mut km = Keymap::default();
        km.set(Command::Save, Some(Shortcut::new(true, true, false, Key::F5)));
        km.set(Command::Find, None); // 显式解绑 -> ""
        let json = export_overrides_json(&km).unwrap();

        // 注入未知命令与非法快捷键，验证容错忽略
        let mut raw: BTreeMap<String, String> = serde_json::from_str(&json).unwrap();
        raw.insert("nonexistent_command".into(), "Ctrl+J".into());
        raw.insert("open".into(), "Invalid+???".into());
        let noisy_json = serde_json::to_string_pretty(&raw).unwrap();

        let restored = import_overrides_json(&noisy_json).unwrap();
        assert_eq!(restored.shortcut(Command::Save), Some(Shortcut::new(true, true, false, Key::F5)));
        assert_eq!(restored.shortcut(Command::Find), None);
        assert_eq!(restored.shortcut(Command::Open), Command::Open.default_shortcut());
    }

    #[test]
    fn conflict_overwrite_unbinds_previous_owner() {
        let mut km = Keymap::default();
        let mut cap = Some(Command::ImportHtml);
        let mut conflict = None;

        // ImportHtml 尝试绑定 Save 的默认快捷键 Ctrl+S -> 产生冲突
        let ctrl_s_mods = egui::Modifiers { ctrl: true, ..Default::default() };
        assert!(apply_capture_key(&mut cap, &mut conflict, &mut km, Key::S, None, ctrl_s_mods));
        assert!(cap.is_none());
        let conf = conflict.expect("Ctrl+S 应与 Save 冲突");
        assert_eq!(conf.existing, Command::Save);

        // 用户确认覆盖：清除 Save 原绑定，赋给 ImportHtml
        km.set(conf.existing, None);
        km.set(conf.target, Some(conf.shortcut));
        assert_eq!(km.shortcut(Command::Save), None);
        assert_eq!(km.shortcut(Command::ImportHtml), Some(conf.shortcut));
    }

    #[test]
    fn reset_all_requires_double_confirmation_within_timeout() {
        let mut state = ShortcutsUiState::default();
        let t0 = Instant::now();
        assert!(!state.trigger_reset_all_step(t0));
        assert!(state.reset_confirm_deadline.is_some());

        // 超时之后再点击需要重新进入待确认，不可直接重置
        assert!(!state.trigger_reset_all_step(t0 + RESET_CONFIRM_TIMEOUT + Duration::from_millis(50)));
        assert!(state.reset_confirm_deadline.is_some());

        // 3 秒内再次点击才会确认执行
        let t1 = t0 + RESET_CONFIRM_TIMEOUT + Duration::from_millis(60);
        assert!(state.trigger_reset_all_step(t1));
        assert!(state.reset_confirm_deadline.is_none());
    }

    #[test]
    fn escape_priority_handles_recording_conflict_and_stacked_dialogs() {
        let ctx = egui::Context::default();
        let mut km = Keymap::default();
        let mut state = ShortcutsUiState { open: true, ..Default::default() };
        let mut settings_open = true;

        // 1. 录制中按 Esc：仅取消录制，不关闭冲突或弹窗
        state.capturing = Some(Command::Save);
        let esc_raw = || egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: Some(Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(esc_raw(), |_ui| {
            assert!(state.capture_input(&ctx, &mut km));
            assert!(state.capturing.is_none());
            assert!(state.open);
        });
        out.textures_delta.clear();

        // 2. 有冲突提示时按 Esc：仅关闭冲突提示
        state.conflict = Some(PendingConflict {
            target: Command::ImportHtml,
            existing: Command::Save,
            shortcut: Shortcut::new(true, false, false, Key::S),
        });
        let mut out = ctx.run_ui(esc_raw(), |_ui| {
            let mut has_conf = state.has_conflict();
            assert!(dialog::handle_dialogs_escape(&ctx, &mut settings_open, &mut state.open, &mut has_conf, false));
            if !has_conf {
                state.dismiss_conflict();
            }
        });
        out.textures_delta.clear();
        assert!(!state.has_conflict());
        assert!(state.open);
        assert!(settings_open);

        // 3. 双弹窗叠放时按 Esc：只关闭上层快捷键弹窗
        let mut out = ctx.run_ui(esc_raw(), |_ui| {
            let mut has_conf = state.has_conflict();
            assert!(dialog::handle_dialogs_escape(&ctx, &mut settings_open, &mut state.open, &mut has_conf, false));
        });
        out.textures_delta.clear();
        assert!(!state.open);
        assert!(settings_open);

        // 4. 仅剩设置弹窗时按 Esc：关闭设置弹窗
        let mut out = ctx.run_ui(esc_raw(), |_ui| {
            let mut has_conf = false;
            assert!(dialog::handle_dialogs_escape(&ctx, &mut settings_open, &mut state.open, &mut has_conf, false));
        });
        out.textures_delta.clear();
        assert!(!settings_open);
    }

    #[test]
    fn shortcuts_dialog_fits_viewport_across_zooms_and_stays_idle() {
        for zoom in [0.75_f32, 1.0, 2.0] {
            let ctx = egui::Context::default();
            crate::fonts::setup(&ctx);
            crate::theme::apply(&ctx, theme::ThemeId::AtelierContrast);
            ctx.set_zoom_factor(zoom);
            let mut state = ShortcutsUiState { open: true, ..Default::default() };
            let mut km = Keymap::default();
            let mut err = None;

            // 跑多帧直至首帧 sizing_pass 与入场动画收敛
            let mut rect = None;
            let mut last_repaint_delay = Duration::ZERO;
            for _ in 0..25 {
                let raw = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1200.0 / zoom, 800.0 / zoom))),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |_ui| {
                    dialog::paint_backdrop_and_sync_layers(&ctx, false, state.open);
                    state.window_ui(&ctx, &mut km, &mut err);
                    rect = ctx.memory(|m| m.area_rect(dialog::shortcuts_dialog_id()));
                });
                out.textures_delta.clear();
                last_repaint_delay = out
                    .viewport_output
                    .get(&egui::ViewportId::ROOT)
                    .map(|v| v.repaint_delay)
                    .unwrap_or(Duration::MAX);
            }

            let screen = ctx.content_rect();
            let r = rect.expect("shortcuts dialog should have area rect");
            assert!(r.height() <= screen.height() * 0.82 + 2.0, "zoom={zoom} r={r:?} screen={screen:?}");
            assert!(r.width() <= screen.width() + 1.0, "zoom={zoom} r={r:?} screen={screen:?}");
            assert!(r.bottom() <= screen.bottom() + 1.0, "zoom={zoom} r={r:?} screen={screen:?}");
            assert!(
                last_repaint_delay > Duration::from_secs(1),
                "zoom={zoom} idle dialog should not request continuous repaint: {last_repaint_delay:?}"
            );
        }
    }
}
