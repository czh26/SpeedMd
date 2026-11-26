//! 快捷键解析、格式化、冲突检测与帧事件统一匹配。

use crate::commands::Command;
use eframe::egui::{self, Event, Key};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Shortcut {
    pub const fn new(ctrl: bool, shift: bool, alt: bool, key: Key) -> Self {
        Self { ctrl, shift, alt, key }
    }

    pub fn from_event(key: Key, physical_key: Option<Key>, modifiers: egui::Modifiers) -> Option<Self> {
        let actual = if is_modifier_key(key) { return None; } else { physical_key_prefer_logical(key, physical_key) };
        let ctrl = modifiers.ctrl || modifiers.command;
        Some(Self { ctrl, shift: modifiers.shift, alt: modifiers.alt, key: actual })
    }
}

fn is_modifier_key(k: Key) -> bool {
    matches!(
        k,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    )
}

fn physical_key_prefer_logical(logical: Key, physical: Option<Key>) -> Key {
    match physical {
        Some(p)
            if matches!(
                p,
                Key::Num0
                    | Key::Num1
                    | Key::Num2
                    | Key::Num3
                    | Key::Num4
                    | Key::Num5
                    | Key::Num6
                    | Key::Num7
                    | Key::Num8
                    | Key::Num9
                    | Key::Backtick
                    | Key::Minus
                    | Key::Equals
                    | Key::Slash
                    | Key::Comma
                    | Key::Period
                    | Key::Semicolon
                    | Key::Quote
                    | Key::OpenBracket
                    | Key::CloseBracket
                    | Key::Backslash
            ) =>
        {
            p
        }
        _ => logical,
    }
}

pub fn key_display_name(key: Key) -> &'static str {
    match key {
        Key::Equals => "=",
        Key::Plus => "+",
        Key::Minus => "-",
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::Backtick => "`",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Semicolon => ";",
        Key::Quote => "'",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::OpenCurlyBracket => "{",
        Key::CloseCurlyBracket => "}",
        Key::Escape => "Esc",
        _ => key.name(),
    }
}

pub fn parse_key(s: &str) -> Option<Key> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(k) = Key::from_name(t) {
        return Some(k);
    }
    let lower = t.to_ascii_lowercase();
    Some(match lower.as_str() {
        "esc" | "escape" => Key::Escape,
        "del" | "delete" => Key::Delete,
        "ins" | "insert" => Key::Insert,
        "pgup" | "pageup" | "page_up" => Key::PageUp,
        "pgdn" | "pagedown" | "page_down" => Key::PageDown,
        "backspace" | "bksp" => Key::Backspace,
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "up" | "arrowup" => Key::ArrowUp,
        "down" | "arrowdown" => Key::ArrowDown,
        "left" | "arrowleft" => Key::ArrowLeft,
        "right" | "arrowright" => Key::ArrowRight,
        "home" => Key::Home,
        "end" => Key::End,
        "=" | "equals" | "equal" => Key::Equals,
        "+" | "plus" => Key::Plus,
        "-" | "minus" => Key::Minus,
        "/" | "slash" => Key::Slash,
        "\\" | "backslash" => Key::Backslash,
        "`" | "backtick" | "grave" | "backquote" => Key::Backtick,
        "," | "comma" => Key::Comma,
        "." | "period" => Key::Period,
        ";" | "semicolon" => Key::Semicolon,
        "'" | "quote" => Key::Quote,
        "[" | "openbracket" => Key::OpenBracket,
        "]" | "closebracket" => Key::CloseBracket,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => return None,
    })
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&'static str> = Vec::with_capacity(4);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.alt {
            parts.push("Alt");
        }
        parts.push(key_display_name(self.key));
        f.write_str(&parts.join("+"))
    }
}

impl FromStr for Shortcut {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(());
        }
        // 处理以 `+` 作为按键本身的情况，例如 "Ctrl++"
        let (mods_part, key_part) = if let Some(prefix) = s.strip_suffix("++") {
            (prefix.split('+').collect::<Vec<_>>(), "+")
        } else if s == "+" {
            (Vec::new(), "+")
        } else {
            let mut segs: Vec<&str> = s.split('+').collect();
            let last = segs.pop().ok_or(())?;
            (segs, last)
        };

        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        for m in mods_part {
            match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                "" => return Err(()),
                _ => return Err(()),
            }
        }
        let key = parse_key(key_part).ok_or(())?;
        Ok(Self { ctrl, shift, alt, key })
    }
}

#[derive(Clone, Debug)]
pub struct Keymap {
    bindings: BTreeMap<Command, Option<Shortcut>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut bindings = BTreeMap::new();
        for &cmd in Command::ALL {
            bindings.insert(cmd, cmd.default_shortcut());
        }
        Self { bindings }
    }
}

impl Keymap {
    pub fn from_overrides(overrides: &BTreeMap<String, String>) -> Self {
        let mut km = Self::default();
        for (id, raw) in overrides {
            let Some(cmd) = Command::from_id(id) else { continue };
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                km.bindings.insert(cmd, None);
            } else if let Ok(sc) = Shortcut::from_str(trimmed) {
                km.bindings.insert(cmd, Some(sc));
            }
        }
        km
    }

    pub fn to_overrides(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for &cmd in Command::ALL {
            let cur = self.shortcut(cmd);
            if cur != cmd.default_shortcut() {
                let val = cur.map(|s| s.to_string()).unwrap_or_default();
                out.insert(cmd.id().to_string(), val);
            }
        }
        out
    }

    pub fn shortcut(&self, cmd: Command) -> Option<Shortcut> {
        self.bindings.get(&cmd).copied().flatten()
    }

    pub fn label(&self, cmd: Command) -> String {
        self.shortcut(cmd).map(|s| s.to_string()).unwrap_or_default()
    }

    pub fn set(&mut self, cmd: Command, sc: Option<Shortcut>) {
        self.bindings.insert(cmd, sc);
    }

    pub fn reset(&mut self, cmd: Command) {
        self.bindings.insert(cmd, cmd.default_shortcut());
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    pub fn find_conflict(&self, target: Command, sc: Shortcut) -> Option<Command> {
        Command::ALL.iter().copied().find(|&c| c != target && self.shortcut(c) == Some(sc))
    }

    /// 精确匹配并从事件队列中移除命中的按键事件，防止 TextEdit 重复接收。
    pub fn match_and_consume(&self, events: &mut Vec<Event>) -> Vec<Command> {
        let mut matched = Vec::new();
        let mut strip_text_chars: Vec<char> = Vec::new();
        let mut i = 0;

        while i < events.len() {
            let hit = match &events[i] {
                Event::Key { key, physical_key, pressed: true, modifiers, .. } => {
                    self.match_event(*key, *physical_key, *modifiers)
                }
                _ => None,
            };
            if let Some((cmd, key_used)) = hit {
                matched.push(cmd);
                if let Some(ch) = text_char_for_key(key_used) {
                    strip_text_chars.push(ch);
                }
                events.remove(i);
            } else {
                i += 1;
            }
        }

        if !strip_text_chars.is_empty() {
            events.retain(|ev| match ev {
                Event::Text(s) if s.chars().count() == 1 => {
                    let c = s.chars().next().unwrap_or('\0');
                    !strip_text_chars.contains(&c)
                }
                _ => true,
            });
        }

        matched
    }

    fn match_event(&self, key: Key, physical_key: Option<Key>, modifiers: egui::Modifiers) -> Option<(Command, Key)> {
        let ctrl = modifiers.ctrl || modifiers.command;
        let shift = modifiers.shift;
        let alt = modifiers.alt;

        for &cmd in Command::ALL {
            let Some(sc) = self.shortcut(cmd) else { continue };
            if ctrl == sc.ctrl
                && shift == sc.shift
                && alt == sc.alt
                && (key == sc.key || physical_key == Some(sc.key))
            {
                return Some((cmd, sc.key));
            }
        }

        // 兜底快捷键（当对应命令仍处于默认绑定时）
        if ctrl && !shift && !alt {
            if (key == Key::PageDown || physical_key == Some(Key::PageDown))
                && self.shortcut(Command::NextTab) == Command::NextTab.default_shortcut()
            {
                return Some((Command::NextTab, Key::PageDown));
            }
            if (key == Key::PageUp || physical_key == Some(Key::PageUp))
                && self.shortcut(Command::PrevTab) == Command::PrevTab.default_shortcut()
            {
                return Some((Command::PrevTab, Key::PageUp));
            }
        }
        if ctrl
            && !alt
            && (key == Key::Plus || physical_key == Some(Key::Plus))
            && self.shortcut(Command::FontBigger) == Command::FontBigger.default_shortcut()
        {
            return Some((Command::FontBigger, Key::Plus));
        }

        None
    }
}

fn text_char_for_key(k: Key) -> Option<char> {
    match k {
        Key::Tab => Some('\t'),
        Key::Backtick => Some('`'),
        Key::Slash => Some('/'),
        Key::Equals => Some('='),
        Key::Plus => Some('+'),
        Key::Minus => Some('-'),
        Key::Comma => Some(','),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Modifiers;

    #[test]
    fn parse_and_format_roundtrip() {
        let cases = [
            "Ctrl+S",
            "Ctrl+Shift+S",
            "Ctrl+Alt+K",
            "Ctrl+Shift+1",
            "Ctrl+Shift+`",
            "Ctrl+/",
            "Ctrl+=",
            "Ctrl+-",
            "Ctrl+,",
            "F3",
            "Shift+F3",
            "Ctrl+Tab",
            "Ctrl+Shift+Tab",
        ];
        for s in cases {
            let sc: Shortcut = s.parse().expect(s);
            assert_eq!(sc.to_string(), s);
        }
    }

    #[test]
    fn exact_modifier_match_distinguishes_shift() {
        let km = Keymap::default();
        let ctrl_shift_s = Event::Key {
            key: Key::S,
            physical_key: Some(Key::S),
            pressed: true,
            repeat: false,
            modifiers: Modifiers { ctrl: true, shift: true, ..Default::default() },
        };
        let mut events = vec![ctrl_shift_s];
        let cmds = km.match_and_consume(&mut events);
        assert_eq!(cmds, vec![Command::SaveAs]);
        assert!(events.is_empty());
    }

    #[test]
    fn physical_key_matches_shifted_digit_and_backtick() {
        let km = Keymap::default();
        let shift_1 = Event::Key {
            key: Key::Exclamationmark,
            physical_key: Some(Key::Num1),
            pressed: true,
            repeat: false,
            modifiers: Modifiers { ctrl: true, shift: true, ..Default::default() },
        };
        let mut events = vec![shift_1];
        assert_eq!(km.match_and_consume(&mut events), vec![Command::SidebarFiles]);

        let shift_tick = Event::Key {
            key: Key::Backtick,
            physical_key: Some(Key::Backtick),
            pressed: true,
            repeat: false,
            modifiers: Modifiers { ctrl: true, shift: true, ..Default::default() },
        };
        let mut events = vec![shift_tick, Event::Text("`".into())];
        assert_eq!(km.match_and_consume(&mut events), vec![Command::CodeBlock]);
        assert!(events.is_empty());
    }

    #[test]
    fn overrides_and_conflict_detection() {
        let mut overrides = BTreeMap::new();
        overrides.insert("save".into(), "Ctrl+Alt+S".into());
        overrides.insert("quit".into(), "".into());
        overrides.insert("unknown_cmd".into(), "Ctrl+M".into());
        overrides.insert("find".into(), "bad_key_name_xyz".into());

        let km = Keymap::from_overrides(&overrides);
        assert_eq!(km.shortcut(Command::Save), Some("Ctrl+Alt+S".parse().unwrap()));
        assert_eq!(km.shortcut(Command::Quit), None);
        assert_eq!(km.shortcut(Command::Find), Command::Find.default_shortcut());
        assert_eq!(km.find_conflict(Command::Open, "Ctrl+Alt+S".parse().unwrap()), Some(Command::Save));

        let exported = km.to_overrides();
        assert_eq!(exported.get("save").map(String::as_str), Some("Ctrl+Alt+S"));
        assert_eq!(exported.get("quit").map(String::as_str), Some(""));
        assert!(!exported.contains_key("find"));
    }

    #[test]
    fn from_event_and_set_reset() {
        let sc = Shortcut::from_event(
            Key::Exclamationmark,
            Some(Key::Num1),
            Modifiers { ctrl: true, shift: true, ..Default::default() },
        );
        assert_eq!(sc, Some(Shortcut::new(true, true, false, Key::Num1)));
        assert_eq!(
            Shortcut::from_event(Key::ShiftLeft, Some(Key::ShiftLeft), Modifiers::SHIFT),
            None
        );

        let mut km = Keymap::default();
        km.set(Command::Save, None);
        assert_eq!(km.shortcut(Command::Save), None);
        km.reset(Command::Save);
        assert_eq!(km.shortcut(Command::Save), Command::Save.default_shortcut());
        km.set(Command::Find, Some(Shortcut::new(true, true, false, Key::F)));
        km.reset_all();
        assert_eq!(km.shortcut(Command::Find), Command::Find.default_shortcut());
    }
}
