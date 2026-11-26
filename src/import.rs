//! HTML 导入：编码探测、主内容 DOM 提取、Markdown 转换与相对路径换算。

use crate::{convert, markdown, winutil};
use htmd::node::{Node, NodeType};
use regex::Regex;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

pub struct ImportedDoc {
    pub title: String,
    pub markdown: String,
    pub base_dir: Option<PathBuf>,
}

/// 编码识别顺序：BOM → 前 1 KB `<meta charset>` → 合法 UTF-8 → GBK 回退。
pub fn decode_html_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode_without_bom_handling(&bytes[2..]).0.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE.decode_without_bom_handling(&bytes[2..]).0.into_owned();
    }

    let head_len = bytes.len().min(1024);
    let head = String::from_utf8_lossy(&bytes[..head_len]);
    static RE_CHARSET: OnceLock<Regex> = OnceLock::new();
    let re = RE_CHARSET.get_or_init(|| Regex::new(r#"(?i)charset\s*=\s*["']?\s*([a-z0-9._-]+)"#).unwrap());
    if let Some(cap) = re.captures(&head)
        && let Some(label) = cap.get(1)
        && let Some(enc) = encoding_rs::Encoding::for_label(label.as_str().as_bytes())
    {
        return enc.decode_without_bom_handling(bytes).0.into_owned();
    }

    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_owned();
    }

    encoding_rs::GBK.decode_without_bom_handling(bytes).0.into_owned()
}

pub fn import_html_file(path: &Path) -> Result<ImportedDoc, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 HTML 失败：{}\n{e}", path.display()))?;
    Ok(import_html_bytes(&bytes, Some(path)))
}

pub fn import_html_bytes(bytes: &[u8], source_path: Option<&Path>) -> ImportedDoc {
    let html = decode_html_bytes(bytes);
    let cleaned = convert::clean_html_input(&html);
    let mut title = source_path
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "导入的文档".to_string());

    let content_html = match htmd::HtmlToMarkdown::new().html_to_tree(&cleaned) {
        Ok(tree) => {
            let root = tree.root();
            if let Some(t) = find_title_text(&root) {
                title = t;
            }
            let mut body_html = String::new();
            if !serialize_preferred_tag(&root, "article", &mut body_html)
                && !serialize_preferred_tag(&root, "main", &mut body_html)
                && !serialize_preferred_tag(&root, "body", &mut body_html)
            {
                serialize_children_clean(&root, &mut body_html);
            }
            body_html
        }
        Err(_) => cleaned,
    };

    let markdown = convert::html_to_markdown(&content_html);
    let base_dir = source_path.and_then(|p| p.parent().map(winutil::clean_path));
    ImportedDoc { title, markdown, base_dir }
}

fn find_title_text(node: &Node<'_>) -> Option<String> {
    if node.node_type() == NodeType::Element && node.tag_name() == Some("title") {
        let mut s = String::new();
        collect_plain_text(node, &mut s);
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    for ch in node.children().iter() {
        if let Some(found) = find_title_text(&ch) {
            return Some(found);
        }
    }
    None
}

fn collect_plain_text(node: &Node<'_>, out: &mut String) {
    match node.node_type() {
        NodeType::Text => {
            if let Some(t) = node.text() {
                out.push_str(&t);
            }
        }
        NodeType::Element => {
            for ch in node.children().iter() {
                collect_plain_text(&ch, out);
            }
        }
        _ => {}
    }
}

fn should_skip_tag(tag: &str) -> bool {
    matches!(
        tag,
        "head" | "nav" | "aside" | "footer" | "script" | "style" | "noscript" | "form" | "svg"
    )
}

fn serialize_preferred_tag(node: &Node<'_>, target: &str, out: &mut String) -> bool {
    for ch in node.children().iter() {
        if ch.node_type() == NodeType::Element {
            let tag = ch.tag_name().unwrap_or("");
            if tag == target {
                serialize_children_clean(&ch, out);
                return true;
            }
            if !should_skip_tag(tag) && serialize_preferred_tag(&ch, target, out) {
                return true;
            }
        }
    }
    false
}

fn serialize_children_clean(parent: &Node<'_>, out: &mut String) {
    for ch in parent.children().iter() {
        serialize_node_clean(&ch, out);
    }
}

fn serialize_node_clean(node: &Node<'_>, out: &mut String) {
    match node.node_type() {
        NodeType::Text => {
            if let Some(t) = node.text() {
                out.push_str(&crate::export::html_escape(&t));
            }
        }
        NodeType::Element => {
            let tag = node.tag_name().unwrap_or("");
            if should_skip_tag(tag) {
                return;
            }
            out.push('<');
            out.push_str(tag);
            for (name, val) in node.attrs().iter() {
                if name.starts_with("on") {
                    continue;
                }
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                out.push_str(&crate::export::html_escape(val));
                out.push('"');
            }
            if matches!(tag, "br" | "hr" | "img" | "source" | "meta" | "link" | "input") {
                out.push('>');
                return;
            }
            out.push('>');
            serialize_children_clean(node, out);
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        _ => {}
    }
}

/// 将导入文档中基于 `old_base` 的相对图片路径换算为相对于 `new_base`。
pub fn rewrite_relative_image_paths(md: &str, old_base: &Path, new_base: &Path) -> String {
    if old_base == new_base {
        return md.to_string();
    }
    static RE_IMG: OnceLock<Regex> = OnceLock::new();
    let re = RE_IMG.get_or_init(|| Regex::new(r#"(!\[[^\]]*\]\()([^)\s]+)"#).unwrap());
    re.replace_all(md, |caps: &regex::Captures| {
        let prefix = &caps[1];
        let raw_url = &caps[2];
        let rewritten = rebase_rel_url(raw_url, old_base, new_base);
        format!("{prefix}{rewritten}")
    })
    .into_owned()
}

fn rebase_rel_url(url: &str, old_base: &Path, new_base: &Path) -> String {
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("data:")
        || url.starts_with("file://")
        || url.starts_with('#')
        || url.starts_with('/')
    {
        return url.to_string();
    }
    let decoded = markdown::percent_decode(url);
    let p = Path::new(&decoded);
    if !p.is_relative() {
        return url.to_string();
    }
    let abs = winutil::clean_path(&old_base.join(p));
    relative_path(&abs, new_base)
}

fn relative_path(target: &Path, base: &Path) -> String {
    let t_comps: Vec<Component<'_>> = target.components().collect();
    let b_comps: Vec<Component<'_>> = base.components().collect();
    let common = t_comps.iter().zip(&b_comps).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return target.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = Vec::new();
    for c in &b_comps[common..] {
        if matches!(c, Component::Normal(_)) {
            parts.push("..".to_string());
        }
    }
    for c in &t_comps[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() { ".".into() } else { parts.join("/") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_bom_meta_gbk_and_fallback_gbk() {
        // 1. UTF-8 BOM
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice("<p>中文UTF8</p>".as_bytes());
        assert_eq!(decode_html_bytes(&with_bom), "<p>中文UTF8</p>");

        // 2. <meta charset="gbk">
        let (gbk_bytes, _, _) = encoding_rs::GBK.encode("<meta charset=\"gbk\"><p>简体中文</p>");
        let decoded = decode_html_bytes(&gbk_bytes);
        assert!(decoded.contains("简体中文"));

        // 3. 无 meta 的纯 GBK 回退
        let (raw_gbk, _, _) = encoding_rs::GBK.encode("<p>纯汉字回退</p>");
        assert_eq!(decode_html_bytes(&raw_gbk), "<p>纯汉字回退</p>");
    }

    #[test]
    fn extracts_article_and_skips_navigation_boilerplate() {
        let html = r#"
        <html><head><title>文章测试</title></head>
        <body>
            <nav><a href="/home">导航栏</a></nav>
            <article>
                <h1>正文标题</h1>
                <p>第一段内容。</p>
                <img src="images/pic.png" alt="配图">
            </article>
            <footer>版权声明</footer>
            <script>console.log("ignore")</script>
        </body></html>
        "#;
        let doc = import_html_bytes(html.as_bytes(), Some(Path::new(r"C:\docs\sub\post.html")));
        assert_eq!(doc.title, "文章测试");
        assert!(doc.markdown.contains("正文标题"));
        assert!(doc.markdown.contains("第一段内容"));
        assert!(doc.markdown.contains("images/pic.png"));
        assert!(!doc.markdown.contains("导航栏"));
        assert!(!doc.markdown.contains("版权声明"));
        assert!(!doc.markdown.contains("console.log"));

        let rebased = rewrite_relative_image_paths(&doc.markdown, Path::new(r"C:\docs\sub"), Path::new(r"C:\docs"));
        assert!(rebased.contains("sub/images/pic.png"));
    }
}
