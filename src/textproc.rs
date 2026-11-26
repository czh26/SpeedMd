//! 共享行内文本扫描器（预览与 HTML 导出共用）：
//! 处理 `==高亮==`、词内 `^上标^` / `~下标~`、`:emoji:` 短码与裸 URL 自动链接。

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment<'a> {
    Plain(&'a str),
    MarkToggle,
    Sup(&'a str),
    Sub(&'a str),
    Emoji(&'static str),
    Autolink { text: &'a str, url: String },
}

pub fn tokenize(t: &str, sup_sub: bool, allow_autolink: bool) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let b = t.as_bytes();
    let (mut i, mut start) = (0usize, 0usize);

    while i < b.len() {
        let c = b[i];
        if c == b'=' && b.get(i + 1) == Some(&b'=') {
            if start < i {
                out.push(Segment::Plain(&t[start..i]));
            }
            out.push(Segment::MarkToggle);
            i += 2;
            start = i;
            continue;
        }
        // 词内的 ^上标^ / ~下标~（pulldown-cmark 按 CommonMark 规则不处理词内的情况，如 H~2~O）
        if sup_sub && (c == b'^' || (c == b'~' && b.get(i + 1) != Some(&b'~') && (i == 0 || b[i - 1] != b'~'))) {
            let mut j = i + 1;
            while j < b.len() && b[j] != c && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == c && j > i + 1 && b.get(j + 1) != Some(&c) {
                if start < i {
                    out.push(Segment::Plain(&t[start..i]));
                }
                let inner = &t[i + 1..j];
                out.push(if c == b'^' { Segment::Sup(inner) } else { Segment::Sub(inner) });
                i = j + 1;
                start = i;
                continue;
            }
        }
        if c == b':'
            && let Some((end, emoji)) = emoji_at(t, i)
        {
            if start < i {
                out.push(Segment::Plain(&t[start..i]));
            }
            out.push(Segment::Emoji(emoji));
            i = end;
            start = i;
            continue;
        }
        if allow_autolink
            && (c == b'h' || c == b'w' || c == b'H' || c == b'W')
            && (i == 0 || !b[i - 1].is_ascii_alphanumeric())
            && let Some((end, url)) = url_at(t, i)
        {
            if start < i {
                out.push(Segment::Plain(&t[start..i]));
            }
            out.push(Segment::Autolink { text: &t[i..end], url });
            i = end;
            start = i;
            continue;
        }
        i += 1;
    }

    if start < t.len() {
        out.push(Segment::Plain(&t[start..]));
    }
    out
}

pub fn emoji_at(t: &str, i: usize) -> Option<(usize, &'static str)> {
    let b = t.as_bytes();
    let mut j = i + 1;
    while j < b.len() && j - i <= 40 {
        let c = b[j];
        if c == b':' {
            break;
        }
        if !(c.is_ascii_alphanumeric() || c == b'_' || c == b'+' || c == b'-') {
            return None;
        }
        j += 1;
    }
    if j >= b.len() || b[j] != b':' || j == i + 1 {
        return None;
    }
    emojis::get_by_shortcode(&t[i + 1..j]).map(|e| (j + 1, e.as_str()))
}

pub fn url_at(t: &str, i: usize) -> Option<(usize, String)> {
    let b = &t.as_bytes()[i..];
    let starts = |p: &str| b.len() >= p.len() && b[..p.len()].eq_ignore_ascii_case(p.as_bytes());
    let prefix = if starts("https://") {
        8
    } else if starts("http://") {
        7
    } else if starts("www.") {
        4
    } else {
        return None;
    };
    let mut end = 0;
    while end < b.len() {
        let c = b[end];
        if c.is_ascii_whitespace() || c == b'<' || c >= 0x80 {
            break;
        }
        end += 1;
    }
    while end > prefix && matches!(b[end - 1], b'.' | b',' | b':' | b';' | b'!' | b'?' | b'"' | b'\'' | b'*' | b'_' | b'~') {
        end -= 1;
    }
    if end > prefix && b[end - 1] == b')' {
        let opens = b[..end].iter().filter(|&&c| c == b'(').count();
        let closes = b[..end].iter().filter(|&&c| c == b')').count();
        if closes > opens {
            end -= 1;
        }
    }
    if end <= prefix {
        return None;
    }
    let url = &t[i..i + end];
    if prefix == 4 {
        if !url[4..].contains('.') {
            return None;
        }
        Some((i + end, format!("http://{url}")))
    } else {
        Some((i + end, url.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_highlight_sup_sub_emoji_and_autolinks() {
        let segs = tokenize("看 ==重点==，H~2~O 与 E=mc^2^ :rocket: https://example.com/a).", true, true);
        assert!(segs.contains(&Segment::MarkToggle));
        assert!(segs.contains(&Segment::Sub("2")));
        assert!(segs.contains(&Segment::Sup("2")));
        assert!(segs.contains(&Segment::Emoji("🚀")));
        assert!(segs.contains(&Segment::Autolink {
            text: "https://example.com/a",
            url: "https://example.com/a".to_string(),
        }));
    }

    #[test]
    fn ignores_sup_sub_when_disabled_and_autolink_inside_links() {
        let segs = tokenize("x^2^ www.example.com", false, false);
        assert_eq!(segs, vec![Segment::Plain("x^2^ www.example.com")]);
    }
}
