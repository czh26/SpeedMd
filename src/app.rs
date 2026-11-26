//! MdPad 顶层应用状态、布局与统一命令分发。

use crate::commands::Command;
use crate::document::{Document, ParseConfig};
use crate::editor::{self, EditorConfig};
use crate::find::{self, FindAction};
use crate::fonts::{self, FontCatalog, FontChoice};
use crate::keymap::Keymap;
use crate::manual::{self, InsertMode, ManualAction, ManualState, ManualViewCfg, SettingDep};
use crate::markdown::{bold_family, ParseOptions};
use crate::preview::PreviewOptions;
use crate::settings::{Settings, SidebarTab, ViewMode};
use crate::sidebar::{SidebarAction, SidebarState};
use crate::stats::{self, StatsOptions};
use crate::tabs::{self, TabAction};
use crate::theme::{
    self, DividerStyle, StatusBarStyle, ThemeId, TopBarBottomLine, PANE_INSET, SIDEBAR_DEFAULT_W, SIDEBAR_MAX_W,
    SIDEBAR_MIN_W, STATUS_BAR_H, STATUS_PILL_H, TOP_BAR_H, TOP_PILL_H,
};
use crate::winutil;
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    pos2, vec2, Align, Align2, CursorIcon, FontId, Frame, Margin, Popup, PopupCloseBehavior, PopupKind, Rect,
    RectAlign, Sense, SetOpenCommand, Stroke, StrokeKind, UiBuilder,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

pub const APP_NAME: &str = "MdPad";

#[derive(Clone)]
enum ConfirmAction {
    CloseTab(u64),
    CloseBatch(Vec<u64>),
    Quit,
}

struct ShotRunner {
    path: PathBuf,
    frames: u32,
    commands: Vec<String>,
    ran_commands: bool,
}

struct BenchRunner {
    frame: u32,
    last_frame_start: Option<Instant>,
    cpu_ms: Vec<f64>,
    wall_ms: Vec<f64>,
    out_path: Option<PathBuf>,
}

impl BenchRunner {
    fn new(out_path: Option<PathBuf>) -> Self {
        Self {
            frame: 0,
            last_frame_start: None,
            cpu_ms: Vec::with_capacity(200),
            wall_ms: Vec::with_capacity(200),
            out_path,
        }
    }
}

pub struct MdApp {
    docs: Vec<Document>,
    active: usize,
    closed_stack: Vec<PathBuf>,
    settings: Settings,
    keymap: Keymap,
    title: String,
    confirm: Option<ConfirmAction>,
    allow_close: bool,
    error: Option<String>,
    editor_rect: Rect,
    preview_rect: Rect,
    font_catalog: Option<FontCatalog>,
    font_scan_rx: Option<mpsc::Receiver<FontCatalog>>,
    settings_open: bool,
    font_search: String,
    stats_popup_open: bool,
    sidebar: SidebarState,
    jobs: crate::jobs::JobHub,
    export_dialog_open: bool,
    export_follow_theme: bool,
    export_embed_images: bool,
    shortcuts_ui: crate::shortcuts_ui::ShortcutsUiState,
    manual: ManualState,
    last_doc_uid: Option<u64>,
    sidebar_width: f32,
    open_menu_once: bool,
    open_theme_menu_once: bool,
    shot: Option<ShotRunner>,
    bench: Option<BenchRunner>,
}

impl MdApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let shot_path = std::env::var_os("MDPAD_SCREENSHOT").map(PathBuf::from);
        let bench_typing = std::env::var("MDPAD_BENCH").as_deref() == Ok("typing");
        let is_test = shot_path.is_some() || bench_typing;
        if is_test {
            cc.egui_ctx.data_mut(|d| d.clear());
        }

        crate::fonts::setup(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| {
            o.zoom_with_keyboard = false;
            o.quit_shortcuts.clear();
        });

        let bench_out = std::env::var_os("MDPAD_BENCH_OUT").map(PathBuf::from);
        let test_cmds: Vec<String> = std::env::var("MDPAD_TEST_COMMANDS")
            .map(|s| s.split(';').map(str::trim).filter(|c| !c.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();

        let mut settings: Settings =
            cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();

        if is_test {
            settings = Settings::default();
            match std::env::var("MDPAD_MODE").as_deref() {
                Ok("edit") => settings.mode = ViewMode::Edit,
                Ok("preview") => settings.mode = ViewMode::Preview,
                Ok("split") => settings.mode = ViewMode::Split,
                _ if bench_typing => settings.mode = ViewMode::Edit,
                _ => {}
            }
            settings.sup_sub = std::env::var("MDPAD_SUPSUB").is_ok();
            if let Ok(folder) = std::env::var("MDPAD_FOLDER") {
                settings.workspace_folder = Some(winutil::clean_path(Path::new(&folder)));
            }
        }

        let env_theme = std::env::var("MDPAD_THEME")
            .ok()
            .and_then(|s| ThemeId::from_env_str(&s))
            .or_else(|| std::env::var("MDPAD_DARK").ok().map(|_| ThemeId::CarbonGold));
        if let Some(t) = env_theme {
            settings.theme = t;
        }
        theme::apply(&cc.egui_ctx, settings.theme);

        settings.ui_zoom = settings.ui_zoom.clamp(0.75, 2.0);
        cc.egui_ctx.set_zoom_factor(settings.ui_zoom);

        let keymap = Keymap::from_overrides(&settings.keybindings);
        let shot = shot_path.map(|path| ShotRunner { path, frames: 0, commands: test_cmds, ran_commands: false });
        let bench = bench_typing.then(|| BenchRunner::new(bench_out));
        let needs_custom_fonts = !settings.preview_font_family.is_empty()
            || !settings.editor_font_family.is_empty()
            || !settings.code_font_family.is_empty();
        let font_scan_rx = needs_custom_fonts.then(|| fonts::start_scan(cc.egui_ctx.clone()));
        let stats_popup_open = std::env::var("MDPAD_STATS_POPUP").is_ok();

        let mut app = Self {
            docs: Vec::new(),
            active: 0,
            closed_stack: Vec::new(),
            settings,
            keymap,
            title: String::new(),
            confirm: None,
            allow_close: false,
            error: None,
            editor_rect: Rect::NOTHING,
            preview_rect: Rect::NOTHING,
            font_catalog: None,
            font_scan_rx,
            settings_open: false,
            font_search: String::new(),
            stats_popup_open,
            sidebar: SidebarState::default(),
            jobs: crate::jobs::JobHub::default(),
            export_dialog_open: false,
            export_follow_theme: true,
            export_embed_images: false,
            shortcuts_ui: crate::shortcuts_ui::ShortcutsUiState::default(),
            manual: ManualState::default(),
            last_doc_uid: None,
            sidebar_width: SIDEBAR_DEFAULT_W,
            open_menu_once: false,
            open_theme_menu_once: false,
            shot,
            bench,
        };

        if bench_typing {
            app.docs.push(Document::from_file(PathBuf::from("bench_100k.md"), build_100k_bench_doc()));
            app.active = 0;
            return app;
        }

        let cli_args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
        if !cli_args.is_empty() {
            for p in cli_args {
                app.open_file(p);
            }
        } else if !is_test
            && app.settings.restore_tabs
            && (!app.settings.session_paths.is_empty() || app.settings.session_manual_open)
        {
            let paths = app.settings.session_paths.clone();
            let saved_active = app.settings.session_active;
            let restore_manual = app.settings.session_manual_open;
            let manual_active = app.settings.session_manual_active;
            for p in paths {
                if p.is_file() {
                    app.open_file_silent(p);
                }
            }
            if !app.docs.is_empty() {
                app.active = saved_active.min(app.docs.len() - 1);
                app.last_doc_uid = Some(app.docs[app.active].uid);
            }
            if restore_manual {
                app.manual.ensure_loaded();
                app.docs.push(Document::new_manual());
                if manual_active || app.docs.len() == 1 {
                    app.active = app.docs.len() - 1;
                    app.manual.focus_search = true;
                }
            }
        }

        if app.docs.is_empty() {
            app.docs.push(Document::new_empty());
            app.active = 0;
        }
        if !app.doc().is_manual {
            app.last_doc_uid = Some(app.doc().uid);
        }

        if let Ok(anchor) = std::env::var("MDPAD_ANCHOR") {
            app.docs[app.active].preview.scroll_to(anchor);
        }
        if let Ok(q) = std::env::var("MDPAD_FIND_QUERY") {
            let doc = &mut app.docs[app.active];
            doc.find.query = q;
            doc.find.open = true;
            if std::env::var("MDPAD_FIND_REGEX").is_ok() {
                doc.find.use_regex = true;
            }
            if std::env::var("MDPAD_FIND_REPLACE").is_ok() {
                doc.find.show_replace = true;
            }
        }
        if let Ok(q) = std::env::var("MDPAD_MANUAL_QUERY") {
            app.manual.query = q;
        }
        if let Ok(cat) = std::env::var("MDPAD_MANUAL_CATEGORY") {
            app.manual.scroll_to_category_name(&cat);
        }
        if let Ok(entry) = std::env::var("MDPAD_MANUAL_ENTRY") {
            app.manual.scroll_to_entry_id(&entry);
        }
        app
    }

    fn doc(&self) -> &Document {
        &self.docs[self.active]
    }

    fn doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.active]
    }

    fn idx_by_uid(&self, uid: u64) -> Option<usize> {
        self.docs.iter().position(|d| d.uid == uid)
    }

    fn parse_opts(&self) -> ParseOptions {
        ParseOptions { sup_sub: self.settings.sup_sub, front_matter: self.settings.front_matter }
    }

    fn font_choice(&self) -> FontChoice {
        FontChoice {
            preview: self.settings.preview_font_family.clone(),
            editor: self.settings.editor_font_family.clone(),
            code: self.settings.code_font_family.clone(),
        }
    }

    fn stats_opts(&self) -> StatsOptions {
        StatsOptions {
            exclude_code: self.settings.stats_exclude_code,
            exclude_syntax: self.settings.stats_exclude_syntax,
        }
    }

    fn ensure_font_scan(&mut self, ctx: &egui::Context) {
        if self.font_catalog.is_none() && self.font_scan_rx.is_none() {
            self.font_scan_rx = Some(fonts::start_scan(ctx.clone()));
        }
    }

    // ---------- 文件与标签操作 ----------

    fn read_doc_from_disk(path: &Path) -> Result<Document, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("无法打开文件：{}\n{e}", path.display()))?;
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        let text = String::from_utf8_lossy(bytes).into_owned().replace("\r\n", "\n");
        Ok(Document::from_file(winutil::clean_path(path), text))
    }

    fn open_file_silent(&mut self, path: PathBuf) {
        if let Ok(doc) = Self::read_doc_from_disk(&path) {
            self.push_or_activate_doc(doc);
        }
    }

    pub fn open_file(&mut self, path: PathBuf) {
        let clean = winutil::clean_path(&path);
        if let Some(idx) = self.find_open_tab(&clean) {
            self.active = idx;
            return;
        }
        match Self::read_doc_from_disk(&clean) {
            Ok(doc) => self.push_or_activate_doc(doc),
            Err(msg) => self.error = Some(msg),
        }
    }

    fn find_open_tab(&self, clean: &Path) -> Option<usize> {
        let target = clean.to_string_lossy().to_ascii_lowercase();
        self.docs.iter().position(|d| {
            d.path.as_ref().is_some_and(|p| p.to_string_lossy().to_ascii_lowercase() == target)
        })
    }

    fn push_or_activate_doc(&mut self, doc: Document) {
        if let Some(p) = &doc.path
            && let Some(idx) = self.find_open_tab(p)
        {
            self.active = idx;
            return;
        }
        if !self.docs.is_empty() && self.docs[self.active].can_reuse_as_blank() {
            self.docs[self.active] = doc;
        } else {
            self.docs.push(doc);
            self.active = self.docs.len() - 1;
        }
    }

    fn open_dialog(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown", "mdown", "txt"])
            .add_filter("所有文件", &["*"])
            .pick_files();
        if let Some(paths) = picked {
            for p in paths {
                self.open_file(p);
            }
        }
    }

    fn new_tab(&mut self) {
        self.docs.push(Document::new_empty());
        self.active = self.docs.len() - 1;
    }

    fn save_doc(&mut self, idx: usize) -> bool {
        if self.docs.get(idx).is_none_or(|d| d.is_manual) {
            return false;
        }
        if let Some(path) = self.docs[idx].path.clone() {
            self.write_doc(idx, path)
        } else {
            self.save_doc_as(idx)
        }
    }

    fn save_doc_as(&mut self, idx: usize) -> bool {
        if self.docs.get(idx).is_none_or(|d| d.is_manual) {
            return false;
        }
        let name = self.docs[idx]
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .or_else(|| self.docs[idx].title_hint.as_ref().map(|t| format!("{t}.md")))
            .unwrap_or_else(|| "未命名.md".into());
        let picked = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("所有文件", &["*"])
            .set_file_name(name)
            .save_file();
        match picked {
            Some(p) => self.write_doc(idx, winutil::clean_path(&p)),
            None => false,
        }
    }

    fn write_doc(&mut self, idx: usize, path: PathBuf) -> bool {
        if let Some(old_base) = self.docs[idx].base_dir_hint.take()
            && let Some(new_base) = path.parent()
        {
            let rebased = crate::import::rewrite_relative_image_paths(&self.docs[idx].text, &old_base, new_base);
            if rebased != self.docs[idx].text {
                self.docs[idx].text = rebased;
                self.docs[idx].bump_version();
            }
        }
        match std::fs::write(&path, self.docs[idx].text.as_bytes()) {
            Ok(()) => {
                self.docs[idx].path = Some(path);
                self.docs[idx].title_hint = None;
                self.docs[idx].dirty = false;
                true
            }
            Err(e) => {
                self.error = Some(format!("保存失败：{}\n{e}", path.display()));
                false
            }
        }
    }

    fn request_close_tab(&mut self, idx: usize) {
        if idx >= self.docs.len() {
            return;
        }
        if self.docs[idx].dirty {
            self.confirm = Some(ConfirmAction::CloseTab(self.docs[idx].uid));
        } else {
            self.force_close_idx(idx);
        }
    }

    fn request_close_batch(&mut self, indices: Vec<usize>) {
        let mut dirty_uids = Vec::new();
        let mut clean_uids = Vec::new();
        for idx in indices {
            if let Some(d) = self.docs.get(idx) {
                if d.dirty {
                    dirty_uids.push(d.uid);
                } else {
                    clean_uids.push(d.uid);
                }
            }
        }
        for uid in clean_uids {
            if let Some(idx) = self.idx_by_uid(uid) {
                self.force_close_idx(idx);
            }
        }
        if !dirty_uids.is_empty() {
            self.confirm = Some(ConfirmAction::CloseBatch(dirty_uids));
        }
    }

    fn force_close_idx(&mut self, idx: usize) {
        if idx >= self.docs.len() {
            return;
        }
        let removed = self.docs.remove(idx);
        if let Some(p) = removed.path {
            self.closed_stack.push(p);
        }
        if self.docs.is_empty() {
            self.docs.push(Document::new_empty());
            self.active = 0;
        } else if self.active >= self.docs.len() {
            self.active = self.docs.len() - 1;
        } else if idx < self.active {
            self.active -= 1;
        }
    }

    fn reopen_closed_tab(&mut self) {
        while let Some(path) = self.closed_stack.pop() {
            if path.is_file() {
                self.open_file(path);
                return;
            }
        }
    }

    fn sync_tabs_after_rename(&mut self, old: &Path, new: &Path) {
        let old_clean = winutil::clean_path(old);
        let new_clean = winutil::clean_path(new);
        for d in &mut self.docs {
            if let Some(p) = &d.path {
                if crate::sidebar::files::paths_eq(p, &old_clean) {
                    d.path = Some(new_clean.clone());
                } else if let Ok(rel) = p.strip_prefix(&old_clean) {
                    d.path = Some(new_clean.join(rel));
                }
            }
        }
    }

    fn sync_tabs_after_delete(&mut self, deleted: &Path) {
        let del_clean = winutil::clean_path(deleted);
        let mut to_close = Vec::new();
        for (i, d) in self.docs.iter_mut().enumerate() {
            let affected = d.path.as_ref().is_some_and(|p| {
                crate::sidebar::files::paths_eq(p, &del_clean) || p.starts_with(&del_clean)
            });
            if affected {
                if d.dirty {
                    d.path = None;
                } else {
                    to_close.push(i);
                }
            }
        }
        for idx in to_close.into_iter().rev() {
            self.docs.remove(idx);
            if self.docs.is_empty() {
                self.docs.push(Document::new_empty());
                self.active = 0;
            } else if self.active >= self.docs.len() {
                self.active = self.docs.len() - 1;
            } else if idx < self.active {
                self.active -= 1;
            }
        }
    }

    fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.docs.len() || to >= self.docs.len() || from == to {
            return;
        }
        let active_uid = self.docs[self.active].uid;
        let doc = self.docs.remove(from);
        self.docs.insert(to, doc);
        if let Some(new_active) = self.idx_by_uid(active_uid) {
            self.active = new_active;
        }
    }

    fn request_quit(&mut self, ctx: &egui::Context) {
        if self.docs.iter().any(|d| d.dirty) {
            self.confirm = Some(ConfirmAction::Quit);
        } else {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn set_theme(&mut self, ctx: &egui::Context, id: ThemeId) {
        self.settings.theme = id;
        if id.is_dark() {
            self.settings.dark_theme = id;
        } else {
            self.settings.light_theme = id;
        }
        theme::apply(ctx, id);
        ctx.request_repaint();
    }

    fn toggle_day_night(&mut self, ctx: &egui::Context) {
        let target = if self.settings.theme.is_dark() {
            if self.settings.light_theme.is_dark() {
                ThemeId::ZenHorizon
            } else {
                self.settings.light_theme
            }
        } else if self.settings.dark_theme.is_dark() {
            self.settings.dark_theme
        } else {
            ThemeId::CarbonGold
        };
        self.set_theme(ctx, target);
    }

    // ---------- 源代码模式与查找替换 ----------

    fn toggle_source_mode(&mut self, ctx: &egui::Context) {
        let theme = self.settings.theme;
        let font_size = self.settings.preview_font_size;
        let opts = self.parse_opts();
        let cur_mode = self.settings.mode;

        if cur_mode == ViewMode::Edit {
            let cur_byte = editor::current_cursor_byte(ctx, self.doc());
            let line_height = self.settings.line_height;
            let para_spacing = self.settings.paragraph_spacing;
            let doc = self.doc_mut();
            let cfg = ParseConfig { theme, font_size, line_height, para_spacing, opts: &opts, immediate: true };
            doc.ensure_parsed(ctx, &cfg);
            if let Some((parsed, _, _)) = doc.parsed.get() {
                let block_idx = parsed
                    .block_ranges
                    .iter()
                    .rposition(|r| r.start <= cur_byte)
                    .unwrap_or(0);
                doc.preview.scroll_to_block(block_idx);
            }
            let prev = doc.view_before_source.take().unwrap_or(ViewMode::Split);
            self.settings.mode = prev;
        } else {
            let line_height = self.settings.line_height;
            let para_spacing = self.settings.paragraph_spacing;
            let doc = self.doc_mut();
            let cfg = ParseConfig { theme, font_size, line_height, para_spacing, opts: &opts, immediate: true };
            doc.ensure_parsed(ctx, &cfg);
            let top_idx = doc.preview.top_block_idx;
            if let Some((parsed, _, _)) = doc.parsed.get()
                && let Some(r) = parsed.block_ranges.get(top_idx)
            {
                doc.pending_selection = Some(r.start..r.start);
                doc.scroll_to_selection = true;
            }
            doc.view_before_source = Some(cur_mode);
            self.settings.mode = ViewMode::Edit;
        }
    }

    fn open_find(&mut self, ctx: &egui::Context, replace: bool) {
        if self.settings.mode == ViewMode::Preview {
            self.settings.mode = ViewMode::Split;
        }
        let sel = editor::current_selection_chars(ctx, self.doc().uid, &self.doc().text);
        let doc = self.doc_mut();
        if sel.start < sel.end {
            let b0 = editor::char_to_byte(&doc.text, sel.start);
            let b1 = editor::char_to_byte(&doc.text, sel.end);
            let s = &doc.text[b0..b1];
            if !s.contains('\n') && s.len() <= 200 {
                doc.find.query = s.to_string();
                doc.find.invalidate();
            }
        }
        doc.find.open = true;
        if replace {
            doc.find.show_replace = true;
        }
        doc.find.focus_query = true;
        doc.find.ensure_updated(ctx, &doc.text, doc.version, true);
    }

    fn step_find(&mut self, ctx: &egui::Context, forward: bool) {
        if self.settings.mode == ViewMode::Preview {
            self.settings.mode = ViewMode::Split;
        }
        let doc = self.doc_mut();
        if !doc.find.open {
            doc.find.open = true;
            doc.find.focus_query = true;
            doc.find.ensure_updated(ctx, &doc.text, doc.version, true);
            return;
        }
        doc.find.ensure_updated(ctx, &doc.text, doc.version, true);
        if let Some(r) = doc.find.step(forward) {
            doc.pending_selection = Some(r);
            doc.scroll_to_selection = true;
        }
    }

    fn replace_current(&mut self, ctx: &egui::Context) {
        let doc = self.doc_mut();
        doc.find.ensure_updated(ctx, &doc.text, doc.version, true);
        let Some(range) = doc.find.current_range() else { return };
        let spec = doc.find.spec();
        let rep = doc.find.replacement.clone();
        let Ok((new_text, new_range)) = find::replace_one(&doc.text, range, &rep, &spec) else { return };

        editor::apply_edit(ctx, doc, |text, _| {
            *text = new_text;
            let c0 = editor::byte_to_char(text, new_range.start);
            let c1 = editor::byte_to_char(text, new_range.end);
            CCursorRange::two(CCursor::new(c0), CCursor::new(c1))
        });

        doc.find.ensure_updated(ctx, &doc.text, doc.version, true);
        doc.find.select_near_offset(new_range.end);
        if let Some(next_r) = doc.find.current_range() {
            doc.pending_selection = Some(next_r);
            doc.scroll_to_selection = true;
        }
    }

    fn replace_all(&mut self, ctx: &egui::Context) {
        let doc = self.doc_mut();
        doc.find.ensure_updated(ctx, &doc.text, doc.version, true);
        if doc.find.matches.is_empty() {
            return;
        }
        let spec = doc.find.spec();
        let rep = doc.find.replacement.clone();
        let Ok((new_text, count)) = find::replace_all(&doc.text, &doc.find.matches, &rep, &spec) else { return };
        if count == 0 {
            return;
        }
        editor::apply_edit(ctx, doc, |text, before| {
            *text = new_text;
            let max_c = text.chars().count();
            let c = before.primary.index.0.min(max_c);
            CCursorRange::one(CCursor::new(c))
        });
        doc.find.ensure_updated(ctx, &doc.text, doc.version, true);
    }

    fn open_manual(&mut self) {
        self.manual.ensure_loaded();
        self.manual.focus_search = true;
        if let Some(idx) = self.docs.iter().position(|d| d.is_manual) {
            self.active = idx;
        } else {
            self.manual.reset_scroll_if_idle();
            self.docs.push(Document::new_manual());
            self.active = self.docs.len() - 1;
        }
    }

    fn insert_from_manual(&mut self, ctx: &egui::Context, code: &str, mode: InsertMode) {
        let target_idx = self
            .last_doc_uid
            .and_then(|uid| self.idx_by_uid(uid))
            .filter(|&i| !self.docs[i].is_manual)
            .or_else(|| self.docs.iter().rposition(|d| !d.is_manual))
            .unwrap_or_else(|| {
                self.docs.push(Document::new_empty());
                self.docs.len() - 1
            });
        if self.settings.mode == ViewMode::Preview {
            self.settings.mode = ViewMode::Split;
        }
        self.active = target_idx;
        let sel = editor::current_selection_chars(ctx, self.docs[target_idx].uid, &self.docs[target_idx].text);
        let patch = manual::build_insert_patch(&self.docs[target_idx].text, sel, code, mode);
        let cur_chars = patch.new_sel_chars.start;
        editor::apply_text_patch(ctx, &mut self.docs[target_idx], patch);
        let b_cur = editor::char_to_byte(&self.docs[target_idx].text, cur_chars);
        self.docs[target_idx].pending_selection = Some(b_cur..b_cur);
        self.docs[target_idx].scroll_to_selection = true;
        let ed_id = editor::editor_id(self.docs[target_idx].uid);
        ctx.memory_mut(|m| m.request_focus(ed_id));
        self.last_doc_uid = Some(self.docs[target_idx].uid);
    }

    // ---------- 命令分发与字号调节 ----------

    fn adjust_font(&mut self, ctx: &egui::Context, delta: Option<f32>) {
        let pointer = ctx.pointer_latest_pos();
        let target_preview = if self.doc().is_manual {
            true
        } else {
            match self.settings.mode {
                ViewMode::Edit => false,
                ViewMode::Preview => true,
                ViewMode::Split => pointer.is_some_and(|p| self.preview_rect.contains(p)),
            }
        };
        let size = if target_preview {
            &mut self.settings.preview_font_size
        } else {
            &mut self.settings.editor_font_size
        };
        match delta {
            Some(d) => *size = (*size + d).clamp(10.0, 32.0),
            None => *size = if target_preview { 16.0 } else { 15.0 },
        }
    }

    pub fn run_command(&mut self, ctx: &egui::Context, cmd: Command) {
        if self.doc().is_manual && !cmd.enabled_on_manual() {
            return;
        }
        match cmd {
            Command::NewTab | Command::New => self.new_tab(),
            Command::CloseTab => self.request_close_tab(self.active),
            Command::ReopenClosedTab => self.reopen_closed_tab(),
            Command::NextTab => {
                if !self.docs.is_empty() {
                    self.active = (self.active + 1) % self.docs.len();
                }
            }
            Command::PrevTab => {
                if !self.docs.is_empty() {
                    self.active = (self.active + self.docs.len() - 1) % self.docs.len();
                }
            }
            Command::Open => self.open_dialog(),
            Command::OpenFolder => {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    self.settings.workspace_folder = Some(winutil::clean_path(&folder));
                    self.settings.sidebar_open = true;
                    self.settings.sidebar_tab = SidebarTab::Files;
                }
            }
            Command::Save => {
                self.save_doc(self.active);
            }
            Command::SaveAs => {
                self.save_doc_as(self.active);
            }
            Command::Quit => self.request_quit(ctx),
            Command::ViewEdit => self.settings.mode = ViewMode::Edit,
            Command::ViewSplit => self.settings.mode = ViewMode::Split,
            Command::ViewPreview => self.settings.mode = ViewMode::Preview,
            Command::ToggleSourceMode => self.toggle_source_mode(ctx),
            Command::Find => self.open_find(ctx, false),
            Command::Replace => self.open_find(ctx, true),
            Command::FindNext => self.step_find(ctx, true),
            Command::FindPrev => self.step_find(ctx, false),
            Command::InlineCode => editor::toggle_inline_code(ctx, self.doc_mut()),
            Command::CodeBlock => editor::insert_code_block(ctx, self.doc_mut()),
            Command::ToggleSidebar => self.settings.sidebar_open = !self.settings.sidebar_open,
            Command::SidebarFiles => {
                self.settings.sidebar_open = true;
                self.settings.sidebar_tab = SidebarTab::Files;
            }
            Command::SidebarArticles => {
                self.settings.sidebar_open = true;
                self.settings.sidebar_tab = SidebarTab::Articles;
            }
            Command::SidebarOutline => {
                self.settings.sidebar_open = true;
                self.settings.sidebar_tab = SidebarTab::Outline;
            }
            Command::FontBigger => self.adjust_font(ctx, Some(1.0)),
            Command::FontSmaller => self.adjust_font(ctx, Some(-1.0)),
            Command::FontReset => self.adjust_font(ctx, None),
            Command::ToggleDayNight => self.toggle_day_night(ctx),
            Command::Theme01 => self.set_theme(ctx, ThemeId::ZenHorizon),
            Command::Theme02 => self.set_theme(ctx, ThemeId::ScarletStreak),
            Command::Theme03 => self.set_theme(ctx, ThemeId::AtelierContrast),
            Command::Theme04 => self.set_theme(ctx, ThemeId::CarbonGold),
            Command::Theme05 => self.set_theme(ctx, ThemeId::TitaniumGlass),
            Command::OpenSettings => {
                self.settings_open = true;
                self.ensure_font_scan(ctx);
            }
            Command::CopyAsMarkdown => {
                let doc = self.doc();
                let md = editor::selected_text(ctx, doc).unwrap_or_else(|| doc.text.clone());
                if let Err(e) = crate::clipboard::set_text(&md) {
                    self.error = Some(e);
                }
            }
            Command::CopyAsHtml => {
                let doc = self.doc();
                let md = editor::selected_text(ctx, doc).unwrap_or_else(|| doc.text.clone());
                let html = crate::export::to_html_fragment(&md, doc.base_dir(), self.settings.sup_sub);
                if let Err(e) = crate::clipboard::set_html(&html, &md) {
                    self.error = Some(e);
                }
            }
            Command::PastePlain => {
                if self.settings.mode != ViewMode::Preview
                    && let Some(text) = crate::clipboard::get_text()
                {
                    let doc = self.doc_mut();
                    editor::insert_text(ctx, doc, &text);
                }
            }
            Command::ImportHtml => {
                if let Some(src) = rfd::FileDialog::new()
                    .add_filter("HTML 网页", &["html", "htm"])
                    .add_filter("所有文件", &["*"])
                    .pick_file()
                {
                    let src = winutil::clean_path(&src);
                    self.jobs.spawn(ctx.clone(), move || match crate::import::import_html_file(&src) {
                        Ok(doc) => crate::jobs::JobResult::ImportDone(doc),
                        Err(e) => crate::jobs::JobResult::Failed(e),
                    });
                }
            }
            Command::ExportHtml => {
                self.export_dialog_open = true;
            }
            Command::ExportHtmlFragment => {
                self.start_export_dialog(ctx, false);
            }
            Command::OpenShortcuts => {
                self.shortcuts_ui.open = true;
            }
            Command::OpenManual => self.open_manual(),
        }
    }

    // ---------- 输入处理 ----------

    fn handle_input(&mut self, ctx: &egui::Context) {
        if self.shortcuts_ui.capture_input(ctx, &mut self.keymap) {
            return;
        }
        let cmds = ctx.input_mut(|i| self.keymap.match_and_consume(&mut i.events));
        for cmd in cmds {
            self.run_command(ctx, cmd);
        }

        // 拖放文件（支持一次拖入多个）
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        for p in dropped {
            self.open_file(p);
        }

        // 关闭窗口时若有未保存文档，拦截并弹窗确认
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.docs.iter().any(|d| d.dirty) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(ConfirmAction::Quit);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let doc = self.doc();
        let name = doc.display_name();
        let title = format!("{}{name} - {APP_NAME}", if doc.dirty { "● " } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // ---------- 界面 ----------

    fn menu_btn(&self, ui: &mut egui::Ui, cmd: Command) -> bool {
        let enabled = !self.doc().is_manual || cmd.enabled_on_manual();
        let btn = egui::Button::new(cmd.label()).shortcut_text(self.keymap.label(cmd));
        ui.add_enabled(enabled, btn).clicked()
    }

    fn top_bar_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let spec = theme::current(&ctx);
        let (top_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOP_BAR_H), Sense::hover());

        let left_zone_w = if self.settings.sidebar_open {
            self.sidebar_width.clamp(SIDEBAR_MIN_W, SIDEBAR_MAX_W)
        } else {
            SIDEBAR_DEFAULT_W
        };
        let split_left_active = spec.top_bar_left_bg.is_some() && self.settings.sidebar_open;
        if let Some(left_bg) = spec.top_bar_left_bg.filter(|_| split_left_active) {
            let left_bg_r = Rect::from_min_max(top_rect.min, pos2(top_rect.left() + left_zone_w, top_rect.max.y));
            let right_bg_r = Rect::from_min_max(pos2(top_rect.left() + left_zone_w, top_rect.min.y), top_rect.max);
            ui.painter().rect_filled(left_bg_r, 0.0, left_bg);
            ui.painter().rect_filled(right_bg_r, 0.0, spec.top_bar_bg);
        } else {
            ui.painter().rect_filled(top_rect, 0.0, spec.top_bar_bg);
        }

        // 底缘线条（01 隐藏、02 红→金 1.5px 渐隐速度线、03–05 1px 结构线）
        match spec.top_bar_bottom_line {
            TopBarBottomLine::None => {}
            TopBarBottomLine::Solid { color, left_override } => {
                let y = top_rect.bottom() - 0.5;
                if split_left_active && let Some(lc) = left_override {
                    let div_x = top_rect.left() + left_zone_w;
                    ui.painter().hline(top_rect.left()..=div_x, y, Stroke::new(1.0, lc));
                    ui.painter().hline(div_x..=top_rect.right(), y, Stroke::new(1.0, color));
                } else {
                    ui.painter().hline(top_rect.x_range(), y, Stroke::new(1.0, color));
                }
            }
            TopBarBottomLine::Gradient { left, right, width } => {
                theme::paint_horizontal_gradient_line(
                    ui.painter(),
                    top_rect.left(),
                    top_rect.right(),
                    top_rect.bottom() - width,
                    width,
                    left,
                    right,
                );
            }
        }

        // 左区（220px 或侧栏等宽）：徽标 + MdPad 字标 + ≡ 菜单钮
        let left_rect = Rect::from_min_size(top_rect.min, vec2(left_zone_w, TOP_BAR_H));
        let left_fg = if split_left_active { spec.top_bar_left_fg } else { spec.text_fg };
        let left_hover = if split_left_active { spec.sidebar.hover_bg } else { spec.hover_bg };

        let badge_rect = Rect::from_min_size(pos2(left_rect.left() + 14.0, left_rect.center().y - 11.0), vec2(22.0, 22.0));
        theme::paint_brand_badge(ui.painter(), badge_rect, spec);
        ui.painter().text(
            pos2(badge_rect.right() + 8.0, left_rect.center().y),
            Align2::LEFT_CENTER,
            APP_NAME,
            FontId::new(15.5, bold_family()),
            left_fg,
        );

        let menu_btn_rect = Rect::from_min_size(pos2(left_rect.right() - 36.0, left_rect.center().y - 14.0), vec2(28.0, 28.0));
        let menu_resp = ui.interact(menu_btn_rect, ui.make_persistent_id("top_hamburger_menu"), Sense::click());
        if menu_resp.hovered() {
            ui.painter().rect_filled(menu_btn_rect, 6.0, left_hover);
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let bar_stroke = Stroke::new(1.6, left_fg);
        for dy in [-4.5_f32, 0.0, 4.5] {
            ui.painter().line_segment(
                [
                    pos2(menu_btn_rect.center().x - 6.5, menu_btn_rect.center().y + dy),
                    pos2(menu_btn_rect.center().x + 6.5, menu_btn_rect.center().y + dy),
                ],
                bar_stroke,
            );
        }

        let menu_open_cmd = if self.open_menu_once {
            self.open_menu_once = false;
            Some(SetOpenCommand::Bool(true))
        } else if menu_resp.clicked() {
            Some(SetOpenCommand::Toggle)
        } else {
            None
        };

        let mut pending_cmd: Option<Command> = None;
        let mut picked_theme: Option<ThemeId> = None;
        Popup::menu(&menu_resp)
            .gap(4.0)
            .open_memory(menu_open_cmd)
            .show(|ui| {
                ui.set_min_width(148.0);
                ui.menu_button("文件", |ui| {
                    for cmd in [Command::NewTab, Command::New, Command::Open, Command::OpenFolder] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    for cmd in [Command::Save, Command::SaveAs, Command::CloseTab, Command::ReopenClosedTab] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    for cmd in [Command::ImportHtml, Command::ExportHtml, Command::ExportHtmlFragment] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if self.menu_btn(ui, Command::Quit) {
                        pending_cmd = Some(Command::Quit);
                        ui.close();
                    }
                });

                ui.menu_button("编辑", |ui| {
                    for cmd in [Command::Find, Command::Replace, Command::FindNext, Command::FindPrev] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    for cmd in [Command::InlineCode, Command::CodeBlock] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    for cmd in [Command::PastePlain, Command::CopyAsMarkdown, Command::CopyAsHtml] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                });

                ui.menu_button("视图", |ui| {
                    for cmd in [Command::ViewEdit, Command::ViewSplit, Command::ViewPreview, Command::ToggleSourceMode] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    for cmd in [Command::ToggleSidebar, Command::SidebarFiles, Command::SidebarArticles, Command::SidebarOutline] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    for cmd in [Command::NextTab, Command::PrevTab] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                    ui.separator();
                    for cmd in [Command::FontBigger, Command::FontSmaller, Command::FontReset] {
                        if self.menu_btn(ui, cmd) {
                            pending_cmd = Some(cmd);
                            ui.close();
                        }
                    }
                });

                ui.menu_button("主题", |ui| {
                    if self.menu_btn(ui, Command::ToggleDayNight) {
                        pending_cmd = Some(Command::ToggleDayNight);
                        ui.close();
                    }
                    ui.separator();
                    for id in ThemeId::ALL {
                        let selected = self.settings.theme == id;
                        if ui.selectable_label(selected, id.full_label()).clicked() {
                            picked_theme = Some(id);
                            ui.close();
                        }
                    }
                });

                egui::containers::menu::SubMenuButton::new("设置")
                    .config(egui::containers::menu::MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside))
                    .ui(ui, |ui| {
                        if self.menu_btn(ui, Command::OpenSettings) {
                            pending_cmd = Some(Command::OpenSettings);
                            ui.close();
                        }
                        if self.menu_btn(ui, Command::OpenShortcuts) {
                            pending_cmd = Some(Command::OpenShortcuts);
                            ui.close();
                        }
                        ui.separator();
                        ui.label(egui::RichText::new("Markdown 扩展语法").strong());
                        ui.checkbox(&mut self.settings.sup_sub, "上标 / 下标（^上标^、~下标~）")
                            .on_hover_text("开启后单个 ~ 表示下标，删除线请使用 ~~文本~~");
                        ui.checkbox(&mut self.settings.front_matter, "预览中显示 YAML Front Matter");
                        ui.checkbox(&mut self.settings.code_line_numbers, "代码块显示行号");
                        ui.separator();
                        ui.label(egui::RichText::new("编辑器与标签").strong());
                        ui.checkbox(&mut self.settings.auto_pair, "自动闭合括号与引号");
                        ui.add_enabled(
                            self.settings.auto_pair,
                            egui::Checkbox::new(&mut self.settings.auto_pair_in_code, "代码块内也自动补全"),
                        );
                        ui.checkbox(&mut self.settings.restore_tabs, "启动时恢复上次打开的标签");
                    });

                ui.menu_button("帮助", |ui| {
                    if self.menu_btn(ui, Command::OpenManual) {
                        pending_cmd = Some(Command::OpenManual);
                        ui.close();
                    }
                });
            });
        menu_resp.on_hover_text("菜单（文件 / 编辑 / 视图 / 主题 / 设置 / 帮助）");

        // 右区：定宽分段模式胶囊（编辑 | 分屏 | 预览）+ ⚡ 主题切换钮
        let mode_w = 144.0;
        let theme_w = 28.0;
        let right_pad = 12.0;
        let gap = 8.0;
        let theme_rect = Rect::from_min_size(
            pos2(top_rect.right() - right_pad - theme_w, top_rect.center().y - TOP_PILL_H * 0.5),
            vec2(theme_w, TOP_PILL_H),
        );
        let mode_rect = Rect::from_min_size(
            pos2(theme_rect.left() - gap - mode_w, top_rect.center().y - TOP_PILL_H * 0.5),
            vec2(mode_w, TOP_PILL_H),
        );

        self.paint_mode_switch(ui, mode_rect, spec);
        self.theme_toggle_button_at(ui, theme_rect);

        // 中区：水平滚动多文档标签栏（03 下激活标签直通 top_rect.bottom()）
        let center_rect = Rect::from_min_max(
            pos2(left_rect.right() + 8.0, top_rect.top()),
            pos2(mode_rect.left() - 10.0, top_rect.bottom()),
        );
        if center_rect.width() > 24.0 {
            let mut center_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(center_rect)
                    .layout(egui::Layout::left_to_right(Align::Center)),
            );
            center_ui.set_clip_rect(ui.clip_rect().intersect(center_rect));
            self.tab_bar(&mut center_ui, top_rect.bottom());
        }

        if let Some(cmd) = pending_cmd {
            self.run_command(&ctx, cmd);
        }
        if let Some(id) = picked_theme {
            self.set_theme(&ctx, id);
        }
    }

    fn paint_mode_switch(&mut self, ui: &mut egui::Ui, rect: Rect, spec: &theme::ThemeSpec) {
        let ms = &spec.mode_switch;
        let is_manual = self.doc().is_manual;
        ui.painter().add(ms.container_shadow.as_shape(rect, ms.outer_radius));
        ui.painter().rect_filled(rect, ms.outer_radius, ms.container_bg);
        ui.painter().rect_stroke(rect, ms.outer_radius, ms.container_stroke, StrokeKind::Inside);

        let items = [
            (ViewMode::Edit, "编辑"),
            (ViewMode::Split, "分屏"),
            (ViewMode::Preview, "预览"),
        ];
        let seg_w = rect.width() / 3.0;
        for (idx, (mode_val, label)) in items.iter().enumerate() {
            let seg_r = Rect::from_min_size(pos2(rect.left() + idx as f32 * seg_w, rect.top()), vec2(seg_w, rect.height()));
            let sense = if is_manual { Sense::hover() } else { Sense::click() };
            let resp = ui.interact(seg_r, ui.make_persistent_id(("mode_seg", label)), sense);
            let active = !is_manual && self.settings.mode == *mode_val;

            let inner_r = seg_r.shrink(2.5);
            if active {
                ui.painter().rect_filled(inner_r, ms.inner_radius, ms.active_bg);
                ui.painter().rect_stroke(inner_r, ms.inner_radius, ms.active_stroke, StrokeKind::Inside);
            } else if !is_manual && resp.hovered() {
                ui.painter().rect_filled(inner_r, ms.inner_radius, spec.hover_bg);
            }
            if !is_manual && resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if idx > 0 {
                let prev_active = !is_manual && self.settings.mode == items[idx - 1].0;
                if !active && !prev_active {
                    ui.painter().vline(
                        seg_r.left(),
                        (rect.top() + 6.0)..=(rect.bottom() - 6.0),
                        Stroke::new(1.0, spec.border),
                    );
                }
            }

            let fg = if is_manual {
                spec.disabled_fg
            } else if active {
                ms.active_fg
            } else {
                ms.inactive_fg
            };
            let font = if active { FontId::new(12.5, bold_family()) } else { FontId::proportional(12.5) };
            ui.painter().text(seg_r.center(), Align2::CENTER_CENTER, label, font, fg);
            if !is_manual && resp.clicked() {
                self.settings.mode = *mode_val;
            }
        }
    }

    fn theme_toggle_button_at(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        let spec = theme::current(&ctx);
        let ms = &spec.mode_switch;
        let resp = ui.interact(rect, ui.make_persistent_id("top_theme_toggle"), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let bg = if resp.is_pointer_button_down_on() {
            spec.active_bg
        } else if resp.hovered() {
            spec.hover_bg
        } else {
            ms.container_bg
        };
        ui.painter().add(ms.container_shadow.as_shape(rect, ms.outer_radius));
        ui.painter().rect_filled(rect, ms.outer_radius, bg);
        ui.painter().rect_stroke(rect, ms.outer_radius, ms.container_stroke, StrokeKind::Inside);
        theme::paint_bolt(ui.painter(), rect.shrink(6.0), spec.accent);

        if resp.clicked() {
            self.toggle_day_night(&ctx);
        }

        let open_cmd = if self.open_theme_menu_once {
            self.open_theme_menu_once = false;
            Some(SetOpenCommand::Bool(true))
        } else if resp.secondary_clicked() {
            Some(SetOpenCommand::Toggle)
        } else {
            None
        };

        let mut picked: Option<ThemeId> = None;
        Popup::from_response(&resp)
            .kind(PopupKind::Menu)
            .layout(egui::Layout::top_down_justified(Align::Min))
            .style(egui::containers::menu::menu_style)
            .align(RectAlign::BOTTOM_END)
            .gap(4.0)
            .open_memory(open_cmd)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(256.0);
                ui.label(egui::RichText::new("选择界面主题").strong());
                ui.weak("左键 ⚡ 快速切换日/夜 · 右键打开列表");
                ui.separator();
                for id in ThemeId::ALL {
                    let s = id.spec();
                    let active = id == self.settings.theme;
                    let (row_r, row_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                    if row_resp.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        ui.painter().rect_filled(row_r, 5.0, spec.hover_bg);
                    } else if active {
                        ui.painter().rect_filled(row_r, 5.0, spec.active_bg);
                    }
                    let sw = Rect::from_min_size(pos2(row_r.left() + 6.0, row_r.center().y - 7.0), vec2(20.0, 14.0));
                    let left_sw = Rect::from_min_max(sw.min, pos2(sw.center().x, sw.max.y));
                    let right_sw = Rect::from_min_max(pos2(sw.center().x, sw.min.y), sw.max);
                    ui.painter().rect_filled(left_sw, 2.0, s.sidebar.bg);
                    ui.painter().rect_filled(right_sw, 2.0, s.accent);
                    ui.painter().rect_stroke(sw, 3.0, Stroke::new(1.0, spec.border), StrokeKind::Inside);

                    let mark = if active { "✓ " } else { "" };
                    let text_col = if active { spec.strong_fg } else { spec.text_fg };
                    ui.painter().text(
                        pos2(sw.right() + 8.0, row_r.center().y),
                        Align2::LEFT_CENTER,
                        format!("{mark}{}", id.short_label()),
                        FontId::proportional(13.0),
                        text_col,
                    );
                    ui.painter().text(
                        pos2(row_r.right() - 6.0, row_r.center().y),
                        Align2::RIGHT_CENTER,
                        id.mode_tag(),
                        FontId::proportional(11.5),
                        spec.muted_fg,
                    );
                    if row_resp.clicked() {
                        picked = Some(id);
                        ui.close();
                    }
                    row_resp.on_hover_text(format!("{} ({}) · {}", id.cn_name(), id.en_name(), id.mode_tag()));
                }
            });
        if let Some(id) = picked {
            self.set_theme(&ctx, id);
        }

        resp.on_hover_text(format!(
            "当前主题：{}\n左键：日/夜热切换（{}）\n右键：选择 5 套主题",
            self.settings.theme.full_label(),
            self.keymap.label(Command::ToggleDayNight),
        ));
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui, top_bar_bottom: f32) {
        let Some(action) = tabs::show(ui, top_bar_bottom, &self.docs, self.active) else { return };
        match action {
            TabAction::Select(i) => {
                if i < self.docs.len() {
                    self.active = i;
                }
            }
            TabAction::Close(i) => self.request_close_tab(i),
            TabAction::CloseOthers(keep) => {
                let others: Vec<usize> = (0..self.docs.len()).filter(|&i| i != keep).collect();
                self.request_close_batch(others);
            }
            TabAction::CloseRight(idx) => {
                let right: Vec<usize> = ((idx + 1)..self.docs.len()).collect();
                self.request_close_batch(right);
            }
            TabAction::NewTab => self.new_tab(),
            TabAction::Save(i) => {
                if i < self.docs.len() {
                    self.save_doc(i);
                }
            }
            TabAction::CopyPath(p) => ui.ctx().copy_text(p.display().to_string()),
            TabAction::Reveal(p) => winutil::reveal_in_explorer(&p),
            TabAction::Move { from, to } => self.move_tab(from, to),
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let spec = theme::current(&ctx);
        let (bar_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), STATUS_BAR_H), Sense::hover());

        let (base_bg, top_border) = match spec.status_bar {
            StatusBarStyle::FloatingPill { bar_bg, top_border, .. } => (bar_bg, top_border),
            StatusBarStyle::FlushTelemetry { bar_bg, top_border, .. } => (bar_bg, Some(top_border)),
        };

        let left_zone_w = self.sidebar_width.clamp(SIDEBAR_MIN_W, SIDEBAR_MAX_W);
        let split_left_active = spec.top_bar_left_bg.is_some() && self.settings.sidebar_open;
        if let Some(left_bg) = spec.top_bar_left_bg.filter(|_| split_left_active) {
            let l_rect = Rect::from_min_max(bar_rect.min, pos2(bar_rect.left() + left_zone_w, bar_rect.max.y));
            let r_rect = Rect::from_min_max(pos2(bar_rect.left() + left_zone_w, bar_rect.min.y), bar_rect.max);
            ui.painter().rect_filled(l_rect, 0.0, left_bg);
            ui.painter().rect_filled(r_rect, 0.0, base_bg);
        } else {
            ui.painter().rect_filled(bar_rect, 0.0, base_bg);
        }
        if let Some(tb) = top_border {
            ui.painter().hline(bar_rect.x_range(), bar_rect.top() + 0.5, Stroke::new(1.0, tb));
        }

        let left_fg = if split_left_active { spec.sidebar.muted_fg } else { spec.muted_fg };
        let doc = self.doc();

        if doc.is_manual {
            let left_text = "内置语法说明书 · 只读 · Esc 清空搜索 · 支持复制与一键插入到当前文档";
            let right_text = format!("{} 个分类 · {} 条语法", self.manual.category_count(), self.manual.entry_count());
            ui.painter().text(
                pos2(bar_rect.left() + 12.0, bar_rect.center().y),
                Align2::LEFT_CENTER,
                left_text,
                FontId::proportional(11.5),
                left_fg,
            );
            ui.painter().text(
                pos2(bar_rect.right() - 12.0, bar_rect.center().y),
                Align2::RIGHT_CENTER,
                right_text,
                FontId::proportional(11.5),
                spec.muted_fg,
            );
            return;
        }

        let full_path = doc.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "未保存的新文件".into());
        let dirty = doc.dirty;
        let full = doc.stats.full;
        let sel = doc.stats.sel;
        let ms = if self.shot.is_some() { 0.2 } else { doc.parsed.last_parse_ms().max(0.1) };

        let stats_text = if let Some(s) = sel {
            format!(
                "{ms:.1} ms · 已选 {} 字 / 共 {} 字 · {} 行",
                stats::format_thousands(s.words),
                stats::format_thousands(full.words),
                stats::format_thousands(full.lines)
            )
        } else {
            format!(
                "{ms:.1} ms · {} 行 · {} 字 · {} 字符",
                stats::format_thousands(full.lines),
                stats::format_thousands(full.words),
                stats::format_thousands(full.chars_with_spaces)
            )
        };

        let pill_font = FontId::proportional(11.5);
        let text_galley = ui.fonts_mut(|f| f.layout_no_wrap(stats_text.clone(), pill_font.clone(), spec.text_fg));
        let bolt_w = 12.0;
        let pill_pad_x = 10.0;
        let pill_w = pill_pad_x * 2.0 + bolt_w + 5.0 + text_galley.size().x;
        let pill_rect = Rect::from_min_size(
            pos2(bar_rect.right() - 12.0 - pill_w, bar_rect.center().y - STATUS_PILL_H * 0.5),
            vec2(pill_w, STATUS_PILL_H),
        );

        let left_max_x = if split_left_active {
            (bar_rect.left() + left_zone_w - 8.0).min(pill_rect.left() - 12.0)
        } else {
            pill_rect.left() - 12.0
        }
        .max(bar_rect.left());
        let left_clip = Rect::from_min_max(bar_rect.min, pos2(left_max_x, bar_rect.max.y));
        let avail_left_w = (left_max_x - (bar_rect.left() + 12.0) - if dirty { 62.0 } else { 4.0 }).max(24.0);
        let display_path = if split_left_active
            && ui.fonts_mut(|f| f.layout_no_wrap(full_path.clone(), pill_font.clone(), left_fg)).size().x > avail_left_w
            && let Some(p) = &doc.path
        {
            let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let with_parent = p
                .parent()
                .and_then(|par| par.file_name())
                .map(|s| format!("…\\{}\\{name}", s.to_string_lossy()))
                .unwrap_or_else(|| name.clone());
            if ui.fonts_mut(|f| f.layout_no_wrap(with_parent.clone(), pill_font.clone(), left_fg)).size().x <= avail_left_w {
                with_parent
            } else {
                format!("…\\{name}")
            }
        } else {
            full_path.clone()
        };

        let clipped = ui.painter().with_clip_rect(left_clip);
        let path_galley = ui.fonts_mut(|f| f.layout_no_wrap(display_path, pill_font.clone(), left_fg));
        let path_x = bar_rect.left() + 12.0;
        clipped.galley(pos2(path_x, bar_rect.center().y - path_galley.size().y * 0.5), path_galley.clone(), left_fg);
        if dirty {
            clipped.text(
                pos2(path_x + path_galley.size().x + 10.0, bar_rect.center().y),
                Align2::LEFT_CENTER,
                "● 未保存",
                pill_font.clone(),
                spec.semantic.dirty_dot,
            );
        }
        if doc.path.is_some() {
            ui.interact(left_clip, ui.make_persistent_id("status_path_hover"), Sense::hover())
                .on_hover_text(full_path);
        }

        let (pill_fg, bolt_col) = match spec.status_bar {
            StatusBarStyle::FloatingPill {
                pill_bg,
                pill_fg,
                bolt_color,
                pill_stroke,
                pill_shadow,
                ..
            } => {
                ui.painter().add(pill_shadow.as_shape(pill_rect, 11));
                ui.painter().rect_filled(pill_rect, 11.0, pill_bg);
                ui.painter().rect_stroke(pill_rect, 11.0, pill_stroke, StrokeKind::Inside);
                (pill_fg, bolt_color)
            }
            StatusBarStyle::FlushTelemetry { bolt_color, text_color, .. } => (text_color, bolt_color),
        };

        let bolt_rect = Rect::from_min_size(
            pos2(pill_rect.left() + pill_pad_x, pill_rect.center().y - 6.0),
            vec2(10.0, 12.0),
        );
        theme::paint_bolt(ui.painter(), bolt_rect, bolt_col);
        ui.painter().text(
            pos2(bolt_rect.right() + 5.0, pill_rect.center().y),
            Align2::LEFT_CENTER,
            stats_text,
            pill_font,
            pill_fg,
        );

        let r = ui.interact(pill_rect, ui.make_persistent_id("status_stats_pill"), Sense::click());
        if r.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if r.on_hover_text("点击查看详细字数统计").clicked() {
            self.stats_popup_open = !self.stats_popup_open;
        }
    }

    fn editor_pane(&mut self, ui: &mut egui::Ui) {
        self.editor_rect = ui.max_rect();
        let cfg = EditorConfig {
            font_size: self.settings.editor_font_size,
            max_content_width: self.settings.max_content_width,
            auto_pair: self.settings.auto_pair,
            auto_pair_in_code: self.settings.auto_pair_in_code,
            paste_html_as_md: self.settings.paste_html_as_md,
            base_dir: self.doc().base_dir().map(Path::to_path_buf),
        };
        if let Some(cmd) = editor::show(ui, self.doc_mut(), &cfg) {
            let ctx = ui.ctx().clone();
            self.run_command(&ctx, cmd);
        }
    }

    fn preview_pane(&mut self, ui: &mut egui::Ui) {
        self.preview_rect = ui.max_rect();
        let theme = self.settings.theme;
        let font_size = self.settings.preview_font_size;
        let line_height = self.settings.line_height;
        let para_spacing = self.settings.paragraph_spacing;
        let opts = self.parse_opts();
        let code_line_numbers = self.settings.code_line_numbers;
        let max_content_width = self.settings.max_content_width;
        let ctx = ui.ctx().clone();

        let actions = {
            let doc = self.doc_mut();
            let pcfg = ParseConfig { theme, font_size, line_height, para_spacing, opts: &opts, immediate: true };
            doc.ensure_parsed(&ctx, &pcfg);
            let base = doc.base_dir().map(Path::to_path_buf);
            if let Some((parsed_doc, th, key)) = doc.parsed.get() {
                let preview_opts = PreviewOptions {
                    base_dir: base.as_deref(),
                    code_line_numbers,
                    max_content_width,
                    allow_local_open: true,
                };
                doc.preview.ui(ui, doc.uid, parsed_doc, th, key, preview_opts)
            } else {
                Default::default()
            }
        };

        if let Some(off) = actions.toggle_task {
            self.doc_mut().toggle_task(off);
        }
        if let Some(p) = actions.open_file {
            self.open_file(p);
        }
    }

    fn manual_pane(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let has_modal = self.settings_open
            || self.shortcuts_ui.open
            || self.confirm.is_some()
            || self.error.is_some()
            || self.export_dialog_open
            || self.stats_popup_open;
        let cfg = ManualViewCfg {
            editor_font_size: self.settings.editor_font_size,
            editor_font_family: &self.settings.editor_font_family,
            preview_font_size: self.settings.preview_font_size,
            line_height: self.settings.line_height,
            para_spacing: self.settings.paragraph_spacing,
            sup_sub: self.settings.sup_sub,
            front_matter: self.settings.front_matter,
            code_line_numbers: self.settings.code_line_numbers,
            max_content_width: self.settings.max_content_width,
            allow_esc_clear: !has_modal,
        };
        if let Some(act) = self.manual.show(ui, &cfg) {
            match act {
                ManualAction::Insert { code, mode } => self.insert_from_manual(&ctx, &code, mode),
                ManualAction::EnableSetting(dep) => match dep {
                    SettingDep::SupSub => self.settings.sup_sub = true,
                    SettingDep::FrontMatter => self.settings.front_matter = true,
                },
            }
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm.clone() else { return };

        match action {
            ConfirmAction::CloseTab(uid) => {
                let Some(idx) = self.idx_by_uid(uid) else {
                    self.confirm = None;
                    return;
                };
                let name = self.docs[idx].display_name();
                let mut choice = None;
                egui::Window::new("未保存的修改")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(format!("文档「{name}」有未保存的修改，是否保存？"));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("保存").clicked() {
                                choice = Some(0);
                            }
                            if ui.button("不保存").clicked() {
                                choice = Some(1);
                            }
                            if ui.button("取消").clicked() {
                                choice = Some(2);
                            }
                        });
                    });
                match choice {
                    Some(0) => {
                        self.confirm = None;
                        if self.save_doc(idx)
                            && let Some(cur) = self.idx_by_uid(uid)
                        {
                            self.force_close_idx(cur);
                        }
                    }
                    Some(1) => {
                        self.confirm = None;
                        self.force_close_idx(idx);
                    }
                    Some(_) => self.confirm = None,
                    None => {}
                }
            }
            ConfirmAction::CloseBatch(uids) => {
                let names: Vec<String> =
                    uids.iter().filter_map(|&u| self.idx_by_uid(u).map(|i| self.docs[i].display_name())).collect();
                if names.is_empty() {
                    self.confirm = None;
                    return;
                }
                let mut choice = None;
                egui::Window::new("未保存的修改")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("以下文档有未保存的修改：");
                        ui.add_space(4.0);
                        for n in &names {
                            ui.label(format!("  ● {n}"));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("全部保存").clicked() {
                                choice = Some(0);
                            }
                            if ui.button("全部不保存").clicked() {
                                choice = Some(1);
                            }
                            if ui.button("取消").clicked() {
                                choice = Some(2);
                            }
                        });
                    });
                match choice {
                    Some(0) => {
                        self.confirm = None;
                        for uid in uids {
                            if let Some(idx) = self.idx_by_uid(uid) {
                                if !self.save_doc(idx) {
                                    return;
                                }
                                if let Some(cur) = self.idx_by_uid(uid) {
                                    self.force_close_idx(cur);
                                }
                            }
                        }
                    }
                    Some(1) => {
                        self.confirm = None;
                        for uid in uids {
                            if let Some(idx) = self.idx_by_uid(uid) {
                                self.force_close_idx(idx);
                            }
                        }
                    }
                    Some(_) => self.confirm = None,
                    None => {}
                }
            }
            ConfirmAction::Quit => {
                let dirty_list: Vec<(u64, String)> =
                    self.docs.iter().filter(|d| d.dirty).map(|d| (d.uid, d.display_name())).collect();
                if dirty_list.is_empty() {
                    self.confirm = None;
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                let mut choice = None;
                egui::Window::new("退出前保存")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("以下文档有未保存的修改，退出前是否保存？");
                        ui.add_space(4.0);
                        for (_, n) in &dirty_list {
                            ui.label(format!("  ● {n}"));
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("全部保存").clicked() {
                                choice = Some(0);
                            }
                            if ui.button("全部不保存").clicked() {
                                choice = Some(1);
                            }
                            if ui.button("取消").clicked() {
                                choice = Some(2);
                            }
                        });
                    });
                match choice {
                    Some(0) => {
                        self.confirm = None;
                        for (uid, _) in dirty_list {
                            if let Some(idx) = self.idx_by_uid(uid)
                                && !self.save_doc(idx)
                            {
                                return;
                            }
                        }
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Some(1) => {
                        self.confirm = None;
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Some(_) => self.confirm = None,
                    None => {}
                }
            }
        }
    }

    fn error_dialog(&mut self, ctx: &egui::Context) {
        let Some(msg) = self.error.clone() else { return };
        egui::Window::new("错误")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(msg);
                if ui.button("确定").clicked() {
                    self.error = None;
                }
            });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        let mut fonts_changed = false;
        let all_fonts = self.font_catalog.as_ref().map(FontCatalog::all_names).unwrap_or_default();
        let mono_fonts = self.font_catalog.as_ref().map(FontCatalog::mono_names).unwrap_or_default();
        let scanning = self.font_scan_rx.is_some();

        egui::Window::new("外观与编辑器设置")
            .open(&mut open)
            .resizable(true)
            .default_width(480.0)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                    ui.label(egui::RichText::new("主题与外观").strong());
                    let mut chosen_theme = self.settings.theme;
                    egui::Grid::new("settings_theme").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("当前界面主题");
                        egui::ComboBox::from_id_salt("settings_theme_combo")
                            .width(260.0)
                            .selected_text(chosen_theme.full_label())
                            .show_ui(ui, |ui| {
                                for id in ThemeId::ALL {
                                    ui.selectable_value(&mut chosen_theme, id, id.full_label());
                                }
                            });
                        ui.end_row();

                        ui.label("日间热切换偏好");
                        egui::ComboBox::from_id_salt("settings_light_pair_combo")
                            .width(260.0)
                            .selected_text(self.settings.light_theme.full_label())
                            .show_ui(ui, |ui| {
                                for id in [ThemeId::ZenHorizon, ThemeId::ScarletStreak, ThemeId::AtelierContrast] {
                                    ui.selectable_value(&mut self.settings.light_theme, id, id.full_label());
                                }
                            });
                        ui.end_row();

                        ui.label("夜间热切换偏好");
                        egui::ComboBox::from_id_salt("settings_dark_pair_combo")
                            .width(260.0)
                            .selected_text(self.settings.dark_theme.full_label())
                            .show_ui(ui, |ui| {
                                for id in [ThemeId::CarbonGold, ThemeId::TitaniumGlass] {
                                    ui.selectable_value(&mut self.settings.dark_theme, id, id.full_label());
                                }
                            });
                        ui.end_row();
                    });
                    if chosen_theme != self.settings.theme {
                        self.set_theme(ctx, chosen_theme);
                    }
                    ui.separator();

                    ui.label(egui::RichText::new("字体").strong());
                    if scanning {
                        ui.weak("正在后台扫描系统字体…");
                    } else if let Some(cat) = &self.font_catalog {
                        ui.horizontal(|ui| {
                            ui.label("筛选");
                            ui.add(egui::TextEdit::singleline(&mut self.font_search).desired_width(180.0).hint_text("输入字体名…"));
                            ui.weak(format!("共 {} 个字体族", cat.families.len()));
                        });
                    }
                    let filter = self.font_search.trim().to_ascii_lowercase();
                    let filter_list = |src: &[String]| -> Vec<String> {
                        if filter.is_empty() {
                            src.to_vec()
                        } else {
                            src.iter().filter(|n| n.to_ascii_lowercase().contains(&filter)).cloned().collect()
                        }
                    };
                    let filtered_all = filter_list(&all_fonts);
                    let filtered_mono = if mono_fonts.is_empty() { &filtered_all } else { &filter_list(&mono_fonts) };

                    egui::Grid::new("settings_fonts").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("预览正文字体");
                        fonts_changed |= font_combo(
                            ui,
                            "preview_font_combo",
                            &mut self.settings.preview_font_family,
                            &filtered_all,
                            "系统默认（Segoe UI + 微软雅黑）",
                        );
                        ui.end_row();

                        ui.label("编辑器字体");
                        fonts_changed |= font_combo(
                            ui,
                            "editor_font_combo",
                            &mut self.settings.editor_font_family,
                            &filtered_all,
                            "系统默认（Consolas + 微软雅黑）",
                        );
                        ui.end_row();

                        ui.label("代码块字体");
                        fonts_changed |= font_combo(
                            ui,
                            "code_font_combo",
                            &mut self.settings.code_font_family,
                            filtered_mono,
                            "系统默认（Consolas + 微软雅黑）",
                        );
                        ui.end_row();
                    });

                    ui.separator();
                    ui.label(egui::RichText::new("排版与缩放").strong());
                    egui::Grid::new("settings_layout").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("编辑器字号");
                        ui.add(egui::Slider::new(&mut self.settings.editor_font_size, 10.0..=32.0).step_by(1.0));
                        ui.end_row();
                        ui.label("预览字号");
                        ui.add(egui::Slider::new(&mut self.settings.preview_font_size, 10.0..=32.0).step_by(1.0));
                        ui.end_row();
                        ui.label("预览行高");
                        ui.add(egui::Slider::new(&mut self.settings.line_height, 1.2..=2.2).step_by(0.05));
                        ui.end_row();
                        ui.label("预览段距");
                        ui.add(egui::Slider::new(&mut self.settings.paragraph_spacing, 0.3..=1.5).step_by(0.05));
                        ui.end_row();
                        ui.label("界面缩放");
                        if ui.add(egui::Slider::new(&mut self.settings.ui_zoom, 0.75..=2.0).step_by(0.05)).changed() {
                            ctx.set_zoom_factor(self.settings.ui_zoom);
                        }
                        ui.end_row();
                    });

                    let mut limit_width = self.settings.max_content_width > 0.0;
                    if ui.checkbox(&mut limit_width, "限制最大阅读/编辑行宽（居中显示）").changed() {
                        self.settings.max_content_width = if limit_width { 820.0 } else { 0.0 };
                    }
                    if limit_width {
                        ui.add(
                            egui::Slider::new(&mut self.settings.max_content_width, 560.0..=1400.0)
                                .step_by(20.0)
                                .text("px"),
                        );
                    }

                    ui.separator();
                    ui.label(egui::RichText::new("Markdown 与编辑器").strong());
                    ui.checkbox(&mut self.settings.sup_sub, "上标 / 下标（^上标^、~下标~）");
                    ui.checkbox(&mut self.settings.front_matter, "预览中显示 YAML Front Matter");
                    ui.checkbox(&mut self.settings.code_line_numbers, "代码块显示行号");
                    ui.checkbox(&mut self.settings.auto_pair, "自动闭合括号与引号");
                    ui.add_enabled(
                        self.settings.auto_pair,
                        egui::Checkbox::new(&mut self.settings.auto_pair_in_code, "代码块内也自动补全"),
                    );
                    ui.checkbox(&mut self.settings.paste_html_as_md, "粘贴网页/富文本时自动转为 Markdown");
                    ui.checkbox(&mut self.settings.restore_tabs, "启动时恢复上次打开的标签");

                    ui.separator();
                    ui.label(egui::RichText::new("字数统计").strong());
                    ui.checkbox(&mut self.settings.stats_exclude_code, "统计时排除代码块与行内代码");
                    ui.checkbox(&mut self.settings.stats_exclude_syntax, "统计时忽略 Markdown 标记符号");

                    ui.separator();
                    if ui.button("自定义快捷键… (Ctrl+K)").clicked() {
                        self.shortcuts_ui.open = true;
                    }
                });
            });
        self.settings_open = open;

        if fonts_changed {
            let choice = self.font_choice();
            fonts::apply_choice(ctx, self.font_catalog.as_ref(), &choice);
        }
    }

    fn start_export_dialog(&mut self, ctx: &egui::Context, full_document: bool) {
        let doc = self.doc();
        let stem = doc
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .or_else(|| doc.title_hint.clone())
            .unwrap_or_else(|| "export".into());
        let suffix = if full_document { ".html" } else { ".fragment.html" };
        let default_name = format!("{stem}{suffix}");

        let Some(picked) = rfd::FileDialog::new()
            .add_filter("HTML 网页", &["html", "htm"])
            .add_filter("所有文件", &["*"])
            .set_file_name(default_name)
            .save_file()
        else {
            return;
        };

        let out_path = winutil::clean_path(&picked);
        let md = doc.text.clone();
        let export_theme = if full_document && self.export_follow_theme {
            self.settings.theme
        } else {
            ThemeId::ZenHorizon
        };
        let opts = crate::export::ExportOptions {
            full_document,
            theme: export_theme,
            embed_images_base64: full_document && self.export_embed_images,
            base_dir: doc.base_dir().map(Path::to_path_buf),
            sup_sub: self.settings.sup_sub,
            fallback_title: stem,
        };
        self.jobs.spawn(ctx.clone(), move || {
            let html = crate::export::export_markdown(&md, &opts);
            match std::fs::write(&out_path, html.as_bytes()) {
                Ok(()) => crate::jobs::JobResult::ExportDone { path: out_path },
                Err(e) => crate::jobs::JobResult::Failed(format!("导出 HTML 失败：{}\n{e}", out_path.display())),
            }
        });
    }

    fn export_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.export_dialog_open {
            return;
        }
        let mut open = self.export_dialog_open;
        let mut do_export = false;
        let mut cancel = false;

        egui::Window::new("导出完整 HTML")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("页面配色").strong());
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.export_follow_theme, true, "跟随当前主题");
                    ui.radio_value(&mut self.export_follow_theme, false, "固定为浅色主题（01 无界疾风）");
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new("图片资源").strong());
                ui.checkbox(&mut self.export_embed_images, "将本地图片转为 Base64 内嵌到 HTML")
                    .on_hover_text("开启后可生成单文件 HTML，方便离线分享；关闭则保留相对路径。");
                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("选择保存路径并导出…").clicked() {
                        do_export = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });

        self.export_dialog_open = open && !cancel && !do_export;
        if do_export {
            self.start_export_dialog(ctx, true);
        }
    }

    fn stats_popup_window(&mut self, ctx: &egui::Context) {
        if !self.stats_popup_open {
            return;
        }
        let mut open = self.stats_popup_open;
        let doc = self.doc();
        let (title_note, s) = match doc.stats.sel {
            Some(sel) => ("当前选区", sel),
            None => ("全文", doc.stats.full),
        };
        egui::Window::new("字数统计")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::RIGHT_BOTTOM, [-12.0, -34.0])
            .show(ctx, |ui| {
                ui.weak(format!("统计范围：{title_note}（已排除 Front Matter）"));
                ui.add_space(4.0);
                egui::Grid::new("stats_detail_grid").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                    ui.label("总字数（中文 + 英文词）");
                    ui.strong(stats::format_thousands(s.words));
                    ui.end_row();
                    ui.label("中日韩字符");
                    ui.label(stats::format_thousands(s.cjk_chars));
                    ui.end_row();
                    ui.label("英文词数");
                    ui.label(stats::format_thousands(s.en_words));
                    ui.end_row();
                    ui.label("字符数（含空格）");
                    ui.label(stats::format_thousands(s.chars_with_spaces));
                    ui.end_row();
                    ui.label("字符数（不含空格）");
                    ui.label(stats::format_thousands(s.chars_no_spaces));
                    ui.end_row();
                    ui.label("行数");
                    ui.label(stats::format_thousands(s.lines));
                    ui.end_row();
                    ui.label("段落数");
                    ui.label(stats::format_thousands(s.paragraphs));
                    ui.end_row();
                    ui.label("预计阅读时长");
                    ui.label(stats::format_read_time(s.read_minutes, s.words));
                    ui.end_row();
                });
                ui.separator();
                ui.checkbox(&mut self.settings.stats_exclude_code, "排除代码块");
                ui.checkbox(&mut self.settings.stats_exclude_syntax, "忽略 Markdown 标记");
            });
        self.stats_popup_open = open;
    }

    fn self_test_screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else { return };
        shot.frames += 1;
        let frames = shot.frames;
        let commands = if !shot.ran_commands && frames == 4 {
            shot.ran_commands = true;
            Some(shot.commands.clone())
        } else {
            None
        };
        if frames == 20 {
            let (path, ctx2) = (shot.path.clone(), ctx.clone());
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let _ = image::save_buffer(&path, img.as_raw(), w as u32, h as u32, image::ColorType::Rgba8);
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if frames < 20 {
            ctx.request_repaint();
        }
        if let Some(cmds) = commands {
            for id in cmds {
                if id == "open_menu" {
                    self.open_menu_once = true;
                } else if id == "open_theme_menu" {
                    self.open_theme_menu_once = true;
                } else if let Some(cmd) = Command::from_id(&id) {
                    self.run_command(ctx, cmd);
                }
            }
        }
    }

    fn bench_pre_frame(&mut self) -> Option<Instant> {
        let b = self.bench.as_mut()?;
        let now = Instant::now();
        b.frame += 1;
        if b.frame > 5 && b.frame <= 205 {
            if let Some(prev) = b.last_frame_start {
                b.wall_ms.push(now.duration_since(prev).as_secs_f64() * 1000.0);
            }
            b.last_frame_start = Some(now);
            let ch = if b.frame.is_multiple_of(2) { "a" } else { "字" };
            let doc = self.doc_mut();
            let pos = doc.text.len() / 2;
            let mut safe = pos;
            while safe > 0 && !doc.text.is_char_boundary(safe) {
                safe -= 1;
            }
            doc.text.insert_str(safe, ch);
            doc.bump_version();
            return Some(now);
        }
        None
    }

    fn bench_post_frame(&mut self, ctx: &egui::Context, cpu_start: Option<Instant>) {
        let chars = self.doc().text.chars().count();
        let Some(b) = self.bench.as_mut() else { return };
        if let Some(start) = cpu_start {
            b.cpu_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        if b.frame < 206 {
            ctx.request_repaint();
        } else if b.frame == 206 {
            let report = format_bench_report(chars, &b.cpu_ms, &b.wall_ms);
            if let Some(p) = &b.out_path {
                let _ = std::fs::write(p, &report);
            }
            println!("{report}");
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn font_combo(
    ui: &mut egui::Ui,
    id_salt: &str,
    selected: &mut String,
    names: &[String],
    default_label: &str,
) -> bool {
    let mut changed = false;
    let display = if selected.is_empty() { default_label } else { selected.as_str() };
    egui::ComboBox::from_id_salt(id_salt)
        .width(260.0)
        .selected_text(display)
        .show_ui(ui, |ui| {
            if ui.selectable_label(selected.is_empty(), default_label).clicked() && !selected.is_empty() {
                selected.clear();
                changed = true;
            }
            for name in names {
                if ui.selectable_label(selected == name, name).clicked() && selected != name {
                    *selected = name.clone();
                    changed = true;
                }
            }
        });
    changed
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn format_bench_report(chars: usize, cpu: &[f64], wall: &[f64]) -> String {
    let mut cpu_s = cpu.to_vec();
    let mut wall_s = wall.to_vec();
    cpu_s.sort_by(f64::total_cmp);
    wall_s.sort_by(f64::total_cmp);
    let avg = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    format!(
        "MDPAD_BENCH=typing chars={} samples={}\ncpu_ms:  avg={:.2} p50={:.2} p95={:.2}\nwall_ms: avg={:.2} p50={:.2} p95={:.2}\n",
        chars,
        cpu.len(),
        avg(cpu),
        percentile(&cpu_s, 0.50),
        percentile(&cpu_s, 0.95),
        avg(wall),
        percentile(&wall_s, 0.50),
        percentile(&wall_s, 0.95),
    )
}

fn build_100k_bench_doc() -> String {
    let chunk = "## 性能测试章节\n\n这是一段用于测试 **10万字** Markdown 编辑器输入性能的样本，包含 `行内代码`、[示例链接](https://example.com) 与常规中英文混排段落。\n\n- 列表项一：Rust + egui 渲染管线\n- 列表项二：按版本号缓存语法高亮\n\n";
    let mut s = String::with_capacity(140_000);
    while s.chars().count() < 100_000 {
        s.push_str(chunk);
    }
    s
}

impl eframe::App for MdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let cpu_start = self.bench_pre_frame();
        let ctx = ui.ctx().clone();
        let ctx = &ctx;

        if let Some(rx) = &self.font_scan_rx
            && let Ok(cat) = rx.try_recv()
        {
            self.font_catalog = Some(cat);
            self.font_scan_rx = None;
            let choice = self.font_choice();
            fonts::apply_choice(ctx, self.font_catalog.as_ref(), &choice);
        }

        let polled = self.jobs.poll();
        for imp in polled.imports {
            self.push_or_activate_doc(Document::from_imported(imp.title, imp.markdown, imp.base_dir));
        }
        if let Some(e) = polled.error {
            self.error = Some(e);
        }

        let theme_before_input = self.settings.theme;
        self.handle_input(ctx);
        if self.settings.theme != theme_before_input {
            *ui.style_mut() = (*ctx.global_style()).clone();
        }
        self.update_title(ctx);
        if !self.doc().is_manual {
            self.last_doc_uid = Some(self.doc().uid);
        }

        let stats_opts = self.stats_opts();
        if !self.doc().is_manual {
            let sel_chars = editor::current_selection_chars(ctx, self.doc().uid, &self.doc().text);
            let doc = self.doc_mut();
            if doc.find.open {
                doc.find.ensure_updated(ctx, &doc.text, doc.version, false);
            }
            doc.stats.ensure_full(ctx, &doc.text, doc.version, stats_opts, doc.last_edit_at);
            let sel_bytes = (sel_chars.start < sel_chars.end).then(|| {
                editor::char_to_byte(&doc.text, sel_chars.start)..editor::char_to_byte(&doc.text, sel_chars.end)
            });
            doc.stats.update_selection(&doc.text, doc.version, sel_bytes, stats_opts);
        }

        if !self.doc().is_manual
            && (self.settings.mode == ViewMode::Edit
                || (self.settings.sidebar_open && self.settings.sidebar_tab == SidebarTab::Outline))
        {
            let theme = self.settings.theme;
            let font_size = self.settings.preview_font_size;
            let line_height = self.settings.line_height;
            let para_spacing = self.settings.paragraph_spacing;
            let opts = self.parse_opts();
            let immediate = self.settings.mode != ViewMode::Edit;
            let doc = self.doc_mut();
            let pcfg = ParseConfig { theme, font_size, line_height, para_spacing, opts: &opts, immediate };
            doc.ensure_parsed(ctx, &pcfg);
        }

        let effective_root = self
            .settings
            .workspace_folder
            .clone()
            .or_else(|| self.doc().base_dir().map(Path::to_path_buf));
        self.sidebar.sync_root(ctx, effective_root.as_deref());
        self.sidebar.poll_watchers(ctx);

        if self.settings.sidebar_open {
            self.sidebar_width = egui::containers::panel::PanelState::load(ctx, ui.make_persistent_id("sidebar"))
                .map_or(self.sidebar_width, |s| s.size().x)
                .clamp(SIDEBAR_MIN_W, SIDEBAR_MAX_W);
        }

        let theme_before_top = self.settings.theme;
        egui::Panel::top("top_bar")
            .exact_size(TOP_BAR_H)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| self.top_bar_ui(ui));
        if self.settings.theme != theme_before_top {
            *ui.style_mut() = (*ctx.global_style()).clone();
        }
        if !self.doc().is_manual {
            self.last_doc_uid = Some(self.doc().uid);
        }
        egui::Panel::bottom("status_bar")
            .exact_size(STATUS_BAR_H)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| self.status_bar(ui));

        if self.settings.sidebar_open {
            let active_path = self.doc().path.clone();
            let doc_uid = self.doc().uid;
            let top_byte = if self.settings.mode == ViewMode::Preview {
                let top_idx = self.doc().preview.top_block_idx;
                self.doc()
                    .parsed
                    .get()
                    .and_then(|(d, _, _)| d.block_ranges.get(top_idx))
                    .map(|r| r.start)
                    .unwrap_or(0)
            } else {
                self.doc().editor_top_byte
            };
            let headings = self
                .doc()
                .parsed
                .get()
                .map(|(d, _, _)| d.headings.clone())
                .unwrap_or_default();

            let (act_opt, sb_w) = self.sidebar.show(
                ui,
                ctx,
                &mut self.settings.sidebar_tab,
                active_path.as_deref(),
                doc_uid,
                &headings,
                top_byte,
            );
            self.sidebar_width = sb_w;
            if let Some(act) = act_opt {
                match act {
                    SidebarAction::OpenFile(p) => self.open_file(p),
                    SidebarAction::OpenFolder => self.run_command(ctx, Command::OpenFolder),
                    SidebarAction::Renamed { old, new } => self.sync_tabs_after_rename(&old, &new),
                    SidebarAction::Deleted(p) => self.sync_tabs_after_delete(&p),
                    SidebarAction::JumpToOutline { anchor, byte_offset } => {
                        let doc = self.doc_mut();
                        doc.preview.scroll_to(anchor);
                        doc.pending_selection = Some(byte_offset..byte_offset);
                        doc.scroll_to_selection = true;
                    }
                }
            }
        }

        self.editor_rect = Rect::NOTHING;
        self.preview_rect = Rect::NOTHING;

        egui::CentralPanel::default()
            .frame(Frame::NONE.inner_margin(Margin::ZERO))
            .show(ui, |ui| {
                let spec = theme::current(ctx);
                let central_rect = ui.available_rect_before_wrap();
                let bg = match spec.divider {
                    DividerStyle::GlassInset { chassis_bg, .. } => chassis_bg,
                    _ => spec.canvas_bg,
                };
                ui.painter().rect_filled(central_rect, 0.0, bg);

                if self.doc().is_manual {
                    self.manual_pane(ui);
                    return;
                }
                if self.settings.mode != ViewMode::Preview {
                    let find_action = find::bar_ui(ui, &mut self.doc_mut().find);
                    if let Some(act) = find_action {
                        match act {
                            FindAction::JumpTo(r) => {
                                let doc = self.doc_mut();
                                doc.pending_selection = Some(r);
                                doc.scroll_to_selection = true;
                            }
                            FindAction::ReplaceCurrent => self.replace_current(ctx),
                            FindAction::ReplaceAll => self.replace_all(ctx),
                            FindAction::Close => {
                                let doc = self.doc_mut();
                                doc.find.open = false;
                                ctx.memory_mut(|m| m.request_focus(editor::editor_id(doc.uid)));
                            }
                        }
                    }
                }

                let avail = ui.available_rect_before_wrap();
                let render_pane = |ui: &mut egui::Ui, outer_rect: Rect, body: &mut dyn FnMut(&mut egui::Ui)| {
                    let inner_rect = outer_rect.shrink(PANE_INSET);
                    if let DividerStyle::GlassInset {
                        plate_bg,
                        rim_stroke,
                        top_highlight,
                        radius,
                        ..
                    } = spec.divider
                    {
                        ui.painter().rect_filled(inner_rect, radius, plate_bg);
                        ui.painter()
                            .rect_stroke(inner_rect, radius, Stroke::new(1.0, rim_stroke), StrokeKind::Inside);
                        ui.painter().hline(
                            (inner_rect.left() + radius)..=(inner_rect.right() - radius),
                            inner_rect.top() + 0.6,
                            Stroke::new(1.0, top_highlight),
                        );
                    }
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .max_rect(inner_rect)
                            .layout(egui::Layout::top_down(Align::Min)),
                    );
                    child.set_clip_rect(ui.clip_rect().intersect(inner_rect));
                    body(&mut child);
                };

                match self.settings.mode {
                    ViewMode::Edit => {
                        render_pane(ui, avail, &mut |ui| self.editor_pane(ui));
                    }
                    ViewMode::Preview => {
                        render_pane(ui, avail, &mut |ui| self.preview_pane(ui));
                    }
                    ViewMode::Split => {
                        let mid_x = avail.center().x.round();
                        let left_rect = Rect::from_min_max(avail.min, pos2(mid_x, avail.max.y));
                        let right_rect = Rect::from_min_max(pos2(mid_x, avail.min.y), avail.max);
                        render_pane(ui, left_rect, &mut |ui| self.editor_pane(ui));
                        render_pane(ui, right_rect, &mut |ui| self.preview_pane(ui));

                        match spec.divider {
                            DividerStyle::Dotted { color, step } => {
                                theme::paint_dotted_vline(ui.painter(), mid_x, avail.top(), avail.bottom(), color, step);
                            }
                            DividerStyle::Solid(color) => {
                                ui.painter().vline(mid_x, avail.y_range(), Stroke::new(1.0, color));
                            }
                            DividerStyle::GlassInset { .. } => {}
                        }
                    }
                }
            });

        self.jobs.floating_banner_ui(ctx);
        self.confirm_dialog(ctx);
        self.error_dialog(ctx);
        self.export_dialog_window(ctx);
        self.settings_window(ctx);
        self.shortcuts_ui.window_ui(ctx, &mut self.keymap, &mut self.error);
        self.stats_popup_window(ctx);
        self.self_test_screenshot(ctx);
        self.bench_post_frame(ctx, cpu_start);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.shot.is_none() && self.bench.is_none() {
            self.settings.keybindings = self.keymap.to_overrides();
            if self.settings.restore_tabs {
                let mut paths = Vec::new();
                let mut active = 0usize;
                for (i, d) in self.docs.iter().enumerate() {
                    if let Some(p) = &d.path {
                        if i == self.active {
                            active = paths.len();
                        }
                        paths.push(p.clone());
                    }
                }
                self.settings.session_paths = paths;
                self.settings.session_active = active;
                self.settings.session_manual_open = self.docs.iter().any(|d| d.is_manual);
                self.settings.session_manual_active = self.doc().is_manual;
            }
            eframe::set_value(storage, eframe::APP_KEY, &self.settings);
        }
    }
}
