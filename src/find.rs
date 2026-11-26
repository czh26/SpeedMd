//! 查找与替换：防抖匹配、正则替换、状态缓存与栏位界面。

use eframe::egui::{self, Color32, Key, Stroke};
use regex::{Regex, RegexBuilder};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::time::{Duration, Instant};

#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct QuerySpec {
    pub query: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
}

#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub show_replace: bool,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub current: usize,
    pub focus_query: bool,
    last_edit: Option<Instant>,
    cached_key: Option<(u64, QuerySpec)>,
    pub matches: Vec<Range<usize>>,
    pub error: Option<String>,
    pub overlay_key: u64,
}

pub enum FindAction {
    JumpTo(Range<usize>),
    ReplaceCurrent,
    ReplaceAll,
    Close,
}

impl FindState {
    pub fn spec(&self) -> QuerySpec {
        QuerySpec {
            query: self.query.clone(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            use_regex: self.use_regex,
        }
    }

    pub fn mark_edited(&mut self) {
        self.last_edit = Some(Instant::now());
    }

    pub fn invalidate(&mut self) {
        self.cached_key = None;
    }

    /// 根据文档版本号与防抖状态更新匹配结果；`immediate == true` 时立即重算。
    pub fn ensure_updated(&mut self, ctx: &egui::Context, text: &str, doc_version: u64, immediate: bool) {
        if !self.open {
            return;
        }
        let spec = self.spec();
        if self.cached_key.as_ref().is_some_and(|(v, s)| *v == doc_version && *s == spec) {
            return;
        }

        if !immediate && let Some(t) = self.last_edit {
            let delay = Duration::from_millis(150);
            let elapsed = t.elapsed();
            if elapsed < delay {
                ctx.request_repaint_after(delay - elapsed);
                return;
            }
        }

        match compute_matches(text, &spec) {
            Ok(m) => {
                self.matches = m;
                self.error = None;
                if self.matches.is_empty() {
                    self.current = 0;
                } else if self.current >= self.matches.len() {
                    self.current = self.matches.len() - 1;
                }
            }
            Err(err) => {
                self.matches.clear();
                self.error = Some(err);
                self.current = 0;
            }
        }

        let mut h = DefaultHasher::new();
        (doc_version, &spec, self.matches.len()).hash(&mut h);
        self.overlay_key = h.finish();
        self.cached_key = Some((doc_version, spec));
    }

    pub fn current_range(&self) -> Option<Range<usize>> {
        self.matches.get(self.current).cloned()
    }

    pub fn step(&mut self, forward: bool) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        if forward {
            self.current = (self.current + 1) % self.matches.len();
        } else {
            self.current = (self.current + self.matches.len() - 1) % self.matches.len();
        }
        self.current_range()
    }

    pub fn select_near_offset(&mut self, byte_off: usize) {
        if self.matches.is_empty() {
            self.current = 0;
            return;
        }
        self.current = self
            .matches
            .iter()
            .position(|r| r.start >= byte_off)
            .unwrap_or(0);
    }
}

fn build_regex(spec: &QuerySpec) -> Result<Regex, String> {
    let pattern: Cow<'_, str> = if spec.use_regex {
        Cow::Borrowed(spec.query.as_str())
    } else {
        Cow::Owned(regex::escape(&spec.query))
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!spec.case_sensitive)
        .build()
        .map_err(|e| e.to_string())
}

fn is_ascii_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn whole_word_ok(text: &str, start: usize, end: usize) -> bool {
    let slice = &text[start..end];
    if let Some(first) = slice.chars().next()
        && is_ascii_word(first)
        && text[..start].chars().next_back().is_some_and(is_ascii_word)
    {
        return false;
    }
    if let Some(last) = slice.chars().next_back()
        && is_ascii_word(last)
        && text[end..].chars().next().is_some_and(is_ascii_word)
    {
        return false;
    }
    true
}

pub fn compute_matches(text: &str, spec: &QuerySpec) -> Result<Vec<Range<usize>>, String> {
    if spec.query.is_empty() {
        return Ok(Vec::new());
    }
    let re = build_regex(spec)?;
    let mut out = Vec::new();
    for m in re.find_iter(text) {
        if m.is_empty() {
            continue;
        }
        if spec.whole_word && !whole_word_ok(text, m.start(), m.end()) {
            continue;
        }
        out.push(m.start()..m.end());
    }
    Ok(out)
}

pub fn replace_one(
    text: &str,
    range: Range<usize>,
    replacement: &str,
    spec: &QuerySpec,
) -> Result<(String, Range<usize>), String> {
    let expanded = if spec.use_regex {
        let re = build_regex(spec)?;
        re.replace(&text[range.clone()], replacement).into_owned()
    } else {
        replacement.to_string()
    };
    let mut out = String::with_capacity(text.len() - range.len() + expanded.len());
    out.push_str(&text[..range.start]);
    out.push_str(&expanded);
    out.push_str(&text[range.end..]);
    let new_range = range.start..(range.start + expanded.len());
    Ok((out, new_range))
}

pub fn replace_all(
    text: &str,
    matches: &[Range<usize>],
    replacement: &str,
    spec: &QuerySpec,
) -> Result<(String, usize), String> {
    if matches.is_empty() {
        return Ok((text.to_string(), 0));
    }
    let re = if spec.use_regex { Some(build_regex(spec)?) } else { None };
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut count = 0usize;
    for r in matches {
        if r.start < last || r.end > text.len() {
            continue;
        }
        out.push_str(&text[last..r.start]);
        if let Some(re) = &re {
            let rep = re.replace(&text[r.clone()], replacement);
            out.push_str(&rep);
        } else {
            out.push_str(replacement);
        }
        last = r.end;
        count += 1;
    }
    out.push_str(&text[last..]);
    Ok((out, count))
}

pub fn bar_ui(ui: &mut egui::Ui, state: &mut FindState) -> Option<FindAction> {
    if !state.open {
        return None;
    }
    let mut action: Option<FindAction> = None;

    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut query_edit = egui::TextEdit::singleline(&mut state.query)
                    .desired_width(220.0)
                    .hint_text("查找…");
                if state.error.is_some() {
                    query_edit = query_edit.text_color(Color32::from_rgb(0xe0, 0x48, 0x48));
                }
                let q_frame = if state.error.is_some() {
                    Stroke::new(1.5, Color32::from_rgb(0xe0, 0x48, 0x48))
                } else {
                    Stroke::NONE
                };
                let q_resp = egui::Frame::new()
                    .stroke(q_frame)
                    .corner_radius(3)
                    .show(ui, |ui| ui.add(query_edit))
                    .inner;
                if state.focus_query {
                    q_resp.request_focus();
                    state.focus_query = false;
                }
                if q_resp.changed() {
                    state.mark_edited();
                }

                if q_resp.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let backward = ui.input(|i| i.modifiers.shift);
                    if let Some(r) = state.step(!backward) {
                        action = Some(FindAction::JumpTo(r));
                    }
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    action = Some(FindAction::Close);
                }

                if ui.selectable_label(state.case_sensitive, "Aa").on_hover_text("区分大小写").clicked() {
                    state.case_sensitive = !state.case_sensitive;
                    state.invalidate();
                }
                if ui.selectable_label(state.whole_word, "W").on_hover_text("全词匹配").clicked() {
                    state.whole_word = !state.whole_word;
                    state.invalidate();
                }
                if ui.selectable_label(state.use_regex, ".*").on_hover_text("正则表达式").clicked() {
                    state.use_regex = !state.use_regex;
                    state.invalidate();
                }

                let count_text = if let Some(err) = &state.error {
                    egui::RichText::new(format!("正则错误：{err}")).color(Color32::from_rgb(0xe0, 0x48, 0x48))
                } else if state.query.is_empty() {
                    egui::RichText::new("").weak()
                } else if state.matches.is_empty() {
                    egui::RichText::new("无匹配").color(Color32::from_rgb(0xe0, 0x9a, 0x3e))
                } else {
                    egui::RichText::new(format!("{} / {}", state.current + 1, state.matches.len())).weak()
                };
                ui.label(count_text);

                if ui.button("↑").on_hover_text("上一个 (Shift+F3)").clicked()
                    && let Some(r) = state.step(false)
                {
                    action = Some(FindAction::JumpTo(r));
                }
                if ui.button("↓").on_hover_text("下一个 (F3)").clicked()
                    && let Some(r) = state.step(true)
                {
                    action = Some(FindAction::JumpTo(r));
                }
                if ui.selectable_label(state.show_replace, "替换").clicked() {
                    state.show_replace = !state.show_replace;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").on_hover_text("关闭 (Esc)").clicked() {
                        action = Some(FindAction::Close);
                    }
                });
            });

            if state.show_replace {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let r_resp = ui.add(
                        egui::TextEdit::singleline(&mut state.replacement)
                            .desired_width(220.0)
                            .hint_text("替换为…"),
                    );
                    if r_resp.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        action = Some(FindAction::ReplaceCurrent);
                    }
                    let has_matches = !state.matches.is_empty() && state.error.is_none();
                    if ui.add_enabled(has_matches, egui::Button::new("替换")).clicked() {
                        action = Some(FindAction::ReplaceCurrent);
                    }
                    if ui.add_enabled(has_matches, egui::Button::new("全部替换")).clicked() {
                        action = Some(FindAction::ReplaceAll);
                    }
                });
            }
        });

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_case_and_cjk_search() {
        let text = "Hello world, hello 世界, HELLO!";
        let spec = QuerySpec { query: "hello".into(), case_sensitive: false, whole_word: false, use_regex: false };
        let m = compute_matches(text, &spec).unwrap();
        assert_eq!(m.len(), 3);

        let spec_case = QuerySpec { case_sensitive: true, ..spec.clone() };
        let m2 = compute_matches(text, &spec_case).unwrap();
        assert_eq!(m2.len(), 1);

        let spec_cjk = QuerySpec { query: "世界".into(), ..spec };
        let m3 = compute_matches(text, &spec_cjk).unwrap();
        assert_eq!(m3.len(), 1);
        assert_eq!(&text[m3[0].clone()], "世界");
    }

    #[test]
    fn whole_word_and_regex_capture_replace() {
        let text = "word wording word_2 用word写";
        let spec = QuerySpec { query: "word".into(), case_sensitive: true, whole_word: true, use_regex: false };
        let m = compute_matches(text, &spec).unwrap();
        assert_eq!(m.len(), 2);

        let src = "foo-12 bar-34";
        let rspec = QuerySpec { query: r"([a-z]+)-(\d+)".into(), case_sensitive: true, whole_word: false, use_regex: true };
        let rm = compute_matches(src, &rspec).unwrap();
        assert_eq!(rm.len(), 2);
        let (replaced, count) = replace_all(src, &rm, "$2:$1", &rspec).unwrap();
        assert_eq!(count, 2);
        assert_eq!(replaced, "12:foo 34:bar");

        // 非正则模式下 `$1` 保持字面量
        let lit_spec = QuerySpec { query: "12".into(), case_sensitive: true, whole_word: false, use_regex: false };
        let lm = compute_matches(src, &lit_spec).unwrap();
        let (lit_out, _) = replace_one(src, lm[0].clone(), "$1", &lit_spec).unwrap();
        assert_eq!(lit_out, "foo-$1 bar-34");
    }

    #[test]
    fn invalid_regex_returns_err_without_panic() {
        let spec = QuerySpec { query: "([unclosed".into(), case_sensitive: false, whole_word: false, use_regex: true };
        assert!(compute_matches("abc", &spec).is_err());
    }
}
