//! 应用持久化设置。

use crate::theme::{deserialize_dark_theme_fallback, ThemeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_RECENT_FILES: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: PathBuf,
    #[serde(default)]
    pub pinned: bool,
    /// Unix 秒级时间戳（打开或保存文件时记录）
    #[serde(default)]
    pub last_opened_unix: u64,
}

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
    /// 粘贴网页/富文本剪贴板内容时自动转为 Markdown（默认关闭）
    #[serde(default)]
    pub paste_html_as_md: bool,
    /// 一次性默认项迁移标记：将旧版默认开启的 `paste_html_as_md` 迁移为默认关闭
    #[serde(default)]
    pub paste_html_default_v2: bool,
    /// 代码块预览显示行号
    pub code_line_numbers: bool,
    /// 字数统计选项
    pub stats_exclude_code: bool,
    pub stats_exclude_syntax: bool,
    /// 自定义快捷键覆盖表（command_id -> shortcut 字符串，"" 表示解绑）
    pub keybindings: BTreeMap<String, String>,
    /// 最近打开/保存的文件记录（置顶项优先）
    #[serde(default)]
    pub recent_files: Vec<RecentFile>,
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
            paste_html_as_md: false,
            paste_html_default_v2: true,
            code_line_numbers: true,
            stats_exclude_code: false,
            stats_exclude_syntax: true,
            keybindings: BTreeMap::new(),
            recent_files: Vec::new(),
        }
    }
}

impl Settings {
    pub fn now_unix_secs() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    fn sort_recent_files(&mut self) {
        self.recent_files
            .sort_by(|a, b| b.pinned.cmp(&a.pinned).then_with(|| b.last_opened_unix.cmp(&a.last_opened_unix)));
    }

    fn truncate_recent_files(&mut self) {
        while self.recent_files.len() > MAX_RECENT_FILES {
            if let Some(idx) = self.recent_files.iter().rposition(|r| !r.pinned) {
                self.recent_files.remove(idx);
            } else {
                self.recent_files.pop();
            }
        }
    }

    pub fn record_recent_file(&mut self, path: &Path) {
        self.record_recent_file_at(path, Self::now_unix_secs());
    }

    pub fn record_recent_file_at(&mut self, path: &Path, now_unix: u64) {
        let clean = crate::winutil::clean_path(path);
        let mut pinned = false;
        if let Some(pos) = self
            .recent_files
            .iter()
            .position(|r| crate::sidebar::files::paths_eq(&r.path, &clean))
        {
            pinned = self.recent_files[pos].pinned;
            self.recent_files.remove(pos);
        }
        let entry = RecentFile { path: clean, pinned, last_opened_unix: now_unix };
        if pinned {
            self.recent_files.insert(0, entry);
        } else {
            let idx = self
                .recent_files
                .iter()
                .position(|r| !r.pinned)
                .unwrap_or(self.recent_files.len());
            self.recent_files.insert(idx, entry);
        }
        self.truncate_recent_files();
    }

    /// 会话恢复时为尚未收录的老标签补记一条最近记录，不打乱已有历史顺序。
    pub fn ensure_recent_seeded(&mut self, path: &Path) {
        let clean = crate::winutil::clean_path(path);
        if self
            .recent_files
            .iter()
            .any(|r| crate::sidebar::files::paths_eq(&r.path, &clean))
        {
            return;
        }
        let mtime_secs = std::fs::metadata(&clean)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or_else(Self::now_unix_secs);
        self.recent_files.push(RecentFile {
            path: clean,
            pinned: false,
            last_opened_unix: mtime_secs,
        });
        self.sort_recent_files();
        self.truncate_recent_files();
    }

    pub fn toggle_pin_recent(&mut self, path: &Path) {
        if let Some(item) = self
            .recent_files
            .iter_mut()
            .find(|r| crate::sidebar::files::paths_eq(&r.path, path))
        {
            item.pinned = !item.pinned;
            self.sort_recent_files();
        }
    }

    pub fn remove_recent(&mut self, path: &Path) {
        self.recent_files
            .retain(|r| !crate::sidebar::files::paths_eq(&r.path, path));
    }

    pub fn clear_unpinned_recents(&mut self) {
        self.recent_files.retain(|r| r.pinned);
    }

    pub fn rename_recent(&mut self, old: &Path, new: &Path) {
        let old_clean = crate::winutil::clean_path(old);
        let new_clean = crate::winutil::clean_path(new);
        for r in &mut self.recent_files {
            if crate::sidebar::files::paths_eq(&r.path, &old_clean) {
                r.path = new_clean.clone();
            } else if let Ok(rel) = r.path.strip_prefix(&old_clean) {
                r.path = new_clean.join(rel);
            }
        }
        let mut deduped: Vec<RecentFile> = Vec::with_capacity(self.recent_files.len());
        for item in self.recent_files.drain(..) {
            if let Some(existing) = deduped
                .iter_mut()
                .find(|d| crate::sidebar::files::paths_eq(&d.path, &item.path))
            {
                existing.pinned |= item.pinned;
                existing.last_opened_unix = existing.last_opened_unix.max(item.last_opened_unix);
            } else {
                deduped.push(item);
            }
        }
        self.recent_files = deduped;
        self.sort_recent_files();
    }

    pub fn delete_recent_prefix(&mut self, deleted: &Path) {
        let del_clean = crate::winutil::clean_path(deleted);
        let del_lower = del_clean.to_string_lossy().to_ascii_lowercase();
        let prefix_bs = format!("{del_lower}\\");
        let prefix_fs = format!("{del_lower}/");
        self.recent_files.retain(|r| {
            let p_lower = crate::winutil::clean_path(&r.path).to_string_lossy().to_ascii_lowercase();
            p_lower != del_lower && !p_lower.starts_with(&prefix_bs) && !p_lower.starts_with(&prefix_fs)
        });
    }

    /// 对持久化配置做一次性默认值迁移：旧版 `paste_html_as_md` 默认是 `true`，
    /// 若存档中尚无 `paste_html_default_v2` 标记，则将其重置为新默认值 `false`，
    /// 此后用户在设置里手动重新勾选时将正常保留。
    pub fn migrate_legacy_defaults(&mut self) {
        if !self.paste_html_default_v2 {
            self.paste_html_as_md = false;
            self.paste_html_default_v2 = true;
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

    #[test]
    fn paste_html_as_md_defaults_to_false_and_migrates_legacy_once() {
        let def = Settings::default();
        assert!(!def.paste_html_as_md);
        assert!(def.paste_html_default_v2);

        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert!(!empty.paste_html_as_md);

        let mut legacy: Settings = serde_json::from_str(r#"{ "paste_html_as_md": true }"#).unwrap();
        legacy.migrate_legacy_defaults();
        assert!(!legacy.paste_html_as_md);
        assert!(legacy.paste_html_default_v2);

        let mut opted_in: Settings =
            serde_json::from_str(r#"{ "paste_html_as_md": true, "paste_html_default_v2": true }"#).unwrap();
        opted_in.migrate_legacy_defaults();
        assert!(opted_in.paste_html_as_md);
        assert!(opted_in.paste_html_default_v2);
    }

    #[test]
    fn recent_files_dedup_pin_cap_and_clear() {
        let mut s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.recent_files.is_empty());

        let p_a = PathBuf::from(r"C:\notes\alpha.md");
        let p_b = PathBuf::from(r"C:\notes\beta.md");
        s.record_recent_file_at(&p_a, 100);
        s.record_recent_file_at(&p_b, 200);
        assert_eq!(s.recent_files.len(), 2);
        assert_eq!(s.recent_files[0].path, p_b);

        // 置顶 alpha 后保持在最前面，即使之后又打开 beta
        s.toggle_pin_recent(&p_a);
        assert!(s.recent_files[0].pinned);
        assert_eq!(s.recent_files[0].path, p_a);

        // 大小写不同路径应去重并保留置顶状态
        s.record_recent_file_at(Path::new(r"C:\Notes\ALPHA.md"), 300);
        assert_eq!(s.recent_files.len(), 2);
        assert!(s.recent_files[0].pinned);
        assert_eq!(s.recent_files[0].last_opened_unix, 300);

        // 超过 MAX_RECENT_FILES 时只裁剪未置顶尾部
        for i in 0..25 {
            s.record_recent_file_at(&PathBuf::from(format!(r"C:\notes\doc_{i}.md")), 400 + i as u64);
        }
        assert_eq!(s.recent_files.len(), MAX_RECENT_FILES);
        assert!(s.recent_files[0].pinned);

        s.remove_recent(&PathBuf::from(r"C:\notes\doc_24.md"));
        assert_eq!(s.recent_files.len(), MAX_RECENT_FILES - 1);

        s.clear_unpinned_recents();
        assert_eq!(s.recent_files.len(), 1);
        assert!(s.recent_files[0].pinned);
    }
}
