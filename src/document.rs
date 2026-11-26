//! 单文档状态与解析/统计缓存。

use crate::highlight::Highlighter;
use crate::markdown::{self, Doc, ParseOptions, Theme};
use crate::preview::PreviewState;
use crate::settings::ViewMode;
use eframe::egui;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

static NEXT_UID: AtomicU64 = AtomicU64::new(1);

#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub chars: usize,
    pub words: usize,
    pub lines: usize,
}

impl Stats {
    pub fn of(text: &str) -> Self {
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

#[derive(Default)]
pub struct ParseCache {
    key: u64,
    cached: Option<(Arc<Doc>, Theme)>,
}

impl ParseCache {
    #[allow(clippy::too_many_arguments)]
    pub fn ensure_parsed(
        &mut self,
        ctx: &egui::Context,
        doc_text: &str,
        version: u64,
        dark: bool,
        font_size: f32,
        opts: &ParseOptions,
        immediate: bool,
        last_edit_at: Option<Instant>,
    ) {
        let mut h = DefaultHasher::new();
        (version, dark, font_size.to_bits(), opts.sup_sub, opts.front_matter).hash(&mut h);
        let key = h.finish();
        if self.cached.is_some() && self.key == key {
            return;
        }
        if !immediate && self.cached.is_some() {
            let delay = Duration::from_millis(200);
            if let Some(t) = last_edit_at {
                let elapsed = t.elapsed();
                if elapsed < delay {
                    ctx.request_repaint_after(delay - elapsed);
                    return;
                }
            }
        }
        let th = Theme::new(dark, font_size);
        let doc = markdown::parse(doc_text, &th, opts);
        self.cached = Some((Arc::new(doc), th));
        self.key = key;
    }

    pub fn get(&self) -> Option<(&Doc, &Theme, u64)> {
        self.cached.as_ref().map(|(d, t)| (&**d, t, self.key))
    }
}

pub struct Document {
    pub uid: u64,
    pub text: String,
    pub path: Option<PathBuf>,
    pub base_dir_hint: Option<PathBuf>,
    pub dirty: bool,
    pub is_welcome: bool,
    pub version: u64,
    pub last_edit_at: Option<Instant>,
    pub parsed: ParseCache,
    pub stats: Stats,
    pub preview: PreviewState,
    pub hl: Highlighter,
    #[allow(dead_code)]
    pub pending_cursor: Option<usize>,
    pub view_before_source: Option<ViewMode>,
}

impl Document {
    pub fn ensure_parsed(
        &mut self,
        ctx: &egui::Context,
        dark: bool,
        font_size: f32,
        opts: &ParseOptions,
        immediate: bool,
    ) {
        self.parsed.ensure_parsed(
            ctx,
            &self.text,
            self.version,
            dark,
            font_size,
            opts,
            immediate,
            self.last_edit_at,
        );
    }

    pub fn new_empty() -> Self {
        Self {
            uid: NEXT_UID.fetch_add(1, Ordering::Relaxed),
            text: String::new(),
            path: None,
            base_dir_hint: None,
            dirty: false,
            is_welcome: false,
            version: 1,
            last_edit_at: None,
            parsed: ParseCache::default(),
            stats: Stats::default(),
            preview: PreviewState::default(),
            hl: Highlighter::default(),
            pending_cursor: None,
            view_before_source: None,
        }
    }

    pub fn from_welcome(welcome: &str) -> Self {
        let mut doc = Self::new_empty();
        doc.text = welcome.replace("\r\n", "\n");
        doc.stats = Stats::of(&doc.text);
        doc.is_welcome = true;
        doc
    }

    pub fn from_file(path: PathBuf, text: String) -> Self {
        let mut doc = Self::new_empty();
        doc.stats = Stats::of(&text);
        doc.text = text;
        doc.path = Some(path);
        doc
    }

    pub fn bump_version(&mut self) {
        self.version = self.version.wrapping_add(1);
        self.last_edit_at = Some(Instant::now());
        self.is_welcome = false;
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "未命名".into())
    }

    pub fn base_dir(&self) -> Option<&Path> {
        self.path
            .as_deref()
            .and_then(Path::parent)
            .or(self.base_dir_hint.as_deref())
    }

    pub fn can_reuse_as_blank(&self) -> bool {
        self.path.is_none() && !self.dirty && (self.text.is_empty() || self.is_welcome)
    }

    pub fn toggle_task(&mut self, offset: usize) {
        let b = self.text.as_bytes();
        if b.get(offset) == Some(&b'[') && b.get(offset + 2) == Some(&b']') {
            let new = match b[offset + 1] {
                b' ' => "x",
                b'x' | b'X' => " ",
                _ => return,
            };
            self.text.replace_range(offset + 1..offset + 2, new);
            self.dirty = true;
            self.stats = Stats::of(&self.text);
            self.bump_version();
        }
    }
}
