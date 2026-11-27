//! 应用持久化设置。

use crate::theme::{deserialize_dark_theme_fallback, ThemeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ViewMode {
    Edit,
    Split,
    Preview,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SidebarTab {
    Files,
    Articles,
    Outline,
}

fn default_true() -> bool {
    true
}

fn default_dark_theme() -> ThemeId {
    ThemeId::CarbonGold
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 当前激活的主题（默认 01 无界疾风）
    #[serde(default)]
    pub theme: ThemeId,
    /// 日/夜热切换：日间偏好主题（默认 01 无界疾风）
    #[serde(default)]
    pub light_theme: ThemeId,
    /// 日/夜热切换：夜间偏好主题（默认 04 黑金疾电）
    #[serde(default = "default_dark_theme", deserialize_with = "deserialize_dark_theme_fallback")]
    pub dark_theme: ThemeId,
    /// 是否回退使用 Windows 系统原生标题栏（修改后重启生效）
    #[serde(default)]
    pub use_system_titlebar: bool,
    pub mode: ViewMode,
    pub editor_font_size: f32,
    pub preview_font_size: f32,
    pub preview_font_family: String,
    pub editor_font_family: String,
    pub code_font_family: String,
    pub line_height: f32,
    pub paragraph_spacing: f32,
    /// 最大内容宽度（0.0 表示不限制）
    pub max_content_width: f32,
    pub ui_zoom: f32,
    /// 上标 ^x^ / 下标 ~x~ 语法（开启后单个 ~ 不再表示删除线）
    pub sup_sub: bool,
    /// 预览中显示 YAML Front Matter
    pub front_matter: bool,
    /// 启动时恢复上次打开的文件标签
    pub restore_tabs: bool,
    pub session_paths: Vec<PathBuf>,
    pub session_active: usize,
    pub session_manual_open: bool,
    pub session_manual_active: bool,
    /// 侧边栏状态
    pub sidebar_open: bool,
    pub sidebar_tab: SidebarTab,
    pub workspace_folder: Option<PathBuf>,
    /// 自动闭合括号与引号
    pub auto_pair: bool,
    /// 在围栏代码块内也自动闭合括号与引号
    pub auto_pair_in_code: bool,
    /// 粘贴网页/富文本剪贴板内容时自动转为 Markdown
    #[serde(default = "default_true")]
    pub paste_html_as_md: bool,
    /// 代码块预览显示行号
    pub code_line_numbers: bool,
    /// 字数统计选项
    pub stats_exclude_code: bool,
    pub stats_exclude_syntax: bool,
    /// 自定义快捷键覆盖表（command_id -> shortcut 字符串，"" 表示解绑）
    pub keybindings: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeId::ZenHorizon,
            light_theme: ThemeId::ZenHorizon,
            dark_theme: ThemeId::CarbonGold,
            use_system_titlebar: false,
            mode: ViewMode::Split,
            editor_font_size: 15.0,
            preview_font_size: 16.0,
            preview_font_family: String::new(),
            editor_font_family: String::new(),
            code_font_family: String::new(),
            line_height: 1.6,
            paragraph_spacing: 0.75,
            max_content_width: 0.0,
            ui_zoom: 1.0,
            sup_sub: false,
            front_matter: true,
            restore_tabs: true,
            session_paths: Vec::new(),
            session_active: 0,
            session_manual_open: false,
            session_manual_active: false,
            sidebar_open: false,
            sidebar_tab: SidebarTab::Files,
            workspace_folder: None,
            auto_pair: true,
            auto_pair_in_code: true,
            paste_html_as_md: true,
            code_line_numbers: true,
            stats_exclude_code: false,
            stats_exclude_syntax: true,
            keybindings: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_unknown_theme_falls_back_to_zen_horizon_preserving_settings() {
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.theme, ThemeId::ZenHorizon);
        assert_eq!(empty.light_theme, ThemeId::ZenHorizon);
        assert_eq!(empty.dark_theme, ThemeId::CarbonGold);

        let legacy_json = r#"{
            "mode": "Preview",
            "editor_font_size": 19.0,
            "sup_sub": true,
            "restore_tabs": false,
            "keybindings": { "save": "Ctrl+Alt+S" }
        }"#;
        let legacy: Settings = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(legacy.theme, ThemeId::ZenHorizon);
        assert_eq!(legacy.light_theme, ThemeId::ZenHorizon);
        assert_eq!(legacy.dark_theme, ThemeId::CarbonGold);
        assert_eq!(legacy.mode, ViewMode::Preview);
        assert!((legacy.editor_font_size - 19.0).abs() < 1e-6);
        assert!(legacy.sup_sub);
        assert!(!legacy.restore_tabs);
        assert_eq!(legacy.keybindings.get("save").map(String::as_str), Some("Ctrl+Alt+S"));

        let invalid_json = r#"{ "theme": "no_such_theme_xyz", "editor_font_size": 21.0 }"#;
        let invalid: Settings = serde_json::from_str(invalid_json).unwrap();
        assert!(!invalid.use_system_titlebar);
        assert_eq!(invalid.theme, ThemeId::ZenHorizon);

        let sys_on: Settings = serde_json::from_str(r#"{ "use_system_titlebar": true }"#).unwrap();
        assert!(sys_on.use_system_titlebar);
        assert!((invalid.editor_font_size - 21.0).abs() < 1e-6);
    }
}
