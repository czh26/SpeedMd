//! MdPad「速度与闪电」主题内核：统一空间骨架常量、5 套定稿主题色板与 egui Visuals 缓存。

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Mesh, Painter, Pos2, Rect, Shadow, Spacing, Stroke, Style, Visuals,
    style::{HandleShape, ScrollStyle, Selection, TextCursorStyle, WidgetVisuals, Widgets},
};
use egui_extras::syntax_highlighting::{CodeTheme, TokenType};
use pulldown_cmark::BlockQuoteKind;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::sync::LazyLock;

// ============================================================================
// 统一空间骨架尺寸常量（5 套主题严格共用，确保热切换 0.0px 零跳动）
// ============================================================================

pub const TOP_BAR_H: f32 = 42.0;
pub const STATUS_BAR_H: f32 = 28.0;
pub const SIDEBAR_DEFAULT_W: f32 = 220.0;
pub const SIDEBAR_MIN_W: f32 = 170.0;
pub const SIDEBAR_MAX_W: f32 = 500.0;
pub const SIDEBAR_HEADER_H: f32 = 34.0;
/// 中央工作区外框内缩量：05 下用于呈现暗钛壳内嵌磨砂玻璃板；01–04 下与画布同色无缝隐形，
/// 从而保证 5 套主题的编辑器/预览区折行宽度与文字坐标 1:1 绝对一致。
pub const PANE_INSET: f32 = 6.0;
pub const TAB_H: f32 = 28.0;
pub const TOP_PILL_H: f32 = 28.0;
pub const STATUS_PILL_H: f32 = 22.0;

// ============================================================================
// const 颜色推导工具（用于派生 hover / active / selection / disabled / 滚动条）
// ============================================================================

/// 线性插值混合：`t = num / den`（`0` 返回 `a`，`den` 返回 `b`）。
const fn mix(a: Color32, b: Color32, num: u32, den: u32) -> Color32 {
    let inv = den - num;
    let r = ((a.r() as u32 * inv + b.r() as u32 * num + den / 2) / den) as u8;
    let g = ((a.g() as u32 * inv + b.g() as u32 * num + den / 2) / den) as u8;
    let bl = ((a.b() as u32 * inv + b.b() as u32 * num + den / 2) / den) as u8;
    Color32::from_rgb(r, g, bl)
}

const fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(c.r(), c.g(), c.b(), a)
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb(((rgb >> 16) & 0xff) as u8, ((rgb >> 8) & 0xff) as u8, (rgb & 0xff) as u8)
}

const fn stroke(width: f32, color: Color32) -> Stroke {
    Stroke { width, color }
}

// ============================================================================
// 主题标识与容错反序列化
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeId {
    /// 01 · 无界疾风 (Zen Horizon) —— 默认主题
    #[default]
    ZenHorizon,
    /// 02 · 白昼疾影 (Scarlet Streak)
    ScarletStreak,
    /// 03 · 半山书房 (Atelier Contrast)
    AtelierContrast,
    /// 04 · 黑金疾电 (Carbon & Gold)
    CarbonGold,
    /// 05 · 钛晶流光 (Titanium Glass)
    TitaniumGlass,
}

impl ThemeId {
    pub const ALL: [ThemeId; 5] = [
        Self::ZenHorizon,
        Self::ScarletStreak,
        Self::AtelierContrast,
        Self::CarbonGold,
        Self::TitaniumGlass,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::ZenHorizon => 0,
            Self::ScarletStreak => 1,
            Self::AtelierContrast => 2,
            Self::CarbonGold => 3,
            Self::TitaniumGlass => 4,
        }
    }

    pub const fn cn_name(self) -> &'static str {
        match self {
            Self::ZenHorizon => "无界疾风",
            Self::ScarletStreak => "白昼疾影",
            Self::AtelierContrast => "半山书房",
            Self::CarbonGold => "黑金疾电",
            Self::TitaniumGlass => "钛晶流光",
        }
    }

    pub const fn en_name(self) -> &'static str {
        match self {
            Self::ZenHorizon => "Zen Horizon",
            Self::ScarletStreak => "Scarlet Streak",
            Self::AtelierContrast => "Atelier Contrast",
            Self::CarbonGold => "Carbon & Gold",
            Self::TitaniumGlass => "Titanium Glass",
        }
    }

    pub const fn mode_tag(self) -> &'static str {
        match self {
            Self::ZenHorizon | Self::ScarletStreak => "浅色",
            Self::AtelierContrast => "撞色",
            Self::CarbonGold | Self::TitaniumGlass => "深色",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::ZenHorizon => "01 · 无界疾风",
            Self::ScarletStreak => "02 · 白昼疾影",
            Self::AtelierContrast => "03 · 半山书房",
            Self::CarbonGold => "04 · 黑金疾电",
            Self::TitaniumGlass => "05 · 钛晶流光",
        }
    }

    pub const fn full_label(self) -> &'static str {
        match self {
            Self::ZenHorizon => "01 · 无界疾风 (Zen Horizon)",
            Self::ScarletStreak => "02 · 白昼疾影 (Scarlet Streak)",
            Self::AtelierContrast => "03 · 半山书房 (Atelier Contrast)",
            Self::CarbonGold => "04 · 黑金疾电 (Carbon & Gold)",
            Self::TitaniumGlass => "05 · 钛晶流光 (Titanium Glass)",
        }
    }

    pub const fn is_dark(self) -> bool {
        matches!(self, Self::CarbonGold | Self::TitaniumGlass)
    }

    pub fn from_number(n: u64) -> Option<Self> {
        match n {
            1 => Some(Self::ZenHorizon),
            2 => Some(Self::ScarletStreak),
            3 => Some(Self::AtelierContrast),
            4 => Some(Self::CarbonGold),
            5 => Some(Self::TitaniumGlass),
            _ => None,
        }
    }

    /// 宽松解析主题名或编号（用于环境变量 `MDPAD_THEME` 与配置反序列化）。
    pub fn from_env_str(raw: &str) -> Option<Self> {
        let norm: String = raw
            .trim()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match norm.as_str() {
            "1" | "01" | "zen" | "zenhorizon" => Some(Self::ZenHorizon),
            "2" | "02" | "scarlet" | "scarletstreak" | "flash" => Some(Self::ScarletStreak),
            "3" | "03" | "atelier" | "ateliercontrast" | "contrast" => Some(Self::AtelierContrast),
            "4" | "04" | "carbon" | "carbongold" | "gold" => Some(Self::CarbonGold),
            "5" | "05" | "titanium" | "titaniumglass" | "glass" => Some(Self::TitaniumGlass),
            _ => None,
        }
    }

    pub fn spec(self) -> &'static ThemeSpec {
        &SPECS[self.index()]
    }
}

struct ThemeIdVisitor {
    fallback: ThemeId,
}

impl<'de> serde::de::Visitor<'de> for ThemeIdVisitor {
    type Value = ThemeId;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("theme identifier or index (1..=5)")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(ThemeId::from_env_str(v).unwrap_or(self.fallback))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(ThemeId::from_env_str(&v).unwrap_or(self.fallback))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(ThemeId::from_number(v).unwrap_or(self.fallback))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(u64::try_from(v).ok().and_then(ThemeId::from_number).unwrap_or(self.fallback))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(self.fallback)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(self.fallback)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Deserialize<'de> for ThemeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(deserializer
            .deserialize_any(ThemeIdVisitor { fallback: ThemeId::ZenHorizon })
            .unwrap_or(ThemeId::ZenHorizon))
    }
}

/// 供 `Settings::dark_theme` 使用的反序列化函数：字段异常时回落到 `04 黑金疾电`。
pub fn deserialize_dark_theme_fallback<'de, D>(deserializer: D) -> Result<ThemeId, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(deserializer
        .deserialize_any(ThemeIdVisitor { fallback: ThemeId::CarbonGold })
        .unwrap_or(ThemeId::CarbonGold))
}

// ============================================================================
// 形态策略枚举与子色板结构体
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeStyle {
    /// 极简几何双三角闪电（01 / 03 / 04 / 05）
    GeometricBolt,
    /// 绯红圆盾 + 金色外环 + 斜角金闪电（02 白昼疾影）
    FlashShield,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TopBarBottomLine {
    /// 隐藏底边分割线（01 无界疾风）
    None,
    /// 1px 实线（03 右侧 / 04 / 05；03 左侧同步使用侧栏内分割线）
    Solid { color: Color32, left_override: Option<Color32> },
    /// 1.5px 水平渐变速度线（02：#DC2626 -> #FACC15）
    Gradient { left: Color32, right: Color32, width: f32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActiveTabStyle {
    /// 01：纯白柔影悬浮胶囊
    SoftShadowPill,
    /// 02：绯红实色白字胶囊 + 2px 金色闪电底线
    CrimsonGoldBolt,
    /// 03：向下连通米白书页 + 顶边 2px 陶土铜标尺
    ConnectedPageTopBar,
    /// 04：深炭按钮面 + 底边 2px 香槟金速度线
    CarbonGoldUnderline,
    /// 05：烟熏玻璃胶囊 + 1px 珍珠银高光内描边
    SmokedGlassRim,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ModeSwitchStyle {
    pub container_bg: Color32,
    pub container_stroke: Stroke,
    pub container_shadow: Shadow,
    pub active_bg: Color32,
    pub active_fg: Color32,
    pub active_stroke: Stroke,
    pub inactive_fg: Color32,
    pub outer_radius: u8,
    pub inner_radius: u8,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WindowControlStyle {
    pub corner_radius: u8,
    pub button_width: f32,
    pub button_height: f32,
    pub button_gap: f32,
    pub section_gap: f32,
    pub right_margin: f32,
    pub icon_stroke_width: f32,
    pub bg_idle: Color32,
    pub stroke_idle: Stroke,
    pub icon_idle_fg: Color32,
    pub icon_unfocused_fg: Color32,
    /// 03 左右分色适配：万一布局落在左侧深色栏时的兼容前景色与悬停底色
    pub on_left_split_idle_fg: Color32,
    pub on_left_split_unfocused_fg: Color32,
    pub on_left_split_hover_bg: Color32,
    pub bg_hover: Color32,
    pub stroke_hover: Stroke,
    pub icon_hover_fg: Color32,
    pub hover_shadow: Shadow,
    pub top_highlight_hover: Option<Color32>,
    pub bg_active: Color32,
    pub stroke_active: Stroke,
    pub icon_active_fg: Color32,
    pub close_bg_hover: Color32,
    pub close_stroke_hover: Stroke,
    pub close_icon_hover: Color32,
    pub close_bg_active: Color32,
    pub close_icon_active: Color32,
    /// Win10 无 DWM 圆角时的 1px 降级窗口外边框色
    pub window_border: Color32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DividerStyle {
    /// 01：2px 点状虚线
    Dotted { color: Color32, step: f32 },
    /// 02 / 03 / 04：1px 细实线
    Solid(Color32),
    /// 05：暗钛底壳内嵌 10px 圆角磨砂玻璃板 + 上缘 1px 珍珠银高光
    GlassInset {
        chassis_bg: Color32,
        plate_bg: Color32,
        rim_stroke: Color32,
        top_highlight: Color32,
        radius: f32,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StatusBarStyle {
    /// 01 / 02 / 03 / 05：右侧悬浮微胶囊
    FloatingPill {
        bar_bg: Color32,
        top_border: Option<Color32>,
        pill_bg: Color32,
        pill_fg: Color32,
        bolt_color: Color32,
        pill_stroke: Stroke,
        pill_shadow: Shadow,
    },
    /// 04：贴底收口鎏金遥测条（不加悬浮气泡框）
    FlushTelemetry {
        bar_bg: Color32,
        top_border: Color32,
        bolt_color: Color32,
        text_color: Color32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct SidebarColors {
    pub bg: Color32,
    /// 面板内分割线（如文件/文章/大纲底线）
    pub border: Color32,
    /// 侧栏右缘与中央工作区之间的外框分割线（01 为透明）
    pub outer_border: Color32,
    pub fg: Color32,
    pub strong_fg: Color32,
    pub muted_fg: Color32,
    pub surface_bg: Color32,
    pub input_bg: Color32,
    pub hover_bg: Color32,
    pub active_bg: Color32,
    /// 激活条目左侧 2px 竖条色
    pub active_bar: Color32,
    /// 顶部「文件 | 文章 | 大纲」激活下划线色（02 为金色，其余为主强调色）
    pub tab_underline: Color32,
    pub selection_bg: Color32,
    pub selection_fg: Color32,
}

#[derive(Clone, Copy, Debug)]
pub struct TabColors {
    pub active_bg: Color32,
    pub idle_bg: Color32,
    pub hover_bg: Color32,
    pub active_fg: Color32,
    pub idle_fg: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub secondary_accent: Color32,
    pub shadow: Shadow,
    pub dirty_active: Color32,
    pub dirty_idle: Color32,
    pub close_hover_bg: Color32,
}

#[derive(Clone, Copy, Debug)]
pub struct EditorPalette {
    pub normal: Color32,
    pub heading: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub quote: Color32,
    pub marker: Color32,
    pub link: Color32,
    pub bold: Color32,
    pub rule: Color32,
    pub mark_bg: Color32,
    pub find_match_bg: Color32,
    pub find_cur_bg: Color32,
    pub trailing_ws: Color32,
    pub current_line_bg: Color32,
}

#[derive(Clone, Copy, Debug)]
pub struct AlertColors {
    pub note: Color32,
    pub tip: Color32,
    pub important: Color32,
    pub warning: Color32,
    pub caution: Color32,
}

impl AlertColors {
    pub const fn color_of(&self, kind: BlockQuoteKind) -> Color32 {
        match kind {
            BlockQuoteKind::Note => self.note,
            BlockQuoteKind::Tip => self.tip,
            BlockQuoteKind::Important => self.important,
            BlockQuoteKind::Warning => self.warning,
            BlockQuoteKind::Caution => self.caution,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewPalette {
    pub text: Color32,
    pub weak: Color32,
    pub heading: Color32,
    pub link: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub code_border: Color32,
    pub callout_bg: Color32,
    pub mark_bg: Color32,
    pub mark_fg: Color32,
    pub quote: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub panel_bg: Color32,
    pub table_head_bg: Color32,
    pub table_stripe_bg: Color32,
    pub alerts: AlertColors,
}

#[derive(Clone, Copy, Debug)]
pub struct CodePalette {
    pub is_dark: bool,
    pub comment: Color32,
    pub function: Color32,
    pub keyword: Color32,
    pub literal: Color32,
    pub string_literal: Color32,
    pub punctuation: Color32,
}

impl CodePalette {
    pub fn to_code_theme(self, font_size: f32) -> CodeTheme {
        let mut ct = if self.is_dark {
            CodeTheme::dark(font_size)
        } else {
            CodeTheme::light(font_size)
        };
        ct.format_mut(TokenType::Comment).color = self.comment;
        ct.format_mut(TokenType::Function).color = self.function;
        ct.format_mut(TokenType::Keyword).color = self.keyword;
        ct.format_mut(TokenType::Literal).color = self.literal;
        ct.format_mut(TokenType::StringLiteral).color = self.string_literal;
        ct.format_mut(TokenType::Punctuation).color = self.punctuation;
        ct
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ManualPalette {
    pub bar_bg: Color32,
    pub toc_bg: Color32,
    pub toc_active_bg: Color32,
    pub toc_active_stroke: Color32,
    pub toc_active_fg: Color32,
    pub toc_hover_bg: Color32,
    pub card_bg: Color32,
    pub src_bg: Color32,
    pub prev_bg: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub tag_setting: Color32,
    pub tag_ext: Color32,
    pub tag_html: Color32,
}

#[derive(Clone, Copy, Debug)]
pub struct SemanticColors {
    pub success: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub dirty_dot: Color32,
}

#[derive(Clone, Copy, Debug)]
pub struct ExportCssPalette {
    pub color_scheme: &'static str,
    pub bg: Color32,
    pub fg: Color32,
    pub heading: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub panel: Color32,
    pub code_bg: Color32,
    pub link: Color32,
    pub mark_bg: Color32,
    pub mark_fg: Color32,
    pub callout_bg: Color32,
    pub alerts: AlertColors,
}

// ============================================================================
// 完整主题规格 ThemeSpec
// ============================================================================

#[derive(Clone, Copy, Debug)]
pub struct ThemeSpec {
    pub id: ThemeId,
    pub is_dark: bool,
    pub canvas_bg: Color32,
    pub top_bar_bg: Color32,
    /// 03 撞色模式下顶栏左侧 220px 使用深胡桃木底色（与左侧栏无缝衔接）
    pub top_bar_left_bg: Option<Color32>,
    pub top_bar_left_fg: Color32,
    pub surface_bg: Color32,
    pub popup_bg: Color32,
    pub input_bg: Color32,
    pub find_bar_bg: Color32,
    pub hover_bg: Color32,
    pub active_bg: Color32,
    pub faint_bg: Color32,
    pub border: Color32,
    pub text_fg: Color32,
    pub strong_fg: Color32,
    pub muted_fg: Color32,
    pub disabled_fg: Color32,
    pub accent: Color32,
    pub secondary_accent: Color32,
    pub selection_bg: Color32,
    pub selection_fg: Color32,
    pub cursor_color: Color32,
    pub window_shadow: Shadow,
    pub popup_shadow: Shadow,
    pub badge: BadgeStyle,
    pub top_bar_bottom_line: TopBarBottomLine,
    pub active_tab_style: ActiveTabStyle,
    pub mode_switch: ModeSwitchStyle,
    pub window_controls: WindowControlStyle,
    pub divider: DividerStyle,
    pub status_bar: StatusBarStyle,
    pub sidebar: SidebarColors,
    pub tabs: TabColors,
    pub editor: EditorPalette,
    pub preview: PreviewPalette,
    pub code: CodePalette,
    pub manual: ManualPalette,
    pub semantic: SemanticColors,
    pub export_css: ExportCssPalette,
}

// ============================================================================
// 5 套定稿主题 Token 定义
// ============================================================================

const ZEN_HORIZON: ThemeSpec = {
    let canvas = hex(0xFAF9F5);
    let surface = hex(0xFFFFFF);
    let callout = hex(0xF2EFE7);
    let text = hex(0x1C1917);
    let muted = hex(0x78716C);
    let split_dot = hex(0xE5E0D5);
    let accent = hex(0xF97316); // 珊瑚橙
    let accent2 = hex(0xEA580C);
    let on_accent = hex(0xFFFFFF);

    // 派生色推导（浅色纸面）：
    // hover_bg = mix(surface, text, 6%), active_bg = mix(surface, accent, 14%)
    // selection_bg = mix(canvas, accent, 20%), disabled_fg = mix(canvas, muted, 50%)
    let hover_bg = mix(surface, text, 6, 100);
    let active_bg = mix(surface, accent, 14, 100);
    let selection_bg = mix(canvas, accent, 20, 100);
    let disabled_fg = mix(canvas, muted, 50, 100);
    let border = hex(0xEAE5DA);
    let strong_fg = hex(0x110F0E);
    let soft_shadow = Shadow { offset: [0, 2], blur: 8, spread: 0, color: alpha(hex(0x1C1917), 18) };
    let alerts = AlertColors {
        note: hex(0x0284C7),
        tip: accent, // 与 01 效果图 TIP 提示块珊瑚橙左条保持一致
        important: hex(0x7C3AED),
        warning: hex(0xD97706),
        caution: hex(0xDC2626),
    };
    let close_red = alerts.caution;

    ThemeSpec {
        id: ThemeId::ZenHorizon,
        is_dark: false,
        canvas_bg: canvas,
        top_bar_bg: canvas,
        top_bar_left_bg: None,
        top_bar_left_fg: text,
        surface_bg: surface,
        popup_bg: surface,
        input_bg: surface,
        find_bar_bg: callout,
        hover_bg,
        active_bg,
        faint_bg: mix(canvas, callout, 55, 100),
        border,
        text_fg: text,
        strong_fg,
        muted_fg: muted,
        disabled_fg,
        accent,
        secondary_accent: accent2,
        selection_bg,
        selection_fg: text,
        cursor_color: accent,
        window_shadow: Shadow { offset: [0, 8], blur: 20, spread: 0, color: alpha(hex(0x1C1917), 24) },
        popup_shadow: soft_shadow,
        badge: BadgeStyle::GeometricBolt,
        top_bar_bottom_line: TopBarBottomLine::None,
        active_tab_style: ActiveTabStyle::SoftShadowPill,
        mode_switch: ModeSwitchStyle {
            container_bg: surface,
            container_stroke: stroke(1.0, alpha(border, 160)),
            container_shadow: soft_shadow,
            active_bg: callout,
            active_fg: text,
            active_stroke: Stroke::NONE,
            inactive_fg: muted,
            outer_radius: 10,
            inner_radius: 7,
        },
        // 派生规则（01 无界疾风窗口控制按钮）：
        // icon_idle = mix(muted, text, 65%), unfocused = mix(canvas, muted, 58%)
        // hover 采用纯白柔影胶囊 + 珊瑚橙 42% 边框点缀；关闭采用 alerts.caution 鲜明危险红
        window_controls: WindowControlStyle {
            corner_radius: 10,
            button_width: 30.0,
            button_height: TOP_PILL_H,
            button_gap: 4.0,
            section_gap: 8.0,
            right_margin: 10.0,
            icon_stroke_width: 1.35,
            bg_idle: Color32::TRANSPARENT,
            stroke_idle: Stroke::NONE,
            icon_idle_fg: mix(muted, text, 65, 100),
            icon_unfocused_fg: mix(canvas, muted, 58, 100),
            on_left_split_idle_fg: mix(muted, text, 65, 100),
            on_left_split_unfocused_fg: mix(canvas, muted, 58, 100),
            on_left_split_hover_bg: surface,
            bg_hover: surface,
            stroke_hover: stroke(1.0, mix(border, accent, 42, 100)),
            icon_hover_fg: strong_fg,
            hover_shadow: soft_shadow,
            top_highlight_hover: None,
            bg_active: active_bg,
            stroke_active: stroke(1.0, accent),
            icon_active_fg: strong_fg,
            close_bg_hover: close_red,
            close_stroke_hover: stroke(1.0, mix(close_red, text, 14, 100)),
            close_icon_hover: on_accent,
            close_bg_active: mix(close_red, text, 20, 100),
            close_icon_active: on_accent,
            window_border: mix(border, text, 14, 100),
        },
        divider: DividerStyle::Dotted { color: split_dot, step: 2.0 },
        status_bar: StatusBarStyle::FloatingPill {
            bar_bg: canvas,
            top_border: None,
            pill_bg: surface,
            pill_fg: text,
            bolt_color: accent,
            pill_stroke: stroke(1.0, alpha(border, 150)),
            pill_shadow: soft_shadow,
        },
        sidebar: SidebarColors {
            bg: canvas,
            border,
            outer_border: Color32::TRANSPARENT,
            fg: text,
            strong_fg: hex(0x110F0E),
            muted_fg: muted,
            surface_bg: surface,
            input_bg: surface,
            hover_bg: mix(canvas, text, 5, 100),
            active_bg: mix(canvas, accent, 10, 100),
            active_bar: accent,
            tab_underline: accent,
            selection_bg,
            selection_fg: text,
        },
        tabs: TabColors {
            active_bg: surface,
            idle_bg: Color32::TRANSPARENT,
            hover_bg: mix(canvas, text, 5, 100),
            active_fg: text,
            idle_fg: muted,
            border,
            accent,
            secondary_accent: accent2,
            shadow: soft_shadow,
            dirty_active: accent,
            dirty_idle: accent,
            close_hover_bg: mix(surface, text, 8, 100),
        },
        editor: EditorPalette {
            normal: text,
            heading: hex(0x110F0E),
            code: hex(0x9A3412),
            code_bg: callout,
            quote: muted,
            marker: accent2,
            link: hex(0x0F766E),
            bold: hex(0x110F0E),
            rule: hex(0xA8A29E),
            mark_bg: hex(0xFEF08A),
            find_match_bg: hex(0xFDE68A),
            find_cur_bg: hex(0xFDBA74),
            trailing_ws: alpha(muted, 130),
            current_line_bg: alpha(callout, 135),
        },
        preview: PreviewPalette {
            text,
            weak: muted,
            heading: hex(0x110F0E),
            link: accent2,
            code: hex(0x9A3412),
            code_bg: callout,
            code_border: border,
            callout_bg: callout,
            mark_bg: hex(0xFEF08A),
            mark_fg: text,
            quote: muted,
            border: split_dot,
            accent,
            on_accent,
            panel_bg: callout,
            table_head_bg: callout,
            table_stripe_bg: mix(canvas, callout, 50, 100),
            alerts,
        },
        code: CodePalette {
            is_dark: false,
            comment: muted,
            function: accent2,
            keyword: hex(0x9A3412),
            literal: hex(0x0F766E),
            string_literal: hex(0x15803D),
            punctuation: hex(0x57534E),
        },
        manual: ManualPalette {
            bar_bg: mix(canvas, callout, 50, 100),
            toc_bg: canvas,
            toc_active_bg: surface,
            toc_active_stroke: accent,
            toc_active_fg: text,
            toc_hover_bg: mix(canvas, text, 5, 100),
            card_bg: surface,
            src_bg: callout,
            prev_bg: canvas,
            border,
            accent,
            tag_setting: hex(0xB45309),
            tag_ext: accent2,
            tag_html: hex(0x0F766E),
        },
        semantic: SemanticColors {
            success: hex(0x15803D),
            warn: hex(0xC2410C),
            error: hex(0xDC2626),
            dirty_dot: accent,
        },
        export_css: ExportCssPalette {
            color_scheme: "light",
            bg: canvas,
            fg: text,
            heading: hex(0x110F0E),
            muted,
            border: split_dot,
            panel: surface,
            code_bg: callout,
            link: accent2,
            mark_bg: hex(0xFEF08A),
            mark_fg: text,
            callout_bg: callout,
            alerts,
        },
    }
};

const SCARLET_STREAK: ThemeSpec = {
    let canvas = hex(0xFFFFFF);
    let chrome = hex(0xFAF7F7);
    let border = hex(0xEFE3E3);
    let callout = hex(0xFEF2F2);
    let code_bg = hex(0xF8FAFC);
    let text = hex(0x18181B);
    let muted = hex(0x64748B);
    let accent = hex(0xDC2626); // 闪电侠绯红
    let gold = hex(0xFACC15);   // 神速力金
    let on_accent = hex(0xFFFFFF);

    // 派生色推导（白底绯红主题）：
    // hover_bg = mix(chrome, text, 6%), active_bg = mix(canvas, accent, 14%)
    // selection_bg = mix(canvas, accent, 16%), disabled_fg = mix(canvas, muted, 50%)
    let hover_bg = mix(chrome, text, 6, 100);
    let active_bg = mix(canvas, accent, 14, 100);
    let selection_bg = mix(canvas, accent, 16, 100);
    let disabled_fg = mix(canvas, muted, 50, 100);
    let soft_shadow = Shadow { offset: [0, 2], blur: 8, spread: 0, color: alpha(hex(0x7F1D1D), 24) };
    let alerts = AlertColors {
        note: hex(0x0284C7),
        tip: accent, // 与 02 效果图 TIP 绯红竖条一致
        important: hex(0x7C3AED),
        warning: hex(0xB45309),
        caution: hex(0xB91C1C),
    };

    ThemeSpec {
        id: ThemeId::ScarletStreak,
        is_dark: false,
        canvas_bg: canvas,
        top_bar_bg: chrome,
        top_bar_left_bg: None,
        top_bar_left_fg: text,
        surface_bg: canvas,
        popup_bg: canvas,
        input_bg: canvas,
        find_bar_bg: chrome,
        hover_bg,
        active_bg,
        faint_bg: code_bg,
        border,
        text_fg: text,
        strong_fg: hex(0x09090B),
        muted_fg: muted,
        disabled_fg,
        accent,
        secondary_accent: gold,
        selection_bg,
        selection_fg: text,
        cursor_color: accent,
        window_shadow: Shadow { offset: [0, 8], blur: 20, spread: 0, color: alpha(hex(0x18181B), 24) },
        popup_shadow: soft_shadow,
        badge: BadgeStyle::FlashShield,
        top_bar_bottom_line: TopBarBottomLine::Gradient { left: accent, right: gold, width: 1.5 },
        active_tab_style: ActiveTabStyle::CrimsonGoldBolt,
        mode_switch: ModeSwitchStyle {
            container_bg: canvas,
            container_stroke: stroke(1.0, border),
            container_shadow: soft_shadow,
            active_bg: accent,
            active_fg: on_accent,
            active_stroke: Stroke::NONE,
            inactive_fg: muted,
            outer_radius: 10,
            inner_radius: 7,
        },
        // 派生规则（02 白昼疾影窗口控制按钮）：
        // hover 使用纯白胶囊配绯红 45% 混边；关闭 hover 使用主题绯红 + 35% 神速力金描边
        window_controls: WindowControlStyle {
            corner_radius: 10,
            button_width: 30.0,
            button_height: TOP_PILL_H,
            button_gap: 4.0,
            section_gap: 8.0,
            right_margin: 10.0,
            icon_stroke_width: 1.35,
            bg_idle: Color32::TRANSPARENT,
            stroke_idle: Stroke::NONE,
            icon_idle_fg: mix(muted, text, 60, 100),
            icon_unfocused_fg: mix(chrome, muted, 58, 100),
            on_left_split_idle_fg: mix(muted, text, 60, 100),
            on_left_split_unfocused_fg: mix(chrome, muted, 58, 100),
            on_left_split_hover_bg: canvas,
            bg_hover: canvas,
            stroke_hover: stroke(1.0, mix(border, accent, 45, 100)),
            icon_hover_fg: hex(0x09090B),
            hover_shadow: soft_shadow,
            top_highlight_hover: None,
            bg_active: active_bg,
            stroke_active: stroke(1.0, accent),
            icon_active_fg: hex(0x09090B),
            close_bg_hover: accent,
            close_stroke_hover: stroke(1.0, mix(accent, gold, 35, 100)),
            close_icon_hover: on_accent,
            close_bg_active: alerts.caution,
            close_icon_active: on_accent,
            window_border: mix(border, text, 14, 100),
        },
        divider: DividerStyle::Solid(border),
        status_bar: StatusBarStyle::FloatingPill {
            bar_bg: chrome,
            top_border: Some(border),
            pill_bg: accent,
            pill_fg: on_accent,
            bolt_color: gold,
            pill_stroke: Stroke::NONE,
            pill_shadow: soft_shadow,
        },
        sidebar: SidebarColors {
            bg: chrome,
            border,
            outer_border: border,
            fg: text,
            strong_fg: hex(0x09090B),
            muted_fg: muted,
            surface_bg: canvas,
            input_bg: canvas,
            hover_bg,
            active_bg: mix(chrome, accent, 10, 100),
            active_bar: accent,
            tab_underline: gold,
            selection_bg,
            selection_fg: text,
        },
        tabs: TabColors {
            active_bg: accent,
            idle_bg: Color32::TRANSPARENT,
            hover_bg,
            active_fg: on_accent,
            idle_fg: muted,
            border,
            accent,
            secondary_accent: gold,
            shadow: soft_shadow,
            dirty_active: gold,
            dirty_idle: accent,
            close_hover_bg: alpha(on_accent, 42),
        },
        editor: EditorPalette {
            normal: text,
            heading: hex(0x09090B),
            code: hex(0xB91C1C),
            code_bg,
            quote: muted,
            marker: accent,
            link: hex(0x0369A1),
            bold: hex(0x09090B),
            rule: hex(0x94A3B8),
            mark_bg: hex(0xFEF08A),
            find_match_bg: hex(0xFDE68A),
            find_cur_bg: hex(0xFCA5A5),
            trailing_ws: alpha(muted, 130),
            current_line_bg: alpha(callout, 130),
        },
        preview: PreviewPalette {
            text,
            weak: muted,
            heading: hex(0x09090B),
            link: accent,
            code: hex(0xB91C1C),
            code_bg,
            code_border: hex(0xE2E8F0),
            callout_bg: callout,
            mark_bg: hex(0xFEF08A),
            mark_fg: text,
            quote: muted,
            border,
            accent,
            on_accent,
            panel_bg: code_bg,
            table_head_bg: chrome,
            table_stripe_bg: code_bg,
            alerts,
        },
        code: CodePalette {
            is_dark: false,
            comment: muted,
            function: hex(0xB45309),
            keyword: accent,
            literal: hex(0xCA8A04),
            string_literal: hex(0x0F766E),
            punctuation: hex(0x52525B),
        },
        manual: ManualPalette {
            bar_bg: chrome,
            toc_bg: chrome,
            toc_active_bg: canvas,
            toc_active_stroke: accent,
            toc_active_fg: accent,
            toc_hover_bg: hover_bg,
            card_bg: canvas,
            src_bg: code_bg,
            prev_bg: canvas,
            border,
            accent,
            tag_setting: hex(0xB45309),
            tag_ext: accent,
            tag_html: hex(0x0369A1),
        },
        semantic: SemanticColors {
            success: hex(0x15803D),
            warn: hex(0xB45309),
            error: accent,
            dirty_dot: accent,
        },
        export_css: ExportCssPalette {
            color_scheme: "light",
            bg: canvas,
            fg: text,
            heading: hex(0x09090B),
            muted,
            border,
            panel: chrome,
            code_bg,
            link: accent,
            mark_bg: hex(0xFEF08A),
            mark_fg: text,
            callout_bg: callout,
            alerts,
        },
    }
};

const ATELIER_CONTRAST: ThemeSpec = {
    let left_bg = hex(0x221E1C);
    let left_border = hex(0x362F2B);
    let left_fg = hex(0xF3EDE4);
    let left_muted = hex(0x9E9388);

    let page_bg = hex(0xFBF8F3);
    let tab_strip = hex(0xF4EFE6);
    let split_line = hex(0xE6DEC8);
    let sandstone = hex(0xEAE3D5);
    let code_bg = hex(0xF3ECE1);
    let text = hex(0x231F1D);
    // 右侧次文字从暖石褐推导（#756B63 在 #FBF8F3 上对比度 4.92:1，达到 WCAG AA）
    let muted = hex(0x756B63);
    let accent = hex(0xD97757); // 陶土铜
    let accent2 = hex(0xB85D3D);
    let on_accent = hex(0xFFFFFF);

    // 派生色推导（半山书房右书页浅色工作区）：
    // hover_bg = mix(tab_strip, text, 6%), active_bg = mix(sandstone, accent, 16%)
    // selection_bg = mix(page_bg, accent, 22%), disabled_fg = mix(page_bg, muted, 50%)
    let hover_bg = mix(tab_strip, text, 6, 100);
    let active_bg = mix(sandstone, accent, 16, 100);
    let selection_bg = mix(page_bg, accent, 22, 100);
    let disabled_fg = mix(page_bg, muted, 50, 100);
    let soft_shadow = Shadow { offset: [0, 2], blur: 6, spread: 0, color: alpha(hex(0x221E1C), 16) };
    let close_red = hex(0xC23B2B);
    let alerts = AlertColors {
        note: hex(0x0369A1),
        tip: accent, // 与 03 效果图 TIP 陶土铜竖条一致
        important: hex(0x7E22CE),
        warning: hex(0xB45309),
        caution: hex(0xB91C1C),
    };

    ThemeSpec {
        id: ThemeId::AtelierContrast,
        is_dark: false,
        canvas_bg: page_bg,
        top_bar_bg: tab_strip,
        top_bar_left_bg: Some(left_bg),
        top_bar_left_fg: left_fg,
        surface_bg: sandstone,
        popup_bg: page_bg,
        input_bg: hex(0xFFFFFF),
        find_bar_bg: tab_strip,
        hover_bg,
        active_bg,
        faint_bg: code_bg,
        border: split_line,
        text_fg: text,
        strong_fg: hex(0x161311),
        muted_fg: muted,
        disabled_fg,
        accent,
        secondary_accent: accent2,
        selection_bg,
        selection_fg: text,
        cursor_color: accent,
        window_shadow: Shadow { offset: [0, 8], blur: 20, spread: 0, color: alpha(left_bg, 28) },
        popup_shadow: soft_shadow,
        badge: BadgeStyle::GeometricBolt,
        top_bar_bottom_line: TopBarBottomLine::Solid {
            color: split_line,
            left_override: Some(left_border),
        },
        active_tab_style: ActiveTabStyle::ConnectedPageTopBar,
        mode_switch: ModeSwitchStyle {
            container_bg: sandstone,
            container_stroke: stroke(1.0, split_line),
            container_shadow: Shadow::NONE,
            active_bg: page_bg,
            active_fg: text,
            active_stroke: stroke(1.0, split_line),
            inactive_fg: muted,
            outer_radius: 9,
            inner_radius: 6,
        },
        // 派生规则（03 半山书房窗口控制按钮）：
        // 右侧悬停使用暖砂岩胶囊 + 陶土铜 50% 边框；左侧兼备胡桃木暖米字色；关闭使用窑变朱砂红 close_red
        window_controls: WindowControlStyle {
            corner_radius: 9,
            button_width: 30.0,
            button_height: TOP_PILL_H,
            button_gap: 4.0,
            section_gap: 8.0,
            right_margin: 10.0,
            icon_stroke_width: 1.35,
            bg_idle: Color32::TRANSPARENT,
            stroke_idle: Stroke::NONE,
            icon_idle_fg: mix(muted, text, 65, 100),
            icon_unfocused_fg: mix(tab_strip, muted, 58, 100),
            on_left_split_idle_fg: mix(left_muted, left_fg, 70, 100),
            on_left_split_unfocused_fg: mix(left_bg, left_muted, 58, 100),
            on_left_split_hover_bg: mix(left_bg, left_fg, 10, 100),
            bg_hover: sandstone,
            stroke_hover: stroke(1.0, mix(split_line, accent, 50, 100)),
            icon_hover_fg: hex(0x161311),
            hover_shadow: soft_shadow,
            top_highlight_hover: None,
            bg_active: active_bg,
            stroke_active: stroke(1.0, accent),
            icon_active_fg: hex(0x161311),
            close_bg_hover: close_red,
            close_stroke_hover: stroke(1.0, mix(close_red, text, 16, 100)),
            close_icon_hover: on_accent,
            close_bg_active: mix(close_red, text, 22, 100),
            close_icon_active: on_accent,
            window_border: mix(split_line, text, 14, 100),
        },
        divider: DividerStyle::Solid(split_line),
        status_bar: StatusBarStyle::FloatingPill {
            bar_bg: page_bg,
            top_border: None,
            pill_bg: code_bg,
            pill_fg: text,
            bolt_color: accent,
            pill_stroke: stroke(1.0, split_line),
            pill_shadow: soft_shadow,
        },
        sidebar: SidebarColors {
            bg: left_bg,
            border: left_border,
            outer_border: left_border,
            fg: left_fg,
            strong_fg: hex(0xFAF6F0),
            muted_fg: left_muted,
            surface_bg: left_border,
            input_bg: mix(left_bg, left_fg, 8, 100),
            hover_bg: mix(left_bg, left_fg, 7, 100),
            active_bg: mix(left_bg, accent, 16, 100),
            active_bar: accent,
            tab_underline: accent,
            selection_bg: mix(left_bg, accent, 32, 100),
            selection_fg: left_fg,
        },
        tabs: TabColors {
            active_bg: page_bg,
            idle_bg: Color32::TRANSPARENT,
            hover_bg: mix(tab_strip, text, 6, 100),
            active_fg: text,
            idle_fg: muted,
            border: split_line,
            accent,
            secondary_accent: accent2,
            shadow: Shadow::NONE,
            dirty_active: accent,
            dirty_idle: accent,
            close_hover_bg: mix(tab_strip, text, 9, 100),
        },
        editor: EditorPalette {
            normal: text,
            heading: hex(0x8C3D24),
            code: hex(0x9A3412),
            code_bg,
            quote: muted,
            marker: accent2,
            link: hex(0x2F6F63),
            bold: hex(0x161311),
            rule: hex(0xA69C8E),
            mark_bg: hex(0xFDE68A),
            find_match_bg: hex(0xFDE68A),
            find_cur_bg: hex(0xFDBA74),
            trailing_ws: alpha(muted, 130),
            current_line_bg: alpha(code_bg, 140),
        },
        preview: PreviewPalette {
            text,
            weak: muted,
            heading: hex(0x161311),
            link: accent2,
            code: hex(0x9A3412),
            code_bg,
            code_border: split_line,
            callout_bg: code_bg,
            mark_bg: hex(0xFDE68A),
            mark_fg: text,
            quote: muted,
            border: split_line,
            accent,
            on_accent,
            panel_bg: code_bg,
            table_head_bg: tab_strip,
            table_stripe_bg: mix(page_bg, code_bg, 50, 100),
            alerts,
        },
        code: CodePalette {
            is_dark: false,
            comment: muted,
            function: accent2,
            keyword: hex(0xB85D3D),
            literal: hex(0x2F6F63),
            string_literal: hex(0x4D7C0F),
            punctuation: hex(0x574F49),
        },
        manual: ManualPalette {
            bar_bg: tab_strip,
            toc_bg: left_bg,
            toc_active_bg: left_border,
            toc_active_stroke: accent,
            toc_active_fg: left_fg,
            toc_hover_bg: mix(left_bg, left_fg, 7, 100),
            card_bg: hex(0xFFFFFF),
            src_bg: code_bg,
            prev_bg: page_bg,
            border: split_line,
            accent,
            tag_setting: hex(0xB45309),
            tag_ext: accent2,
            tag_html: hex(0x2F6F63),
        },
        semantic: SemanticColors {
            success: hex(0x2F7D4C),
            warn: hex(0xB45309),
            error: close_red,
            dirty_dot: accent,
        },
        export_css: ExportCssPalette {
            color_scheme: "light",
            bg: page_bg,
            fg: text,
            heading: hex(0x161311),
            muted,
            border: split_line,
            panel: tab_strip,
            code_bg,
            link: accent2,
            mark_bg: hex(0xFDE68A),
            mark_fg: text,
            callout_bg: code_bg,
            alerts,
        },
    }
};

const CARBON_GOLD: ThemeSpec = {
    let canvas = hex(0x141413);
    let chrome = hex(0x1C1B19);
    let tab_active = hex(0x22201D);
    let btn_bg = hex(0x24221F);
    let border = hex(0x2C2A26);
    let text = hex(0xF5F2EB);
    let muted = hex(0x9A948A);
    let accent = hex(0xEAB308); // 磨砂香槟金
    let accent2 = hex(0xF59E0B);
    let on_accent = hex(0x141413);

    // 派生色推导（暖炭黑夜间流派）：
    // hover_bg = mix(btn_bg, text, 8%), active_bg = mix(btn_bg, accent, 18%)
    // selection_bg = mix(canvas, accent, 26%), disabled_fg = mix(canvas, muted, 50%)
    let hover_bg = mix(btn_bg, text, 8, 100);
    let active_bg = mix(btn_bg, accent, 18, 100);
    let selection_bg = mix(canvas, accent, 26, 100);
    let disabled_fg = mix(canvas, muted, 50, 100);
    let dark_shadow = Shadow { offset: [0, 8], blur: 20, spread: 0, color: alpha(hex(0x000000), 115) };
    let alerts = AlertColors {
        note: hex(0x60A5FA),
        tip: accent, // 与 04 效果图 TIP 鎏金竖条一致
        important: hex(0xC084FC),
        warning: accent2,
        caution: hex(0xF87171),
    };

    let strong_fg = hex(0xFFFDF8);
    let close_bg = mix(canvas, alerts.caution, 52, 100);

    ThemeSpec {
        id: ThemeId::CarbonGold,
        is_dark: true,
        canvas_bg: canvas,
        top_bar_bg: chrome,
        top_bar_left_bg: None,
        top_bar_left_fg: text,
        surface_bg: btn_bg,
        popup_bg: chrome,
        input_bg: canvas,
        find_bar_bg: chrome,
        hover_bg,
        active_bg,
        faint_bg: tab_active,
        border,
        text_fg: text,
        strong_fg,
        muted_fg: muted,
        disabled_fg,
        accent,
        secondary_accent: accent2,
        selection_bg,
        selection_fg: text,
        cursor_color: accent,
        window_shadow: dark_shadow,
        popup_shadow: Shadow { offset: [0, 6], blur: 14, spread: 0, color: alpha(hex(0x000000), 100) },
        badge: BadgeStyle::GeometricBolt,
        top_bar_bottom_line: TopBarBottomLine::Solid { color: border, left_override: None },
        active_tab_style: ActiveTabStyle::CarbonGoldUnderline,
        mode_switch: ModeSwitchStyle {
            container_bg: btn_bg,
            container_stroke: stroke(1.0, border),
            container_shadow: Shadow::NONE,
            active_bg: tab_active,
            active_fg: text,
            active_stroke: stroke(1.0, accent),
            inactive_fg: muted,
            outer_radius: 8,
            inner_radius: 5,
        },
        // 派生规则（04 黑金疾电窗口控制按钮）：
        // hover 使用 hover_bg + 76% 透明度香槟金描边，按下图标转为鎏金；关闭采用低饱和暗石榴红 mix(canvas, caution, 52%)
        window_controls: WindowControlStyle {
            corner_radius: 8,
            button_width: 30.0,
            button_height: TOP_PILL_H,
            button_gap: 4.0,
            section_gap: 8.0,
            right_margin: 10.0,
            icon_stroke_width: 1.35,
            bg_idle: Color32::TRANSPARENT,
            stroke_idle: Stroke::NONE,
            icon_idle_fg: mix(muted, text, 65, 100),
            icon_unfocused_fg: mix(chrome, muted, 55, 100),
            on_left_split_idle_fg: mix(muted, text, 65, 100),
            on_left_split_unfocused_fg: mix(chrome, muted, 55, 100),
            on_left_split_hover_bg: hover_bg,
            bg_hover: hover_bg,
            stroke_hover: stroke(1.0, alpha(accent, 195)),
            icon_hover_fg: strong_fg,
            hover_shadow: Shadow::NONE,
            top_highlight_hover: None,
            bg_active: active_bg,
            stroke_active: stroke(1.0, accent),
            icon_active_fg: accent,
            close_bg_hover: close_bg,
            close_stroke_hover: stroke(1.0, mix(border, accent, 55, 100)),
            close_icon_hover: strong_fg,
            close_bg_active: mix(close_bg, alerts.caution, 25, 100),
            close_icon_active: strong_fg,
            window_border: mix(border, accent, 26, 100),
        },
        divider: DividerStyle::Solid(border),
        status_bar: StatusBarStyle::FlushTelemetry {
            bar_bg: chrome,
            top_border: border,
            bolt_color: accent,
            text_color: accent,
        },
        sidebar: SidebarColors {
            bg: chrome,
            border,
            outer_border: border,
            fg: text,
            strong_fg: hex(0xFFFDF8),
            muted_fg: muted,
            surface_bg: btn_bg,
            input_bg: canvas,
            hover_bg,
            active_bg: tab_active,
            active_bar: accent,
            tab_underline: accent,
            selection_bg,
            selection_fg: text,
        },
        tabs: TabColors {
            active_bg: tab_active,
            idle_bg: Color32::TRANSPARENT,
            hover_bg,
            active_fg: text,
            idle_fg: muted,
            border,
            accent,
            secondary_accent: accent2,
            shadow: Shadow::NONE,
            dirty_active: accent,
            dirty_idle: accent2,
            close_hover_bg: hover_bg,
        },
        editor: EditorPalette {
            normal: text,
            heading: accent,
            code: accent2,
            code_bg: chrome,
            quote: muted,
            marker: accent,
            link: hex(0xFCD34D),
            bold: hex(0xFFFDF8),
            rule: hex(0x6E685F),
            mark_bg: hex(0x5B470B),
            find_match_bg: hex(0x4D3D0D),
            find_cur_bg: hex(0x854D0E),
            trailing_ws: alpha(muted, 130),
            current_line_bg: alpha(chrome, 160),
        },
        preview: PreviewPalette {
            text,
            weak: muted,
            heading: hex(0xFFFDF8),
            link: accent,
            code: accent2,
            code_bg: chrome,
            code_border: border,
            callout_bg: chrome,
            mark_bg: hex(0x5B470B),
            mark_fg: hex(0xFFFDF8),
            quote: muted,
            border,
            accent,
            on_accent,
            panel_bg: chrome,
            table_head_bg: tab_active,
            table_stripe_bg: mix(canvas, chrome, 60, 100),
            alerts,
        },
        code: CodePalette {
            is_dark: true,
            comment: muted,
            function: accent2,
            keyword: accent,
            literal: hex(0xFDE047),
            string_literal: hex(0xA3E635),
            punctuation: hex(0xC2BBB0),
        },
        manual: ManualPalette {
            bar_bg: chrome,
            toc_bg: chrome,
            toc_active_bg: tab_active,
            toc_active_stroke: accent,
            toc_active_fg: accent,
            toc_hover_bg: hover_bg,
            card_bg: chrome,
            src_bg: canvas,
            prev_bg: tab_active,
            border,
            accent,
            tag_setting: accent,
            tag_ext: accent2,
            tag_html: hex(0xFCD34D),
        },
        semantic: SemanticColors {
            success: hex(0x4ADE80),
            warn: accent2,
            error: hex(0xF87171),
            dirty_dot: accent,
        },
        export_css: ExportCssPalette {
            color_scheme: "dark",
            bg: canvas,
            fg: text,
            heading: hex(0xFFFDF8),
            muted,
            border,
            panel: chrome,
            code_bg: tab_active,
            link: accent,
            mark_bg: hex(0x5B470B),
            mark_fg: hex(0xFFFDF8),
            callout_bg: chrome,
            alerts,
        },
    }
};

const TITANIUM_GLASS: ThemeSpec = {
    let chassis = hex(0x1C1C1F);
    let border = hex(0x2E2E32);
    let glass_plate = hex(0x242428);
    let glass_card = hex(0x2C2C31);
    let active_pill = hex(0x3A3A40);
    let text = hex(0xF5F5F7);
    let muted = hex(0x8E8E93);
    let accent = hex(0xFFFFFF); // 液态银白
    let accent2 = hex(0xD1D1D6);
    let on_accent = hex(0x1C1C1F);
    let rim_stroke = alpha(accent, 38);     // ~15% 珍珠银描边
    let top_highlight = alpha(accent, 56);  // ~22% 上缘高光线

    // 派生色推导（纯中性钛金属深调）：
    // hover_bg = mix(glass_card, accent, 8%), active_bg = active_pill
    // selection_bg = mix(glass_plate, accent, 22%), disabled_fg = mix(chassis, muted, 50%)
    let hover_bg = mix(glass_card, accent, 8, 100);
    let active_bg = active_pill;
    let selection_bg = mix(glass_plate, accent, 22, 100);
    let disabled_fg = mix(chassis, muted, 50, 100);
    let glass_shadow = Shadow { offset: [0, 6], blur: 16, spread: 0, color: alpha(hex(0x000000), 110) };
    // 05 主张零彩色光污染：提示块以纯中性银阶区分，同时保持极高可读性
    let alerts = AlertColors {
        note: hex(0xD1D1D6),
        tip: accent, // 与 05 效果图 TIP 液态银白竖条一致
        important: hex(0xE5E5EA),
        warning: hex(0xC7C7CC),
        caution: hex(0xF2F2F7),
    };

    let err_red = hex(0xF87171);
    let close_bg = mix(chassis, err_red, 48, 100);

    ThemeSpec {
        id: ThemeId::TitaniumGlass,
        is_dark: true,
        canvas_bg: glass_plate,
        top_bar_bg: chassis,
        top_bar_left_bg: None,
        top_bar_left_fg: text,
        surface_bg: glass_card,
        popup_bg: glass_plate,
        input_bg: chassis,
        find_bar_bg: chassis,
        hover_bg,
        active_bg,
        faint_bg: glass_card,
        border,
        text_fg: text,
        strong_fg: accent,
        muted_fg: muted,
        disabled_fg,
        accent,
        secondary_accent: accent2,
        selection_bg,
        selection_fg: accent,
        cursor_color: accent,
        window_shadow: glass_shadow,
        popup_shadow: glass_shadow,
        badge: BadgeStyle::GeometricBolt,
        top_bar_bottom_line: TopBarBottomLine::Solid { color: border, left_override: None },
        active_tab_style: ActiveTabStyle::SmokedGlassRim,
        mode_switch: ModeSwitchStyle {
            container_bg: glass_plate,
            container_stroke: stroke(1.0, rim_stroke),
            container_shadow: Shadow::NONE,
            active_bg: active_pill,
            active_fg: accent,
            active_stroke: stroke(1.0, top_highlight),
            inactive_fg: muted,
            outer_radius: 10,
            inner_radius: 7,
        },
        // 派生规则（05 钛晶流光窗口控制按钮）：
        // hover 为烟熏玻璃 active_pill + 15% 珍珠银描边 + 上缘 22% 珍珠银高光；关闭使用低饱和烟熏玫瑰钛红 mix(chassis, err_red, 48%)
        window_controls: WindowControlStyle {
            corner_radius: 10,
            button_width: 30.0,
            button_height: TOP_PILL_H,
            button_gap: 4.0,
            section_gap: 8.0,
            right_margin: 10.0,
            icon_stroke_width: 1.35,
            bg_idle: Color32::TRANSPARENT,
            stroke_idle: Stroke::NONE,
            icon_idle_fg: mix(muted, text, 68, 100),
            icon_unfocused_fg: mix(chassis, muted, 55, 100),
            on_left_split_idle_fg: mix(muted, text, 68, 100),
            on_left_split_unfocused_fg: mix(chassis, muted, 55, 100),
            on_left_split_hover_bg: active_pill,
            bg_hover: active_pill,
            stroke_hover: stroke(1.0, rim_stroke),
            icon_hover_fg: accent,
            hover_shadow: glass_shadow,
            top_highlight_hover: Some(top_highlight),
            bg_active: mix(active_pill, accent, 14, 100),
            stroke_active: stroke(1.0, top_highlight),
            icon_active_fg: accent,
            close_bg_hover: close_bg,
            close_stroke_hover: stroke(1.0, alpha(accent, 68)),
            close_icon_hover: accent,
            close_bg_active: mix(close_bg, err_red, 25, 100),
            close_icon_active: accent,
            window_border: mix(border, accent, 20, 100),
        },
        divider: DividerStyle::GlassInset {
            chassis_bg: chassis,
            plate_bg: glass_plate,
            rim_stroke,
            top_highlight,
            radius: 10.0,
        },
        status_bar: StatusBarStyle::FloatingPill {
            bar_bg: chassis,
            top_border: Some(border),
            pill_bg: glass_card,
            pill_fg: text,
            bolt_color: accent,
            pill_stroke: stroke(1.0, rim_stroke),
            pill_shadow: glass_shadow,
        },
        sidebar: SidebarColors {
            bg: chassis,
            border,
            outer_border: border,
            fg: text,
            strong_fg: accent,
            muted_fg: muted,
            surface_bg: glass_card,
            input_bg: hex(0x161618),
            hover_bg,
            active_bg: glass_card,
            active_bar: accent,
            tab_underline: accent,
            selection_bg,
            selection_fg: accent,
        },
        tabs: TabColors {
            active_bg: active_pill,
            idle_bg: Color32::TRANSPARENT,
            hover_bg,
            active_fg: accent,
            idle_fg: muted,
            border: rim_stroke,
            accent,
            secondary_accent: top_highlight,
            shadow: Shadow::NONE,
            dirty_active: accent,
            dirty_idle: accent2,
            close_hover_bg: alpha(accent, 36),
        },
        editor: EditorPalette {
            normal: text,
            heading: accent,
            code: accent2,
            code_bg: glass_card,
            quote: muted,
            marker: accent2,
            link: accent,
            bold: accent,
            rule: hex(0x6E6E73),
            mark_bg: hex(0x48484F),
            find_match_bg: hex(0x484852),
            find_cur_bg: hex(0x63636E),
            trailing_ws: alpha(muted, 130),
            current_line_bg: alpha(glass_card, 145),
        },
        preview: PreviewPalette {
            text,
            weak: muted,
            heading: accent,
            link: accent,
            code: hex(0xE5E5EA),
            code_bg: glass_card,
            code_border: rim_stroke,
            callout_bg: glass_card,
            mark_bg: hex(0x48484F),
            mark_fg: accent,
            quote: muted,
            border: rim_stroke,
            accent,
            on_accent,
            panel_bg: glass_card,
            table_head_bg: active_pill,
            table_stripe_bg: mix(glass_plate, glass_card, 55, 100),
            alerts,
        },
        code: CodePalette {
            is_dark: true,
            comment: muted,
            function: accent,
            keyword: accent2,
            literal: hex(0xE5E5EA),
            string_literal: hex(0xC7C7CC),
            punctuation: hex(0xAEAEB2),
        },
        manual: ManualPalette {
            bar_bg: chassis,
            toc_bg: chassis,
            toc_active_bg: active_pill,
            toc_active_stroke: top_highlight,
            toc_active_fg: accent,
            toc_hover_bg: hover_bg,
            card_bg: glass_card,
            src_bg: chassis,
            prev_bg: glass_plate,
            border: rim_stroke,
            accent,
            tag_setting: hex(0xE5E5EA),
            tag_ext: hex(0xD1D1D6),
            tag_html: hex(0xC7C7CC),
        },
        semantic: SemanticColors {
            success: hex(0x4ADE80),
            warn: hex(0xFBBF24),
            error: hex(0xF87171),
            dirty_dot: accent,
        },
        export_css: ExportCssPalette {
            color_scheme: "dark",
            bg: chassis,
            fg: text,
            heading: accent,
            muted,
            border,
            panel: glass_plate,
            code_bg: glass_card,
            link: accent,
            mark_bg: hex(0x48484F),
            mark_fg: accent,
            callout_bg: glass_card,
            alerts,
        },
    }
};

pub static SPECS: [ThemeSpec; 5] = [ZEN_HORIZON, SCARLET_STREAK, ATELIER_CONTRAST, CARBON_GOLD, TITANIUM_GLASS];

// ============================================================================
// 预构建 egui Visuals 与 Spacing 缓存（每帧零重建开销）
// ============================================================================

struct PrebuiltTheme {
    visuals: Visuals,
    sidebar_visuals: Visuals,
}

struct WidgetPalette {
    panel_bg: Color32,
    surface_bg: Color32,
    hover_bg: Color32,
    active_bg: Color32,
    border: Color32,
    text_fg: Color32,
    strong_fg: Color32,
    accent: Color32,
}

fn build_widget_visuals(p: WidgetPalette) -> Widgets {
    Widgets {
        noninteractive: WidgetVisuals {
            weak_bg_fill: p.panel_bg,
            bg_fill: p.panel_bg,
            bg_stroke: Stroke::new(1.0, p.border),
            fg_stroke: Stroke::new(1.0, p.text_fg),
            corner_radius: CornerRadius::same(6),
            expansion: 0.0,
        },
        inactive: WidgetVisuals {
            weak_bg_fill: p.surface_bg,
            bg_fill: p.surface_bg,
            bg_stroke: Stroke::new(1.0, mix(p.border, p.text_fg, 18, 100)),
            fg_stroke: Stroke::new(1.0, p.text_fg),
            corner_radius: CornerRadius::same(6),
            expansion: 0.0,
        },
        hovered: WidgetVisuals {
            weak_bg_fill: p.hover_bg,
            bg_fill: p.hover_bg,
            bg_stroke: Stroke::new(1.0, p.accent),
            fg_stroke: Stroke::new(1.5, p.strong_fg),
            corner_radius: CornerRadius::same(6),
            expansion: 0.0,
        },
        active: WidgetVisuals {
            weak_bg_fill: p.active_bg,
            bg_fill: p.active_bg,
            bg_stroke: Stroke::new(1.0, p.accent),
            fg_stroke: Stroke::new(1.5, p.strong_fg),
            corner_radius: CornerRadius::same(6),
            expansion: 0.0,
        },
        open: WidgetVisuals {
            weak_bg_fill: p.hover_bg,
            bg_fill: p.surface_bg,
            bg_stroke: Stroke::new(1.0, p.accent),
            fg_stroke: Stroke::new(1.0, p.strong_fg),
            corner_radius: CornerRadius::same(6),
            expansion: 0.0,
        },
    }
}

fn build_visuals(spec: &ThemeSpec) -> Visuals {
    let mut v = if spec.is_dark { Visuals::dark() } else { Visuals::light() };
    v.dark_mode = spec.is_dark;
    v.override_text_color = None;
    v.weak_text_color = Some(spec.muted_fg);
    v.widgets = build_widget_visuals(WidgetPalette {
        panel_bg: spec.popup_bg,
        surface_bg: spec.surface_bg,
        hover_bg: spec.hover_bg,
        active_bg: spec.active_bg,
        border: spec.border,
        text_fg: spec.text_fg,
        strong_fg: spec.strong_fg,
        accent: spec.accent,
    });
    v.selection = Selection {
        bg_fill: spec.selection_bg,
        stroke: Stroke::new(1.0, spec.selection_fg),
    };
    v.ime_composition.active_underline_stroke = Stroke::new(2.0, spec.accent);
    v.ime_composition.inactive_underline_stroke = Stroke::new(2.0, alpha(spec.accent, 128));
    v.hyperlink_color = spec.preview.link;
    v.faint_bg_color = spec.faint_bg;
    v.extreme_bg_color = spec.input_bg;
    v.text_edit_bg_color = Some(spec.input_bg);
    v.code_bg_color = spec.preview.code_bg;
    v.warn_fg_color = spec.semantic.warn;
    v.error_fg_color = spec.semantic.error;
    v.window_corner_radius = CornerRadius::same(10);
    v.window_shadow = spec.window_shadow;
    v.window_fill = spec.popup_bg;
    v.window_stroke = Stroke::new(1.0, spec.border);
    v.menu_corner_radius = CornerRadius::same(8);
    v.panel_fill = spec.canvas_bg;
    v.popup_shadow = spec.popup_shadow;
    v.text_cursor = TextCursorStyle {
        stroke: Stroke::new(2.0, spec.cursor_color),
        ..Default::default()
    };
    v.handle_shape = HandleShape::Circle;
    v.disabled_alpha = 0.48;
    v
}

fn build_sidebar_visuals(spec: &ThemeSpec, base: &Visuals) -> Visuals {
    let mut v = base.clone();
    let s = &spec.sidebar;
    v.dark_mode = spec.id == ThemeId::AtelierContrast || spec.is_dark;
    v.weak_text_color = Some(s.muted_fg);
    v.panel_fill = s.bg;
    v.extreme_bg_color = s.input_bg;
    v.text_edit_bg_color = Some(s.input_bg);
    v.faint_bg_color = s.surface_bg;
    v.widgets = build_widget_visuals(WidgetPalette {
        panel_bg: s.bg,
        surface_bg: s.surface_bg,
        hover_bg: s.hover_bg,
        active_bg: s.active_bg,
        border: s.border,
        text_fg: s.fg,
        strong_fg: s.strong_fg,
        accent: spec.accent,
    });
    v.selection = Selection {
        bg_fill: s.selection_bg,
        stroke: Stroke::new(1.0, s.selection_fg),
    };
    v.text_cursor = TextCursorStyle {
        stroke: Stroke::new(2.0, spec.accent),
        ..Default::default()
    };
    v
}

static PREBUILT: LazyLock<[PrebuiltTheme; 5]> = LazyLock::new(|| {
    ThemeId::ALL.map(|id| {
        let spec = id.spec();
        let visuals = build_visuals(spec);
        let sidebar_visuals = build_sidebar_visuals(spec, &visuals);
        PrebuiltTheme { visuals, sidebar_visuals }
    })
});

fn unified_spacing() -> Spacing {
    Spacing {
        window_margin: egui::Margin::same(12),
        menu_margin: egui::Margin::same(6),
        button_padding: vec2(10.0, 4.0),
        scroll: ScrollStyle {
            floating: true,
            bar_width: 8.0,
            floating_width: 3.0,
            handle_min_length: 20.0,
            dormant_background_opacity: 0.0,
            active_background_opacity: 0.16,
            interact_background_opacity: 0.28,
            dormant_handle_opacity: 0.30,
            active_handle_opacity: 0.55,
            interact_handle_opacity: 0.85,
            ..ScrollStyle::floating()
        },
        ..Default::default()
    }
}

static THEME_CTX_KEY: LazyLock<egui::Id> = LazyLock::new(|| egui::Id::unique("mdpad_active_theme_id"));

/// 将指定主题完整注入到 `egui::Context`（同时覆盖 Dark 与 Light 两套全局 Style，杜绝弹窗/菜单残留默认灰蓝）。
pub fn apply(ctx: &egui::Context, id: ThemeId) {
    let spec = id.spec();
    let prebuilt = &PREBUILT[id.index()];
    let spacing = unified_spacing();

    ctx.data_mut(|d| d.insert_temp(*THEME_CTX_KEY, id));
    ctx.set_theme(if spec.is_dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|style: &mut Style| {
        style.visuals = prebuilt.visuals.clone();
        style.spacing = spacing.clone();
    });
}

pub fn current_id(ctx: &egui::Context) -> ThemeId {
    ctx.data(|d| d.get_temp::<ThemeId>(*THEME_CTX_KEY)).unwrap_or_default()
}

pub fn current(ctx: &egui::Context) -> &'static ThemeSpec {
    current_id(ctx).spec()
}

pub fn sidebar_visuals(ctx: &egui::Context) -> &'static Visuals {
    &PREBUILT[current_id(ctx).index()].sidebar_visuals
}

// ============================================================================
// 共享矢量绘制辅助（闪电徽标 / 02 圆盾金电徽章 / 渐变速度线 / 2px 点线）
// ============================================================================

/// 在 `rect` 中绘制尖锐的双三角矢量闪电。
pub fn paint_bolt(painter: &Painter, rect: Rect, color: Color32) {
    let p = |nx: f32, ny: f32| pos2(rect.left() + rect.width() * nx, rect.top() + rect.height() * ny);
    let mut mesh = Mesh::default();
    let pts = [
        p(0.60, 0.08),
        p(0.20, 0.54),
        p(0.50, 0.54),
        p(0.40, 0.92),
        p(0.80, 0.46),
        p(0.50, 0.46),
    ];
    for pt in pts {
        mesh.colored_vertex(pt, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 5);
    mesh.add_triangle(5, 2, 3);
    mesh.add_triangle(5, 3, 4);
    painter.add(mesh);
}

/// 绘制顶栏品牌徽标：普通几何闪电或「02 白昼疾影」圆盾金电徽章。
pub fn paint_brand_badge(painter: &Painter, rect: Rect, spec: &ThemeSpec) {
    match spec.badge {
        BadgeStyle::GeometricBolt => {
            paint_bolt(painter, rect.shrink(2.0), spec.accent);
        }
        BadgeStyle::FlashShield => {
            let center = rect.center();
            let radius = rect.width().min(rect.height()) * 0.46;
            painter.circle_filled(center, radius, spec.accent);
            painter.circle_stroke(center, radius, Stroke::new(1.8, spec.secondary_accent));
            let inner = Rect::from_center_size(center, vec2(radius * 1.32, radius * 1.42));
            paint_bolt(painter, inner, spec.secondary_accent);
        }
    }
}

/// 沿 `x_range` 在 `y` 处绘制水平线性渐变细线（用于 02 顶栏 `#DC2626 -> #FACC15` 速度渐隐细线）。
pub fn paint_horizontal_gradient_line(
    painter: &Painter,
    x_start: f32,
    x_end: f32,
    y_top: f32,
    thickness: f32,
    left: Color32,
    right: Color32,
) {
    if x_end <= x_start {
        return;
    }
    let y_bot = y_top + thickness;
    let mut mesh = Mesh::default();
    mesh.colored_vertex(pos2(x_start, y_top), left);
    mesh.colored_vertex(pos2(x_end, y_top), right);
    mesh.colored_vertex(pos2(x_end, y_bot), right);
    mesh.colored_vertex(pos2(x_start, y_bot), left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// 绘制 2px 间隔点状虚线（用于 01 无界疾风分屏中轴）。
pub fn paint_dotted_vline(painter: &Painter, x: f32, y_top: f32, y_bottom: f32, color: Color32, step: f32) {
    let mut y = y_top + 4.0;
    let stride = (step * 2.5).max(4.0);
    while y + step <= y_bottom - 4.0 {
        painter.line_segment([pos2(x, y), pos2(x, y + step)], Stroke::new(1.0, color));
        y += stride;
    }
}

/// 绘制矢量双矩形复制图标（替代易受字体缺字影响的 Unicode 符号）。
pub fn paint_copy_icon(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let stroke = Stroke::new(1.25, color);
    let back = Rect::from_center_size(center + vec2(-size * 0.14, -size * 0.14), vec2(size * 0.56, size * 0.66));
    let front = Rect::from_center_size(center + vec2(size * 0.14, size * 0.14), vec2(size * 0.56, size * 0.66));
    painter.line_segment([back.left_bottom(), back.left_top()], stroke);
    painter.line_segment([back.left_top(), back.right_top()], stroke);
    painter.rect_stroke(front, 1.5, stroke, egui::StrokeKind::Middle);
}

// ============================================================================
// 单元测试：WCAG 2.1 对比度 & 容错反序列化
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn srgb_luminance(c: Color32) -> f64 {
        let ch = |u: u8| {
            let v = u as f64 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    fn contrast_ratio(a: Color32, b: Color32) -> f64 {
        let l1 = srgb_luminance(a);
        let l2 = srgb_luminance(b);
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn all_five_themes_meet_wcag_aa_contrast() {
        for id in ThemeId::ALL {
            let s = id.spec();
            let body_canvas = contrast_ratio(s.text_fg, s.canvas_bg);
            let body_code = contrast_ratio(s.preview.text, s.preview.code_bg);
            let muted_canvas = contrast_ratio(s.muted_fg, s.canvas_bg);
            let sel_ratio = contrast_ratio(s.selection_fg, s.selection_bg);
            let sidebar_body = contrast_ratio(s.sidebar.fg, s.sidebar.bg);
            let sidebar_muted = contrast_ratio(s.sidebar.muted_fg, s.sidebar.bg);
            eprintln!(
                "WCAG {id:?}: body/canvas={body_canvas:.2}, body/code={body_code:.2}, muted/canvas={muted_canvas:.2}, sel={sel_ratio:.2}, sidebar={sidebar_body:.2}, sidebar_muted={sidebar_muted:.2}"
            );

            assert!(body_canvas >= 4.5, "{id:?} body/canvas {body_canvas:.2}");
            assert!(body_code >= 4.5, "{id:?} body/code {body_code:.2}");
            assert!(muted_canvas >= 3.0, "{id:?} muted/canvas {muted_canvas:.2}");
            assert!(sel_ratio >= 4.5, "{id:?} selection {sel_ratio:.2}");
            assert!(sidebar_body >= 4.5, "{id:?} sidebar body {sidebar_body:.2}");
            assert!(sidebar_muted >= 3.0, "{id:?} sidebar muted {sidebar_muted:.2}");
        }
        // 02 绯红胶囊反白文字对比度
        let s2 = ThemeId::ScarletStreak.spec();
        let on_acc2 = contrast_ratio(s2.preview.on_accent, s2.accent);
        eprintln!("WCAG ScarletStreak on_accent/accent={on_acc2:.2}");
        assert!(on_acc2 >= 4.5);
    }

    #[test]
    fn window_controls_meet_wcag_contrast() {
        for id in ThemeId::ALL {
            let s = id.spec();
            let wc = &s.window_controls;
            let idle = contrast_ratio(wc.icon_idle_fg, s.top_bar_bg);
            let hover = contrast_ratio(wc.icon_hover_fg, wc.bg_hover);
            let close_hover = contrast_ratio(wc.close_icon_hover, wc.close_bg_hover);
            let left_idle = contrast_ratio(wc.on_left_split_idle_fg, s.sidebar.bg);
            eprintln!(
                "WCAG WinCtrl {id:?}: idle={idle:.2}, hover={hover:.2}, close_hover={close_hover:.2}, left_idle={left_idle:.2}"
            );

            assert!(idle >= 3.0, "{id:?} win_ctrl idle {idle:.2}");
            assert!(hover >= 4.5, "{id:?} win_ctrl hover {hover:.2}");
            assert!(close_hover >= 4.5, "{id:?} win_ctrl close_hover {close_hover:.2}");
            assert!(left_idle >= 4.5, "{id:?} win_ctrl left_idle {left_idle:.2}");
        }
    }

    #[test]
    fn theme_id_deserialization_and_env_parsing_fallback_to_zen_horizon() {
        assert_eq!(ThemeId::default(), ThemeId::ZenHorizon);
        assert_eq!(ThemeId::from_env_str("1"), Some(ThemeId::ZenHorizon));
        assert_eq!(ThemeId::from_env_str("02"), Some(ThemeId::ScarletStreak));
        assert_eq!(ThemeId::from_env_str("AtelierContrast"), Some(ThemeId::AtelierContrast));
        assert_eq!(ThemeId::from_env_str("carbon_gold"), Some(ThemeId::CarbonGold));
        assert_eq!(ThemeId::from_env_str("5"), Some(ThemeId::TitaniumGlass));
        assert_eq!(ThemeId::from_env_str("nonexistent"), None);

        let unknown: ThemeId = serde_json::from_str("\"future_theme_99\"").unwrap();
        assert_eq!(unknown, ThemeId::ZenHorizon);
        let bad_num: ThemeId = serde_json::from_str("99").unwrap();
        assert_eq!(bad_num, ThemeId::ZenHorizon);
        let null_val: ThemeId = serde_json::from_str("null").unwrap();
        assert_eq!(null_val, ThemeId::ZenHorizon);
        let valid: ThemeId = serde_json::from_str("\"ScarletStreak\"").unwrap();
        assert_eq!(valid, ThemeId::ScarletStreak);
    }
}
