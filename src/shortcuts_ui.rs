//! 自定义快捷键窗口：搜索过滤、按键录制、冲突检测与 JSON 导入/导出。

use crate::commands::Command;
use crate::keymap::{Keymap, Shortcut};
use eframe::egui::{self, Align2, Color32, Event, Key};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct PendingConflict {
    target: Command,
    existing: Command,
    shortcut: Shortcut,
}

#[derive(Default)]
pub struct ShortcutsUiState {
    pub open: bool,
    pub search: String,
    capturing: Option<Command>,
    conflict: Option<PendingConflict>,
}

pub fn export_overrides_json(keymap: &Keymap) -> Result<String, String> {
    serde_json::to_string_pretty(&keymap.to_overrides()).map_err(|e| format!("序列化快捷键 JSON 失败：{e}"))
}

pub fn import_overrides_json(json: &str) -> Result<Keymap, String> {
    let map: BTreeMap<String, String> =
        serde_json::from_str(json).map_err(|e| format!("解析快捷键 JSON 失败：{e}"))?;
    Ok(Keymap::from_overrides(&map))
}

/// 若处于录制状态，则吞掉本帧按键/文本事件并尝试捕获新快捷键（返回 true 表示已拦截输入）。
fn apply_capture_key(
    capturing: &mut Option<Command>,
    conflict: &mut Option<PendingConflict>,
    keymap: &mut Keymap,
    key: Key,
    physical_key: Option<Key>,
    modifiers: egui::Modifiers,
) -> bool {
    let Some(target) = *capturing else { return false };
    let no_mods = !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift;
    if key == Key::Escape && no_mods {
        *capturing = None;
        return true;
    }
    if matches!(key, Key::Backspace | Key::Delete) && no_mods {
        keymap.set(target, None);
        *capturing = None;
        return true;
    }
    if let Some(sc) = Shortcut::from_event(key, physical_key, modifiers) {
        if let Some(existing) = keymap.find_conflict(target, sc) {
            *conflict = Some(PendingConflict { target, existing, shortcut: sc });
        } else {
            keymap.set(target, Some(sc));
        }
        *capturing = None;
        return true;
    }
    false
}

impl ShortcutsUiState {
    pub fn capture_input(&mut self, ctx: &egui::Context, keymap: &mut Keymap) -> bool {
        if self.capturing.is_none() {
            return false;
        }
        ctx.input_mut(|i| {
            let mut captured = false;
            for ev in &i.events {
                if !captured
                    && let Event::Key { pressed: true, key, physical_key, modifiers, .. } = ev
                {
                    captured = apply_capture_key(
                        &mut self.capturing,
                        &mut self.conflict,
                        keymap,
                        *key,
                        *physical_key,
                        *modifiers,
                    );
                }
            }
            i.events.retain(|ev| !matches!(ev, Event::Key { .. } | Event::Text(_)));
        });
        true
    }

    pub fn window_ui(&mut self, ctx: &egui::Context, keymap: &mut Keymap, error_out: &mut Option<String>) {
        if !self.open {
            return;
        }
        let mut open = self.open;

        egui::Window::new("自定义快捷键")
            .open(&mut open)
            .resizable(true)
            .default_width(680.0)
            .default_height(500.0)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("搜索");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .desired_width(200.0)
                            .hint_text("输入命令名或快捷键…"),
                    );
                    if !self.search.is_empty() && ui.small_button("清除").clicked() {
                        self.search.clear();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("全部恢复默认").clicked() {
                            keymap.reset_all();
                            self.capturing = None;
                            self.conflict = None;
                        }
                        if ui.button("导出 JSON…").clicked() {
                            match export_overrides_json(keymap) {
                                Ok(json) => {
                                    if let Some(p) = rfd::FileDialog::new()
                                        .add_filter("JSON", &["json"])
                                        .set_file_name("mdpad-shortcuts.json")
                                        .save_file()
                                        && let Err(e) = std::fs::write(&p, json.as_bytes())
                                    {
                                        *error_out = Some(format!("导出快捷键失败：{e}"));
                                    }
                                }
                                Err(e) => *error_out = Some(e),
                            }
                        }
                        if ui.button("导入 JSON…").clicked()
                            && let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file()
                        {
                            match std::fs::read_to_string(&p)
                                .map_err(|e| format!("读取快捷键 JSON 失败：{e}"))
                                .and_then(|s| import_overrides_json(&s))
                            {
                                Ok(new_km) => {
                                    *keymap = new_km;
                                    self.capturing = None;
                                    self.conflict = None;
                                }
                                Err(e) => *error_out = Some(e),
                            }
                        }
                    });
                });

                ui.weak("提示：点击「当前快捷键」按钮后按下新组合键；Esc 取消录制，Backspace / Delete 解除绑定。");
                ui.add_space(4.0);

                if let Some(conf) = self.conflict {
                    let warn = Color32::from_rgb(0xd2, 0x99, 0x22);
                    egui::Frame::new()
                        .fill(warn.gamma_multiply(0.14))
                        .stroke(egui::Stroke::new(1.0, warn))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!(
                                    "快捷键 [{}] 已被「{}」占用，是否覆盖？",
                                    conf.shortcut,
                                    conf.existing.label()
                                ));
                                if ui.button("覆盖（清除原命令绑定）").clicked() {
                                    keymap.set(conf.existing, None);
                                    keymap.set(conf.target, Some(conf.shortcut));
                                    self.conflict = None;
                                }
                                if ui.button("取消").clicked() {
                                    self.conflict = None;
                                }
                            });
                        });
                    ui.add_space(6.0);
                }

                ui.separator();
                let filter = self.search.trim().to_ascii_lowercase();

                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    egui::Grid::new("shortcuts_table")
                        .num_columns(5)
                        .striped(true)
                        .spacing([14.0, 6.0])
                        .show(ui, |ui| {
                            ui.strong("分类");
                            ui.strong("命令");
                            ui.strong("当前快捷键");
                            ui.strong("默认快捷键");
                            ui.strong("操作");
                            ui.end_row();

                            for &cmd in Command::ALL {
                                let cur = keymap.shortcut(cmd);
                                let cur_str = cur.map(|s| s.to_string()).unwrap_or_else(|| "未绑定".into());
                                let def_str = cmd
                                    .default_shortcut()
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| "—".into());
                                if !filter.is_empty()
                                    && !cmd.label().to_ascii_lowercase().contains(&filter)
                                    && !cmd.category().to_ascii_lowercase().contains(&filter)
                                    && !cmd.id().to_ascii_lowercase().contains(&filter)
                                    && !cur_str.to_ascii_lowercase().contains(&filter)
                                {
                                    continue;
                                }

                                ui.weak(cmd.category());
                                ui.label(cmd.label());

                                let recording = self.capturing == Some(cmd);
                                let btn_text = if recording {
                                    egui::RichText::new("按下按键… (Esc 取消)").color(ui.visuals().selection.stroke.color)
                                } else {
                                    egui::RichText::new(&cur_str)
                                };
                                let btn = egui::Button::new(btn_text).min_size(egui::vec2(130.0, 22.0));
                                if ui.add(btn).clicked() {
                                    self.conflict = None;
                                    self.capturing = if recording { None } else { Some(cmd) };
                                }

                                ui.weak(def_str);

                                let modified = cur != cmd.default_shortcut();
                                if ui.add_enabled(modified, egui::Button::new("恢复默认")).clicked() {
                                    keymap.reset(cmd);
                                    if self.capturing == Some(cmd) {
                                        self.capturing = None;
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
            });

        self.open = open;
        if !self.open {
            self.capturing = None;
            self.conflict = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_preserves_overrides_and_unbound_and_ignores_invalid() {
        let mut km = Keymap::default();
        km.set(Command::Save, Some(Shortcut::new(true, true, false, Key::F5)));
        km.set(Command::Find, None); // 显式解绑 -> ""
        let json = export_overrides_json(&km).unwrap();

        // 注入未知命令与非法快捷键，验证容错忽略
        let mut raw: BTreeMap<String, String> = serde_json::from_str(&json).unwrap();
        raw.insert("nonexistent_command".into(), "Ctrl+J".into());
        raw.insert("open".into(), "Invalid+???".into());
        let noisy_json = serde_json::to_string_pretty(&raw).unwrap();

        let restored = import_overrides_json(&noisy_json).unwrap();
        assert_eq!(restored.shortcut(Command::Save), Some(Shortcut::new(true, true, false, Key::F5)));
        assert_eq!(restored.shortcut(Command::Find), None);
        assert_eq!(restored.shortcut(Command::Open), Command::Open.default_shortcut());
    }

    #[test]
    fn conflict_overwrite_unbinds_previous_owner() {
        let mut km = Keymap::default();
        let mut cap = Some(Command::ImportHtml);
        let mut conflict = None;

        // ImportHtml 尝试绑定 Save 的默认快捷键 Ctrl+S -> 产生冲突
        let ctrl_s_mods = egui::Modifiers { ctrl: true, ..Default::default() };
        assert!(apply_capture_key(&mut cap, &mut conflict, &mut km, Key::S, None, ctrl_s_mods));
        assert!(cap.is_none());
        let conf = conflict.expect("Ctrl+S 应与 Save 冲突");
        assert_eq!(conf.existing, Command::Save);

        // 用户确认覆盖：清除 Save 原绑定，赋给 ImportHtml
        km.set(conf.existing, None);
        km.set(conf.target, Some(conf.shortcut));
        assert_eq!(km.shortcut(Command::Save), None);
        assert_eq!(km.shortcut(Command::ImportHtml), Some(conf.shortcut));
    }
}
