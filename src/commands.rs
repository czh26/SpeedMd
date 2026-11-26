//! 统一命令枚举与元数据。

use crate::keymap::Shortcut;
use eframe::egui::Key;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Command {
    NewTab,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PrevTab,
    New,
    Open,
    OpenFolder,
    Save,
    SaveAs,
    ImportHtml,
    ExportHtml,
    ExportHtmlFragment,
    Quit,
    ViewEdit,
    ViewSplit,
    ViewPreview,
    ToggleSourceMode,
    Find,
    Replace,
    FindNext,
    FindPrev,
    ToggleSidebar,
    SidebarFiles,
    SidebarArticles,
    SidebarOutline,
    InlineCode,
    CodeBlock,
    CopyAsMarkdown,
    CopyAsHtml,
    PastePlain,
    FontBigger,
    FontSmaller,
    FontReset,
    ToggleDayNight,
    Theme01,
    Theme02,
    Theme03,
    Theme04,
    Theme05,
    OpenShortcuts,
    OpenSettings,
    OpenManual,
}

impl Command {
    pub const ALL: &[Command] = &[
        Self::NewTab,
        Self::CloseTab,
        Self::ReopenClosedTab,
        Self::NextTab,
        Self::PrevTab,
        Self::New,
        Self::Open,
        Self::OpenFolder,
        Self::Save,
        Self::SaveAs,
        Self::ImportHtml,
        Self::ExportHtml,
        Self::ExportHtmlFragment,
        Self::Quit,
        Self::ViewEdit,
        Self::ViewSplit,
        Self::ViewPreview,
        Self::ToggleSourceMode,
        Self::Find,
        Self::Replace,
        Self::FindNext,
        Self::FindPrev,
        Self::ToggleSidebar,
        Self::SidebarFiles,
        Self::SidebarArticles,
        Self::SidebarOutline,
        Self::InlineCode,
        Self::CodeBlock,
        Self::CopyAsMarkdown,
        Self::CopyAsHtml,
        Self::PastePlain,
        Self::FontBigger,
        Self::FontSmaller,
        Self::FontReset,
        Self::ToggleDayNight,
        Self::Theme01,
        Self::Theme02,
        Self::Theme03,
        Self::Theme04,
        Self::Theme05,
        Self::OpenShortcuts,
        Self::OpenSettings,
        Self::OpenManual,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::NewTab => "new_tab",
            Self::CloseTab => "close_tab",
            Self::ReopenClosedTab => "reopen_closed_tab",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
            Self::New => "new",
            Self::Open => "open",
            Self::OpenFolder => "open_folder",
            Self::Save => "save",
            Self::SaveAs => "save_as",
            Self::ImportHtml => "import_html",
            Self::ExportHtml => "export_html",
            Self::ExportHtmlFragment => "export_html_fragment",
            Self::Quit => "quit",
            Self::ViewEdit => "view_edit",
            Self::ViewSplit => "view_split",
            Self::ViewPreview => "view_preview",
            Self::ToggleSourceMode => "toggle_source_mode",
            Self::Find => "find",
            Self::Replace => "replace",
            Self::FindNext => "find_next",
            Self::FindPrev => "find_prev",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::SidebarFiles => "sidebar_files",
            Self::SidebarArticles => "sidebar_articles",
            Self::SidebarOutline => "sidebar_outline",
            Self::InlineCode => "inline_code",
            Self::CodeBlock => "code_block",
            Self::CopyAsMarkdown => "copy_as_markdown",
            Self::CopyAsHtml => "copy_as_html",
            Self::PastePlain => "paste_plain",
            Self::FontBigger => "font_bigger",
            Self::FontSmaller => "font_smaller",
            Self::FontReset => "font_reset",
            Self::ToggleDayNight => "toggle_theme",
            Self::Theme01 => "theme_01",
            Self::Theme02 => "theme_02",
            Self::Theme03 => "theme_03",
            Self::Theme04 => "theme_04",
            Self::Theme05 => "theme_05",
            Self::OpenShortcuts => "open_shortcuts",
            Self::OpenSettings => "open_settings",
            Self::OpenManual => "open_manual",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NewTab => "新建标签页",
            Self::CloseTab => "关闭标签页",
            Self::ReopenClosedTab => "重新打开关闭的标签",
            Self::NextTab => "下一个标签页",
            Self::PrevTab => "上一个标签页",
            Self::New => "新建文档",
            Self::Open => "打开文件…",
            Self::OpenFolder => "打开文件夹…",
            Self::Save => "保存",
            Self::SaveAs => "另存为…",
            Self::ImportHtml => "导入 HTML…",
            Self::ExportHtml => "导出 HTML…",
            Self::ExportHtmlFragment => "导出 HTML 片段…",
            Self::Quit => "退出",
            Self::ViewEdit => "编辑视图",
            Self::ViewSplit => "分屏视图",
            Self::ViewPreview => "预览视图",
            Self::ToggleSourceMode => "切换源代码模式",
            Self::Find => "查找…",
            Self::Replace => "替换…",
            Self::FindNext => "查找下一个",
            Self::FindPrev => "查找上一个",
            Self::ToggleSidebar => "显示/隐藏侧边栏",
            Self::SidebarFiles => "侧边栏：文件树",
            Self::SidebarArticles => "侧边栏：文章列表",
            Self::SidebarOutline => "侧边栏：大纲",
            Self::InlineCode => "行内代码",
            Self::CodeBlock => "代码块",
            Self::CopyAsMarkdown => "复制为 Markdown",
            Self::CopyAsHtml => "复制为 HTML",
            Self::PastePlain => "纯文本粘贴",
            Self::FontBigger => "增大字号",
            Self::FontSmaller => "减小字号",
            Self::FontReset => "重置字号",
            Self::ToggleDayNight => "切换日/夜主题",
            Self::Theme01 => "选择主题 01：无界疾风",
            Self::Theme02 => "选择主题 02：白昼疾影",
            Self::Theme03 => "选择主题 03：半山书房",
            Self::Theme04 => "选择主题 04：黑金疾电",
            Self::Theme05 => "选择主题 05：钛晶流光",
            Self::OpenShortcuts => "快捷键设置…",
            Self::OpenSettings => "外观与编辑器设置…",
            Self::OpenManual => "语法说明书",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::NewTab
            | Self::CloseTab
            | Self::ReopenClosedTab
            | Self::NextTab
            | Self::PrevTab
            | Self::New
            | Self::Open
            | Self::OpenFolder
            | Self::Save
            | Self::SaveAs
            | Self::ImportHtml
            | Self::ExportHtml
            | Self::ExportHtmlFragment
            | Self::Quit => "文件与标签",
            Self::Find
            | Self::Replace
            | Self::FindNext
            | Self::FindPrev
            | Self::InlineCode
            | Self::CodeBlock
            | Self::CopyAsMarkdown
            | Self::CopyAsHtml
            | Self::PastePlain => "编辑与剪贴板",
            Self::ViewEdit
            | Self::ViewSplit
            | Self::ViewPreview
            | Self::ToggleSourceMode
            | Self::FontBigger
            | Self::FontSmaller
            | Self::FontReset => "视图与字号",
            Self::ToggleSidebar | Self::SidebarFiles | Self::SidebarArticles | Self::SidebarOutline => "侧边栏",
            Self::ToggleDayNight
            | Self::Theme01
            | Self::Theme02
            | Self::Theme03
            | Self::Theme04
            | Self::Theme05 => "外观与主题",
            Self::OpenShortcuts | Self::OpenSettings => "设置",
            Self::OpenManual => "帮助",
        }
    }

    pub fn default_shortcut(self) -> Option<Shortcut> {
        let ctrl = |key| Some(Shortcut::new(true, false, false, key));
        let ctrl_shift = |key| Some(Shortcut::new(true, true, false, key));
        match self {
            Self::NewTab => ctrl(Key::T),
            Self::CloseTab => ctrl(Key::W),
            Self::ReopenClosedTab => ctrl_shift(Key::T),
            Self::NextTab => ctrl(Key::Tab),
            Self::PrevTab => ctrl_shift(Key::Tab),
            Self::New => ctrl(Key::N),
            Self::Open => ctrl(Key::O),
            Self::OpenFolder => ctrl_shift(Key::O),
            Self::Save => ctrl(Key::S),
            Self::SaveAs => ctrl_shift(Key::S),
            Self::ImportHtml | Self::ExportHtml | Self::ExportHtmlFragment | Self::CopyAsMarkdown => None,
            Self::Quit => ctrl(Key::Q),
            Self::ViewEdit => ctrl(Key::Num1),
            Self::ViewSplit => ctrl(Key::Num2),
            Self::ViewPreview => ctrl(Key::Num3),
            Self::ToggleSourceMode => ctrl(Key::Slash),
            Self::Find => ctrl(Key::F),
            Self::Replace => ctrl(Key::H),
            Self::FindNext => Some(Shortcut::new(false, false, false, Key::F3)),
            Self::FindPrev => Some(Shortcut::new(false, true, false, Key::F3)),
            Self::ToggleSidebar => ctrl(Key::B),
            Self::SidebarFiles => ctrl_shift(Key::Num1),
            Self::SidebarArticles => ctrl_shift(Key::Num2),
            Self::SidebarOutline => ctrl_shift(Key::Num3),
            Self::InlineCode => ctrl(Key::Backtick),
            Self::CodeBlock => ctrl_shift(Key::Backtick),
            Self::CopyAsHtml => ctrl_shift(Key::C),
            Self::PastePlain => ctrl_shift(Key::V),
            Self::FontBigger => ctrl(Key::Equals),
            Self::FontSmaller => ctrl(Key::Minus),
            Self::FontReset => ctrl(Key::Num0),
            Self::ToggleDayNight => ctrl_shift(Key::D),
            Self::Theme01 | Self::Theme02 | Self::Theme03 | Self::Theme04 | Self::Theme05 => None,
            Self::OpenShortcuts => ctrl(Key::K),
            Self::OpenSettings => ctrl(Key::Comma),
            Self::OpenManual => Some(Shortcut::new(false, false, false, Key::F1)),
        }
    }

    pub fn enabled_on_manual(self) -> bool {
        !matches!(
            self,
            Self::Save
                | Self::SaveAs
                | Self::ExportHtml
                | Self::ExportHtmlFragment
                | Self::ViewEdit
                | Self::ViewSplit
                | Self::ViewPreview
                | Self::ToggleSourceMode
                | Self::Find
                | Self::Replace
                | Self::FindNext
                | Self::FindPrev
                | Self::InlineCode
                | Self::CodeBlock
                | Self::CopyAsMarkdown
                | Self::CopyAsHtml
                | Self::PastePlain
        )
    }
}
