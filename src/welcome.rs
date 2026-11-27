//! 欢迎主页（设计图 01 · 极简「闪电启动台」）：居中聚光灯布局、2×2 核心入口、最近文档与拖放提示条。

use crate::commands::Command;
use crate::keymap::{Keymap, Shortcut};
use crate::markdown::bold_family;
use crate::settings::{RecentFile, Settings};
use crate::theme::{self, ThemeId, ThemeSpec};
use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Pos2, Rect, Sense, Shadow, Stroke,
    StrokeKind,
};
use std::path::PathBuf;

const MAX_RECENT_DISPLAY: usize = 5;

pub enum WelcomeAction {
    RunCommand(Command),
    OpenRecent(PathBuf),
    TogglePin(PathBuf),
    RemoveRecent(PathBuf),
    ClearUnpinnedRecents,
    CopyPath(PathBuf),
    RevealInExplorer(PathBuf),
    CreateSampleDoc,
}

#[derive(Clone, Copy)]
enum TileIcon {
    NewDoc,
    OpenFile,
    OpenFolder,
    Manual,
}

struct WelcomePalette {
    card_bg: Color32,
    well_bg: Color32,
    tile_hover_bg: Color32,
    row_hover_bg: Color32,
    card_shadow: Shadow,
    glass_highlight: Option<Color32>,
}

impl WelcomePalette {
    fn from_spec(spec: &ThemeSpec) -> Self {
        let card_bg = if spec.id == ThemeId::CarbonGold {
            spec.surface_bg
        } else {
            spec.manual.card_bg
        };
        let well_bg = match spec.id {
            ThemeId::CarbonGold => Color32::from_rgb(0x2B, 0x28, 0x23),
            ThemeId::TitaniumGlass => spec.active_bg,
            _ => spec.preview.callout_bg,
        };
        let tile_hover_bg = if spec.is_dark {
            lerp_color(card_bg, spec.strong_fg, 0.045)
        } else {
            lerp_color(card_bg, spec.accent, 0.045)
        };
        let card_shadow = if spec.is_dark {
            Shadow::NONE
        } else {
            Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: Color32::from_black_alpha(12),
            }
        };
        Self {
            card_bg,
            well_bg,
            tile_hover_bg,
            row_hover_bg: spec.dialog.row_hover_bg,
            card_shadow,
            glass_highlight: spec.dialog.glass_highlight,
        }
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix_ch = |x: u8, y: u8| ((x as f32) + ((y as f32) - (x as f32)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(
        mix_ch(a.r(), b.r()),
        mix_ch(a.g(), b.g()),
        mix_ch(a.b(), b.b()),
        mix_ch(a.a(), b.a()),
    )
}

/// 将 Unix 时间戳格式化为简洁的中文相对时间（如 `"12 分钟前"`、`"昨天"`）。
pub fn format_relative_time(last_opened_unix: u64, now_unix: u64) -> String {
    if last_opened_unix == 0 {
        return String::new();
    }
    let diff = now_unix.saturating_sub(last_opened_unix);
    if diff < 60 {
        "刚刚".into()
    } else if diff < 3_600 {
        format!("{} 分钟前", diff / 60)
    } else if diff < 86_400 {
        format!("{} 小时前", diff / 3_600)
    } else if diff < 2 * 86_400 {
        "昨天".into()
    } else if diff < 30 * 86_400 {
        format!("{} 天前", diff / 86_400)
    } else if diff < 365 * 86_400 {
        format!("{} 个月前", diff / (30 * 86_400))
    } else {
        format!("{} 年前", diff / (365 * 86_400))
    }
}

pub fn show(
    ui: &mut egui::Ui,
    recent_files: &[RecentFile],
    keymap: &Keymap,
    files_hovered: bool,
) -> Option<WelcomeAction> {
    let spec = theme::current(ui.ctx());
    let pal = WelcomePalette::from_spec(spec);
    let mut action: Option<WelcomeAction> = None;
    let now_unix = Settings::now_unix_secs();

    egui::ScrollArea::vertical()
        .id_salt("welcome_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let avail = ui.available_size();
            let max_w = (avail.x - 40.0).clamp(280.0, 600.0);
            let two_col = max_w >= 460.0;
            let visible_recents = recent_files.len().min(MAX_RECENT_DISPLAY);
            let recent_rows = if visible_recents > 0 { visible_recents } else { 2 };
            let recent_card_h = 34.0 + (recent_rows as f32) * 38.0 + if visible_recents == 0 { 30.0 } else { 0.0 };
            let tiles_h = if two_col { 64.0 * 2.0 + 12.0 } else { 64.0 * 4.0 + 36.0 };
            let est_h = 46.0 + 20.0 + tiles_h + 16.0 + recent_card_h + 14.0 + 40.0;
            let top_pad = ((avail.y - est_h) * 0.44).max(18.0);
            let side_pad = ((avail.x - max_w) * 0.5).round().max(0.0);

            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            ui.add_space(top_pad);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if side_pad > 0.0 {
                    ui.add_space(side_pad);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.set_width(max_w);

                    // 1. 品牌头部（Chiseled ⚡ 徽标 + ThunderMD + 版本/快捷键入口）
                    if let Some(act) = show_brand_header(ui, max_w, spec, &pal, keymap) {
                        action = Some(act);
                    }
                    ui.add_space(20.0);

                    // 2. 核心动作 2×2 磁贴
                    if let Some(act) = show_action_tiles(ui, max_w, two_col, spec, &pal, keymap) {
                        action = Some(act);
                    }
                    ui.add_space(16.0);

                    // 3. 最近打开的文稿（或首次冷启动示例引导）
                    if let Some(act) = show_recent_card(ui, max_w, recent_files, now_unix, spec, &pal) {
                        action = Some(act);
                    }
                    ui.add_space(14.0);

                    // 4. 底部虚线拖放提示条
                    show_drop_hint(ui, max_w, spec, &pal, keymap, files_hovered);
                });
            });
            ui.add_space(24.0);
        });

    action
}

fn show_brand_header(
    ui: &mut egui::Ui,
    max_w: f32,
    spec: &ThemeSpec,
    pal: &WelcomePalette,
    keymap: &Keymap,
) -> Option<WelcomeAction> {
    let mut action = None;
    let (hdr_rect, _) = ui.allocate_exact_size(vec2(max_w, 46.0), Sense::hover());

    let badge_rect = Rect::from_min_size(pos2(hdr_rect.left(), hdr_rect.top() + 1.0), vec2(44.0, 44.0));
    ui.painter().add(pal.card_shadow.as_shape(badge_rect, 11.0));
    ui.painter().rect_filled(badge_rect, 11.0, theme::BOLT_YELLOW);
    if !spec.is_dark {
        ui.painter().rect_stroke(
            badge_rect,
            11.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(202, 138, 4, 95)),
            StrokeKind::Inside,
        );
    }
    theme::paint_bolt_on(
        ui.painter(),
        badge_rect.shrink(9.0),
        Color32::from_rgb(0x1C, 0x19, 0x17),
        theme::BOLT_YELLOW,
    );

    let title_x = badge_rect.right() + 13.0;
    let title_galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            "ThunderMD".into(),
            FontId::new(19.0, bold_family()),
            spec.strong_fg,
        )
    });
    let sub_galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            "极速 Markdown 创作空间 · 毫秒级实时渲染".into(),
            FontId::proportional(12.5),
            spec.muted_fg,
        )
    });
    ui.painter()
        .galley(pos2(title_x, hdr_rect.top() + 2.0), title_galley.clone(), spec.strong_fg);
    ui.painter()
        .galley(pos2(title_x, hdr_rect.top() + 26.0), sub_galley.clone(), spec.muted_fg);

    // 右侧胶囊（版本标 + 快捷键设置入口）
    if max_w >= 430.0 {
        let sc_str = keymap.label(Command::OpenShortcuts);
        let sc_label = if sc_str.is_empty() {
            "快捷键设置".to_string()
        } else {
            format!("{sc_str} 快捷键")
        };
        let sc_galley = ui.fonts_mut(|f| f.layout_no_wrap(sc_label, FontId::proportional(11.5), spec.text_fg));
        let sc_w = sc_galley.size().x + 18.0;
        let sc_rect = Rect::from_min_size(
            pos2(hdr_rect.right() - sc_w, hdr_rect.center().y - 13.0),
            vec2(sc_w, 26.0),
        );

        let ver_text = concat!("v", env!("CARGO_PKG_VERSION"));
        let ver_galley = ui.fonts_mut(|f| {
            f.layout_no_wrap(ver_text.into(), FontId::new(11.5, FontFamily::Monospace), spec.muted_fg)
        });
        let ver_w = ver_galley.size().x + 14.0;
        let ver_rect = Rect::from_min_size(
            pos2(sc_rect.left() - 8.0 - ver_w, hdr_rect.center().y - 13.0),
            vec2(ver_w, 26.0),
        );

        let text_right = (title_x + title_galley.size().x).max(title_x + sub_galley.size().x);
        if ver_rect.left() > text_right + 12.0 {
            ui.painter().rect_filled(ver_rect, 6.0, pal.card_bg);
            ui.painter()
                .rect_stroke(ver_rect, 6.0, Stroke::new(1.0, spec.border), StrokeKind::Inside);
            ui.painter().galley(
                pos2(
                    ver_rect.center().x - ver_galley.size().x * 0.5,
                    ver_rect.center().y - ver_galley.size().y * 0.5,
                ),
                ver_galley,
                spec.muted_fg,
            );
        }

        if sc_rect.left() > text_right + 12.0 {
            let sc_resp = ui.interact(sc_rect, ui.make_persistent_id("welcome_hdr_shortcuts"), Sense::click());
            let sc_bg = if sc_resp.hovered() { spec.hover_bg } else { pal.well_bg };
            let sc_border = if sc_resp.hovered() { spec.accent } else { spec.border };
            if sc_resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            ui.painter().rect_filled(sc_rect, 6.0, sc_bg);
            ui.painter()
                .rect_stroke(sc_rect, 6.0, Stroke::new(1.0, sc_border), StrokeKind::Inside);
            let sc_fg = if sc_resp.hovered() { spec.strong_fg } else { spec.text_fg };
            ui.painter().galley(
                pos2(
                    sc_rect.center().x - sc_galley.size().x * 0.5,
                    sc_rect.center().y - sc_galley.size().y * 0.5,
                ),
                sc_galley,
                sc_fg,
            );
            if sc_resp.on_hover_text("打开快捷键设置 (Ctrl+K)").clicked() {
                action = Some(WelcomeAction::RunCommand(Command::OpenShortcuts));
            }
        }
    }

    action
}

fn show_action_tiles(
    ui: &mut egui::Ui,
    max_w: f32,
    two_col: bool,
    spec: &ThemeSpec,
    pal: &WelcomePalette,
    keymap: &Keymap,
) -> Option<WelcomeAction> {
    let tiles = [
        (Command::New, "新建空白文档", "立即进入编辑与分屏", TileIcon::NewDoc),
        (Command::Open, "打开本地文件", "支持 .md / .markdown / .txt", TileIcon::OpenFile),
        (Command::OpenFolder, "打开工作目录", "侧边栏索引整库文章摘要", TileIcon::OpenFolder),
        (Command::OpenManual, "内置语法说明书", "交互示例与一键插入文档", TileIcon::Manual),
    ];

    let gap = 12.0;
    let tile_h = 64.0;
    let mut action = None;

    if two_col {
        let tile_w = ((max_w - gap) * 0.5).floor();
        for row in 0..2 {
            if row > 0 {
                ui.add_space(gap);
            }
            let (row_rect, _) = ui.allocate_exact_size(vec2(max_w, tile_h), Sense::hover());
            for col in 0..2 {
                let idx = row * 2 + col;
                let (cmd, title, sub, icon) = tiles[idx];
                let x = row_rect.left() + (col as f32) * (tile_w + gap);
                let rect = Rect::from_min_size(pos2(x, row_rect.top()), vec2(tile_w, tile_h));
                if paint_action_tile(ui, rect, cmd, title, sub, icon, spec, pal, keymap) {
                    action = Some(WelcomeAction::RunCommand(cmd));
                }
            }
        }
    } else {
        for (idx, (cmd, title, sub, icon)) in tiles.iter().enumerate() {
            if idx > 0 {
                ui.add_space(gap);
            }
            let (rect, _) = ui.allocate_exact_size(vec2(max_w, tile_h), Sense::hover());
            if paint_action_tile(ui, rect, *cmd, title, sub, *icon, spec, pal, keymap) {
                action = Some(WelcomeAction::RunCommand(*cmd));
            }
        }
    }

    action
}

#[allow(clippy::too_many_arguments)]
fn paint_action_tile(
    ui: &mut egui::Ui,
    rect: Rect,
    cmd: Command,
    title: &str,
    subtitle: &str,
    icon: TileIcon,
    spec: &ThemeSpec,
    pal: &WelcomePalette,
    keymap: &Keymap,
) -> bool {
    let id = ui.make_persistent_id(("welcome_tile", cmd.id()));
    let resp = ui.interact(rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let hover_t = ui.ctx().animate_bool_with_time(id.with("h"), resp.hovered(), 0.12);
    let bg = if resp.is_pointer_button_down_on() {
        lerp_color(pal.card_bg, spec.active_bg, 0.7)
    } else {
        lerp_color(pal.card_bg, pal.tile_hover_bg, hover_t)
    };
    let border_col = lerp_color(spec.border, spec.accent, hover_t * 0.88);

    ui.painter().add(pal.card_shadow.as_shape(rect, 10.0));
    ui.painter().rect_filled(rect, 10.0, bg);
    ui.painter()
        .rect_stroke(rect, 10.0, Stroke::new(1.0, border_col), StrokeKind::Inside);
    if let Some(hi) = pal.glass_highlight {
        ui.painter()
            .hline((rect.left() + 8.0)..=(rect.right() - 8.0), rect.top() + 0.6, Stroke::new(1.0, hi));
    }

    // 左侧图标槽
    let box_r = Rect::from_min_size(pos2(rect.left() + 12.0, rect.center().y - 17.0), vec2(34.0, 34.0));
    let well_col = lerp_color(pal.well_bg, spec.active_bg, hover_t * 0.45);
    ui.painter().rect_filled(box_r, 8.0, well_col);
    let icon_col = lerp_color(spec.text_fg, spec.accent, hover_t);
    paint_tile_icon(ui.painter(), box_r.center(), icon, icon_col);

    // 快捷键键帽（右上与标题同行对齐，保证下方副标题独享整行宽度不被挤压）
    let text_left = box_r.right() + 11.0;
    let right_pad = rect.right() - 12.0;
    let title_y = rect.top() + 14.0;
    let sub_y = rect.top() + 36.0;

    let keys_w = if let Some(sc) = keymap.shortcut(cmd) {
        paint_shortcut_keycaps(ui, right_pad, title_y + 7.5, sc, spec, hover_t)
    } else {
        0.0
    };

    let title_clip_right = if keys_w > 0.0 {
        (right_pad - keys_w - 8.0).max(text_left + 20.0)
    } else {
        right_pad
    };
    let title_galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(title.to_string(), FontId::new(13.5, bold_family()), spec.strong_fg)
    });
    ui.painter()
        .with_clip_rect(Rect::from_min_max(pos2(text_left, rect.top()), pos2(title_clip_right, rect.bottom())))
        .galley(pos2(text_left, title_y), title_galley, spec.strong_fg);

    let sub_galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(subtitle.to_string(), FontId::proportional(11.5), spec.muted_fg)
    });
    ui.painter()
        .with_clip_rect(Rect::from_min_max(pos2(text_left, rect.top()), pos2(right_pad, rect.bottom())))
        .galley(pos2(text_left, sub_y), sub_galley, spec.muted_fg);

    resp.clicked()
}

fn paint_shortcut_keycaps(
    ui: &egui::Ui,
    right_x: f32,
    center_y: f32,
    sc: Shortcut,
    spec: &ThemeSpec,
    hover_t: f32,
) -> f32 {
    let (parts, len) = sc.keycaps();
    let op = 0.85 + 0.15 * hover_t;
    let fg = spec.muted_fg.gamma_multiply(op);
    let plus_col = spec.muted_fg.gamma_multiply(0.65 * op);
    let cap_font = FontId::new(10.0, FontFamily::Monospace);
    let plus_font = FontId::proportional(10.0);
    let pad = vec2(4.5, 1.5);
    let gap = 2.5;

    let mut caps = Vec::with_capacity(len);
    let mut total_w = 0.0;
    let plus_galley = ui.fonts_mut(|f| f.layout_no_wrap("+".into(), plus_font, plus_col));
    let plus_w = plus_galley.size().x;

    for (i, &part) in parts[..len].iter().enumerate() {
        if i > 0 {
            total_w += plus_w + gap * 2.0;
        }
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(part.to_string(), cap_font.clone(), fg));
        let cap_w = (galley.size().x + pad.x * 2.0).max(17.0);
        let cap_h = (galley.size().y + pad.y * 2.0).max(16.0);
        total_w += cap_w;
        caps.push((galley, cap_w, cap_h));
    }

    let mut x = right_x - total_w;
    for (i, (galley, cap_w, cap_h)) in caps.into_iter().enumerate() {
        if i > 0 {
            x += gap;
            ui.painter().galley(
                pos2(x, center_y - plus_galley.size().y * 0.5),
                plus_galley.clone(),
                plus_col,
            );
            x += plus_w + gap;
        }
        let cap_r = Rect::from_min_size(pos2(x, center_y - cap_h * 0.5), vec2(cap_w, cap_h));
        ui.painter().rect_filled(cap_r, 4.0, spec.dialog.keycap_bg);
        ui.painter()
            .rect_stroke(cap_r, 4.0, Stroke::new(1.0, spec.dialog.keycap_border), StrokeKind::Inside);
        ui.painter().hline(
            (cap_r.left() + 2.5)..=(cap_r.right() - 2.5),
            cap_r.bottom() - 0.6,
            Stroke::new(1.2, spec.dialog.keycap_bottom_border),
        );
        if let Some(hi) = spec.dialog.glass_highlight {
            ui.painter().hline(
                (cap_r.left() + 2.5)..=(cap_r.right() - 2.5),
                cap_r.top() + 0.6,
                Stroke::new(1.0, hi),
            );
        }
        ui.painter().galley(
            pos2(cap_r.center().x - galley.size().x * 0.5, cap_r.center().y - galley.size().y * 0.5 - 0.3),
            galley,
            fg,
        );
        x += cap_w;
    }

    total_w
}

fn show_recent_card(
    ui: &mut egui::Ui,
    max_w: f32,
    recent_files: &[RecentFile],
    now_unix: u64,
    spec: &ThemeSpec,
    pal: &WelcomePalette,
) -> Option<WelcomeAction> {
    let mut action: Option<WelcomeAction> = None;
    let visible = &recent_files[..recent_files.len().min(MAX_RECENT_DISPLAY)];
    let has_unpinned = recent_files.iter().any(|r| !r.pinned);

    let header_h = 34.0;
    let row_h = 38.0;
    let body_h = if visible.is_empty() {
        row_h * 2.0 + 30.0
    } else {
        (visible.len() as f32) * row_h
    };
    let card_h = header_h + body_h;
    let (card_rect, _) = ui.allocate_exact_size(vec2(max_w, card_h), Sense::hover());

    ui.painter().add(pal.card_shadow.as_shape(card_rect, 10.0));
    ui.painter().rect_filled(card_rect, 10.0, pal.card_bg);

    // 头部栏
    let hdr_r = Rect::from_min_size(card_rect.min, vec2(max_w, header_h));
    let hdr_left = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            "最近打开的文稿".into(),
            FontId::new(12.0, bold_family()),
            spec.muted_fg,
        )
    });
    ui.painter().galley(
        pos2(hdr_r.left() + 14.0, hdr_r.center().y - hdr_left.size().y * 0.5),
        hdr_left,
        spec.muted_fg,
    );

    if visible.is_empty() {
        let rt = ui.fonts_mut(|f| f.layout_no_wrap("快速上手".into(), FontId::proportional(11.5), spec.muted_fg));
        ui.painter().galley(
            pos2(hdr_r.right() - 14.0 - rt.size().x, hdr_r.center().y - rt.size().y * 0.5),
            rt,
            spec.muted_fg,
        );
    } else if has_unpinned {
        let clear_galley = ui.fonts_mut(|f| f.layout_no_wrap("清空记录".into(), FontId::proportional(11.5), spec.muted_fg));
        let clear_w = clear_galley.size().x + 8.0;
        let clear_r = Rect::from_min_size(
            pos2(hdr_r.right() - 10.0 - clear_w, hdr_r.center().y - 10.0),
            vec2(clear_w, 20.0),
        );
        let clear_resp = ui.interact(clear_r, ui.make_persistent_id("welcome_clear_recent"), Sense::click());
        if clear_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            ui.painter().rect_filled(clear_r, 4.0, pal.row_hover_bg);
        }
        let clear_fg = if clear_resp.hovered() { spec.accent } else { spec.muted_fg };
        ui.painter().galley(
            pos2(
                clear_r.center().x - clear_galley.size().x * 0.5,
                clear_r.center().y - clear_galley.size().y * 0.5,
            ),
            clear_galley,
            clear_fg,
        );
        if clear_resp.on_hover_text("清除所有未置顶的最近记录").clicked() {
            action = Some(WelcomeAction::ClearUnpinnedRecents);
        }

        let prefix_g = ui.fonts_mut(|f| {
            f.layout_no_wrap("点击即开 ·".into(), FontId::proportional(11.5), spec.muted_fg)
        });
        ui.painter().galley(
            pos2(clear_r.left() - 2.0 - prefix_g.size().x, hdr_r.center().y - prefix_g.size().y * 0.5),
            prefix_g,
            spec.muted_fg,
        );
    } else {
        let rt = ui.fonts_mut(|f| f.layout_no_wrap("已置顶常驻".into(), FontId::proportional(11.5), spec.muted_fg));
        ui.painter().galley(
            pos2(hdr_r.right() - 14.0 - rt.size().x, hdr_r.center().y - rt.size().y * 0.5),
            rt,
            spec.muted_fg,
        );
    }

    ui.painter()
        .hline((card_rect.left() + 1.0)..=(card_rect.right() - 1.0), hdr_r.bottom(), Stroke::new(1.0, spec.border));

    // 主体行
    let div_col = spec.border.gamma_multiply(0.6);
    if visible.is_empty() {
        let r1 = Rect::from_min_size(pos2(card_rect.left(), hdr_r.bottom()), vec2(max_w, row_h));
        if paint_onboarding_row(
            ui,
            r1,
            "welcome_sample_doc",
            true,
            "体验示例 Markdown 文稿",
            "一键生成",
            "含任务列表、代码块与告警块 →",
            spec,
            pal,
        ) {
            action = Some(WelcomeAction::CreateSampleDoc);
        }
        ui.painter().hline((card_rect.left() + 14.0)..=(card_rect.right() - 14.0), r1.bottom(), Stroke::new(1.0, div_col));

        let r2 = Rect::from_min_size(pos2(card_rect.left(), r1.bottom()), vec2(max_w, row_h));
        if paint_onboarding_row(
            ui,
            r2,
            "welcome_import_html",
            false,
            "导入本地 HTML 网页转为 Markdown",
            "格式转换",
            "选择文件 →",
            spec,
            pal,
        ) {
            action = Some(WelcomeAction::RunCommand(Command::ImportHtml));
        }
        ui.painter().hline((card_rect.left() + 14.0)..=(card_rect.right() - 14.0), r2.bottom(), Stroke::new(1.0, div_col));

        let r3 = Rect::from_min_size(pos2(card_rect.left(), r2.bottom()), vec2(max_w, 30.0));
        let hint_g = ui.fonts_mut(|f| {
            f.layout_no_wrap(
                "打开或保存过的本地文档会自动显示在这里，并支持置顶常驻".into(),
                FontId::proportional(11.5),
                spec.muted_fg,
            )
        });
        ui.painter()
            .with_clip_rect(r3.shrink2(vec2(14.0, 0.0)))
            .galley(pos2(r3.left() + 14.0, r3.center().y - hint_g.size().y * 0.5), hint_g, spec.muted_fg);
    } else {
        for (i, item) in visible.iter().enumerate() {
            let row_top = hdr_r.bottom() + (i as f32) * row_h;
            let row_rect = Rect::from_min_size(pos2(card_rect.left(), row_top), vec2(max_w, row_h));
            if i > 0 {
                ui.painter().hline(
                    (card_rect.left() + 14.0)..=(card_rect.right() - 14.0),
                    row_top,
                    Stroke::new(1.0, div_col),
                );
            }
            if let Some(act) = paint_recent_row(ui, row_rect, i, visible.len(), item, now_unix, spec, pal) {
                action = Some(act);
            }
        }
    }

    ui.painter()
        .rect_stroke(card_rect, 10.0, Stroke::new(1.0, spec.border), StrokeKind::Inside);
    if let Some(hi) = pal.glass_highlight {
        ui.painter().hline(
            (card_rect.left() + 8.0)..=(card_rect.right() - 8.0),
            card_rect.top() + 0.6,
            Stroke::new(1.0, hi),
        );
    }

    action
}

#[allow(clippy::too_many_arguments)]
fn paint_onboarding_row(
    ui: &mut egui::Ui,
    row_rect: Rect,
    id_salt: &str,
    is_bolt: bool,
    title: &str,
    badge: &str,
    right_hint: &str,
    spec: &ThemeSpec,
    pal: &WelcomePalette,
) -> bool {
    let inner = row_rect.shrink2(vec2(4.0, 2.0));
    let resp = ui.interact(inner, ui.make_persistent_id(id_salt), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(inner, 6.0, pal.row_hover_bg);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let icon_cx = row_rect.left() + 21.0;
    let cy = row_rect.center().y;
    if is_bolt {
        let br = Rect::from_center_size(pos2(icon_cx, cy), vec2(11.0, 15.0));
        let bg_under = if resp.hovered() { pal.row_hover_bg } else { pal.card_bg };
        theme::paint_bolt_on(ui.painter(), br, theme::BOLT_YELLOW, bg_under);
    } else {
        paint_import_icon(ui.painter(), pos2(icon_cx, cy), if resp.hovered() { spec.accent } else { spec.muted_fg });
    }

    let hint_g = ui.fonts_mut(|f| f.layout_no_wrap(right_hint.into(), FontId::proportional(11.5), spec.muted_fg));
    let hint_x = row_rect.right() - 14.0 - hint_g.size().x;
    let title_x = row_rect.left() + 36.0;
    let title_col = if resp.hovered() { spec.strong_fg } else { spec.text_fg };
    let title_g = ui.fonts_mut(|f| f.layout_no_wrap(title.into(), FontId::new(13.0, bold_family()), title_col));
    let badge_g = ui.fonts_mut(|f| f.layout_no_wrap(badge.into(), FontId::proportional(11.0), spec.muted_fg));
    let badge_w = badge_g.size().x + 12.0;

    let clip_right = (hint_x - 10.0).max(title_x + 40.0);
    ui.painter()
        .with_clip_rect(Rect::from_min_max(pos2(title_x, row_rect.top()), pos2(clip_right, row_rect.bottom())))
        .galley(pos2(title_x, cy - title_g.size().y * 0.5), title_g.clone(), title_col);

    let badge_x = title_x + title_g.size().x + 8.0;
    if badge_x + badge_w <= clip_right {
        let br = Rect::from_min_size(pos2(badge_x, cy - 9.5), vec2(badge_w, 19.0));
        ui.painter().rect_filled(br, 4.0, pal.well_bg);
        ui.painter().galley(
            pos2(br.center().x - badge_g.size().x * 0.5, br.center().y - badge_g.size().y * 0.5),
            badge_g,
            spec.muted_fg,
        );
    }

    if hint_x > title_x + 80.0 {
        ui.painter()
            .galley(pos2(hint_x, cy - hint_g.size().y * 0.5), hint_g, spec.muted_fg);
    }

    resp.clicked()
}

#[allow(clippy::too_many_arguments)]
fn paint_recent_row(
    ui: &mut egui::Ui,
    row_rect: Rect,
    idx: usize,
    total_rows: usize,
    item: &RecentFile,
    now_unix: u64,
    spec: &ThemeSpec,
    pal: &WelcomePalette,
) -> Option<WelcomeAction> {
    let mut action: Option<WelcomeAction> = None;
    let is_last = idx + 1 == total_rows;
    let hover_rect = row_rect.shrink2(vec2(3.0, 2.0));
    let hover_radius = CornerRadius {
        nw: 6,
        ne: 6,
        sw: if is_last { 8 } else { 6 },
        se: if is_last { 8 } else { 6 },
    };

    let row_id = ui.make_persistent_id(("welcome_recent_row", idx, &item.path));
    let row_resp = ui.interact(hover_rect, row_id, Sense::click());
    let pointer_in_row = ui.rect_contains_pointer(hover_rect);
    let cy = row_rect.center().y;

    let mut sub_hovered = false;
    let mut pin_clicked = false;
    let mut remove_clicked = false;
    let mut buttons_left_x = row_rect.right() - 14.0;

    if pointer_in_row {
        ui.painter().rect_filled(hover_rect, hover_radius, pal.row_hover_bg);

        let remove_r = Rect::from_min_size(pos2(row_rect.right() - 32.0, cy - 11.0), vec2(22.0, 22.0));
        let pin_r = Rect::from_min_size(pos2(remove_r.left() - 26.0, cy - 11.0), vec2(22.0, 22.0));
        buttons_left_x = pin_r.left() - 8.0;

        let pin_resp = ui.interact(pin_r, row_id.with("pin"), Sense::click());
        let remove_resp = ui.interact(remove_r, row_id.with("remove"), Sense::click());
        sub_hovered = pin_resp.hovered() || remove_resp.hovered();

        if pin_resp.hovered() {
            ui.painter().rect_filled(pin_r, 5.0, spec.hover_bg);
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let pin_col = if item.pinned {
            spec.accent
        } else if pin_resp.hovered() {
            spec.text_fg
        } else {
            spec.muted_fg
        };
        paint_pin_icon(ui.painter(), pin_r.center(), 12.0, pin_col, item.pinned);
        let pin_tip = if item.pinned { "取消置顶" } else { "置顶此文稿" };
        if pin_resp.on_hover_text(pin_tip).clicked() {
            pin_clicked = true;
        }

        if remove_resp.hovered() {
            ui.painter().rect_filled(remove_r, 5.0, spec.hover_bg);
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let x_col = if remove_resp.hovered() { spec.text_fg } else { spec.muted_fg };
        theme::paint_close_x_icon(ui.painter(), remove_r.center(), 3.8, 1.4, x_col);
        if remove_resp.on_hover_text("从最近列表中移除").clicked() {
            remove_clicked = true;
        }
    }

    if row_resp.hovered() && !sub_hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    // 左侧图标（置顶图钉 / 文档页）
    let icon_center = pos2(row_rect.left() + 21.0, cy);
    if item.pinned {
        paint_pin_icon(ui.painter(), icon_center, 13.0, spec.accent, true);
    } else {
        paint_doc_icon(ui.painter(), icon_center, 13.0, spec.muted_fg);
    }

    // 相对时间
    let rel_time = format_relative_time(item.last_opened_unix, now_unix);
    let time_g = ui.fonts_mut(|f| f.layout_no_wrap(rel_time, FontId::proportional(11.5), spec.muted_fg));
    let time_x = buttons_left_x - time_g.size().x;
    let text_left = row_rect.left() + 36.0;
    let name_clip_right = (time_x - 10.0).max(text_left + 32.0);
    if time_x > text_left + 44.0 && !time_g.text().is_empty() {
        ui.painter()
            .galley(pos2(time_x, cy - time_g.size().y * 0.5), time_g, spec.muted_fg);
    }

    // 文件名与父目录胶囊
    let file_name = item
        .path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| item.path.display().to_string());
    let name_font = if item.pinned {
        FontId::new(13.0, bold_family())
    } else {
        FontId::proportional(13.0)
    };
    let name_col = if item.pinned || pointer_in_row { spec.strong_fg } else { spec.text_fg };
    let name_g = ui.fonts_mut(|f| f.layout_no_wrap(file_name, name_font, name_col));
    ui.painter()
        .with_clip_rect(Rect::from_min_max(pos2(text_left, row_rect.top()), pos2(name_clip_right, row_rect.bottom())))
        .galley(pos2(text_left, cy - name_g.size().y * 0.5), name_g.clone(), name_col);

    if let Some(parent_name) = item.path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned())
        && !parent_name.is_empty()
    {
        let tag_g = ui.fonts_mut(|f| f.layout_no_wrap(parent_name, FontId::proportional(11.0), spec.muted_fg));
        let tag_w = tag_g.size().x + 12.0;
        let tag_x = text_left + name_g.size().x + 8.0;
        if tag_x + tag_w <= name_clip_right {
            let tag_r = Rect::from_min_size(pos2(tag_x, cy - 9.5), vec2(tag_w, 19.0));
            ui.painter().rect_filled(tag_r, 4.0, pal.well_bg);
            ui.painter().galley(
                pos2(tag_r.center().x - tag_g.size().x * 0.5, tag_r.center().y - tag_g.size().y * 0.5),
                tag_g,
                spec.muted_fg,
            );
        }
    }

    row_resp.context_menu(|ui| {
        if ui.button("打开").clicked() {
            action = Some(WelcomeAction::OpenRecent(item.path.clone()));
            ui.close();
        }
        let pin_label = if item.pinned { "取消置顶" } else { "置顶到顶部" };
        if ui.button(pin_label).clicked() {
            action = Some(WelcomeAction::TogglePin(item.path.clone()));
            ui.close();
        }
        ui.separator();
        if ui.button("复制文件路径").clicked() {
            action = Some(WelcomeAction::CopyPath(item.path.clone()));
            ui.close();
        }
        if ui.button("在资源管理器中显示").clicked() {
            action = Some(WelcomeAction::RevealInExplorer(item.path.clone()));
            ui.close();
        }
        ui.separator();
        if ui.button("从最近列表中移除").clicked() {
            action = Some(WelcomeAction::RemoveRecent(item.path.clone()));
            ui.close();
        }
    });

    let full_tip = item.path.display().to_string();
    let row_clicked = row_resp.on_hover_text(full_tip).clicked();

    if pin_clicked {
        Some(WelcomeAction::TogglePin(item.path.clone()))
    } else if remove_clicked {
        Some(WelcomeAction::RemoveRecent(item.path.clone()))
    } else if row_clicked && action.is_none() {
        Some(WelcomeAction::OpenRecent(item.path.clone()))
    } else {
        action
    }
}

fn show_drop_hint(
    ui: &mut egui::Ui,
    max_w: f32,
    spec: &ThemeSpec,
    pal: &WelcomePalette,
    keymap: &Keymap,
    files_hovered: bool,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(max_w, 40.0), Sense::hover());
    if files_hovered {
        ui.painter().rect_filled(rect, 10.0, spec.active_bg);
    } else {
        ui.painter()
            .rect_filled(rect, 10.0, lerp_color(spec.canvas_bg, pal.well_bg, 0.45));
    }
    let stroke = if files_hovered {
        Stroke::new(1.5, spec.accent)
    } else {
        Stroke::new(1.0, spec.border)
    };
    paint_dashed_rounded_rect(ui.painter(), rect, 10.0, stroke);

    let msg = if files_hovered {
        "松开鼠标即可打开或导入拖入的文件 / 文件夹".to_string()
    } else {
        let sc = keymap.label(Command::OpenShortcuts);
        if sc.is_empty() {
            "拖入 .md / .html / 文件夹 到窗口即可直接打开或导入".to_string()
        } else {
            format!("拖入 .md / .html / 文件夹 到窗口即可直接打开或导入 · 按 {sc} 自定义快捷键")
        }
    };
    let fg = if files_hovered { spec.accent } else { spec.muted_fg };
    let font = if files_hovered {
        FontId::new(12.5, bold_family())
    } else {
        FontId::proportional(12.0)
    };
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(msg, font, fg));
    ui.painter()
        .with_clip_rect(rect.shrink2(vec2(12.0, 0.0)))
        .galley(
            pos2(
                (rect.center().x - galley.size().x * 0.5).max(rect.left() + 12.0),
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            fg,
        );
}

// ============================================================================
// 矢量图标与虚线圆角矩形绘制
// ============================================================================

fn paint_tile_icon(painter: &egui::Painter, center: Pos2, icon: TileIcon, color: Color32) {
    let st = Stroke::new(1.45, color);
    match icon {
        TileIcon::NewDoc => {
            let r = Rect::from_center_size(center, vec2(13.0, 16.0));
            let fold = 4.0;
            let pts = vec![
                r.left_top(),
                pos2(r.right() - fold, r.top()),
                pos2(r.right(), r.top() + fold),
                r.right_bottom(),
                r.left_bottom(),
            ];
            painter.add(egui::epaint::PathShape::closed_line(pts, st));
            painter.line_segment([pos2(r.right() - fold, r.top()), pos2(r.right() - fold, r.top() + fold)], st);
            painter.line_segment([pos2(r.right() - fold, r.top() + fold), pos2(r.right(), r.top() + fold)], st);
            let pc = pos2(center.x, center.y + 1.5);
            painter.line_segment([pos2(pc.x - 2.8, pc.y), pos2(pc.x + 2.8, pc.y)], st);
            painter.line_segment([pos2(pc.x, pc.y - 2.8), pos2(pc.x, pc.y + 2.8)], st);
        }
        TileIcon::OpenFile => {
            let r = Rect::from_center_size(center, vec2(13.0, 16.0));
            let fold = 4.0;
            let pts = vec![
                r.left_top(),
                pos2(r.right() - fold, r.top()),
                pos2(r.right(), r.top() + fold),
                r.right_bottom(),
                r.left_bottom(),
            ];
            painter.add(egui::epaint::PathShape::closed_line(pts, st));
            painter.line_segment([pos2(r.right() - fold, r.top()), pos2(r.right() - fold, r.top() + fold)], st);
            painter.line_segment([pos2(r.right() - fold, r.top() + fold), pos2(r.right(), r.top() + fold)], st);
            painter.hline((r.left() + 3.0)..=(r.right() - 3.0), center.y + 0.5, st);
            painter.hline((r.left() + 3.0)..=(r.right() - 4.5), center.y + 4.0, st);
        }
        TileIcon::OpenFolder => {
            let r = Rect::from_center_size(center, vec2(16.0, 13.0));
            let pts = vec![
                pos2(r.left(), r.top() + 1.5),
                pos2(r.left() + 6.0, r.top() + 1.5),
                pos2(r.left() + 7.5, r.top() + 3.5),
                pos2(r.right(), r.top() + 3.5),
                r.right_bottom(),
                r.left_bottom(),
            ];
            painter.add(egui::epaint::PathShape::closed_line(pts, st));
        }
        TileIcon::Manual => {
            let r = Rect::from_center_size(center, vec2(16.0, 13.0));
            let mid = r.center().x;
            painter.line_segment([pos2(mid, r.top() + 1.5), pos2(mid, r.bottom() - 0.5)], st);
            let left_page = vec![
                pos2(mid, r.top() + 1.5),
                pos2(r.left() + 1.5, r.top() + 0.5),
                pos2(r.left() + 0.5, r.bottom() - 1.5),
                pos2(mid, r.bottom() - 0.5),
            ];
            let right_page = vec![
                pos2(mid, r.top() + 1.5),
                pos2(r.right() - 1.5, r.top() + 0.5),
                pos2(r.right() - 0.5, r.bottom() - 1.5),
                pos2(mid, r.bottom() - 0.5),
            ];
            painter.add(egui::epaint::PathShape::line(left_page, st));
            painter.add(egui::epaint::PathShape::line(right_page, st));
        }
    }
}

fn paint_doc_icon(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let st = Stroke::new(1.25, color);
    let r = Rect::from_center_size(center, vec2(size * 0.78, size));
    let fold = size * 0.24;
    let pts = vec![
        r.left_top(),
        pos2(r.right() - fold, r.top()),
        pos2(r.right(), r.top() + fold),
        r.right_bottom(),
        r.left_bottom(),
    ];
    painter.add(egui::epaint::PathShape::closed_line(pts, st));
    painter.hline((r.left() + 2.5)..=(r.right() - 2.5), center.y - 0.5, st);
    painter.hline((r.left() + 2.5)..=(r.right() - 3.5), center.y + 2.5, st);
}

fn paint_pin_icon(painter: &egui::Painter, center: Pos2, size: f32, color: Color32, filled: bool) {
    let s = size / 14.0;
    let pts = vec![
        pos2(center.x - 3.8 * s, center.y - 5.2 * s),
        pos2(center.x + 3.8 * s, center.y - 5.2 * s),
        pos2(center.x + 3.8 * s, center.y + 5.2 * s),
        pos2(center.x, center.y + 2.2 * s),
        pos2(center.x - 3.8 * s, center.y + 5.2 * s),
    ];
    if filled {
        let upper = vec![pts[0], pts[1], pts[2], pts[3], pts[4]];
        // 分拆为左右两个凸多边形以无锯齿填充书签燕尾形状
        let left_half = vec![pts[0], pos2(center.x, pts[0].y), pts[3], pts[4]];
        let right_half = vec![pos2(center.x, pts[0].y), pts[1], pts[2], pts[3]];
        let _ = upper;
        painter.add(egui::epaint::PathShape::convex_polygon(left_half, color, Stroke::NONE));
        painter.add(egui::epaint::PathShape::convex_polygon(right_half, color, Stroke::NONE));
    } else {
        painter.add(egui::epaint::PathShape::closed_line(pts, Stroke::new(1.25, color)));
    }
}

fn paint_import_icon(painter: &egui::Painter, center: Pos2, color: Color32) {
    let st = Stroke::new(1.3, color);
    let r = Rect::from_center_size(center, vec2(12.0, 12.0));
    painter.add(egui::epaint::PathShape::line(
        vec![
            pos2(r.left(), r.center().y + 1.0),
            r.left_bottom(),
            r.right_bottom(),
            pos2(r.right(), r.center().y + 1.0),
        ],
        st,
    ));
    painter.line_segment([pos2(center.x, r.top()), pos2(center.x, r.bottom() - 3.0)], st);
    painter.line_segment([pos2(center.x - 3.0, r.bottom() - 5.5), pos2(center.x, r.bottom() - 2.8)], st);
    painter.line_segment([pos2(center.x + 3.0, r.bottom() - 5.5), pos2(center.x, r.bottom() - 2.8)], st);
}

fn paint_dashed_rounded_rect(painter: &egui::Painter, rect: Rect, radius: f32, stroke: Stroke) {
    let r = radius.min(rect.width() * 0.5).min(rect.height() * 0.5);
    let draw_dashed = |p0: Pos2, p1: Pos2| {
        let len = p0.distance(p1);
        if len <= 1.0 {
            return;
        }
        let dir = (p1 - p0) / len;
        let dash = 5.0;
        let gap = 4.0;
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

    // 四角圆弧衔接
    let draw_arc = |center: Pos2, start_ang: f32, end_ang: f32| {
        let steps = 4;
        let mut pts = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let a = start_ang + (end_ang - start_ang) * (i as f32 / steps as f32);
            pts.push(pos2(center.x + a.cos() * r, center.y + a.sin() * r));
        }
        painter.add(egui::epaint::PathShape::line(pts, stroke));
    };
    use std::f32::consts::{FRAC_PI_2, PI};
    draw_arc(pos2(rect.left() + r, rect.top() + r), PI, PI + FRAC_PI_2);
    draw_arc(pos2(rect.right() - r, rect.top() + r), -FRAC_PI_2, 0.0);
    draw_arc(pos2(rect.right() - r, rect.bottom() - r), 0.0, FRAC_PI_2);
    draw_arc(pos2(rect.left() + r, rect.bottom() - r), FRAC_PI_2, PI);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_formats_all_buckets() {
        let now = 1_700_000_000;
        assert_eq!(format_relative_time(0, now), "");
        assert_eq!(format_relative_time(now - 10, now), "刚刚");
        assert_eq!(format_relative_time(now - 12 * 60, now), "12 分钟前");
        assert_eq!(format_relative_time(now - 3 * 3_600, now), "3 小时前");
        assert_eq!(format_relative_time(now - 30 * 3_600, now), "昨天");
        assert_eq!(format_relative_time(now - 5 * 86_400, now), "5 天前");
        assert_eq!(format_relative_time(now - 65 * 86_400, now), "2 个月前");
        assert_eq!(format_relative_time(now - 400 * 86_400, now), "1 年前");
    }

    #[test]
    fn welcome_renders_and_tessellates_cleanly_across_all_themes_and_sizes() {
        let now = Settings::now_unix_secs();
        let recents = [
            RecentFile {
                path: PathBuf::from(r"C:\docs\Tech-Notes\ThunderMD 渲染管线优化.md"),
                pinned: true,
                last_opened_unix: now.saturating_sub(720),
            },
            RecentFile {
                path: PathBuf::from(r"C:\docs\Reading\2026 年度阅读书单.md"),
                pinned: false,
                last_opened_unix: now.saturating_sub(90_000),
            },
        ];
        let keymap = Keymap::default();

        for id in ThemeId::ALL {
            for size in [[1200.0_f32, 800.0], [480.0, 320.0]] {
                for list in [&[][..], &recents[..]] {
                    let ctx = egui::Context::default();
                    crate::fonts::setup(&ctx);
                    theme::apply(&ctx, id);
                    let raw = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]))),
                        ..Default::default()
                    };
                    let mut out = ctx.run_ui(raw, |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            let _ = show(ui, list, &keymap, false);
                        });
                    });
                    out.textures_delta.clear();
                    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                    assert!(
                        !primitives.is_empty(),
                        "{id:?} {size:?} produced zero primitives"
                    );
                }
            }
        }
    }

    fn collect_rect_shapes(shape: &egui::epaint::Shape, out: &mut Vec<Rect>) {
        match shape {
            egui::epaint::Shape::Rect(r) => out.push(r.rect),
            egui::epaint::Shape::Vec(v) => {
                for s in v {
                    collect_rect_shapes(s, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn welcome_stays_horizontally_centered_on_wide_viewports() {
        let keymap = Keymap::default();
        for win_w in [1200.0_f32, 1920.0, 2560.0] {
            let win_h = 900.0;
            let ctx = egui::Context::default();
            crate::fonts::setup(&ctx);
            theme::apply(&ctx, ThemeId::ZenHorizon);
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(win_w, win_h))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
                    let _ = show(ui, &[], &keymap, false);
                });
            });
            out.textures_delta.clear();

            let mut rects = Vec::new();
            for cs in &out.shapes {
                collect_rect_shapes(&cs.shape, &mut rects);
            }
            let card_rects: Vec<Rect> = rects
                .into_iter()
                .filter(|r| (r.width() - 600.0).abs() < 1.0 && r.height() > 35.0)
                .collect();
            assert!(
                !card_rects.is_empty(),
                "expected 600pt-wide welcome card rects at viewport width {win_w}"
            );
            for r in card_rects {
                assert!(
                    (r.center().x - win_w * 0.5).abs() <= 2.0,
                    "welcome card off-center at win_w={win_w}: rect={r:?}"
                );
            }
        }
    }
}
