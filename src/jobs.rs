//! 后台任务调度中心（HTML 导入 / 导出异步执行 + 完成提示条）。

use crate::import::ImportedDoc;
use crate::theme::{self, STATUS_BAR_H};
use crate::winutil;
use eframe::egui::{self, Align2, Frame, Id, Margin, Stroke};
use std::path::PathBuf;
use std::sync::mpsc;

pub enum JobResult {
    ExportDone { path: PathBuf },
    ImportDone(ImportedDoc),
    Failed(String),
}

pub struct PollEvents {
    pub imports: Vec<ImportedDoc>,
    pub error: Option<String>,
}

pub struct JobHub {
    tx: mpsc::Sender<JobResult>,
    rx: mpsc::Receiver<JobResult>,
    pub running: usize,
    pub last_exported: Option<PathBuf>,
}

impl Default for JobHub {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, running: 0, last_exported: None }
    }
}

impl JobHub {
    pub fn spawn(&mut self, ctx: egui::Context, work: impl FnOnce() -> JobResult + Send + 'static) {
        let tx = self.tx.clone();
        self.running += 1;
        std::thread::spawn(move || {
            let res = work();
            let _ = tx.send(res);
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self) -> PollEvents {
        let mut imports = Vec::new();
        let mut error = None;
        while let Ok(res) = self.rx.try_recv() {
            self.running = self.running.saturating_sub(1);
            match res {
                JobResult::ExportDone { path } => self.last_exported = Some(path),
                JobResult::ImportDone(doc) => imports.push(doc),
                JobResult::Failed(e) => error = Some(e),
            }
        }
        PollEvents { imports, error }
    }

    pub fn has_banner(&self) -> bool {
        self.running > 0 || self.last_exported.is_some()
    }

    pub fn floating_banner_ui(&mut self, ctx: &egui::Context) {
        if !self.has_banner() {
            return;
        }
        let spec = theme::current(ctx);
        egui::Area::new(Id::unique("jobs_floating_banner"))
            .anchor(Align2::RIGHT_BOTTOM, [-14.0, -(STATUS_BAR_H + 10.0)])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(spec.popup_bg)
                    .stroke(Stroke::new(1.0, spec.border))
                    .shadow(spec.popup_shadow)
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        let Some(path) = self.last_exported.clone() else {
                            if self.running > 0 {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label("正在后台处理 HTML 任务…");
                                });
                            }
                            return;
                        };

                        let mut dismiss = false;
                        ui.horizontal(|ui| {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string());
                            ui.label(
                                egui::RichText::new(format!("✓ 已导出 HTML：{name}"))
                                    .strong()
                                    .color(spec.semantic.success),
                            );
                            if ui.button("用浏览器打开").clicked() {
                                let _ = winutil::open_default(&path.to_string_lossy());
                            }
                            if ui.button("打开所在文件夹").clicked() {
                                winutil::reveal_in_explorer(&path);
                            }
                            if ui.small_button("×").on_hover_text("关闭提示").clicked() {
                                dismiss = true;
                            }
                        });
                        if dismiss {
                            self.last_exported = None;
                        }
                    });
            });
    }
}
