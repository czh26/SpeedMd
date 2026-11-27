//! 编辑器面板：TextEdit 包装、编程式 EditOps（单步撤销）、括号/围栏自动补全、跳转到偏移。

use crate::clipboard;
use crate::commands::Command;
use crate::convert;
use crate::document::Document;
use crate::highlight::FindOverlay;
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
    vec2, Align, Event, FontFamily, ImeEvent, Key, TextEdit,
};
use std::ops::Range;
use std::path::{Path, PathBuf};

pub fn editor_id(uid: u64) -> egui::Id {
    egui::Id::unique(("editor_text", uid))
}

/// 字符索引 -> UTF-8 字节偏移（超界则钳位到字符串末尾）。
pub fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

/// UTF-8 字节偏移 -> 字符索引（自动向下对齐到最近 char 边界）。
pub fn byte_to_char(s: &str, byte_idx: usize) -> usize {
    let mut b = byte_idx.min(s.len());
    while b > 0 && !s.is_char_boundary(b) {
        b -= 1;
    }
    s[..b].chars().count()
}

pub fn sorted_char_range(r: CCursorRange) -> Range<usize> {
    let a = r.primary.index.0;
    let b = r.secondary.index.0;
    a.min(b)..a.max(b)
}

pub fn current_selection_chars(ctx: &egui::Context, uid: u64, text: &str) -> Range<usize> {
    if let Some(state) = egui::TextEdit::load_state(ctx, editor_id(uid))
        && let Some(cr) = state.cursor.char_range()
    {
        let r = sorted_char_range(cr);
        let max_c = text.chars().count();
        return r.start.min(max_c)..r.end.min(max_c);
    }
    0..0
}

pub fn current_cursor_byte(ctx: &egui::Context, doc: &Document) -> usize {
    let r = current_selection_chars(ctx, doc.uid, &doc.text);
    char_to_byte(&doc.text, r.start)
}

/// 编程式修改文本：一次调用 = 一个撤销点（确保一次 Ctrl+Z 精确回到修改前）。
pub fn apply_edit(
    ctx: &egui::Context,
    doc: &mut Document,
    edit: impl FnOnce(&mut String, CCursorRange) -> CCursorRange,
) {
    let id = editor_id(doc.uid);
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let default_pos = CCursor::new(doc.text.chars().count());
    let before = state.cursor.char_range().unwrap_or_else(|| CCursorRange::one(default_pos));
    let mut undoer = state.undoer();
    undoer.add_undo(&(before, doc.text.clone()));

    let after = edit(&mut doc.text, before);

    undoer.add_undo(&(after, doc.text.clone()));
    state.set_undoer(undoer);
    state.cursor.set_char_range(Some(after));
    state.store(ctx, id);

    doc.dirty = true;
    doc.bump_version();
    doc.find.invalidate();
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoPairInput {
    Text(String),
    Backspace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPatch {
    pub replace_bytes: Range<usize>,
    pub insert: String,
    pub new_sel_chars: Range<usize>,
}

impl TextPatch {
    pub fn apply(&self, text: &mut String) -> CCursorRange {
        text.replace_range(self.replace_bytes.clone(), &self.insert);
        if self.new_sel_chars.start == self.new_sel_chars.end {
            CCursorRange::one(CCursor::new(self.new_sel_chars.start))
        } else {
            CCursorRange::two(CCursor::new(self.new_sel_chars.start), CCursor::new(self.new_sel_chars.end))
        }
    }
}

pub fn apply_text_patch(ctx: &egui::Context, doc: &mut Document, patch: TextPatch) {
    apply_edit(ctx, doc, |text, _| patch.apply(text));
}

pub fn selected_text(ctx: &egui::Context, doc: &Document) -> Option<String> {
    let r = current_selection_chars(ctx, doc.uid, &doc.text);
    if r.start < r.end {
        let b0 = char_to_byte(&doc.text, r.start);
        let b1 = char_to_byte(&doc.text, r.end);
        Some(doc.text[b0..b1].to_string())
    } else {
        None
    }
}

/// 在当前光标或选区位置插入纯文本（统一换行、可一次 Ctrl+Z 撤销）。
pub fn insert_text(ctx: &egui::Context, doc: &mut Document, insertion: &str) {
    let norm = insertion.replace("\r\n", "\n").replace('\r', "\n");
    apply_edit(ctx, doc, |text, before| {
        let r = sorted_char_range(before);
        let max_c = text.chars().count();
        let c0 = r.start.min(max_c);
        let c1 = r.end.min(max_c);
        let b0 = char_to_byte(text, c0);
        let b1 = char_to_byte(text, c1);
        text.replace_range(b0..b1, &norm);
        let after_c = c0 + norm.chars().count();
        CCursorRange::one(CCursor::new(after_c))
    });
}

/// 智能粘贴：优先尝试网页 HTML 转 Markdown，其次剪贴板图像落盘为 `assets/*.png`，最后纯文本。
pub fn smart_paste(
    ctx: &egui::Context,
    doc: &mut Document,
    fallback_text: Option<&str>,
    paste_html_as_md: bool,
    base_dir: Option<&Path>,
) -> bool {
    if paste_html_as_md
        && let Some(html) = clipboard::get_html()
    {
        let md = convert::html_to_markdown(&html);
        if !md.trim().is_empty() {
            insert_text(ctx, doc, &md);
            return true;
        }
    }
    if fallback_text.is_none_or(str::is_empty)
        && let Some((_, url)) = clipboard::get_and_save_image(base_dir)
    {
        insert_text(ctx, doc, &format!("![]({url})"));
        return true;
    }
    if let Some(txt) = fallback_text.filter(|s| !s.is_empty()).map(str::to_owned).or_else(clipboard::get_text) {
        insert_text(ctx, doc, &txt);
        return true;
    }
    false
}

fn open_pair_close(ch: char) -> Option<char> {
    Some(match ch {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' => '"',
        '\'' => '\'',
        '`' => '`',
        '（' => '）',
        '【' => '】',
        '“' => '”',
        '‘' => '’',
        '《' => '》',
        '「' => '」',
        _ => return None,
    })
}

fn wrap_pair(ch: char) -> Option<(char, char)> {
    if let Some(close) = open_pair_close(ch) {
        return Some((ch, close));
    }
    Some(match ch {
        '*' => ('*', '*'),
        '_' => ('_', '_'),
        '~' => ('~', '~'),
        '=' => ('=', '='),
        _ => return None,
    })
}

fn is_closing_delim(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`' | '）' | '】' | '”' | '’' | '》' | '」')
}

/// 判断给定字节偏移是否处于围栏代码块（``` / ~~~）内部。
pub fn is_in_fenced_code(text: &str, byte_pos: usize) -> bool {
    let prefix = &text[..byte_pos.min(text.len())];
    let mut fence: Option<(char, usize)> = None;
    for line in prefix.split('\n') {
        let trimmed = line.trim_start();
        let first = match trimmed.chars().next() {
            Some(c @ ('`' | '~')) => c,
            _ => continue,
        };
        let count = trimmed.chars().take_while(|&c| c == first).count();
        if count < 3 {
            continue;
        }
        match fence {
            None => fence = Some((first, count)),
            Some((fc, fl)) if fc == first && count >= fl && trimmed[count..].trim().is_empty() => {
                fence = None;
            }
            _ => {}
        }
    }
    fence.is_some()
}

/// 纯函数自动补全规则：支持有选区包裹、中英文成对补全、跳过已有右符号、don't/三反引号防误触、空对同删。
pub fn autopair(
    text: &str,
    sel_chars: Range<usize>,
    input: AutoPairInput,
    enabled: bool,
    allow_in_code: bool,
) -> Option<TextPatch> {
    if !enabled {
        return None;
    }
    let b_start = char_to_byte(text, sel_chars.start);
    let b_end = char_to_byte(text, sel_chars.end);
    if !allow_in_code && is_in_fenced_code(text, b_start) {
        return None;
    }

    if sel_chars.start < sel_chars.end {
        let AutoPairInput::Text(s) = input else { return None };
        let mut chars = s.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let (open, close) = wrap_pair(ch)?;
        let selected = &text[b_start..b_end];
        let insert = format!("{open}{selected}{close}");
        return Some(TextPatch {
            replace_bytes: b_start..b_end,
            insert,
            new_sel_chars: (sel_chars.start + 1)..(sel_chars.end + 1),
        });
    }

    match input {
        AutoPairInput::Backspace => {
            if sel_chars.start == 0 {
                return None;
            }
            let left = text[..b_start].chars().next_back()?;
            let right = text[b_start..].chars().next()?;
            if open_pair_close(left) == Some(right) {
                let l_start = b_start - left.len_utf8();
                let r_end = b_start + right.len_utf8();
                return Some(TextPatch {
                    replace_bytes: l_start..r_end,
                    insert: String::new(),
                    new_sel_chars: (sel_chars.start - 1)..(sel_chars.start - 1),
                });
            }
            None
        }
        AutoPairInput::Text(s) => {
            let mut chars = s.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None;
            }

            // 1. 若右侧紧邻相同的闭合符号，直接跳过
            if is_closing_delim(ch) && text[b_start..].starts_with(ch) {
                return Some(TextPatch {
                    replace_bytes: b_start..b_start,
                    insert: String::new(),
                    new_sel_chars: (sel_chars.start + 1)..(sel_chars.start + 1),
                });
            }

            // 2. 开符号自动补全（不补全 * _ ~ =）
            let close = open_pair_close(ch)?;
            if ch == '\'' && text[..b_start].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            if ch == '`' && text[..b_start].ends_with("``") {
                return None;
            }

            Some(TextPatch {
                replace_bytes: b_start..b_start,
                insert: format!("{ch}{close}"),
                new_sel_chars: (sel_chars.start + 1)..(sel_chars.start + 1),
            })
        }
    }
}

/// 回车围栏补全：在未闭合的 ```` ```lang ```` 行尾按 Enter 时自动补全闭合围栏并将光标停在中间空行。
pub fn fence_enter_patch(text: &str, sel_chars: Range<usize>) -> Option<TextPatch> {
    if sel_chars.start != sel_chars.end {
        return None;
    }
    let b_pos = char_to_byte(text, sel_chars.start);
    let after = &text[b_pos..];
    if !after.is_empty() && !after.starts_with('\n') && !after.starts_with("\r\n") {
        return None;
    }

    let line_start = text[..b_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_before = &text[line_start..b_pos];
    let trimmed = line_before.trim_start();
    let indent = &line_before[..line_before.len() - trimmed.len()];
    let fence_ch = trimmed.chars().next().filter(|&c| c == '`' || c == '~')?;
    let fence_len = trimmed.chars().take_while(|&c| c == fence_ch).count();
    if fence_len < 3 {
        return None;
    }
    let info = trimmed[fence_len..].trim();
    if info.contains(fence_ch) {
        return None;
    }

    // 统计全文该围栏符号的围栏行总数，奇数说明尚未闭合
    let mut fence_lines = 0usize;
    for line in text.lines() {
        let t = line.trim_start();
        let cnt = t.chars().take_while(|&c| c == fence_ch).count();
        if cnt >= 3 {
            fence_lines += 1;
        }
    }
    if fence_lines.is_multiple_of(2) {
        return None;
    }

    let fence: String = std::iter::repeat_n(fence_ch, fence_len).collect();
    let insert = format!("\n{indent}\n{indent}{fence}");
    let mid_cursor = sel_chars.start + 1 + indent.chars().count();
    Some(TextPatch {
        replace_bytes: b_pos..b_pos,
        insert,
        new_sel_chars: mid_cursor..mid_cursor,
    })
}

pub fn toggle_inline_code(ctx: &egui::Context, doc: &mut Document) {
    apply_edit(ctx, doc, |text, before| {
        let r = sorted_char_range(before);
        let b0 = char_to_byte(text, r.start);
        let b1 = char_to_byte(text, r.end);
        let sel = &text[b0..b1];
        if r.start < r.end {
            let insert = format!("`{sel}`");
            text.replace_range(b0..b1, &insert);
            CCursorRange::two(CCursor::new(r.start + 1), CCursor::new(r.end + 1))
        } else {
            text.replace_range(b0..b0, "``");
            CCursorRange::one(CCursor::new(r.start + 1))
        }
    });
}

pub fn insert_code_block(ctx: &egui::Context, doc: &mut Document) {
    apply_edit(ctx, doc, |text, before| {
        let r = sorted_char_range(before);
        let b0 = char_to_byte(text, r.start);
        let b1 = char_to_byte(text, r.end);
        let prefix = if b0 > 0 && !text[..b0].ends_with('\n') { "\n" } else { "" };
        let suffix = if b1 < text.len() && !text[b1..].starts_with('\n') { "\n" } else { "" };
        if r.start < r.end {
            let sel = text[b0..b1].to_string();
            let insert = format!("{prefix}```\n{sel}\n```{suffix}");
            text.replace_range(b0..b1, &insert);
            let start_c = byte_to_char(text, b0 + prefix.len() + 4);
            let end_c = start_c + sel.chars().count();
            CCursorRange::two(CCursor::new(start_c), CCursor::new(end_c))
        } else {
            let insert = format!("{prefix}```\n\n```{suffix}");
            text.replace_range(b0..b0, &insert);
            let cur_c = byte_to_char(text, b0 + prefix.len() + 4);
            CCursorRange::one(CCursor::new(cur_c))
        }
    });
}

pub struct EditorConfig {
    pub font_size: f32,
    pub max_content_width: f32,
    pub auto_pair: bool,
    pub auto_pair_in_code: bool,
    pub paste_html_as_md: bool,
    pub base_dir: Option<PathBuf>,
}

#[derive(Clone, Copy)]
enum ContextAction {
    Cut,
    Copy,
    Paste,
    Cmd(Command),
}

fn process_pre_events(ctx: &egui::Context, doc: &mut Document, cfg: &EditorConfig) {
    let id = editor_id(doc.uid);
    if !ctx.memory(|m| m.has_focus(id)) {
        return;
    }

    let ime_id = egui::Id::unique(("editor_ime", doc.uid));
    let mut composing = ctx.data_mut(|d| d.get_temp::<bool>(ime_id).unwrap_or(false));
    let mut events = ctx.input_mut(|i| std::mem::take(&mut i.events));
    let mut i = 0;

    while i < events.len() {
        let mut consumed = false;
        match &events[i] {
            Event::Paste(s) => {
                smart_paste(ctx, doc, Some(s), cfg.paste_html_as_md, cfg.base_dir.as_deref());
                consumed = true;
            }
            Event::PasteImage(img) => {
                if let Ok((_, url)) =
                    clipboard::save_rgba_image(img.width(), img.height(), img.as_raw(), cfg.base_dir.as_deref())
                {
                    insert_text(ctx, doc, &format!("![]({url})"));
                }
                consumed = true;
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                composing = !text.is_empty();
            }
            Event::Ime(ImeEvent::Commit(s)) => {
                composing = false;
                let sel = current_selection_chars(ctx, doc.uid, &doc.text);
                if let Some(patch) = autopair(
                    &doc.text,
                    sel,
                    AutoPairInput::Text(s.clone()),
                    cfg.auto_pair,
                    cfg.auto_pair_in_code,
                ) {
                    apply_text_patch(ctx, doc, patch);
                    consumed = true;
                }
            }
            Event::Text(s) if !composing => {
                let sel = current_selection_chars(ctx, doc.uid, &doc.text);
                if let Some(patch) = autopair(
                    &doc.text,
                    sel,
                    AutoPairInput::Text(s.clone()),
                    cfg.auto_pair,
                    cfg.auto_pair_in_code,
                ) {
                    apply_text_patch(ctx, doc, patch);
                    consumed = true;
                }
            }
            Event::Key { key: Key::Backspace, pressed: true, modifiers, .. }
                if !composing && !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift =>
            {
                let sel = current_selection_chars(ctx, doc.uid, &doc.text);
                if let Some(patch) = autopair(
                    &doc.text,
                    sel,
                    AutoPairInput::Backspace,
                    cfg.auto_pair,
                    cfg.auto_pair_in_code,
                ) {
                    apply_text_patch(ctx, doc, patch);
                    consumed = true;
                }
            }
            Event::Key { key: Key::Enter, pressed: true, modifiers, .. }
                if !composing && !modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift =>
            {
                let sel = current_selection_chars(ctx, doc.uid, &doc.text);
                if let Some(patch) = fence_enter_patch(&doc.text, sel) {
                    apply_text_patch(ctx, doc, patch);
                    consumed = true;
                }
            }
            _ => {}
        }

        if consumed {
            events.remove(i);
        } else {
            i += 1;
        }
    }

    ctx.input_mut(|i| i.events = events);
    ctx.data_mut(|d| d.insert_temp(ime_id, composing));
}

pub fn show(ui: &mut egui::Ui, doc: &mut Document, cfg: &EditorConfig) -> Option<Command> {
    let ctx = ui.ctx().clone();
    process_pre_events(&ctx, doc, cfg);

    let id = editor_id(doc.uid);
    let mut scroll_target_char: Option<usize> = None;
    if let Some(byte_range) = doc.pending_selection.take() {
        let c0 = byte_to_char(&doc.text, byte_range.start);
        let c1 = byte_to_char(&doc.text, byte_range.end);
        let cr = if c0 == c1 {
            CCursorRange::one(CCursor::new(c0))
        } else {
            CCursorRange::two(CCursor::new(c0), CCursor::new(c1))
        };
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(cr));
        state.store(&ctx, id);
        if doc.scroll_to_selection {
            scroll_target_char = Some(c0);
            doc.scroll_to_selection = false;
        }
    }

    let theme_id = crate::theme::current_id(&ctx);
    let font_id = egui::FontId::new(cfg.font_size, FontFamily::Name("editor".into()));
    let uid = doc.uid;
    let version = doc.version;
    let max_width = cfg.max_content_width;
    let has_selection = {
        let r = current_selection_chars(&ctx, uid, &doc.text);
        r.start < r.end
    };
    let mut changed = false;
    let mut top_byte = doc.editor_top_byte;
    let mut ctx_action: Option<ContextAction> = None;

    {
        let Document { text, hl, find, .. } = doc;
        let find_synced = find.open && find.is_synced_with(version) && !find.matches.is_empty();
        let find_text_sig = find_synced.then(|| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(text.as_bytes(), &mut h);
            (text.len(), std::hash::Hasher::finish(&h))
        });

        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let s = buf.as_str();
            let overlay = find_text_sig.and_then(|(orig_len, orig_hash)| {
                if s.len() != orig_len {
                    return None;
                }
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(s.as_bytes(), &mut h);
                (std::hash::Hasher::finish(&h) == orig_hash).then_some(FindOverlay {
                    matches: find.matches.as_slice(),
                    current: Some(find.current),
                    key: find.overlay_key,
                })
            });
            let mut job = hl.highlight(version, s, theme_id, &font_id, overlay);
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        egui::ScrollArea::vertical()
            .id_salt(("editor_scroll", uid))
            .auto_shrink(false)
            .show(ui, |ui| {
                let avail_size = ui.available_size();
                let (pad, target_w) = if max_width > 0.0 && avail_size.x > max_width + 24.0 {
                    (((avail_size.x - max_width) * 0.5).max(0.0), max_width)
                } else {
                    (0.0, f32::INFINITY)
                };
                let min_sz = if pad > 0.0 { vec2(target_w, avail_size.y) } else { avail_size };
                let mut render_edit = |ui: &mut egui::Ui| {
                    let cur_line_slot = ui.painter().add(egui::Shape::Noop);
                    let out = TextEdit::multiline(text)
                        .id(id)
                        .desired_width(target_w)
                        .min_size(min_sz)
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 18, right: 16, top: 14, bottom: 32 }))
                        .lock_focus(true)
                        .layouter(&mut layouter)
                        .show(ui);
                    if out.response.response.has_focus()
                        && let Some(cr) = out.cursor_range
                        && cr.primary == cr.secondary
                    {
                        let spec = crate::theme::current(&ctx);
                        let c_rect = out.galley.pos_from_cursor(cr.primary).translate(out.galley_pos.to_vec2());
                        let band = egui::Rect::from_min_max(
                            egui::pos2(out.response.response.rect.left(), c_rect.top()),
                            egui::pos2(out.response.response.rect.right(), c_rect.bottom()),
                        );
                        ui.painter().set(
                            cur_line_slot,
                            egui::Shape::rect_filled(band, 0.0, spec.editor.current_line_bg),
                        );
                    }
                    out
                };
                let output = if pad > 0.0 {
                    ui.horizontal(|ui| {
                        ui.add_space(pad);
                        render_edit(ui)
                    })
                    .inner
                } else {
                    render_edit(ui)
                };

                let rel_y = (ui.clip_rect().top() - output.galley_pos.y).max(0.0);
                let top_char = output.galley.cursor_from_pos(vec2(0.0, rel_y)).index.0;
                top_byte = char_to_byte(text, top_char);

                if let Some(c) = scroll_target_char {
                    let rect = output
                        .galley
                        .pos_from_cursor(CCursor::new(c))
                        .translate(output.galley_pos.to_vec2());
                    ui.scroll_to_rect(rect.expand2(vec2(0.0, 28.0)), Some(Align::Center));
                }
                if output.response.changed() {
                    changed = true;
                }
                let resp = output.response.response;
                resp.context_menu(|ui| {
                    if ui.add_enabled(has_selection, egui::Button::new("剪切\tCtrl+X")).clicked() {
                        ctx_action = Some(ContextAction::Cut);
                        ui.close();
                    }
                    if ui.add_enabled(has_selection, egui::Button::new("复制\tCtrl+C")).clicked() {
                        ctx_action = Some(ContextAction::Copy);
                        ui.close();
                    }
                    if ui.button("粘贴\tCtrl+V").clicked() {
                        ctx_action = Some(ContextAction::Paste);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("纯文本粘贴\tCtrl+Shift+V").clicked() {
                        ctx_action = Some(ContextAction::Cmd(Command::PastePlain));
                        ui.close();
                    }
                    if ui.button("复制为 Markdown").clicked() {
                        ctx_action = Some(ContextAction::Cmd(Command::CopyAsMarkdown));
                        ui.close();
                    }
                    if ui.button("复制为 HTML\tCtrl+Shift+C").clicked() {
                        ctx_action = Some(ContextAction::Cmd(Command::CopyAsHtml));
                        ui.close();
                    }
                });
            });
    }

    doc.editor_top_byte = top_byte;
    if changed {
        let was_clean = !doc.dirty;
        doc.dirty = true;
        doc.bump_version();
        doc.find.invalidate();
        if was_clean || doc.find.open {
            ctx.request_repaint();
        }
    }

    let mut command = None;
    if let Some(act) = ctx_action {
        match act {
            ContextAction::Cut => {
                if let Some(sel) = selected_text(&ctx, doc) {
                    let _ = clipboard::set_text(&sel);
                    insert_text(&ctx, doc, "");
                }
            }
            ContextAction::Copy => {
                if let Some(sel) = selected_text(&ctx, doc) {
                    let _ = clipboard::set_text(&sel);
                }
            }
            ContextAction::Paste => {
                smart_paste(&ctx, doc, None, cfg.paste_html_as_md, cfg.base_dir.as_deref());
            }
            ContextAction::Cmd(c) => command = Some(c),
        }
        ctx.memory_mut(|m| m.request_focus(id));
    }

    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_byte_conversion_with_cjk_and_emoji() {
        let s = "a中🚀b";
        assert_eq!(char_to_byte(s, 0), 0);
        assert_eq!(char_to_byte(s, 1), 1);
        assert_eq!(char_to_byte(s, 2), 4);
        assert_eq!(char_to_byte(s, 3), 8);
        assert_eq!(char_to_byte(s, 4), 9);
        assert_eq!(byte_to_char(s, 4), 2);
        assert_eq!(byte_to_char(s, 6), 2); // 中间字节安全回退
        assert_eq!(byte_to_char(s, 9), 4);
    }

    #[test]
    fn autopair_rules_cover_all_cases() {
        // 1. 有选区包裹（含 * _ ~ = 和中文括号）
        let mut s = "hello世界".to_string();
        let p = autopair(&s, 2..5, AutoPairInput::Text("*".into()), true, true).unwrap();
        p.apply(&mut s);
        assert_eq!(s, "he*llo*世界");
        assert_eq!(p.new_sel_chars, 3..6);

        // 连续再按一次 * 得到 **llo**
        let p2 = autopair(&s, p.new_sel_chars, AutoPairInput::Text("*".into()), true, true).unwrap();
        p2.apply(&mut s);
        assert_eq!(s, "he**llo**世界");

        // 2. 无选区不补全 * _ ~ =
        assert!(autopair("abc", 1..1, AutoPairInput::Text("*".into()), true, true).is_none());
        assert!(autopair("abc", 1..1, AutoPairInput::Text("~".into()), true, true).is_none());

        // 3. 中文 IME Commit 补全与 Backspace 空对同删
        let mut cn = "测试".to_string();
        let p_cn = autopair(&cn, 2..2, AutoPairInput::Text("（".into()), true, true).unwrap();
        p_cn.apply(&mut cn);
        assert_eq!(cn, "测试（）");
        assert_eq!(p_cn.new_sel_chars, 3..3);

        let p_bs = autopair(&cn, 3..3, AutoPairInput::Backspace, true, true).unwrap();
        p_bs.apply(&mut cn);
        assert_eq!(cn, "测试");
        assert_eq!(p_bs.new_sel_chars, 2..2);

        // 4. 跳过已有右括号
        let skip = autopair("(a)", 2..2, AutoPairInput::Text(")".into()), true, true).unwrap();
        assert_eq!(skip.insert, "");
        assert_eq!(skip.new_sel_chars, 3..3);

        // 5. don't 不补全单引号
        assert!(autopair("don", 3..3, AutoPairInput::Text("'".into()), true, true).is_none());

        // 6. 第三个反引号不补全第 4 个
        assert!(autopair("``", 2..2, AutoPairInput::Text("`".into()), true, true).is_none());

        // 7. 代码块内开关控制
        let code = "```rust\nlet x = ";
        let c_len = code.chars().count();
        assert!(autopair(code, c_len..c_len, AutoPairInput::Text("(".into()), true, false).is_none());
        assert!(autopair(code, c_len..c_len, AutoPairInput::Text("(".into()), true, true).is_some());
    }

    #[test]
    fn fence_enter_completes_unclosed_block_only() {
        let mut unclosed = "  ```rust".to_string();
        let c = unclosed.chars().count();
        let p = fence_enter_patch(&unclosed, c..c).unwrap();
        p.apply(&mut unclosed);
        assert_eq!(unclosed, "  ```rust\n  \n  ```");
        assert_eq!(p.new_sel_chars, 12..12);

        let closed = "```rust\n\n```";
        assert!(fence_enter_patch(closed, 7..7).is_none());
    }

    fn collect_rects_and_texts<'a>(
        shape: &'a egui::Shape,
        rects: &mut Vec<&'a egui::epaint::RectShape>,
        texts: &mut Vec<&'a egui::epaint::TextShape>,
    ) {
        match shape {
            egui::Shape::Rect(r) => rects.push(r),
            egui::Shape::Text(t) => texts.push(t),
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_rects_and_texts(s, rects, texts);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn typing_at_end_of_wrapped_line_never_jumps_cursor_to_next_line() {
        let ctx = egui::Context::default();
        crate::fonts::setup(&ctx);
        crate::theme::apply(&ctx, crate::theme::ThemeId::ZenHorizon);
        let spec = crate::theme::current(&ctx);

        let initial = "## 贡献\n\n\
            fjhoisafhiaofhioahfioahsiofhiffasfasfafsaffsafasuigiugyu8og8ygyo8ug8yog78yg87jorjpoasfujaopsfaspofjaspfsadspoadiposadioasdisaodisaodisapdosaidoasidpoasdiasoiopiio\
            pppppppppppppppppppp\n\
            欢迎通过 [Issue](https://github.com/czh26/ThunderMD/issues) 反馈问题\n";
        let mut doc = Document::from_file("test.md".into(), initial.into());
        let nl_byte = doc.text.find("\n欢迎通过").unwrap();
        doc.pending_selection = Some(nl_byte..nl_byte);

        let ed_id = editor_id(doc.uid);
        ctx.memory_mut(|m| m.request_focus(ed_id));

        let cfg = EditorConfig {
            font_size: 16.0,
            max_content_width: 0.0,
            auto_pair: true,
            auto_pair_in_code: true,
            paste_html_as_md: false,
            base_dir: None,
        };

        let run_frame = |doc: &mut Document, events: Vec<Event>| -> (f32, egui::Rect, Option<egui::Rect>) {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(480.0, 600.0))),
                focused: true,
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = show(ui, doc, &cfg);
                });
            });
            out.textures_delta.clear();

            let mut rects = Vec::new();
            let mut texts = Vec::new();
            for cs in &out.shapes {
                collect_rects_and_texts(&cs.shape, &mut rects, &mut texts);
            }
            let ed_text = texts
                .iter()
                .find(|t| t.galley.text().contains("欢迎通过"))
                .expect("editor galley should be painted");
            let huan_row = ed_text
                .galley
                .rows
                .iter()
                .find(|r| r.glyphs.iter().any(|g| g.chr == '欢'))
                .expect("row containing '欢' should exist");
            let huan_row_top = ed_text.pos.y + huan_row.pos.y;
            let band = rects
                .iter()
                .find(|r| r.fill == spec.editor.current_line_bg)
                .expect("current line highlight band should be painted")
                .rect;
            let ime_cursor = out.platform_output.ime.map(|ime| ime.cursor_rect);
            (huan_row_top, band, ime_cursor)
        };

        // 第 0 帧完成聚焦与初始布局，后续每帧在行尾输入 'p' 并紧跟空闲帧验证不跳行
        let (huan_top, band, ime) = run_frame(&mut doc, Vec::new());
        assert!(band.center().y < huan_top, "initial frame band={band:?} huan_top={huan_top}");
        if let Some(c) = ime {
            assert!(c.center().y < huan_top, "initial frame ime={c:?} huan_top={huan_top}");
        }

        for step in 0..5 {
            let (huan_top, band, ime) = run_frame(&mut doc, vec![Event::Text("p".into())]);
            assert!(
                band.center().y < huan_top && band.bottom() <= huan_top + 1.0,
                "keystroke step {step} jumped to next line: band={band:?}, huan_top={huan_top}"
            );
            if let Some(c) = ime {
                assert!(
                    c.center().y < huan_top,
                    "keystroke step {step} ime jumped to next line: ime={c:?}, huan_top={huan_top}"
                );
            }

            let (huan_top_idle, band_idle, ime_idle) = run_frame(&mut doc, Vec::new());
            assert!(
                band_idle.center().y < huan_top_idle && band_idle.bottom() <= huan_top_idle + 1.0,
                "idle step {step} drifted: band={band_idle:?}, huan_top={huan_top_idle}"
            );
            if let Some(c) = ime_idle {
                assert!(c.center().y < huan_top_idle);
            }
        }

        assert!(doc.text.contains("ppppppppppppppppppppppppp\n欢迎通过"));
    }

    #[test]
    fn typing_before_cjk_with_open_find_bar_never_panics() {
        let ctx = egui::Context::default();
        crate::fonts::setup(&ctx);
        crate::theme::apply(&ctx, crate::theme::ThemeId::ZenHorizon);

        let initial = "pppp\n欢迎通过\n";
        let mut doc = Document::from_file("find_test.md".into(), initial.into());
        let nl_byte = doc.text.find("\n欢迎通过").unwrap();
        doc.pending_selection = Some(nl_byte..nl_byte);
        doc.find.open = true;
        doc.find.query = "欢迎".into();
        doc.find.ensure_updated(&ctx, &doc.text, doc.version, true);
        let old_match = doc.find.matches[0].clone();

        ctx.memory_mut(|m| m.request_focus(editor_id(doc.uid)));
        let cfg = EditorConfig {
            font_size: 16.0,
            max_content_width: 0.0,
            auto_pair: true,
            auto_pair_in_code: true,
            paste_html_as_md: false,
            base_dir: None,
        };

        let run_step = |doc: &mut Document, events: Vec<Event>| {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(480.0, 400.0))),
                focused: true,
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = show(ui, doc, &cfg);
                });
            });
            out.textures_delta.clear();
        };

        run_step(&mut doc, Vec::new());
        run_step(&mut doc, vec![Event::Text("p".into())]);
        doc.find.ensure_updated(&ctx, &doc.text, doc.version, false);
        run_step(&mut doc, Vec::new());

        doc.find.ensure_updated(&ctx, &doc.text, doc.version, true);
        assert_eq!(doc.text, "ppppp\n欢迎通过\n");
        assert_eq!(doc.find.matches, vec![(old_match.start + 1)..(old_match.end + 1)]);
    }
}
