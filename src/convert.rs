//! HTML ↔ Markdown 清理与转换。

use regex::Regex;
use std::sync::OnceLock;

/// 转换前清理 HTML：删除 `<script>`、`<style>`、`<noscript>`、Word 条件注释、普通注释、`<o:p>` 与 `mso-*` 样式。
pub fn clean_html_input(html: &str) -> String {
    static RE_SCRIPT: OnceLock<Regex> = OnceLock::new();
    static RE_STYLE: OnceLock<Regex> = OnceLock::new();
    static RE_NOSCRIPT: OnceLock<Regex> = OnceLock::new();
    static RE_WORD_COND: OnceLock<Regex> = OnceLock::new();
    static RE_COMMENT: OnceLock<Regex> = OnceLock::new();
    static RE_OFFICE_TAG: OnceLock<Regex> = OnceLock::new();
    static RE_MSO_PROP: OnceLock<Regex> = OnceLock::new();

    let re_script = RE_SCRIPT.get_or_init(|| Regex::new(r"(?is)<script\b[^>]*>[\s\S]*?</script>").unwrap());
    let re_style = RE_STYLE.get_or_init(|| Regex::new(r"(?is)<style\b[^>]*>[\s\S]*?</style>").unwrap());
    let re_noscript = RE_NOSCRIPT.get_or_init(|| Regex::new(r"(?is)<noscript\b[^>]*>[\s\S]*?</noscript>").unwrap());
    let re_word_cond = RE_WORD_COND.get_or_init(|| Regex::new(r"(?is)<!--\[if[\s\S]*?<!\[endif\]-->").unwrap());
    let re_comment = RE_COMMENT.get_or_init(|| Regex::new(r"<!--[\s\S]*?-->").unwrap());
    let re_office_tag = RE_OFFICE_TAG.get_or_init(|| Regex::new(r"(?is)</?o:\w+[^>]*>").unwrap());
    let re_mso_prop = RE_MSO_PROP.get_or_init(|| Regex::new(r#"(?i)\bmso-[a-z0-9-]+\s*:\s*[^;"']*;?\s*"#).unwrap());

    let s = re_script.replace_all(html, "");
    let s = re_style.replace_all(&s, "");
    let s = re_noscript.replace_all(&s, "");
    let s = re_word_cond.replace_all(&s, "");
    let s = re_comment.replace_all(&s, "");
    let s = re_office_tag.replace_all(&s, "");
    re_mso_prop.replace_all(&s, "").into_owned()
}

/// HTML 片段转干净 Markdown：前清理 + `htmd` + 后清理（`&nbsp;`、行尾空格、连续空行）。
pub fn html_to_markdown(html: &str) -> String {
    let cleaned = clean_html_input(html);
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript"])
        .build();
    let raw = converter.convert(&cleaned).unwrap_or_default();
    postprocess_markdown(&raw)
}

pub fn postprocess_markdown(raw: &str) -> String {
    let norm = raw.replace("\r\n", "\n").replace('\r', "\n");
    let norm = norm.replace("&nbsp;", " ").replace('\u{00a0}', " ");

    let mut lines: Vec<&str> = Vec::new();
    let mut blank_run = 0usize;
    let mut in_fence = false;

    for line in norm.split('\n') {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if in_fence {
            blank_run = 0;
            lines.push(line);
            continue;
        }
        let clean_line = line.trim_end_matches([' ', '\t']);
        if clean_line.is_empty() {
            blank_run += 1;
            // 合并连续空行：最多保留 1 个空行（避免出现 3+ 连续空行）
            if blank_run <= 1 && !lines.is_empty() {
                lines.push("");
            }
        } else {
            blank_run = 0;
            lines.push(clean_line);
        }
    }

    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_scripts_styles_comments_and_word_markup() {
        let word_html = r#"
        <html><head><style>.MsoNormal{margin:0;}</style></head>
        <body>
        <!--[if gte mso 9]><xml><w:WordDocument></w:WordDocument></xml><![endif]-->
        <!-- 普通注释 -->
        <script>alert('xss')</script>
        <h2 style="mso-spacerun:yes; color:red;">标题</h2>
        <p class="MsoNormal">第一段&nbsp;&nbsp;内容<o:p></o:p></p>
        <p><br></p><p><br></p><p><br></p><p><br></p>
        <p>第二段</p>
        </body></html>
        "#;
        let md = html_to_markdown(word_html);
        assert!(!md.contains("alert"));
        assert!(!md.contains("WordDocument"));
        assert!(!md.contains("普通注释"));
        assert!(!md.contains("mso-"));
        assert!(!md.contains("o:p"));
        assert!(!md.contains("&nbsp;"));
        assert!(!md.contains('\u{00a0}'));
        assert!(md.contains("## 标题") || md.contains("标题"));
        assert!(md.contains("第一段"));
        assert!(md.contains("第二段"));
        assert!(!md.contains("\n\n\n"));
    }

    #[test]
    fn postprocess_strips_trailing_spaces_and_collapses_blanks() {
        let dirty = "  \n# Hello   \r\n\r\n\r\n\r\n\r\nWorld\t \n\n  \n";
        let clean = postprocess_markdown(dirty);
        assert_eq!(clean, "# Hello\n\nWorld");
    }
}
