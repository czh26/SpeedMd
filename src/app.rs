//! MdPad 顶层应用状态、布局与统一命令分发。

use crate::commands::Command;
use crate::document::{Document, ParseConfig};
use crate::editor::{self, EditorConfig};
use crate::find::{self, FindAction};
use crate::fonts::{self, FontCatalog, FontChoice};
use crate::keymap::Keymap;
use crate::markdown::ParseOptions;
use crate::preview::PreviewOptions;
use crate::settings::{Settings, SidebarTab, ViewMode};
use crate::sidebar::{SidebarAction, SidebarState};
use crate::stats::{self, StatsOptions};
use crate::tabs::{self, TabAction};
use crate::winutil;
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    Align2, CursorIcon, Rect, Sense,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

pub const APP_NAME: &str = "MdPad";
const WELCOME: &str = include_str!("welcome.md");

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
    shot: Option<ShotRunner>,
    bench: Option<BenchRunner>,
}

impl MdApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::setup(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| {
            o.zoom_with_keyboard = false;
            o.quit_shortcuts.clear();
        });

        let shot_path = std::env::var_os("MDPAD_SCREENSHOT").map(PathBuf::from);
        let bench_typing = std::env::var("MDPAD_BENCH").as_deref() == Ok("typing");
        let bench_out = std::env::var_os("MDPAD_BENCH_OUT").map(PathBuf::from);
        let test_cmds: Vec<String> = std::env::var("MDPAD_TEST_COMMANDS")
            .map(|s| s.split(';').map(str::trim).filter(|c| !c.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();

        let mut settings: Settings =
            cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();

        let is_test = shot_path.is_some() || bench_typing;
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
            if std::env::var("MDPAD_DARK").is_ok() {
                cc.egui_ctx.set_theme(egui::Theme::Dark);
            } else {
                cc.egui_ctx.set_theme(egui::Theme::Light);
            }
            if let Ok(folder) = std::env::var("MDPAD_FOLDER") {
                settings.workspace_folder = Some(winutil::clean_path(Path::new(&folder)));
            }
        }

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
        } else if !is_test && app.settings.restore_tabs && !app.settings.session_paths.is_empty() {
            let paths = app.settings.session_paths.clone();
            let saved_active = app.settings.session_active;
            for p in paths {
                if p.is_file() {
                    app.open_file_silent(p);
                }
            }
            if !app.docs.is_empty() {
                app.active = saved_active.min(app.docs.len() - 1);
            }
        }

        if app.docs.is_empty() {
            app.docs.push(Document::from_welcome(WELCOME));
            app.active = 0;
        }

        if let Ok(anchor) = std::env::var("MDPAD_ANCHOR") {
            app.docs[app.active].preview.scroll_to(anchor);
        }
        if let Ok(q) = std::env::var("MDPAD_FIND_QUERY") {
            app.docs[app.active].find.query = q;
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
        if let Some(path) = self.docs[idx].path.clone() {
            self.write_doc(idx, path)
        } else {
            self.save_doc_as(idx)
        }
    }

    fn save_doc_as(&mut self, idx: usize) -> bool {
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
                self.docs[idx].is_welcome = false;
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

    // ---------- 源代码模式与查找替换 ----------

    fn toggle_source_mode(&mut self, ctx: &egui::Context) {
        let dark = ctx.global_style().visuals.dark_mode;
        let font_size = self.settings.preview_font_size;
        let opts = self.parse_opts();
        let cur_mode = self.settings.mode;

        if cur_mode == ViewMode::Edit {
            let cur_byte = editor::current_cursor_byte(ctx, self.doc());
            let line_height = self.settings.line_height;
            let para_spacing = self.settings.paragraph_spacing;
            let doc = self.doc_mut();
            let cfg = ParseConfig { dark, font_size, line_height, para_spacing, opts: &opts, immediate: true };
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
            let cfg = ParseConfig { dark, font_size, line_height, para_spacing, opts: &opts, immediate: true };
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

    // ---------- 命令分发与字号调节 ----------

    fn adjust_font(&mut self, ctx: &egui::Context, delta: Option<f32>) {
        let pointer = ctx.pointer_latest_pos();
        let target_preview = match self.settings.mode {
            ViewMode::Edit => false,
            ViewMode::Preview => true,
            ViewMode::Split => pointer.is_some_and(|p| self.preview_rect.contains(p)),
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
            Command::OpenShortcuts => {}
        }
    }

    // ---------- 输入处理 ----------

    fn handle_input(&mut self, ctx: &egui::Context) {
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
        let btn = egui::Button::new(cmd.label()).shortcut_text(self.keymap.label(cmd));
        ui.add(btn).clicked()
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut pending_cmd: Option<Command> = None;

        egui::MenuBar::new().ui(ui, |ui| {
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

            ui.menu_button("设置", |ui| {
                if self.menu_btn(ui, Command::OpenSettings) {
                    pending_cmd = Some(Command::OpenSettings);
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

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::widgets::global_theme_preference_buttons(ui);
                ui.separator();
                let mode = &mut self.settings.mode;
                ui.selectable_value(mode, ViewMode::Preview, "预览");
                ui.selectable_value(mode, ViewMode::Split, "分屏");
                ui.selectable_value(mode, ViewMode::Edit, "编辑");
            });
        });

        if let Some(cmd) = pending_cmd {
            self.run_command(&ctx, cmd);
        }
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let Some(action) = tabs::show(ui, &self.docs, self.active) else { return };
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
        let doc = self.doc();
        let path = doc.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "未保存的新文件".into());
        let dirty = doc.dirty;
        let full = doc.stats.full;
        let sel = doc.stats.sel;

        let stats_text = if let Some(s) = sel {
            format!(
                "已选 {} 字 / 共 {} 字   {} 行",
                stats::format_thousands(s.words),
                stats::format_thousands(full.words),
                stats::format_thousands(full.lines)
            )
        } else {
            format!(
                "{} 行   {} 字   {} 字符",
                stats::format_thousands(full.lines),
                stats::format_thousands(full.words),
                stats::format_thousands(full.chars_with_spaces)
            )
        };

        let mut clicked_stats = false;
        ui.horizontal(|ui| {
            ui.weak(path);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.add(egui::Label::new(egui::RichText::new(stats_text).weak()).sense(Sense::click()));
                if r.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if r.on_hover_text("点击查看详细字数统计").clicked() {
                    clicked_stats = true;
                }
                if dirty {
                    ui.colored_label(egui::Color32::from_rgb(0xe0, 0x9a, 0x3e), "● 未保存");
                }
            });
        });
        if clicked_stats {
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
        let dark = ui.visuals().dark_mode;
        let font_size = self.settings.preview_font_size;
        let line_height = self.settings.line_height;
        let para_spacing = self.settings.paragraph_spacing;
        let opts = self.parse_opts();
        let code_line_numbers = self.settings.code_line_numbers;
        let max_content_width = self.settings.max_content_width;
        let ctx = ui.ctx().clone();

        let actions = {
            let doc = self.doc_mut();
            let pcfg = ParseConfig { dark, font_size, line_height, para_spacing, opts: &opts, immediate: true };
            doc.ensure_parsed(&ctx, &pcfg);
            let base = doc.base_dir().map(Path::to_path_buf);
            if let Some((parsed_doc, th, key)) = doc.parsed.get() {
                let preview_opts = PreviewOptions {
                    base_dir: base.as_deref(),
                    code_line_numbers,
                    max_content_width,
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
        let dark_theme = full_document && self.export_follow_theme && ctx.global_style().visuals.dark_mode;
        let opts = crate::export::ExportOptions {
            full_document,
            dark_theme,
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
                    ui.radio_value(&mut self.export_follow_theme, true, "跟随当前明/暗主题");
                    ui.radio_value(&mut self.export_follow_theme, false, "固定为浅色主题");
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
                if let Some(cmd) = Command::from_id(&id) {
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

        self.handle_input(ctx);
        self.update_title(ctx);

        let stats_opts = self.stats_opts();
        let sel_chars = editor::current_selection_chars(ctx, self.doc().uid, &self.doc().text);
        {
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

        if self.settings.mode == ViewMode::Edit || (self.settings.sidebar_open && self.settings.sidebar_tab == SidebarTab::Outline) {
            let dark = ui.visuals().dark_mode;
            let font_size = self.settings.preview_font_size;
            let line_height = self.settings.line_height;
            let para_spacing = self.settings.paragraph_spacing;
            let opts = self.parse_opts();
            let immediate = self.settings.mode != ViewMode::Edit;
            let doc = self.doc_mut();
            let pcfg = ParseConfig { dark, font_size, line_height, para_spacing, opts: &opts, immediate };
            doc.ensure_parsed(ctx, &pcfg);
        }

        let effective_root = self
            .settings
            .workspace_folder
            .clone()
            .or_else(|| self.doc().base_dir().map(Path::to_path_buf));
        self.sidebar.sync_root(ctx, effective_root.as_deref());
        self.sidebar.poll_watchers(ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("tabs").show(ui, |ui| self.tab_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        if self.jobs.has_banner() {
            egui::Panel::bottom("jobs_banner").show(ui, |ui| self.jobs.banner_ui(ui));
        }

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

            if let Some(act) = self.sidebar.show(
                ui,
                ctx,
                &mut self.settings.sidebar_tab,
                active_path.as_deref(),
                doc_uid,
                &headings,
                top_byte,
            ) {
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
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(0))
            .show(ui, |ui| {
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

                match self.settings.mode {
                    ViewMode::Edit => self.editor_pane(ui),
                    ViewMode::Preview => self.preview_pane(ui),
                    ViewMode::Split => {
                        ui.columns(2, |cols| {
                            self.editor_pane(&mut cols[0]);
                            self.preview_pane(&mut cols[1]);
                            let x = cols[0].max_rect().right();
                            let y_range = cols[0].max_rect().y_range();
                            cols[0].painter().vline(
                                x,
                                y_range,
                                egui::Stroke::new(1.0, cols[0].visuals().widgets.noninteractive.bg_stroke.color),
                            );
                        });
                    }
                }
            });

        self.confirm_dialog(ctx);
        self.error_dialog(ctx);
        self.export_dialog_window(ctx);
        self.settings_window(ctx);
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
            }
            eframe::set_value(storage, eframe::APP_KEY, &self.settings);
        }
    }
}
