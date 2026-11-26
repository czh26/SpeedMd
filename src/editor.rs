//! 编辑器面板：TextEdit 包装（按文档 uid 隔离状态）。

use crate::document::{Document, Stats};
use eframe::egui;

pub fn editor_id(uid: u64) -> egui::Id {
    egui::Id::unique(("editor_text", uid))
}

pub fn show(ui: &mut egui::Ui, doc: &mut Document, font_size: f32) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let font_id = egui::FontId::monospace(font_size);
    let uid = doc.uid;
    let version = doc.version;
    let mut changed = false;

    let inner = {
        let Document { text, hl, .. } = doc;
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = hl.highlight(version, buf.as_str(), dark, &font_id);
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        egui::ScrollArea::vertical()
            .id_salt(("editor_scroll", uid))
            .auto_shrink(false)
            .show(ui, |ui| {
                let resp = ui.add_sized(
                    ui.available_size(),
                    egui::TextEdit::multiline(text)
                        .id(editor_id(uid))
                        .frame(egui::Frame::NONE)
                        .lock_focus(true)
                        .margin(egui::Margin::symmetric(12, 10))
                        .layouter(&mut layouter),
                );
                if resp.changed() {
                    changed = true;
                }
                resp
            })
    };

    if changed {
        doc.dirty = true;
        doc.stats = Stats::of(&doc.text);
        doc.bump_version();
    }
    inner.inner
}
