#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // release 版不弹出控制台窗口

mod fonts;
mod highlight;
mod markdown;
mod preview;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const APP_NAME: &str = "MdPad";
const WELCOME: &str = include_str!("welcome.md");

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ViewMode {
    Edit,
    Split,
    Preview,
}

/// 持久化保存的设置
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    mode: ViewMode,
    editor_font_size: f32,
    preview_font_size: f32,
    /// 上标 ^x^ / 下标 ~x~ 语法（开启后单个 ~ 不再表示删除线）
    sup_sub: bool,
    /// 预览中显示 YAML Front Matter
    front_matter: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { mode: ViewMode::Split, editor_font_size: 15.0, preview_font_size: 16.0, sup_sub: false, front_matter: true }
    }
}

/// 需要先确认“是否保存”的操作
#[derive(Clone)]
enum Pending {
    New,
    Open(Option<PathBuf>),
    Close,
}

#[derive(Default, Clone, Copy)]
struct Stats {
    chars: usize,
    words: usize,
    lines: usize,
}

impl Stats {
    fn of(text: &str) -> Self {
        let (mut chars, mut words, mut lines, mut in_word) = (0, 0, 1, false);
        for c in text.chars() {
            chars += 1;
            if c == '\n' {
                lines += 1;
            }
            if is_cjk(c) {
                words += 1;
                in_word = false;
            } else if c.is_alphanumeric() {
                if !in_word {
                    words += 1;
                    in_word = true;
                }
            } else {
                in_word = false;
            }
        }
        Self { chars, words, lines }
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3040..=0x30FF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

struct MdApp {
    text: String,
    path: Option<PathBuf>,
    dirty: bool,
    settings: Settings,
    stats: Stats,
    preview: preview::Preview,
    hl: highlight::Highlighter,
    title: String,
    confirm: Option<Pending>,
    allow_close: bool,
    error: Option<String>,
    /// 自测用：设置环境变量 MDPAD_SCREENSHOT=xxx.png 时，启动后自动截图并退出
    shot: Option<(PathBuf, u32)>,
}

impl MdApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        fonts::setup(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let shot = std::env::var_os("MDPAD_SCREENSHOT").map(|p| (PathBuf::from(p), 0));
        let mut settings: Settings =
            cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        if shot.is_some() {
            // 自测模式：使用固定设置，可用环境变量覆盖
            settings = Settings::default();
            match std::env::var("MDPAD_MODE").as_deref() {
                Ok("edit") => settings.mode = ViewMode::Edit,
                Ok("preview") => settings.mode = ViewMode::Preview,
                _ => {}
            }
            settings.sup_sub = std::env::var("MDPAD_SUPSUB").is_ok();
            if std::env::var("MDPAD_DARK").is_ok() {
                cc.egui_ctx.set_theme(egui::Theme::Dark);
            } else {
                cc.egui_ctx.set_theme(egui::Theme::Light);
            }
        }

        let mut app = Self {
            text: String::new(),
            path: None,
            dirty: false,
            settings,
            stats: Stats::default(),
            preview: preview::Preview::default(),
            hl: highlight::Highlighter::default(),
            title: String::new(),
            confirm: None,
            allow_close: false,
            error: None,
            shot,
        };
        // 支持“打开方式”/拖文件到 exe 上：第一个命令行参数作为文件路径
        if let Some(arg) = std::env::args_os().nth(1) {
            app.load(PathBuf::from(arg));
        } else {
            app.text = WELCOME.replace("\r\n", "\n");
            app.stats = Stats::of(&app.text);
        }
        if let Ok(anchor) = std::env::var("MDPAD_ANCHOR") {
            app.preview.scroll_to(anchor);
        }
        app
    }

    // ---------- 文件操作 ----------

    fn load(&mut self, path: PathBuf) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
                self.text = String::from_utf8_lossy(bytes).into_owned();
                self.stats = Stats::of(&self.text);
                self.path = Some(path);
                self.dirty = false;
            }
            Err(e) => self.error = Some(format!("无法打开文件：{}\n{e}", path.display())),
        }
    }

    fn write_to(&mut self, path: PathBuf) -> bool {
        match std::fs::write(&path, self.text.as_bytes()) {
            Ok(()) => {
                self.path = Some(path);
                self.dirty = false;
                true
            }
            Err(e) => {
                self.error = Some(format!("保存失败：{}\n{e}", path.display()));
                false
            }
        }
    }

    fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) => self.write_to(p),
            None => self.save_as(),
        }
    }

    fn save_as(&mut self) -> bool {
        let name = self
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未命名.md".into());
        let picked = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown"])
            .add_filter("所有文件", &["*"])
            .set_file_name(name)
            .save_file();
        match picked {
            Some(p) => self.write_to(p),
            None => false,
        }
    }

    fn request(&mut self, ctx: &egui::Context, action: Pending) {
        if self.dirty {
            self.confirm = Some(action);
        } else {
            self.perform(ctx, action);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, action: Pending) {
        match action {
            Pending::New => {
                self.text.clear();
                self.path = None;
                self.dirty = false;
                self.stats = Stats::of(&self.text);
            }
            Pending::Open(path) => {
                let path = path.or_else(|| {
                    rfd::FileDialog::new()
                        .add_filter("Markdown", &["md", "markdown", "mdown", "txt"])
                        .add_filter("所有文件", &["*"])
                        .pick_file()
                });
                if let Some(p) = path {
                    self.load(p);
                }
            }
            Pending::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// 预览里勾选 / 取消任务列表复选框 → 修改源码中的 [ ] / [x]
    fn toggle_task(&mut self, offset: usize) {
        let b = self.text.as_bytes();
        if b.get(offset) == Some(&b'[') && b.get(offset + 2) == Some(&b']') {
            let new = match b[offset + 1] {
                b' ' => "x",
                b'x' | b'X' => " ",
                _ => return,
            };
            self.text.replace_range(offset + 1..offset + 2, new);
            self.dirty = true;
        }
    }

    // ---------- 输入 ----------

    fn handle_input(&mut self, ctx: &egui::Context) {
        let sc = |m: Modifiers, k: Key| KeyboardShortcut::new(m, k);
        let shift_cmd = Modifiers::COMMAND | Modifiers::SHIFT;

        // 注意：带 Shift 的组合要先检测
        if ctx.input_mut(|i| i.consume_shortcut(&sc(shift_cmd, Key::S))) {
            self.save_as();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::S))) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::O))) {
            self.request(ctx, Pending::Open(None));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, Key::N))) {
            self.request(ctx, Pending::New);
        }
        for (key, mode) in [(Key::Num1, ViewMode::Edit), (Key::Num2, ViewMode::Split), (Key::Num3, ViewMode::Preview)] {
            if ctx.input_mut(|i| i.consume_shortcut(&sc(Modifiers::COMMAND, key))) {
                self.settings.mode = mode;
            }
        }

        // 拖放文件
        let dropped: Option<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| !p.as_os_str().is_empty())
        });
        if let Some(p) = dropped {
            self.request(ctx, Pending::Open(Some(p)));
        }

        // 关闭窗口时若有未保存修改，先拦截
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未命名".into());
        let title = format!("{}{name} - {APP_NAME}", if self.dirty { "● " } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // ---------- 界面 ----------

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("文件", |ui| {
                if ui.button("新建        Ctrl+N").clicked() {
                    self.request(&ctx, Pending::New);
                }
                if ui.button("打开…       Ctrl+O").clicked() {
                    self.request(&ctx, Pending::Open(None));
                }
                ui.separator();
                if ui.button("保存        Ctrl+S").clicked() {
                    self.save();
                }
                if ui.button("另存为…  Ctrl+Shift+S").clicked() {
                    self.save_as();
                }
                ui.separator();
                if ui.button("退出").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("设置", |ui| {
                ui.label(egui::RichText::new("Markdown 扩展语法").strong());
                ui.checkbox(&mut self.settings.sup_sub, "上标 / 下标（^上标^、~下标~）")
                    .on_hover_text("开启后单个 ~ 表示下标，删除线请使用 ~~文本~~");
                ui.checkbox(&mut self.settings.front_matter, "预览中显示 YAML Front Matter");
                ui.separator();
                ui.label(egui::RichText::new("字号").strong());
                egui::Grid::new("font_sizes").num_columns(2).show(ui, |ui| {
                    ui.label("编辑器");
                    ui.add(egui::Slider::new(&mut self.settings.editor_font_size, 10.0..=32.0).step_by(1.0));
                    ui.end_row();
                    ui.label("预览");
                    ui.add(egui::Slider::new(&mut self.settings.preview_font_size, 10.0..=32.0).step_by(1.0));
                    ui.end_row();
                });
                ui.weak("界面缩放：Ctrl + 加号 / 减号 / 0");
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
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let path = self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "未保存的新文件".into());
            ui.weak(path);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(format!("{} 行   {} 字   {} 字符", self.stats.lines, self.stats.words, self.stats.chars));
                if self.dirty {
                    ui.colored_label(egui::Color32::from_rgb(0xe0, 0x9a, 0x3e), "● 未保存");
                }
            });
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        let font_id = egui::FontId::monospace(self.settings.editor_font_size);
        let hl = &mut self.hl;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = hl.highlight(buf.as_str(), dark, &font_id);
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        egui::ScrollArea::vertical()
            .id_salt("editor_scroll")
            .auto_shrink(false)
            .show(ui, |ui| {
                let resp = ui.add_sized(
                    ui.available_size(),
                    egui::TextEdit::multiline(&mut self.text)
                        .frame(egui::Frame::NONE)
                        .lock_focus(true)
                        .margin(egui::Margin::symmetric(12, 10))
                        .layouter(&mut layouter),
                );
                if resp.changed() {
                    self.dirty = true;
                    self.stats = Stats::of(&self.text);
                }
            });
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        let base = self.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let opts = preview::Options {
            font_size: self.settings.preview_font_size,
            sup_sub: self.settings.sup_sub,
            front_matter: self.settings.front_matter,
        };
        let actions = self.preview.ui(ui, &self.text, &opts, base.as_deref());
        if let Some(off) = actions.toggle_task {
            self.toggle_task(off);
        }
        if let Some(p) = actions.open_file {
            let ctx = ui.ctx().clone();
            self.request(&ctx, Pending::Open(Some(p)));
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm.clone() else { return };
        let mut choice = None;
        egui::Window::new("未保存的修改")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("当前文档有未保存的修改，是否保存？");
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
                if self.save() {
                    self.perform(ctx, action);
                }
            }
            Some(1) => {
                self.confirm = None;
                self.dirty = false;
                self.perform(ctx, action);
            }
            Some(_) => self.confirm = None,
            None => {}
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

    fn self_test_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = self.shot.as_mut() else { return };
        *frames += 1;
        if *frames == 15 {
            let (path, ctx2) = (path.clone(), ctx.clone());
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let _ = image::save_buffer(&path, img.as_raw(), w as u32, h as u32, image::ColorType::Rgba8);
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if *frames < 15 {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for MdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        self.handle_input(ctx);
        self.update_title(ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.global_style()).inner_margin(0))
            .show(ui, |ui| match self.settings.mode {
                ViewMode::Edit => self.editor(ui),
                ViewMode::Preview => self.preview(ui),
                ViewMode::Split => {
                    ui.columns(2, |cols| {
                        self.editor(&mut cols[0]);
                        self.preview(&mut cols[1]);
                    });
                }
            });

        self.confirm_dialog(ctx);
        self.error_dialog(ctx);
        self.self_test_screenshot(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.shot.is_none() {
            eframe::set_value(storage, eframe::APP_KEY, &self.settings);
        }
    }
}

fn main() -> eframe::Result {
    let mut size = [1200.0, 800.0];
    if let Ok(h) = std::env::var("MDPAD_HEIGHT") {
        size[1] = h.parse().unwrap_or(800.0);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size(size)
            .with_min_inner_size([480.0, 320.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(MdApp::new(cc)))))
}
