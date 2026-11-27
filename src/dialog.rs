//! 通用弹窗外壳与主题化 UI 组件库：供设置、快捷键及其他弹窗复用，所有颜色严格取自 `theme::current(ctx)`。

use crate::keymap::Shortcut;
use crate::markdown::bold_family;
use crate::theme::{self, ThemeId, TOP_BAR_H};
use crate::window_frame::{self, WindowControlBtn};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, AreaState, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Id,
    LayerId, Margin, Order, Pos2, Rect, Response, Sense, Stroke, StrokeKind, UiBuilder, Vec2, WindowDrag,
};
use std::ops::RangeInclusive;

/// 弹窗外框圆角（逻辑像素）。
pub const DIALOG_CORNER_RADIUS: u8 = 12;
/// 自绘头部固定高度。
pub const DIALOG_HEADER_H: f32 = 52.0;
/// 自绘底部栏固定高度。
pub const DIALOG_FOOTER_H: f32 = 52.0;
/// 设置行默认高度。
pub const SETTING_ROW_H: f32 = 34.0;
/// 设置行左侧标签列基准宽度。
pub const SETTING_LABEL_COL_W: f32 = 150.0;

pub fn settings_dialog_id() -> Id {
    Id::unique("thundermd_settings_dialog")
}

pub fn shortcuts_dialog_id() -> Id {
    Id::unique("thundermd_shortcuts_dialog")
}

fn backdrop_area_id() -> Id {
    Id::unique("thundermd_dialog_backdrop")
}

fn header_drag_id(dialog_id: Id) -> Id {
    dialog_id.with("header_drag")
}

fn drag_offset_key(dialog_id: Id) -> Id {
    dialog_id.with("drag_offset")
}

/// 弹窗关闭时重置记忆偏移，确保下次打开仍居中。
pub fn reset_dialog_drag_offset(ctx: &egui::Context, dialog_id: Id) {
    ctx.data_mut(|d| d.remove::<Vec2>(drag_offset_key(dialog_id)));
}

/// 在弹窗下方绘制单层半透明遮罩，并处理「设置 + 快捷键」双弹窗叠放时的精确 z-order：
/// - 仅打开一个弹窗：`Background -> Scrim -> Dialog`
/// - 从设置弹窗打开快捷键弹窗：`Background -> Settings -> Scrim -> Shortcuts`
///
/// 保证编辑区始终只承受 1 层遮罩不过黑，而下层设置弹窗被遮罩自然压暗且不可点击。
pub fn paint_backdrop_and_sync_layers(ctx: &egui::Context, settings_open: bool, shortcuts_open: bool) {
    let any_open = settings_open || shortcuts_open;
    let scrim_id = backdrop_area_id();
    let scrim_t = ctx.animate_bool_with_time(scrim_id.with("fade"), any_open, 0.12);
    if !any_open && scrim_t <= 0.005 {
        return;
    }

    let spec = theme::current(ctx);
    let scrim_color = spec.dialog.scrim.gamma_multiply(scrim_t);
    let screen = ctx.content_rect();
    let scrim_layer = LayerId::new(Order::Middle, scrim_id);
    let settings_layer = LayerId::new(Order::Middle, settings_dialog_id());
    let shortcuts_layer = LayerId::new(Order::Middle, shortcuts_dialog_id());

    // 若快捷键弹窗打开并已完成首帧尺寸测算，将遮罩放到它正下方、设置弹窗之上；
    // 否则让遮罩留在活跃弹窗下方。
    let mut active_top_layer = None;
    if any_open {
        if shortcuts_open {
            let ready = AreaState::load(ctx, shortcuts_dialog_id()).is_some_and(|s| s.size.is_some());
            if ready {
                active_top_layer = Some(shortcuts_layer);
            } else if settings_open && AreaState::load(ctx, settings_dialog_id()).is_some_and(|s| s.size.is_some()) {
                active_top_layer = Some(settings_layer);
            }
        } else if settings_open && AreaState::load(ctx, settings_dialog_id()).is_some_and(|s| s.size.is_some()) {
            active_top_layer = Some(settings_layer);
        }
    }

    // 保留顶栏可点击（弹窗未覆盖处），方便用户随时点顶栏 ⚡ 或主题菜单切换 5 套主题；
    // 但当下面还有设置弹窗时，整个编辑区与下层弹窗都被遮罩拦截。
    let top_clear = if screen.height() > TOP_BAR_H + 80.0 {
        TOP_BAR_H
    } else {
        0.0
    };
    let mut scrim_rect = Rect::from_min_max(pos2(screen.left(), screen.top() + top_clear), screen.max);
    if settings_open && shortcuts_open
        && let Some(st) = AreaState::load(ctx, settings_dialog_id())
    {
        scrim_rect = scrim_rect.union(st.rect());
    }

    egui::Area::new(scrim_id)
        .order(Order::Middle)
        .fixed_pos(scrim_rect.min)
        .default_size(scrim_rect.size())
        .movable(false)
        .interactable(any_open)
        .fade_in(false)
        .constrain(false)
        .show(ctx, |ui| {
            let (rect, _resp) = ui.allocate_exact_size(scrim_rect.size(), Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, scrim_color);
        });

    if let Some(top_layer) = active_top_layer {
        ctx.memory_mut(|mem| {
            let areas = mem.areas_mut();
            areas.move_to_top(scrim_layer);
            areas.move_to_top(top_layer);
            areas.set_sublayer(scrim_layer, top_layer);
        });
    }
}

/// 在弹窗上方统一处理 Esc 关闭优先级：
/// 1) `shortcuts_capturing` 由 `capture_input` 率先消费；
/// 2) 下拉菜单 / Popup 展开或模态确认/错误框打开时不抢 Esc；
/// 3) 快捷键弹窗若有冲突提示，Esc 优先关闭冲突提示；
/// 4) 两个弹窗叠放时 Esc 只关闭最上层快捷键弹窗；否则关闭当前打开的弹窗。
pub fn handle_dialogs_escape(
    ctx: &egui::Context,
    settings_open: &mut bool,
    shortcuts_open: &mut bool,
    shortcuts_has_conflict: &mut bool,
    blocking_overlay_open: bool,
) -> bool {
    if (!*settings_open && !*shortcuts_open) || blocking_overlay_open || egui::Popup::is_any_open(ctx) {
        return false;
    }
    if !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        return false;
    }
    if *shortcuts_open {
        if *shortcuts_has_conflict {
            *shortcuts_has_conflict = false;
        } else {
            *shortcuts_open = false;
        }
    } else if *settings_open {
        *settings_open = false;
    }
    true
}

/// 计算当前视口下弹窗的自适应尺寸与屏幕居中坐标（高度不超过 `ctx.content_rect().height() * 0.82`）。
pub fn dialog_layout_bounds(ctx: &egui::Context, desired_w: f32, max_desired_h: f32) -> (f32, f32, Rect) {
    let screen = ctx.content_rect();
    let w = desired_w.min((screen.width() - 24.0).max(280.0));
    let cap_h = (screen.height() * 0.82).floor().min((screen.height() - 16.0).max(160.0));
    let h = max_desired_h.min(cap_h).max(180.0_f32.min(screen.height() - 12.0));
    (w, h, screen)
}

/// 弹窗内容三段回调，便于在 `dialog_shell` 单一闭包内顺序共享 `&mut` 状态。
pub enum DialogSection {
    SubHeader,
    Body,
    Footer,
}

/// 弹窗外壳：无默认标题栏、主题边框圆角阴影、居中可由头部拖拽移动、固定头部 + 可选固定子头部 + 自适应滚动内容区 + 固定底部栏。
#[allow(clippy::too_many_arguments)]
pub fn dialog_shell(
    ctx: &egui::Context,
    dialog_id: Id,
    desired_w: f32,
    max_desired_h: f32,
    scroll_id: &str,
    title: &str,
    subtitle: &str,
    open: &mut bool,
    mut content: impl FnMut(&mut egui::Ui, DialogSection),
) -> Option<Rect> {
    if !*open {
        reset_dialog_drag_offset(ctx, dialog_id);
        return None;
    }

    let spec = theme::current(ctx);
    let (dialog_w, max_total_h, screen) = dialog_layout_bounds(ctx, desired_w, max_desired_h);

    // 在 Area/Window::begin 之前读取上一帧头部拖动结果，避免被 Area::end 覆盖位置状态。
    let drag_id = header_drag_id(dialog_id);
    let offset_id = drag_offset_key(dialog_id);
    let mut drag_offset = ctx.data(|d| d.get_temp::<Vec2>(offset_id)).unwrap_or(Vec2::ZERO);
    if let Some(drag_resp) = ctx.read_response(drag_id)
        && drag_resp.dragged()
    {
        let delta = ctx.input(|i| i.pointer.delta());
        if delta != Vec2::ZERO {
            drag_offset += delta;
        }
    }
    let max_dx = ((screen.width() - dialog_w) * 0.5 - 8.0).max(0.0);
    let max_dy = ((screen.height() - max_total_h) * 0.5 - 8.0).max(0.0);
    drag_offset.x = drag_offset.x.clamp(-max_dx, max_dx);
    drag_offset.y = drag_offset.y.clamp(-max_dy, max_dy);
    ctx.data_mut(|d| d.insert_temp(offset_id, drag_offset));

    let anchor_pos = screen.center() + drag_offset;
    let mut close_clicked = false;
    let mut window_rect = None;

    let frame = Frame::new()
        .fill(spec.dialog.bg)
        .stroke(Stroke::new(1.0, spec.dialog.border))
        .corner_radius(CornerRadius::same(DIALOG_CORNER_RADIUS))
        .shadow(spec.window_shadow)
        .inner_margin(Margin::ZERO);

    let win_resp = egui::Window::new(title)
        .id(dialog_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .drag_area(WindowDrag::Off)
        .order(Order::Middle)
        .constrain_to(screen)
        .pivot(Align2::CENTER_CENTER)
        .fixed_pos(anchor_pos)
        .fixed_size(vec2(dialog_w, max_total_h))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(dialog_w);
            ui.spacing_mut().item_spacing = vec2(8.0, 0.0);

            if dialog_header(ui, dialog_id, title, subtitle) {
                close_clicked = true;
            }

            let sub_resp = ui.scope(|ui| {
                ui.set_width(dialog_w);
                content(ui, DialogSection::SubHeader);
            });
            let sub_h = sub_resp.response.rect.height();

            let avail_body_h = (max_total_h - DIALOG_HEADER_H - DIALOG_FOOTER_H - sub_h).max(72.0);
            egui::ScrollArea::vertical()
                .id_salt(scroll_id)
                .max_height(avail_body_h)
                .min_scrolled_height(avail_body_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(dialog_w);
                    content(ui, DialogSection::Body);
                });

            dialog_footer(ui, dialog_w, |ui| content(ui, DialogSection::Footer));
        });

    if let Some(resp) = win_resp {
        let r = resp.response.rect;
        window_rect = Some(r);
        if let Some(hi) = spec.dialog.top_highlight {
            let rad = DIALOG_CORNER_RADIUS as f32;
            let painter = ctx.layer_painter(LayerId::new(Order::Middle, dialog_id));
            painter.hline((r.left() + rad)..=(r.right() - rad), r.top() + 1.0, Stroke::new(1.0, hi));
        }
    }

    if close_clicked {
        *open = false;
        reset_dialog_drag_offset(ctx, dialog_id);
    }
    window_rect
}

/// 自绘弹窗头部（高 52pt）：左 3×16 `accent` 竖条 + 标题与副标题，右关闭按钮（复用 `window_controls` 配色与矢量 ×），底 1px 分隔线，支持拖动窗口。
pub fn dialog_header(ui: &mut egui::Ui, dialog_id: Id, title: &str, subtitle: &str) -> bool {
    let ctx = ui.ctx().clone();
    let spec = theme::current(&ctx);
    let ppp = ctx.pixels_per_point().max(1.0);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, DIALOG_HEADER_H), Sense::hover());

    // 柔和顶部区域底色（保留顶部圆角避免盖住外框边线）
    let inner_r = rect.shrink(1.0);
    ui.painter().rect_filled(
        inner_r,
        CornerRadius {
            nw: DIALOG_CORNER_RADIUS.saturating_sub(1),
            ne: DIALOG_CORNER_RADIUS.saturating_sub(1),
            sw: 0,
            se: 0,
        },
        spec.dialog.chrome_bg,
    );

    // 底部 1px 分隔线
    let line_y = theme::snap_line_coord(rect.bottom() - 0.5, 1.0, ppp);
    ui.painter()
        .hline(rect.left()..=rect.right(), line_y, Stroke::new(1.0, spec.border));

    // 右侧关闭按钮（与 window_controls 交互与图标一致）
    let close_sz = 28.0;
    let close_rect = Rect::from_center_size(
        pos2(rect.right() - 16.0 - close_sz * 0.5, rect.center().y),
        vec2(close_sz, close_sz),
    );
    let close_id = dialog_id.with("close_btn");
    let close_resp = ui.interact(close_rect, close_id, Sense::click());
    if close_resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }

    // 头部其余区域作为窗口拖拽热区
    let drag_rect = Rect::from_min_max(rect.min, pos2(close_rect.left() - 6.0, rect.bottom()));
    let drag_resp = ui.interact(drag_rect, header_drag_id(dialog_id), Sense::drag());
    if drag_resp.hovered() && !close_resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::Grab);
    } else if drag_resp.dragged() {
        ctx.set_cursor_icon(CursorIcon::Grabbing);
    }

    // 左侧 3×16 accent 竖条 + 标题 + 副标题
    let bar_rect = Rect::from_center_size(pos2(rect.left() + 18.0 + 1.5, rect.center().y - 7.0), vec2(3.0, 16.0));
    ui.painter().rect_filled(bar_rect, 1.5, spec.accent);

    let text_left = bar_rect.right() + 10.0;
    let text_max_w = (close_rect.left() - text_left - 10.0).max(40.0);
    let title_galley = ui.fonts_mut(|f| {
        f.layout(
            title.to_string(),
            FontId::new(15.0, bold_family()),
            spec.strong_fg,
            text_max_w,
        )
    });
    let sub_galley = ui.fonts_mut(|f| {
        f.layout(
            subtitle.to_string(),
            FontId::proportional(11.5),
            spec.muted_fg,
            text_max_w,
        )
    });
    let total_th = title_galley.size().y + 2.0 + sub_galley.size().y;
    let top_y = theme::snap_px(rect.center().y - total_th * 0.5, ppp);
    ui.painter().galley(pos2(text_left, top_y), title_galley.clone(), spec.strong_fg);
    ui.painter().galley(
        pos2(text_left, top_y + title_galley.size().y + 2.0),
        sub_galley,
        spec.muted_fg,
    );

    // 绘制关闭按钮（悬停时使用 close_bg_hover / close_icon_hover）
    let hover_t = ctx.animate_bool_with_time(close_id.with("hover"), close_resp.hovered(), 0.10);
    let active_t = ctx.animate_bool_with_time(close_id.with("active"), close_resp.is_pointer_button_down_on(), 0.08);
    let (btn_bg, btn_stroke, icon_fg) =
        window_frame::compute_window_btn_colors(spec, WindowControlBtn::Close, false, true, hover_t, active_t);
    let wc = &spec.window_controls;
    let cr = CornerRadius::same(wc.corner_radius.min(7));
    if hover_t > 0.01 && btn_bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(close_rect, cr, btn_bg);
    }
    if hover_t > 0.01 && btn_stroke != Stroke::NONE {
        ui.painter().rect_stroke(close_rect, cr, btn_stroke, StrokeKind::Inside);
    }
    theme::paint_close_x_icon(ui.painter(), close_rect.center(), 4.6, 1.55, icon_fg);

    close_resp.on_hover_text("关闭 (Esc)").clicked()
}

/// 自绘弹窗固定底部栏（高 52pt）：上方 1px `border` 分隔线 + 左右内边距 16pt。
pub fn dialog_footer(ui: &mut egui::Ui, width: f32, add_contents: impl FnOnce(&mut egui::Ui)) {
    let ctx = ui.ctx().clone();
    let spec = theme::current(&ctx);
    let ppp = ctx.pixels_per_point().max(1.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, DIALOG_FOOTER_H), Sense::hover());

    let inner_r = rect.shrink(1.0);
    ui.painter().rect_filled(
        inner_r,
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: DIALOG_CORNER_RADIUS.saturating_sub(1),
            se: DIALOG_CORNER_RADIUS.saturating_sub(1),
        },
        spec.dialog.chrome_bg,
    );
    let line_y = theme::snap_line_coord(rect.top() + 0.5, 1.0, ppp);
    ui.painter()
        .hline(rect.left()..=rect.right(), line_y, Stroke::new(1.0, spec.border));

    let content_rect = rect.shrink2(vec2(16.0, 8.0));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(content_rect));
    add_contents(&mut child);
}

// ============================================================================
// 按钮与标签组件
// ============================================================================

/// 主按钮：`accent` 底色 + `on_accent` 文字，圆角 6。
pub fn primary_button(ui: &mut egui::Ui, label: &str) -> Response {
    let spec = theme::current(ui.ctx());
    primary_color_button(ui, label, spec.accent, spec.dialog.on_accent)
}

/// 使用指定填充色与前景色的高强调主按钮（例如冲突条使用 `semantic.warn` 与 `dialog.on_warn`）。
pub fn primary_color_button(ui: &mut egui::Ui, label: &str, fill: Color32, fg: Color32) -> Response {
    let spec = theme::current(ui.ctx());
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), FontId::new(13.0, bold_family()), fg));
    let desired = vec2((galley.size().x + 28.0).max(72.0), 30.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let bg = if resp.is_pointer_button_down_on() {
        fill.gamma_multiply(0.84)
    } else if resp.hovered() {
        if theme::is_light_surface(fill) {
            fill.gamma_multiply(0.90)
        } else {
            fill.gamma_multiply(1.08)
        }
    } else {
        fill
    };
    ui.painter().rect_filled(rect, 6.0, bg);
    if let Some(hi) = spec.dialog.glass_highlight {
        ui.painter().hline((rect.left() + 6.0)..=(rect.right() - 6.0), rect.top() + 0.6, Stroke::new(1.0, hi));
    }
    ui.painter()
        .galley(pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5), galley, fg);
    resp
}

/// 次要按钮：透明底 + `1px border` 描边，悬停时填充 `hover_bg`，圆角 6。
pub fn secondary_button(ui: &mut egui::Ui, label: &str) -> Response {
    let spec = theme::current(ui.ctx());
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(13.0), spec.text_fg));
    let desired = vec2((galley.size().x + 24.0).max(68.0), 30.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let bg = if resp.is_pointer_button_down_on() {
        spec.active_bg
    } else if resp.hovered() {
        spec.hover_bg
    } else {
        Color32::TRANSPARENT
    };
    let stroke_col = if resp.hovered() { spec.accent.gamma_multiply(0.65) } else { spec.border };
    ui.painter().rect_filled(rect, 6.0, bg);
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(1.0, stroke_col), StrokeKind::Inside);
    ui.painter().galley(
        pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5),
        galley,
        if resp.hovered() { spec.strong_fg } else { spec.text_fg },
    );
    resp
}

/// 两段并排次要按钮组（供「导入 JSON | 导出 JSON」等并列操作使用）。
pub fn segmented_secondary_pair(ui: &mut egui::Ui, left_label: &str, right_label: &str) -> (Response, Response) {
    let spec = theme::current(ui.ctx());
    let font = FontId::proportional(12.5);
    let lg = ui.fonts_mut(|f| f.layout_no_wrap(left_label.to_string(), font.clone(), spec.text_fg));
    let rg = ui.fonts_mut(|f| f.layout_no_wrap(right_label.to_string(), font, spec.text_fg));
    let lw = (lg.size().x + 22.0).max(76.0);
    let rw = (rg.size().x + 22.0).max(76.0);
    let h = 30.0;
    let (full, _) = ui.allocate_exact_size(vec2(lw + rw, h), Sense::hover());

    let l_rect = Rect::from_min_size(full.min, vec2(lw, h));
    let r_rect = Rect::from_min_size(pos2(l_rect.right(), full.top()), vec2(rw, h));
    let l_resp = ui.interact(l_rect, ui.make_persistent_id(("seg_pair_l", left_label)), Sense::click());
    let r_resp = ui.interact(r_rect, ui.make_persistent_id(("seg_pair_r", right_label)), Sense::click());
    if l_resp.hovered() || r_resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let paint_half = |r: Rect, resp: &Response, cr: CornerRadius, g: std::sync::Arc<egui::Galley>| {
        let bg = if resp.is_pointer_button_down_on() {
            spec.active_bg
        } else if resp.hovered() {
            spec.hover_bg
        } else {
            Color32::TRANSPARENT
        };
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(r, cr, bg);
        }
        let fg = if resp.hovered() { spec.strong_fg } else { spec.text_fg };
        ui.painter()
            .galley(pos2(r.center().x - g.size().x * 0.5, r.center().y - g.size().y * 0.5), g, fg);
    };

    paint_half(l_rect, &l_resp, CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, lg);
    paint_half(r_rect, &r_resp, CornerRadius { nw: 0, sw: 0, ne: 6, se: 6 }, rg);
    ui.painter()
        .rect_stroke(full, 6.0, Stroke::new(1.0, spec.border), StrokeKind::Inside);
    ui.painter()
        .vline(l_rect.right(), (full.top() + 5.0)..=(full.bottom() - 5.0), Stroke::new(1.0, spec.border));

    (l_resp, r_resp)
}

/// 危险按钮：`semantic.error` 文字，悬停底色为 `semantic.error` 约 12% 透明度；`armed = true` 时强化确认态底色与描边。
pub fn danger_button(ui: &mut egui::Ui, label: &str, armed: bool) -> Response {
    let spec = theme::current(ui.ctx());
    let err = spec.semantic.error;
    let galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            label.to_string(),
            if armed {
                FontId::new(13.0, bold_family())
            } else {
                FontId::proportional(13.0)
            },
            err,
        )
    });
    let desired = vec2((galley.size().x + 22.0).max(84.0), 30.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let bg = if armed {
        if resp.hovered() {
            err.gamma_multiply(0.22)
        } else {
            err.gamma_multiply(0.14)
        }
    } else if resp.is_pointer_button_down_on() {
        err.gamma_multiply(0.18)
    } else if resp.hovered() {
        err.gamma_multiply(0.12)
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 6.0, bg);
    }
    if armed {
        ui.painter()
            .rect_stroke(rect, 6.0, Stroke::new(1.0, err.gamma_multiply(0.75)), StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter()
            .rect_stroke(rect, 6.0, Stroke::new(1.0, err.gamma_multiply(0.35)), StrokeKind::Inside);
    }
    ui.painter()
        .galley(pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5), galley, err);
    resp
}

/// 矢量绘制逆时针恢复默认图标按钮 `↺`。
pub fn reset_icon_button(ui: &mut egui::Ui, id_salt: impl std::hash::Hash, tip: &str) -> Response {
    let spec = theme::current(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, 5.0, spec.hover_bg);
        ui.painter()
            .rect_stroke(rect, 5.0, Stroke::new(1.0, spec.border), StrokeKind::Inside);
    }
    let _ = id_salt;
    let col = if resp.hovered() { spec.strong_fg } else { spec.muted_fg };
    let c = rect.center();
    let r = 5.2_f32;
    let steps = 14usize;
    let mut pts = Vec::with_capacity(steps + 1);
    let start_a = -0.35_f32;
    let end_a = 4.25_f32;
    for i in 0..=steps {
        let a = start_a + (end_a - start_a) * (i as f32 / steps as f32);
        pts.push(pos2(c.x + r * a.cos(), c.y + r * a.sin()));
    }
    ui.painter().add(egui::epaint::PathShape::line(pts, Stroke::new(1.45, col)));
    // 顶部向左箭头尖
    let head = pos2(c.x - 1.5, c.y - r);
    ui.painter().line_segment([head, pos2(head.x + 3.2, head.y - 2.6)], Stroke::new(1.45, col));
    ui.painter().line_segment([head, pos2(head.x + 3.0, head.y + 2.6)], Stroke::new(1.45, col));

    resp.on_hover_text(tip)
}

/// 胶囊语义标签：底色取语义色约 14% 透明度，文字用该语义色。
pub fn pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), FontId::proportional(11.0), color));
    let sz = galley.size() + vec2(12.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(sz, Sense::hover());
    ui.painter().rect_filled(rect, (sz.y * 0.5).round(), color.gamma_multiply(0.14));
    ui.painter()
        .rect_stroke(rect, (sz.y * 0.5).round(), Stroke::new(1.0, color.gamma_multiply(0.45)), StrokeKind::Inside);
    ui.painter()
        .galley(pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5), galley, color);
}

// ============================================================================
// 分组卡片、设置行、开关、滑块、搜索框、下拉框、分段控件
// ============================================================================

/// 分组卡片：填充 `card_bg`，描边 `card_border`，圆角 8，内边距 `[14, 12]`。
pub fn section_card(
    ui: &mut egui::Ui,
    title: &str,
    right_extra: Option<&str>,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let spec = theme::current(ui.ctx());
    let resp = Frame::new()
        .fill(spec.dialog.card_bg)
        .stroke(Stroke::new(1.0, spec.dialog.card_border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).size(13.0).strong().color(spec.strong_fg));
                if let Some(note) = right_extra {
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(egui::RichText::new(note).size(11.5).color(spec.muted_fg));
                    });
                }
            });
            ui.add_space(6.0);
            add_contents(ui);
        });
    if let Some(hi) = spec.dialog.glass_highlight {
        let r = resp.response.rect;
        ui.painter().hline((r.left() + 8.0)..=(r.right() - 8.0), r.top() + 0.6, Stroke::new(1.0, hi));
    }
}

/// 在卡片内绘制一条柔和分隔线。
pub fn card_divider(ui: &mut egui::Ui) {
    let spec = theme::current(ui.ctx());
    ui.add_space(4.0);
    let y = ui.cursor().top();
    let x0 = ui.cursor().left();
    let x1 = x0 + ui.available_width();
    ui.painter().hline(x0..=x1, y, Stroke::new(1.0, spec.dialog.card_border));
    ui.add_space(5.0);
}

fn is_pointer_hovered_in_layer(ui: &egui::Ui, rect: Rect) -> bool {
    let ctx = ui.ctx();
    let Some(pos) = ctx.pointer_hover_pos() else {
        return false;
    };
    rect.contains(pos) && ctx.layer_id_at(pos) == Some(ui.layer_id())
}

/// 带左侧固定标签列的设置行（行高约 34pt，悬停柔和圆角底色）。
pub fn setting_row(ui: &mut egui::Ui, label: &str, add_control: impl FnOnce(&mut egui::Ui)) {
    let spec = theme::current(ui.ctx());
    let row_w = ui.available_width();
    let label_col_w = SETTING_LABEL_COL_W.min((row_w * 0.42).max(96.0));
    let (row_rect, _) = ui.allocate_exact_size(vec2(row_w, SETTING_ROW_H), Sense::hover());
    if is_pointer_hovered_in_layer(ui, row_rect) {
        ui.painter().rect_filled(row_rect, 6.0, spec.dialog.row_hover_bg);
    }

    let inner = row_rect.shrink2(vec2(6.0, 2.0));
    let label_rect = Rect::from_min_size(inner.min, vec2(label_col_w, inner.height()));
    let ctrl_rect = Rect::from_min_max(pos2(label_rect.right() + 10.0, inner.top()), inner.max);

    let mut label_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(label_rect)
            .layout(egui::Layout::left_to_right(Align::Center)),
    );
    label_ui.label(egui::RichText::new(label).size(13.0).color(spec.text_fg));

    let mut ctrl_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(ctrl_rect)
            .layout(egui::Layout::left_to_right(Align::Center)),
    );
    ctrl_ui.set_clip_rect(ui.clip_rect().intersect(ctrl_rect.expand(2.0)));
    add_control(&mut ctrl_ui);
}

/// 36×20 主题化开关控件，带 `ctx.animate_bool` 轨道/圆点过渡动画（供后续弹窗直接复用）。
#[allow(dead_code)]
pub fn toggle(ui: &mut egui::Ui, id: Id, on: &mut bool, enabled: bool) -> Response {
    let spec = theme::current(ui.ctx());
    let desired = vec2(36.0, 20.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, mut resp) = ui.allocate_exact_size(desired, sense);
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if enabled && resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }

    paint_toggle_at(ui, id.with("anim"), rect, *on, enabled, resp.hovered(), spec);
    resp
}

fn paint_toggle_at(
    ui: &mut egui::Ui,
    anim_id: Id,
    rect: Rect,
    on: bool,
    enabled: bool,
    hovered: bool,
    spec: &theme::ThemeSpec,
) {
    let t = ui.ctx().animate_bool_with_time(anim_id, on, 0.12);
    let radius = rect.height() * 0.5;
    let off_track = spec.input_bg;
    let on_track = if enabled { spec.accent } else { spec.disabled_fg };
    let track_bg = lerp_color(off_track, on_track, t);
    let border_col = if !enabled {
        spec.disabled_fg.gamma_multiply(0.5)
    } else if on {
        spec.accent
    } else if hovered {
        spec.accent.gamma_multiply(0.75)
    } else {
        spec.border
    };

    ui.painter().rect_filled(rect, radius, track_bg);
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1.0, border_col), StrokeKind::Inside);

    let knob_r = 7.0;
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob_col = if !enabled {
        spec.dialog.bg.gamma_multiply(0.85)
    } else {
        lerp_color(spec.muted_fg, spec.dialog.on_accent, t)
    };
    ui.painter().circle_filled(pos2(knob_x, rect.center().y), knob_r, knob_col);
}

/// 开关设置行（整行点击可切换，支持 `indent_level` 缩进、右侧可选额外标签/胶囊及禁用态）。
#[allow(clippy::too_many_arguments)]
pub fn setting_toggle_row(
    ui: &mut egui::Ui,
    id_salt: &str,
    label: &str,
    value: &mut bool,
    enabled: bool,
    indent_level: u8,
    hover_tip: Option<&str>,
    extra_badge: impl FnOnce(&mut egui::Ui),
) -> Response {
    let spec = theme::current(ui.ctx());
    let row_w = ui.available_width();
    let indent = indent_level as f32 * 18.0;
    let (row_rect, mut row_resp) = ui.allocate_exact_size(vec2(row_w, SETTING_ROW_H), Sense::hover());

    let toggle_sz = vec2(36.0, 20.0);
    let toggle_rect = Rect::from_center_size(
        pos2(row_rect.right() - 6.0 - toggle_sz.x * 0.5, row_rect.center().y),
        toggle_sz,
    );
    let click_rect = Rect::from_min_max(pos2(row_rect.left() + indent, row_rect.top()), row_rect.max);
    let id = ui.make_persistent_id(id_salt);
    let interact = ui.interact(click_rect, id, if enabled { Sense::click() } else { Sense::hover() });
    if enabled && interact.hovered() {
        ui.painter().rect_filled(row_rect, 6.0, spec.dialog.row_hover_bg);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    if indent_level > 0 {
        let guide_x = row_rect.left() + 8.0;
        ui.painter().vline(
            guide_x,
            (row_rect.top() + 5.0)..=(row_rect.bottom() - 5.0),
            Stroke::new(1.5, spec.border),
        );
    }

    if enabled && interact.clicked() {
        *value = !*value;
        row_resp.mark_changed();
    }

    let text_col = if enabled { spec.text_fg } else { spec.disabled_fg };
    let text_rect = Rect::from_min_max(
        pos2(row_rect.left() + 6.0 + indent, row_rect.top()),
        pos2(toggle_rect.left() - 10.0, row_rect.bottom()),
    );
    let mut left_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(egui::Layout::left_to_right(Align::Center)),
    );
    left_ui.set_clip_rect(ui.clip_rect().intersect(text_rect));
    left_ui.label(egui::RichText::new(label).size(13.0).color(text_col));
    extra_badge(&mut left_ui);

    // 仅由 paint_toggle_at 绘制视觉状态，保留独立 toggle() 供后续弹窗直接复用
    paint_toggle_at(ui, id.with("anim"), toggle_rect, *value, enabled, interact.hovered(), spec);

    if let Some(tip) = hover_tip {
        let _ = interact.on_hover_text(tip);
    }
    row_resp
}

/// 带进度轨道着色的自定义主题滑块：已划部分使用 `accent`，剩余部分使用 `border`，右侧带固定宽度等宽数值标签。
pub fn value_slider(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step: f32,
    formatted_value: &str,
    value_col_w: f32,
) -> (bool, bool) {
    let spec = theme::current(ui.ctx());
    let total_w = ui.available_width();
    let label_w = value_col_w.min((total_w * 0.34).max(52.0));
    let track_w = (total_w - label_w - 10.0).max(70.0);
    let (full_rect, _) = ui.allocate_exact_size(vec2(total_w, 26.0), Sense::hover());

    let interact_rect = Rect::from_min_size(full_rect.min, vec2(track_w, full_rect.height()));
    let id = ui.make_persistent_id(id_salt);
    let resp = ui.interact(interact_rect, id, Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let min_v = *range.start();
    let max_v = *range.end();
    let span = (max_v - min_v).max(f32::EPSILON);

    let pad_x = 8.0;
    let rail_left = interact_rect.left() + pad_x;
    let rail_right = (interact_rect.right() - pad_x).max(rail_left + 12.0);
    let rail_w = rail_right - rail_left;
    let cy = interact_rect.center().y;

    let mut changed = false;
    if (resp.clicked() || resp.dragged())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let frac = ((pos.x - rail_left) / rail_w).clamp(0.0, 1.0);
        let mut next = min_v + frac * span;
        if step > 0.0 {
            next = min_v + ((next - min_v) / step).round() * step;
        }
        next = next.clamp(min_v, max_v);
        if (next - *value).abs() > 1e-5 {
            *value = next;
            changed = true;
        }
    }

    let frac = ((*value - min_v) / span).clamp(0.0, 1.0);
    let knob_x = rail_left + frac * rail_w;
    let rail_h = 4.0;
    let full_rail = Rect::from_min_max(pos2(rail_left, cy - rail_h * 0.5), pos2(rail_right, cy + rail_h * 0.5));
    ui.painter().rect_filled(full_rail, rail_h * 0.5, spec.border);

    let filled_rail = Rect::from_min_max(full_rail.min, pos2(knob_x.max(rail_left + rail_h * 0.5), full_rail.max.y));
    ui.painter().rect_filled(filled_rail, rail_h * 0.5, spec.accent);

    let knob_r = if resp.dragged() {
        8.0
    } else if resp.hovered() {
        7.5
    } else {
        6.5
    };
    ui.painter().circle_filled(pos2(knob_x, cy), knob_r, spec.dialog.bg);
    ui.painter()
        .circle_stroke(pos2(knob_x, cy), knob_r, Stroke::new(2.0, spec.accent));

    // 右侧固定宽度的等宽数值+单位胶囊标签
    let val_rect = Rect::from_min_max(pos2(interact_rect.right() + 10.0, full_rect.top()), full_rect.max);
    let val_galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            formatted_value.to_string(),
            FontId::new(12.0, FontFamily::Monospace),
            spec.strong_fg,
        )
    });
    ui.painter().rect_filled(val_rect.shrink2(vec2(0.0, 2.0)), 5.0, spec.input_bg);
    ui.painter()
        .rect_stroke(val_rect.shrink2(vec2(0.0, 2.0)), 5.0, Stroke::new(1.0, spec.border), StrokeKind::Inside);
    ui.painter().galley(
        pos2(
            val_rect.right() - 8.0 - val_galley.size().x,
            val_rect.center().y - val_galley.size().y * 0.5,
        ),
        val_galley,
        spec.strong_fg,
    );

    let committed = changed && !resp.dragged() || resp.drag_stopped();
    (changed, committed)
}

/// 主题化搜索框：`input_bg` 背景、圆角 6、聚焦时 `accent` 描边，左边放放大镜图标，有内容时右边显示 × 清空。
pub fn search_input(ui: &mut egui::Ui, id_salt: &str, text: &mut String, width: f32, hint: &str) -> Response {
    let spec = theme::current(ui.ctx());
    let id = ui.make_persistent_id(id_salt);
    let has_focus = ui.memory(|m| m.has_focus(id));
    let desired_w = width.min(ui.available_width().max(110.0));
    let (rect, _) = ui.allocate_exact_size(vec2(desired_w, 30.0), Sense::hover());

    let border_col = if has_focus {
        spec.accent
    } else if is_pointer_hovered_in_layer(ui, rect) {
        spec.accent.gamma_multiply(0.55)
    } else {
        spec.border
    };
    ui.painter().rect_filled(rect, 6.0, spec.input_bg);
    ui.painter()
        .rect_stroke(rect, 6.0, Stroke::new(if has_focus { 1.3 } else { 1.0 }, border_col), StrokeKind::Inside);

    // 左侧放大镜图标
    let icon_center = pos2(rect.left() + 14.0, rect.center().y - 1.0);
    let icon_col = if has_focus { spec.accent } else { spec.muted_fg };
    ui.painter().circle_stroke(icon_center, 4.3, Stroke::new(1.35, icon_col));
    ui.painter().line_segment(
        [pos2(icon_center.x + 3.0, icon_center.y + 3.0), pos2(icon_center.x + 6.0, icon_center.y + 6.0)],
        Stroke::new(1.5, icon_col),
    );

    let right_pad = if text.is_empty() { 8.0 } else { 26.0 };
    let edit_rect = Rect::from_min_max(pos2(rect.left() + 26.0, rect.top() + 3.0), pos2(rect.right() - right_pad, rect.bottom() - 3.0));
    let mut edit_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(edit_rect)
            .layout(egui::Layout::left_to_right(Align::Center)),
    );
    let resp = edit_ui.add(
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(Frame::NONE)
            .desired_width(edit_rect.width())
            .hint_text(egui::RichText::new(hint).color(spec.muted_fg)),
    );

    if !text.is_empty() {
        let clear_rect = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(18.0, 18.0));
        let clear_resp = ui.interact(clear_rect, id.with("clear"), Sense::click());
        if clear_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            ui.painter().circle_filled(clear_rect.center(), 8.0, spec.hover_bg);
        }
        theme::paint_close_x_icon(
            ui.painter(),
            clear_rect.center(),
            3.3,
            1.4,
            if clear_resp.hovered() { spec.strong_fg } else { spec.muted_fg },
        );
        if clear_resp.on_hover_text("清除搜索").clicked() {
            text.clear();
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    resp
}

/// 主题化下拉选择框：`input_bg` 底色、圆角 6、展开时描边用 `accent`。
pub fn styled_combo(
    ui: &mut egui::Ui,
    id_salt: &str,
    width: f32,
    selected_text: &str,
    add_items: impl FnOnce(&mut egui::Ui),
) {
    let spec = theme::current(ui.ctx());
    let w = width.min(ui.available_width().max(120.0));
    ui.scope(|ui| {
        let style = ui.style_mut();
        style.visuals.widgets.inactive.weak_bg_fill = spec.input_bg;
        style.visuals.widgets.inactive.bg_fill = spec.input_bg;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, spec.border);
        style.visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
        style.visuals.widgets.hovered.weak_bg_fill = spec.input_bg;
        style.visuals.widgets.hovered.bg_fill = spec.hover_bg;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, spec.accent);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
        style.visuals.widgets.open.weak_bg_fill = spec.input_bg;
        style.visuals.widgets.open.bg_fill = spec.input_bg;
        style.visuals.widgets.open.bg_stroke = Stroke::new(1.2, spec.accent);
        style.visuals.widgets.open.corner_radius = CornerRadius::same(6);

        egui::ComboBox::from_id_salt(id_salt)
            .width(w)
            .truncate()
            .selected_text(egui::RichText::new(selected_text).color(spec.text_fg))
            .show_ui(ui, add_items);
    });
}

/// 分段选择器（供「日间 / 夜间热切换偏好」使用）。
pub fn segmented_control<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id_salt: &str,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let spec = theme::current(ui.ctx());
    let count = options.len().max(1);
    let total_w = ui.available_width().min(320.0);
    let seg_w = total_w / count as f32;
    let h = 30.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total_w, h), Sense::hover());

    ui.painter().rect_filled(outer, 7.0, spec.input_bg);
    ui.painter()
        .rect_stroke(outer, 7.0, Stroke::new(1.0, spec.border), StrokeKind::Inside);

    let mut changed = false;
    for (idx, (val, label)) in options.iter().enumerate() {
        let seg_r = Rect::from_min_size(pos2(outer.left() + idx as f32 * seg_w, outer.top()), vec2(seg_w, h));
        let inner_r = seg_r.shrink(2.5);
        let resp = ui.interact(seg_r, ui.make_persistent_id((id_salt, idx)), Sense::click());
        let active = *current == *val;
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }

        if active {
            ui.painter().rect_filled(inner_r, 5.0, spec.dialog.bg);
            ui.painter()
                .rect_stroke(inner_r, 5.0, Stroke::new(1.0, spec.accent), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(inner_r, 5.0, spec.hover_bg);
        }

        let fg = if active { spec.strong_fg } else { spec.muted_fg };
        let font = if active { FontId::new(12.0, bold_family()) } else { FontId::proportional(12.0) };
        let galley = ui.fonts_mut(|f| f.layout_no_wrap((*label).to_string(), font, fg));
        ui.painter().galley(
            pos2(seg_r.center().x - galley.size().x * 0.5, seg_r.center().y - galley.size().y * 0.5),
            galley,
            fg,
        );
        if resp.clicked() && !active {
            *current = *val;
            changed = true;
        }
    }
    changed
}

// ============================================================================
// 主题缩略卡（使用各主题自身配色绘制迷你编辑器预览）
// ============================================================================

/// 5 套主题缩略卡：用目标主题的 `canvas_bg / top_bar_bg / sidebar.bg / accent / text_fg` 绘制迷你界面示意，
/// 下方标注 `short_label` 与 `mode_tag`；选中加 2px `accent` 外环与右上角对勾徽章。
pub fn theme_swatch(ui: &mut egui::Ui, id: ThemeId, selected: bool, card_w: f32) -> Response {
    let cur = theme::current(ui.ctx());
    let target = id.spec();
    let card_h = 84.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let bg = if selected {
        cur.active_bg.gamma_multiply(0.55)
    } else if resp.hovered() {
        cur.hover_bg
    } else {
        cur.dialog.bg
    };
    let outer_stroke = if selected {
        Stroke::new(2.0, cur.accent)
    } else if resp.hovered() {
        Stroke::new(1.2, cur.accent.gamma_multiply(0.65))
    } else {
        Stroke::new(1.0, cur.border)
    };

    ui.painter().rect_filled(rect, 8.0, bg);
    ui.painter().rect_stroke(rect, 8.0, outer_stroke, StrokeKind::Inside);

    // 迷你界面示意框
    let preview_r = Rect::from_min_max(pos2(rect.left() + 6.0, rect.top() + 6.0), pos2(rect.right() - 6.0, rect.top() + 50.0));
    let top_h = 10.0;
    let sb_w = (preview_r.width() * 0.28).round();

    // 画布主区
    ui.painter().rect_filled(preview_r, 4.0, target.canvas_bg);
    // 顶栏（支持 03 撞色分色）
    let top_r = Rect::from_min_size(preview_r.min, vec2(preview_r.width(), top_h));
    if let Some(left_top) = target.top_bar_left_bg {
        ui.painter().rect_filled(
            Rect::from_min_size(top_r.min, vec2(sb_w, top_h)),
            CornerRadius { nw: 4, ne: 0, sw: 0, se: 0 },
            left_top,
        );
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(top_r.left() + sb_w, top_r.top()), top_r.max),
            CornerRadius { nw: 0, ne: 4, sw: 0, se: 0 },
            target.top_bar_bg,
        );
    } else {
        ui.painter().rect_filled(
            top_r,
            CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 },
            target.top_bar_bg,
        );
    }
    // 侧栏
    let sb_r = Rect::from_min_max(pos2(preview_r.left(), top_r.bottom()), pos2(preview_r.left() + sb_w, preview_r.bottom()));
    ui.painter().rect_filled(
        sb_r,
        CornerRadius { nw: 0, ne: 0, sw: 4, se: 0 },
        target.sidebar.bg,
    );
    // 顶栏强调点与正文模拟横线
    ui.painter()
        .circle_filled(pos2(top_r.right() - 6.0, top_r.center().y), 2.3, target.accent);
    ui.painter().hline(
        (sb_r.left() + 3.0)..=(sb_r.right() - 4.0),
        sb_r.top() + 6.0,
        Stroke::new(2.0, target.accent),
    );
    ui.painter().hline(
        (sb_r.left() + 3.0)..=(sb_r.right() - 6.0),
        sb_r.top() + 12.0,
        Stroke::new(1.5, target.sidebar.muted_fg),
    );

    let main_x = sb_r.right() + 6.0;
    let main_r_x = preview_r.right() - 6.0;
    if main_r_x > main_x + 8.0 {
        ui.painter().hline(
            main_x..=(main_x + (main_r_x - main_x) * 0.65),
            top_r.bottom() + 7.0,
            Stroke::new(2.4, target.text_fg),
        );
        ui.painter().hline(
            main_x..=main_r_x,
            top_r.bottom() + 14.0,
            Stroke::new(1.6, target.muted_fg),
        );
        ui.painter().hline(
            main_x..=(main_x + (main_r_x - main_x) * 0.8),
            top_r.bottom() + 20.0,
            Stroke::new(1.6, target.muted_fg),
        );
        // 底部迷你强调药丸
        let pill_r = Rect::from_min_size(pos2(main_r_x - 14.0, preview_r.bottom() - 7.0), vec2(11.0, 4.0));
        ui.painter().rect_filled(pill_r, 2.0, target.accent);
    }

    ui.painter()
        .rect_stroke(preview_r, 4.0, Stroke::new(1.0, target.border), StrokeKind::Inside);

    // 选中态右上角对勾徽章
    if selected {
        let badge_c = pos2(preview_r.right() - 7.0, preview_r.top() + 7.0);
        ui.painter().circle_filled(badge_c, 6.5, cur.accent);
        let ck = Stroke::new(1.6, cur.dialog.on_accent);
        ui.painter().line_segment([pos2(badge_c.x - 3.0, badge_c.y + 0.2), pos2(badge_c.x - 0.6, badge_c.y + 2.5)], ck);
        ui.painter().line_segment([pos2(badge_c.x - 0.6, badge_c.y + 2.5), pos2(badge_c.x + 3.2, badge_c.y - 2.3)], ck);
    }

    // 底部 `short_label` + `mode_tag`
    let name_galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            id.short_label().to_string(),
            FontId::new(10.5, if selected { bold_family() } else { FontFamily::Proportional }),
            if selected { cur.strong_fg } else { cur.text_fg },
        )
    });
    let tag_col = if selected { cur.accent } else { cur.muted_fg };
    let tag_galley = ui.fonts_mut(|f| f.layout_no_wrap(id.mode_tag().to_string(), FontId::proportional(10.0), tag_col));
    ui.painter().galley(
        pos2(rect.center().x - name_galley.size().x * 0.5, preview_r.bottom() + 5.0),
        name_galley,
        cur.text_fg,
    );
    ui.painter().galley(
        pos2(rect.center().x - tag_galley.size().x * 0.5, preview_r.bottom() + 20.0),
        tag_galley,
        tag_col,
    );

    resp.on_hover_text(id.full_label())
}

// ============================================================================
// 键帽与快捷键录制组件
// ============================================================================

/// 渲染单个键帽：等宽字体、`keycap_bg` 填充、圆角 4、立体底边、05 玻璃微高光，内边距 `[6, 2]`。
pub fn keycap(ui: &mut egui::Ui, text: &str, opacity: f32, mini: bool) {
    let spec = theme::current(ui.ctx());
    let op = opacity.clamp(0.1, 1.0);
    let font_sz = if mini { 10.5 } else { 11.5 };
    let pad = if mini { vec2(5.0, 1.5) } else { vec2(6.0, 2.0) };

    let fg = spec.text_fg.gamma_multiply(op);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), FontId::new(font_sz, FontFamily::Monospace), fg));
    let sz = vec2((galley.size().x + pad.x * 2.0).max(if mini { 18.0 } else { 22.0 }), galley.size().y + pad.y * 2.0);
    let (rect, _) = ui.allocate_exact_size(sz, Sense::hover());

    ui.painter()
        .rect_filled(rect, 4.0, spec.dialog.keycap_bg.gamma_multiply(op));
    ui.painter().rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, spec.dialog.keycap_border.gamma_multiply(op)),
        StrokeKind::Inside,
    );
    // 底部加深 1px 立体键帽阴影边
    ui.painter().hline(
        (rect.left() + 3.0)..=(rect.right() - 3.0),
        rect.bottom() - 0.6,
        Stroke::new(1.4, spec.dialog.keycap_bottom_border.gamma_multiply(op)),
    );
    if let Some(hi) = spec.dialog.glass_highlight {
        ui.painter().hline(
            (rect.left() + 3.0)..=(rect.right() - 3.0),
            rect.top() + 0.6,
            Stroke::new(1.0, hi.gamma_multiply(op)),
        );
    }

    ui.painter().galley(
        pos2(rect.center().x - galley.size().x * 0.5, rect.center().y - galley.size().y * 0.5 - 0.4),
        galley,
        fg,
    );
}

/// 基于 `Shortcut` 的结构化字段绘制一组键帽（键帽之间用 `muted_fg` 的 `+` 连接）。
pub fn shortcut_keys(ui: &mut egui::Ui, sc: Shortcut, opacity: f32, mini: bool) {
    let spec = theme::current(ui.ctx());
    let (parts, len) = sc.keycaps();
    let plus_col = spec.muted_fg.gamma_multiply(opacity.clamp(0.1, 1.0));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = if mini { 3.0 } else { 4.0 };
        for (i, part) in parts[..len].iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new("+").size(if mini { 10.5 } else { 11.5 }).color(plus_col));
            }
            keycap(ui, part, opacity, mini);
        }
    });
}

/// 绘制圆角虚线框（供「未绑定」状态使用）。
pub fn paint_dashed_rounded_rect(painter: &egui::Painter, rect: Rect, radius: f32, stroke: Stroke) {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let draw_dashed = |p0: Pos2, p1: Pos2| {
        let len = p0.distance(p1);
        if len <= 1.0 {
            return;
        }
        let dir = (p1 - p0) / len;
        let dash = 4.0;
        let gap = 3.0;
        let mut d = 0.0;
        while d < len {
            let d_end = (d + dash).min(len);
            painter.line_segment([p0 + dir * d, p0 + dir * d_end], stroke);
            d += dash + gap;
        }
    };
    draw_dashed(pos2(rect.left() + r, rect.top()), pos2(rect.right() - r, rect.top()));
    draw_dashed(pos2(rect.left() + r, rect.bottom()), pos2(rect.right() - r, rect.bottom()));
    draw_dashed(pos2(rect.left(), rect.top() + r), pos2(rect.left(), rect.bottom() - r));
    draw_dashed(pos2(rect.right(), rect.top() + r), pos2(rect.right(), rect.bottom() - r));
}

/// 快捷键列点击录制控件：
/// - 已绑定：可点击区域（最小宽约 150）内展示结构化键帽，悬停描边变 `accent`
/// - 未绑定：`disabled_fg` 虚线框 + 「未绑定」
/// - 录制态：`accent` 描边 + `accent` 10% 底色 + 呼吸圆点（仅录制时周期刷新）+ 「按下新组合键…」
pub fn shortcut_recorder(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    current: Option<Shortcut>,
    recording: bool,
    min_width: f32,
) -> Response {
    let spec = theme::current(ui.ctx());
    let w = min_width.max(120.0);
    let h = 28.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    if recording {
        let time = ui.input(|i| i.time);
        let pulse = 0.38 + 0.62 * ((time * 3.6).sin() * 0.5 + 0.5) as f32;
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));

        ui.painter().rect_filled(rect, 6.0, spec.accent.gamma_multiply(0.10));
        ui.painter()
            .rect_stroke(rect, 6.0, Stroke::new(1.4, spec.accent), StrokeKind::Inside);

        let dot_c = pos2(rect.left() + 12.0, rect.center().y);
        ui.painter()
            .circle_filled(dot_c, 3.4, spec.accent.gamma_multiply(pulse));

        let g = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                "按下新组合键…".to_string(),
                FontId::new(12.0, bold_family()),
                spec.accent,
            )
        });
        ui.painter()
            .galley(pos2(dot_c.x + 8.0, rect.center().y - g.size().y * 0.5), g, spec.accent);
    } else if let Some(sc) = current {
        let hovered = resp.hovered();
        let bg = if hovered {
            spec.hover_bg.gamma_multiply(0.65)
        } else {
            Color32::TRANSPARENT
        };
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 6.0, bg);
        }
        let border_col = if hovered {
            spec.accent
        } else {
            spec.border.gamma_multiply(0.55)
        };
        ui.painter()
            .rect_stroke(rect, 6.0, Stroke::new(1.0, border_col), StrokeKind::Inside);

        let inner = rect.shrink2(vec2(6.0, 2.0));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(ui.clip_rect().intersect(inner));
        shortcut_keys(&mut child, sc, 1.0, false);
    } else {
        let hovered = resp.hovered();
        let stroke_col = if hovered { spec.accent } else { spec.disabled_fg };
        paint_dashed_rounded_rect(ui.painter(), rect.shrink(0.5), 5.0, Stroke::new(1.0, stroke_col));
        let txt_col = if hovered { spec.text_fg } else { spec.disabled_fg };
        let g = ui.fonts_mut(|f| f.layout_no_wrap("未绑定".to_string(), FontId::proportional(12.0), txt_col));
        ui.painter().galley(
            pos2(rect.left() + 10.0, rect.center().y - g.size().y * 0.5),
            g,
            txt_col,
        );
    }

    let _ = id_salt;
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let [ar, ag, ab, aa] = a.to_array();
    let [br, bg, bb, ba] = b.to_array();
    let ch = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(ch(ar, br), ch(ag, bg), ch(ab, bb), ch(aa, ba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_shell_respects_82_percent_height_across_zooms_and_stays_idle() {
        for zoom in [0.75_f32, 1.0, 2.0] {
            let ctx = egui::Context::default();
            crate::fonts::setup(&ctx);
            theme::apply(&ctx, ThemeId::TitaniumGlass);
            ctx.set_zoom_factor(zoom);

            let mut open = true;
            let mut val = 16.0_f32;
            let mut flag = true;
            let mut rect = None;
            let mut last_delay = std::time::Duration::ZERO;

            for _ in 0..25 {
                let raw = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0 / zoom, 800.0 / zoom))),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |_ui| {
                    paint_backdrop_and_sync_layers(&ctx, true, false);
                    rect = dialog_shell(
                        &ctx,
                        settings_dialog_id(),
                        560.0,
                        680.0,
                        "test_settings_scroll",
                        "外观与编辑器设置",
                        "调整主题、字体与编辑体验，修改即时生效",
                        &mut open,
                        |ui, section| match section {
                            DialogSection::SubHeader => {}
                            DialogSection::Body => {
                                section_card(ui, "排版与缩放", None, |ui| {
                                    setting_row(ui, "预览字号", |ui| {
                                        let _ = value_slider(ui, "s", &mut val, 10.0..=32.0, 1.0, "16 pt", 64.0);
                                    });
                                    let _ = setting_toggle_row(ui, "t", "启用选项", &mut flag, true, 0, None, |_| {});
                                });
                            }
                            DialogSection::Footer => {
                                let _ = secondary_button(ui, "自定义快捷键");
                                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                    let _ = primary_button(ui, "完成");
                                });
                            }
                        },
                    );
                });
                out.textures_delta.clear();
                last_delay = out
                    .viewport_output
                    .get(&egui::ViewportId::ROOT)
                    .map(|v| v.repaint_delay)
                    .unwrap_or(std::time::Duration::MAX);
            }

            let screen = ctx.content_rect();
            let r = rect.expect("settings dialog shell should return rect");
            assert!(r.height() <= screen.height() * 0.82 + 2.0, "zoom={zoom} r={r:?} screen={screen:?}");
            assert!(r.width() <= screen.width() + 1.0, "zoom={zoom} r={r:?} screen={screen:?}");
            assert!(r.bottom() <= screen.bottom() + 1.0, "zoom={zoom} r={r:?} screen={screen:?}");
            assert!(
                last_delay > std::time::Duration::from_secs(1),
                "zoom={zoom} idle settings shell should not repaint continuously: {last_delay:?}"
            );
        }
    }
}
