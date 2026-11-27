//! 自定义无边框窗口框架：顶栏几何布局计算、标题栏控制按钮绘制、四边四角缩放命中与 Win10 边框降级。

use crate::theme::{self, ThemeSpec, WindowControlStyle, SIDEBAR_DEFAULT_W, SIDEBAR_MAX_W, SIDEBAR_MIN_W, TOP_PILL_H};
use eframe::egui::{
    self, pos2, vec2, Color32, Context, CornerRadius, CursorIcon, LayerId, Order, Painter, PointerButton, Pos2, Rect,
    ResizeDirection, Sense, Shadow, Stroke, StrokeKind, ViewportCommand,
};
use std::ops::RangeInclusive;
use std::path::PathBuf;

pub const RESIZE_EDGE_PX: f32 = 5.0;
pub const RESIZE_CORNER_PX: f32 = 10.0;
pub const MANUAL_RIGHT_RESIZE_EDGE_PX: f32 = 3.0;
const COMPACT_WIDTH_THRESHOLD: f32 = 620.0;
const SYSTEM_TITLEBAR_FLAG_FILE: &str = "use_system_titlebar.flag";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowControlBtn {
    Minimize,
    Maximize,
    Close,
}

impl WindowControlBtn {
    pub fn from_env_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "minimize" | "min" => Some(Self::Minimize),
            "maximize" | "restore" | "max" => Some(Self::Maximize),
            "close" | "x" => Some(Self::Close),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WindowBtnLayout {
    pub kind: WindowControlBtn,
    pub visual_rect: Rect,
    pub hit_rect: Rect,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TopBarLayout {
    /// 左侧栏实际分色宽度（用于 03 撞色背景与侧栏无缝衔接）
    pub split_bg_width: f32,
    /// 左侧品牌徽标与菜单按钮所占区域（窄窗口下平滑收紧以避免与标签栏/右侧按钮重叠）
    pub left_controls_rect: Rect,
    pub badge_rect: Rect,
    pub wordmark_pos: Pos2,
    pub menu_btn_rect: Rect,
    pub tabs_rect: Rect,
    pub mode_rect: Rect,
    pub theme_btn_rect: Rect,
    pub window_buttons: Option<[WindowBtnLayout; 3]>,
}

/// 计算当前窗口宽度下侧边栏与左分色顶栏允许的有效宽度区间（宽窗口保持 170..=500，窄窗口下平滑让位给标签栏和右侧控件）。
pub fn effective_sidebar_bounds(win_width: f32) -> (f32, f32) {
    if win_width < COMPACT_WIDTH_THRESHOLD {
        // 480px 时：right≈230px，tabs+gap≈90px，剩余 160px 给左侧与侧栏
        let max_w = (win_width - 230.0 - 90.0).clamp(124.0, SIDEBAR_MIN_W);
        (124.0_f32.min(max_w), max_w)
    } else {
        let max_w = (win_width - 180.0).clamp(SIDEBAR_MIN_W, SIDEBAR_MAX_W);
        (SIDEBAR_MIN_W, max_w)
    }
}

pub fn clamp_sidebar_width(win_width: f32, raw_w: f32) -> f32 {
    let (min_w, max_w) = effective_sidebar_bounds(win_width);
    raw_w.clamp(min_w, max_w)
}

/// 根据顶栏矩形、侧栏状态和窗口控制按钮参数计算各区域几何矩形（纯函数，无 UI 状态副作用）。
pub fn compute_top_bar_layout(
    top_rect: Rect,
    sidebar_open: bool,
    sidebar_w: f32,
    show_window_controls: bool,
    maximized: bool,
    wc: &WindowControlStyle,
) -> TopBarLayout {
    let (sb_min, sb_max) = effective_sidebar_bounds(top_rect.width());
    let compact = top_rect.width() < COMPACT_WIDTH_THRESHOLD;
    let gap = if compact { 4.0 } else { wc.section_gap };
    let right_margin = if show_window_controls {
        if compact { 6.0 } else { wc.right_margin }
    } else if compact {
        8.0
    } else {
        12.0
    };
    let mode_w = if compact { 112.0 } else { 144.0 };
    let theme_w = if compact { 25.0 } else { 28.0 };
    let btn_w = if compact { (wc.button_width - 5.0).max(24.0) } else { wc.button_width };
    let btn_gap = if compact { 2.0 } else { wc.button_gap };
    let btn_h = wc.button_height;
    let cy = top_rect.center().y;

    let mut right_cursor = top_rect.right() - right_margin;
    let window_buttons = if show_window_controls {
        let close_vis = Rect::from_min_size(pos2(right_cursor - btn_w, cy - btn_h * 0.5), vec2(btn_w, btn_h));
        let max_vis = Rect::from_min_size(pos2(close_vis.left() - btn_gap - btn_w, cy - btn_h * 0.5), vec2(btn_w, btn_h));
        let min_vis = Rect::from_min_size(pos2(max_vis.left() - btn_gap - btn_w, cy - btn_h * 0.5), vec2(btn_w, btn_h));

        // 窗口未最大化时，最外圈 5px 留给四边缩放感应区；最大化后关闭按钮贴合屏幕最右上角零死角点击
        let outer_resize_pad = if maximized { 0.0 } else { RESIZE_EDGE_PX };
        let top_hit = top_rect.top() + outer_resize_pad;
        let close_hit = Rect::from_min_max(
            pos2(close_vis.left() - btn_gap * 0.5, top_hit),
            pos2(top_rect.right() - outer_resize_pad, top_rect.bottom()),
        );
        let max_hit = Rect::from_min_max(
            pos2(max_vis.left() - btn_gap * 0.5, top_hit),
            pos2(close_vis.left() - btn_gap * 0.5, top_rect.bottom()),
        );
        let min_hit = Rect::from_min_max(
            pos2(min_vis.left(), top_hit),
            pos2(max_vis.left() - btn_gap * 0.5, top_rect.bottom()),
        );

        right_cursor = min_vis.left() - gap;
        Some([
            WindowBtnLayout { kind: WindowControlBtn::Minimize, visual_rect: min_vis, hit_rect: min_hit },
            WindowBtnLayout { kind: WindowControlBtn::Maximize, visual_rect: max_vis, hit_rect: max_hit },
            WindowBtnLayout { kind: WindowControlBtn::Close, visual_rect: close_vis, hit_rect: close_hit },
        ])
    } else {
        None
    };

    let theme_btn_rect = Rect::from_min_size(pos2(right_cursor - theme_w, cy - TOP_PILL_H * 0.5), vec2(theme_w, TOP_PILL_H));
    let mode_rect = Rect::from_min_size(pos2(theme_btn_rect.left() - gap - mode_w, cy - TOP_PILL_H * 0.5), vec2(mode_w, TOP_PILL_H));

    // 侧栏打开时，左分色宽与左控件区保持完全同宽，防止窄窗标签落入深色左栏
    let min_left_w = 124.0;
    let min_tabs_w = 78.0;
    let tabs_margin = if compact { 5.0 } else { 10.0 };
    let max_left_w = (mode_rect.left() - tabs_margin - 5.0 - min_tabs_w - top_rect.left()).max(min_left_w);
    let split_bg_width = if sidebar_open {
        sidebar_w.clamp(sb_min, sb_max).min(max_left_w)
    } else {
        SIDEBAR_DEFAULT_W
    };
    let left_w = if sidebar_open {
        split_bg_width
    } else {
        SIDEBAR_DEFAULT_W.min(max_left_w)
    };
    let left_controls_rect = Rect::from_min_size(top_rect.min, vec2(left_w, top_rect.height()));

    let left_pad = if compact { 8.0 } else { 14.0 };
    let badge_sz = if compact { 19.0 } else { 22.0 };
    let badge_rect = Rect::from_min_size(pos2(left_controls_rect.left() + left_pad, cy - badge_sz * 0.5), vec2(badge_sz, badge_sz));
    let wordmark_pos = pos2(badge_rect.right() + if compact { 5.0 } else { 8.0 }, cy);
    let menu_right_pad = if compact { 4.0 } else { 8.0 };
    let menu_btn_rect = Rect::from_min_size(
        pos2(left_controls_rect.right() - menu_right_pad - 28.0, cy - 14.0),
        vec2(28.0, 28.0),
    );

    let tabs_left = left_controls_rect.right() + if compact { 4.0 } else { 8.0 };
    let tabs_right = (mode_rect.left() - tabs_margin).max(tabs_left);
    let tabs_rect = Rect::from_min_max(pos2(tabs_left, top_rect.top()), pos2(tabs_right, top_rect.bottom()));

    TopBarLayout {
        split_bg_width,
        left_controls_rect,
        badge_rect,
        wordmark_pos,
        menu_btn_rect,
        tabs_rect,
        mode_rect,
        theme_btn_rect,
        window_buttons,
    }
}

/// 判定指针坐标 `pos` 是否处于四边/四角缩放感应区内（纯函数）。
pub fn hit_test_resize_border(
    win_rect: Rect,
    pos: Pos2,
    edge_px: f32,
    corner_px: f32,
    manual_body_y: Option<&RangeInclusive<f32>>,
    manual_right_edge_px: f32,
) -> Option<ResizeDirection> {
    if !win_rect.contains(pos) {
        return None;
    }
    let dx_left = pos.x - win_rect.left();
    let dx_right = win_rect.right() - pos.x;
    let dy_top = pos.y - win_rect.top();
    let dy_bottom = win_rect.bottom() - pos.y;

    let right_edge = match manual_body_y {
        Some(yr) if yr.contains(&pos.y) => manual_right_edge_px.min(edge_px),
        _ => edge_px,
    };

    let on_left = dx_left < edge_px;
    let on_right = dx_right < right_edge;
    let on_top = dy_top < edge_px;
    let on_bottom = dy_bottom < edge_px;
    if !on_left && !on_right && !on_top && !on_bottom {
        return None;
    }

    let near_left = dx_left < corner_px;
    let near_right = dx_right < corner_px;
    let near_top = dy_top < corner_px;
    let near_bottom = dy_bottom < corner_px;

    Some(match (on_top || near_top, on_bottom || near_bottom, on_left || near_left, on_right || near_right) {
        (true, _, true, _) if on_top || on_left => ResizeDirection::NorthWest,
        (true, _, _, true) if on_top || on_right => ResizeDirection::NorthEast,
        (_, true, true, _) if on_bottom || on_left => ResizeDirection::SouthWest,
        (_, true, _, true) if on_bottom || on_right => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::North,
        (_, true, _, _) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        (_, _, _, true) => ResizeDirection::East,
        _ => return None,
    })
}

pub fn resize_cursor(dir: ResizeDirection) -> CursorIcon {
    match dir {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

fn scale_premultiplied(c: Color32, factor: f32) -> Color32 {
    let f = factor.clamp(0.0, 1.0);
    if f <= 0.0 || c == Color32::TRANSPARENT {
        return Color32::TRANSPARENT;
    }
    if f >= 1.0 {
        return c;
    }
    let [r, g, b, a] = c.to_array();
    let sc = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(sc(r), sc(g), sc(b), sc(a))
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

fn lerp_stroke(a: Stroke, b: Stroke, t: f32) -> Stroke {
    let t = t.clamp(0.0, 1.0);
    let a_empty = a.width <= 0.01 || a.color == Color32::TRANSPARENT;
    let b_empty = b.width <= 0.01 || b.color == Color32::TRANSPARENT;
    match (a_empty, b_empty) {
        (true, true) => Stroke::NONE,
        (true, false) => {
            if t <= 0.01 {
                Stroke::NONE
            } else {
                Stroke::new(b.width, scale_premultiplied(b.color, t))
            }
        }
        (false, true) => {
            if t >= 0.99 {
                Stroke::NONE
            } else {
                Stroke::new(a.width, scale_premultiplied(a.color, 1.0 - t))
            }
        }
        (false, false) => {
            let w = a.width + (b.width - a.width) * t;
            if w <= 0.01 {
                Stroke::NONE
            } else {
                Stroke::new(w, lerp_color(a.color, b.color, t))
            }
        }
    }
}

fn snap_px(v: f32, ppp: f32) -> f32 {
    (v * ppp).round() / ppp
}

fn snap_stroke_width(logical_w: f32, ppp: f32) -> f32 {
    (logical_w * ppp).round().max(1.0) / ppp
}

fn snap_line_coord(v: f32, phys_stroke_px: f32, ppp: f32) -> f32 {
    let px = v * ppp;
    let snapped = if phys_stroke_px as i32 % 2 != 0 {
        px.floor() + 0.5
    } else {
        px.round()
    };
    snapped / ppp
}

/// 使用 `Painter` 像素对齐绘制矢量窗口控制图标，确保 100%~200% DPI 缩放下锐利清晰。
pub fn paint_window_control_icon(
    painter: &Painter,
    rect: Rect,
    kind: WindowControlBtn,
    maximized: bool,
    color: Color32,
    base_stroke_width: f32,
) {
    let ppp = painter.pixels_per_point().max(1.0);
    let phys_px = (base_stroke_width * ppp).round().max(1.0);
    let sw = phys_px / ppp;
    let stroke = Stroke::new(sw, color);
    let cx = snap_px(rect.center().x, ppp);
    let cy = snap_px(rect.center().y, ppp);
    let half = 4.6_f32;

    match kind {
        WindowControlBtn::Minimize => {
            let y = snap_line_coord(cy, phys_px, ppp);
            let x0 = snap_px(cx - half, ppp);
            let x1 = snap_px(cx + half, ppp);
            painter.line_segment([pos2(x0, y), pos2(x1, y)], stroke);
        }
        WindowControlBtn::Maximize if !maximized => {
            let x0 = snap_line_coord(cx - half, phys_px, ppp);
            let y0 = snap_line_coord(cy - half, phys_px, ppp);
            let x1 = snap_line_coord(cx + half, phys_px, ppp);
            let y1 = snap_line_coord(cy + half, phys_px, ppp);
            let box_r = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
            painter.rect_stroke(box_r, CornerRadius::same(1), stroke, StrokeKind::Middle);
        }
        WindowControlBtn::Maximize => {
            // 向下还原：错开双框，后框不穿透前框区域
            let sz = 7.4_f32;
            let off = 2.0_f32;
            let fx0 = snap_line_coord(cx - (sz + off) * 0.5, phys_px, ppp);
            let fy1 = snap_line_coord(cy + (sz + off) * 0.5, phys_px, ppp);
            let fx1 = snap_line_coord(fx0 + sz, phys_px, ppp);
            let fy0 = snap_line_coord(fy1 - sz, phys_px, ppp);
            let front = Rect::from_min_max(pos2(fx0, fy0), pos2(fx1, fy1));
            painter.rect_stroke(front, CornerRadius::same(1), stroke, StrokeKind::Middle);

            let bx0 = snap_line_coord(fx0 + off, phys_px, ppp);
            let by0 = snap_line_coord(fy0 - off, phys_px, ppp);
            let bx1 = snap_line_coord(fx1 + off, phys_px, ppp);
            let by1 = snap_line_coord(fy1 - off, phys_px, ppp);
            painter.line_segment([pos2(bx0, fy0), pos2(bx0, by0)], stroke);
            painter.line_segment([pos2(bx0, by0), pos2(bx1, by0)], stroke);
            painter.line_segment([pos2(bx1, by0), pos2(bx1, by1)], stroke);
            painter.line_segment([pos2(bx1, by1), pos2(fx1, by1)], stroke);
        }
        WindowControlBtn::Close => {
            let w = snap_stroke_width(base_stroke_width, ppp);
            let diag_stroke = Stroke::new(w, color);
            let r = half - 0.3;
            painter.line_segment([pos2(cx - r, cy - r), pos2(cx + r, cy + r)], diag_stroke);
            painter.line_segment([pos2(cx + r, cy - r), pos2(cx - r, cy + r)], diag_stroke);
        }
    }
}

/// 计算窗口控制按钮在给定悬停/按下进度下的（背景、描边、图标）颜色。
///
/// 过渡期间以按钮下方实色顶栏表面色作为渐变起点（替代 `Color32::TRANSPARENT`），
/// 避免向透明黑插值以及半透明状态透出下方 `hover_shadow` 暗底导致移出时泛黑灰。
pub(crate) fn compute_window_btn_colors(
    spec: &ThemeSpec,
    kind: WindowControlBtn,
    on_left_split: bool,
    focused: bool,
    hover_t: f32,
    active_t: f32,
) -> (Color32, Stroke, Color32) {
    let wc = &spec.window_controls;
    let idle_fg = match (on_left_split, focused) {
        (true, true) => wc.on_left_split_idle_fg,
        (true, false) => wc.on_left_split_unfocused_fg,
        (false, true) => wc.icon_idle_fg,
        (false, false) => wc.icon_unfocused_fg,
    };
    let hover_t = hover_t.clamp(0.0, 1.0);
    let active_t = active_t.clamp(0.0, 1.0);
    if hover_t <= 0.005 && active_t <= 0.005 {
        return (wc.bg_idle, wc.stroke_idle, idle_fg);
    }

    let bar_surface_bg = if on_left_split {
        spec.top_bar_left_bg.unwrap_or(spec.top_bar_bg)
    } else {
        spec.top_bar_bg
    };
    let idle_bg = if wc.bg_idle == Color32::TRANSPARENT {
        bar_surface_bg
    } else {
        wc.bg_idle
    };

    if kind == WindowControlBtn::Close {
        let bg_h = lerp_color(idle_bg, wc.close_bg_hover, hover_t);
        let bg = lerp_color(bg_h, wc.close_bg_active, active_t);
        let st = lerp_stroke(wc.stroke_idle, wc.close_stroke_hover, hover_t);
        let fg_h = lerp_color(idle_fg, wc.close_icon_hover, hover_t);
        let fg = lerp_color(fg_h, wc.close_icon_active, active_t);
        (bg, st, fg)
    } else {
        let hover_bg_base = if on_left_split { wc.on_left_split_hover_bg } else { wc.bg_hover };
        let bg_h = lerp_color(idle_bg, hover_bg_base, hover_t);
        let bg = lerp_color(bg_h, wc.bg_active, active_t);
        let st_h = lerp_stroke(wc.stroke_idle, wc.stroke_hover, hover_t);
        let st = lerp_stroke(st_h, wc.stroke_active, active_t);
        let fg_h = lerp_color(idle_fg, wc.icon_hover_fg, hover_t);
        let fg = lerp_color(fg_h, wc.icon_active_fg, active_t);
        (bg, st, fg)
    }
}

/// 在顶栏右侧绘制最小化、最大化/还原、关闭按钮，并返回触发的视口动作。
pub fn paint_window_controls(
    ui: &mut egui::Ui,
    buttons: &[WindowBtnLayout; 3],
    spec: &ThemeSpec,
    split_left_x: Option<f32>,
    maximized: bool,
    focused: bool,
    test_hover: Option<WindowControlBtn>,
) -> Option<ViewportCommand> {
    let ctx = ui.ctx().clone();
    let wc = &spec.window_controls;
    let mut cmd: Option<ViewportCommand> = None;

    for b in buttons {
        let id = ui.make_persistent_id(("win_ctrl", b.kind as u8));
        let resp = ui.interact(b.hit_rect, id, Sense::click());
        let is_hover = resp.hovered() || test_hover == Some(b.kind);
        let is_down = resp.is_pointer_button_down_on();
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }

        let hover_t = if test_hover == Some(b.kind) {
            std::env::var("MDPAD_TEST_HOVER_T")
                .ok()
                .and_then(|s| s.trim().parse::<f32>().ok())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0)
        } else {
            ctx.animate_bool_with_time(id.with("hover"), is_hover, 0.10)
        };
        let active_t = ctx.animate_bool_with_time(id.with("active"), is_down, 0.08);

        let on_left_split = split_left_x.is_some_and(|dx| b.visual_rect.center().x < dx);
        let (bg, stroke, fg) = compute_window_btn_colors(spec, b.kind, on_left_split, focused, hover_t, active_t);

        if hover_t > 0.02 && wc.hover_shadow != Shadow::NONE {
            let mut sh = wc.hover_shadow;
            sh.color = scale_premultiplied(sh.color, hover_t);
            ui.painter().add(sh.as_shape(b.visual_rect, wc.corner_radius));
        }
        if bg != Color32::TRANSPARENT {
            ui.painter().rect_filled(b.visual_rect, wc.corner_radius, bg);
        }
        if stroke != Stroke::NONE {
            ui.painter()
                .rect_stroke(b.visual_rect, wc.corner_radius, stroke, StrokeKind::Inside);
        }
        if let Some(hi) = wc.top_highlight_hover
            && hover_t > 0.02
            && b.kind != WindowControlBtn::Close
        {
            let r = wc.corner_radius as f32;
            ui.painter().hline(
                (b.visual_rect.left() + r)..=(b.visual_rect.right() - r),
                b.visual_rect.top() + 0.6,
                Stroke::new(1.0, scale_premultiplied(hi, hover_t)),
            );
        }

        paint_window_control_icon(
            ui.painter(),
            b.visual_rect,
            b.kind,
            maximized,
            fg,
            wc.icon_stroke_width,
        );

        let tip = match b.kind {
            WindowControlBtn::Minimize => "最小化",
            WindowControlBtn::Maximize if maximized => "向下还原",
            WindowControlBtn::Maximize => "最大化",
            WindowControlBtn::Close => "关闭",
        };
        let clicked = resp.on_hover_text(tip).clicked();
        if clicked {
            cmd = Some(match b.kind {
                WindowControlBtn::Minimize => ViewportCommand::Minimized(true),
                WindowControlBtn::Maximize => ViewportCommand::Maximized(!maximized),
                WindowControlBtn::Close => ViewportCommand::Close,
            });
        }
    }

    cmd
}

/// 处理无边框窗口的四边四角拖拽缩放感应及 Windows 10（非圆角）1px 边框降级绘制。
pub fn handle_window_frame_chrome(
    ctx: &Context,
    use_system_titlebar_active: bool,
    dwm_rounded: bool,
    maximized: bool,
    manual_body_y: Option<RangeInclusive<f32>>,
) {
    if use_system_titlebar_active {
        return;
    }

    let win_rect = ctx.content_rect();
    if !maximized && !ctx.egui_is_using_pointer() {
        let hover_pos = ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos()));
        if let Some(pos) = hover_pos {
            let on_bg = ctx.layer_id_at(pos).is_none_or(|l| l == LayerId::background());
            if on_bg
                && let Some(dir) = hit_test_resize_border(
                    win_rect,
                    pos,
                    RESIZE_EDGE_PX,
                    RESIZE_CORNER_PX,
                    manual_body_y.as_ref(),
                    MANUAL_RIGHT_RESIZE_EDGE_PX,
                )
            {
                ctx.set_cursor_icon(resize_cursor(dir));
                if ctx.input(|i| i.pointer.button_pressed(PointerButton::Primary)) {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
                }
            }
        }
    }

    if !dwm_rounded && !maximized {
        let spec = theme::current(ctx);
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, egui::Id::unique("mdpad_win_border")));
        painter.rect_stroke(
            win_rect.shrink(0.5),
            0.0,
            Stroke::new(1.0, spec.window_controls.window_border),
            StrokeKind::Middle,
        );
    }
}

fn flag_file_path(app_id: &str) -> Option<PathBuf> {
    eframe::storage_dir(app_id).map(|d| d.join(SYSTEM_TITLEBAR_FLAG_FILE))
}

fn parse_use_system_flag_from_ron(ron: &str) -> Option<bool> {
    if ron.contains("use_system_titlebar:true") || ron.contains("use_system_titlebar: true") {
        Some(true)
    } else if ron.contains("use_system_titlebar:false") || ron.contains("use_system_titlebar: false") {
        Some(false)
    } else {
        None
    }
}

/// 在创建原生窗口前同步读取“使用系统标题栏”首选项；测试环境变量 `MDPAD_SYSTEM_TITLEBAR=1` 优先。
pub fn load_startup_use_system_titlebar(app_id: &str) -> bool {
    if let Ok(v) = std::env::var("MDPAD_SYSTEM_TITLEBAR") {
        return matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes");
    }
    if std::env::var_os("MDPAD_SCREENSHOT").is_some() || std::env::var("MDPAD_BENCH").as_deref() == Ok("typing") {
        return false;
    }
    if let Some(flag_p) = flag_file_path(app_id)
        && let Ok(s) = std::fs::read_to_string(flag_p)
    {
        return matches!(s.trim(), "1" | "true");
    }
    if let Some(data_dir) = eframe::storage_dir(app_id)
        && let Ok(ron) = std::fs::read_to_string(data_dir.join("app.ron"))
        && let Some(v) = parse_use_system_flag_from_ron(&ron)
    {
        return v;
    }
    false
}

/// 将“使用系统标题栏”开关同步写入旁路标记文件，供下次启动前创建 `ViewportBuilder` 时直接读取。
pub fn save_startup_use_system_titlebar(app_id: &str, enabled: bool) {
    let Some(path) = flag_file_path(app_id) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, if enabled { "1" } else { "0" });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeId, TOP_BAR_H};

    #[test]
    fn top_bar_layout_wide_and_minimum_480_never_overlap() {
        for id in ThemeId::ALL {
            let wc = &id.spec().window_controls;
            for width in [1200.0_f32, 620.0, 480.0] {
                let top = Rect::from_min_size(Pos2::ZERO, vec2(width, TOP_BAR_H));
                let l = compute_top_bar_layout(top, true, 220.0, true, false, wc);
                let btns = l.window_buttons.expect("custom window buttons must be present");

                assert!(l.badge_rect.right() < l.menu_btn_rect.left(), "{id:?} w={width}");
                assert!(l.left_controls_rect.right() < l.tabs_rect.left(), "{id:?} w={width}");
                assert!((l.left_controls_rect.width() - l.split_bg_width).abs() < 0.01, "{id:?} w={width}");
                assert!(l.tabs_rect.width() >= 76.0, "{id:?} tabs width too small at {width}: {}", l.tabs_rect.width());
                assert!(l.tabs_rect.right() < l.mode_rect.left(), "{id:?} w={width}");
                assert!(l.mode_rect.right() < l.theme_btn_rect.left(), "{id:?} w={width}");
                assert!(l.theme_btn_rect.right() < btns[0].visual_rect.left(), "{id:?} w={width}");
                assert!(btns[0].visual_rect.right() < btns[1].visual_rect.left(), "{id:?} w={width}");
                assert!(btns[1].visual_rect.right() < btns[2].visual_rect.left(), "{id:?} w={width}");
                assert!(btns[2].visual_rect.right() <= top.right(), "{id:?} w={width}");

                // 最大化下关闭热区覆盖屏幕最右上角像素
                let max_l = compute_top_bar_layout(top, true, 220.0, true, true, wc);
                let max_btns = max_l.window_buttons.unwrap();
                assert!(max_btns[2].hit_rect.contains(pos2(width - 0.1, 0.1)), "{id:?} w={width}");
            }

            // 回退系统标题栏时不生成自绘窗口三按钮
            let sys_l = compute_top_bar_layout(Rect::from_min_size(Pos2::ZERO, vec2(480.0, TOP_BAR_H)), false, 220.0, false, false, wc);
            assert!(sys_l.window_buttons.is_none());
            assert!(sys_l.tabs_rect.right() < sys_l.mode_rect.left());
            assert!(sys_l.mode_rect.right() < sys_l.theme_btn_rect.left());
        }
    }

    #[test]
    fn resize_hit_test_covers_all_eight_directions_without_stealing_interior() {
        let win = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        let manual_y = 42.0..=772.0;

        // 四角 L 型热区
        assert_eq!(hit_test_resize_border(win, pos2(2.0, 2.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::NorthWest));
        assert_eq!(hit_test_resize_border(win, pos2(8.5, 2.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::NorthWest));
        assert_eq!(hit_test_resize_border(win, pos2(2.0, 8.5), 5.0, 10.0, None, 3.0), Some(ResizeDirection::NorthWest));
        assert_eq!(hit_test_resize_border(win, pos2(1198.0, 2.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::NorthEast));
        assert_eq!(hit_test_resize_border(win, pos2(2.0, 798.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::SouthWest));
        assert_eq!(hit_test_resize_border(win, pos2(1198.0, 798.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::SouthEast));

        // 四边中段
        assert_eq!(hit_test_resize_border(win, pos2(600.0, 2.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::North));
        assert_eq!(hit_test_resize_border(win, pos2(600.0, 798.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::South));
        assert_eq!(hit_test_resize_border(win, pos2(2.0, 400.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::West));
        assert_eq!(hit_test_resize_border(win, pos2(1197.0, 400.0), 5.0, 10.0, None, 3.0), Some(ResizeDirection::East));

        // 内部点与外部点不得命中（含紧贴角点内侧的 7,7）
        assert_eq!(hit_test_resize_border(win, pos2(7.0, 7.0), 5.0, 10.0, None, 3.0), None);
        assert_eq!(hit_test_resize_border(win, pos2(600.0, 400.0), 5.0, 10.0, None, 3.0), None);
        assert_eq!(hit_test_resize_border(win, pos2(-1.0, 400.0), 5.0, 10.0, None, 3.0), None);

        // 语法说明书模式下右侧滚动条中段收窄到 3px
        assert_eq!(hit_test_resize_border(win, pos2(1196.0, 400.0), 5.0, 10.0, Some(&manual_y), 3.0), None);
        assert_eq!(hit_test_resize_border(win, pos2(1198.0, 400.0), 5.0, 10.0, Some(&manual_y), 3.0), Some(ResizeDirection::East));
    }

    #[test]
    fn env_hover_parser_and_ron_flag_fallback_work() {
        assert_eq!(WindowControlBtn::from_env_str("minimize"), Some(WindowControlBtn::Minimize));
        assert_eq!(WindowControlBtn::from_env_str("MAX"), Some(WindowControlBtn::Maximize));
        assert_eq!(WindowControlBtn::from_env_str("close"), Some(WindowControlBtn::Close));
        assert_eq!(WindowControlBtn::from_env_str("unknown"), None);

        assert_eq!(parse_use_system_flag_from_ron("(use_system_titlebar:true)"), Some(true));
        assert_eq!(parse_use_system_flag_from_ron("(use_system_titlebar: false)"), Some(false));
        assert_eq!(parse_use_system_flag_from_ron("(theme: ZenHorizon)"), None);
    }

    #[test]
    fn window_control_transition_colors_have_no_dark_or_gray_dip() {
        assert_eq!(lerp_color(Color32::TRANSPARENT, Color32::WHITE, 0.5).to_array(), [128, 128, 128, 128]);

        for id in ThemeId::ALL {
            let spec = id.spec();
            let wc = &spec.window_controls;
            for kind in [
                WindowControlBtn::Minimize,
                WindowControlBtn::Maximize,
                WindowControlBtn::Close,
            ] {
                for on_left_split in [false, true] {
                    let bar_bg = if on_left_split {
                        spec.top_bar_left_bg.unwrap_or(spec.top_bar_bg)
                    } else {
                        spec.top_bar_bg
                    };
                    let target_bg = if kind == WindowControlBtn::Close {
                        wc.close_bg_hover
                    } else if on_left_split {
                        wc.on_left_split_hover_bg
                    } else {
                        wc.bg_hover
                    };

                    for t in [0.01_f32, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
                        let (bg, _st, _fg) = compute_window_btn_colors(spec, kind, on_left_split, true, t, 0.0);
                        assert_eq!(
                            bg.a(),
                            255,
                            "{id:?} {kind:?} left={on_left_split} t={t} leaked semi-transparent bg over shadow"
                        );
                        for ch_idx in 0..3 {
                            let lo = bar_bg.to_array()[ch_idx].min(target_bg.to_array()[ch_idx]).saturating_sub(1);
                            let hi = bar_bg.to_array()[ch_idx].max(target_bg.to_array()[ch_idx]).saturating_add(1);
                            let val = bg.to_array()[ch_idx];
                            assert!(
                                (lo..=hi).contains(&val),
                                "{id:?} {kind:?} left={on_left_split} t={t} ch={ch_idx} val={val} outside {lo}..={hi}"
                            );
                        }
                    }

                    let (idle_bg, idle_st, _) = compute_window_btn_colors(spec, kind, on_left_split, true, 0.0, 0.0);
                    assert_eq!(idle_bg, wc.bg_idle);
                    assert_eq!(idle_st, wc.stroke_idle);
                }
            }
        }
    }
}
