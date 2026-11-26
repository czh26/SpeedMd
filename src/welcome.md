---
title: 欢迎使用 MdPad
author: MdPad
tags: [markdown, rust, egui]
---

# 欢迎使用 MdPad

一个用 **Rust + egui** 写的极速 Markdown 编辑器。下面演示所有支持的语法。

[TOC]

## 文本样式

**粗体**、*斜体*、***粗斜体***、~~删除线~~、<u>下划线</u>、==高亮文本==、`行内代码`、<kbd>Ctrl</kbd>+<kbd>S</kbd>

HTML 写法同样支持：<mark>mark 高亮</mark>、<sup>上标</sup> 与 <sub>下标</sub>，例如 x<sup>2</sup>、H<sub>2</sub>O。

在「设置」中开启上标 / 下标后，还可以写成 E=mc^2^、H~2~O。

## 列表

- 无序列表
  - 第二层
    - 第三层
- 另一项

1. 有序列表
2. 第二项
   1. 嵌套有序
   2. 继续

### 任务列表

- [x] 实时预览
- [x] 编辑器语法高亮
- [ ] 在预览里点击复选框可以直接勾选（会同步修改源码）

## 引用与分割线

> 这是一段引用。
>
> > 引用可以嵌套。

> [!NOTE]
> GFM 提示块：备注。

> [!TIP]
> 提示：把 `.md` 文件直接拖到窗口里即可打开。

> [!WARNING]
> 警告：关闭前记得保存。

---

## 链接、脚注与 Emoji

- 普通链接：[Rust 官网](https://www.rust-lang.org)
- GFM 自动链接：https://github.com 和 www.example.com
- 跳转到本文标题：[回到文本样式](#文本样式)
- 脚注引用：Markdown 由 John Gruber 发明[^1]，GFM 是 GitHub 的扩展[^gfm]。
- Emoji 短码：:smile: :rocket: :+1: :heart: :tada: :sparkles:

## 表格

| 左对齐 | 居中 | 右对齐 |
|:-------|:----:|-------:|
| 苹果 | 🍎 | 5.00 |
| 香蕉 | 🍌 | 12.50 |
| **合计** | — | ==17.50== |

## 代码

```rust
fn main() {
    let name = "Markdown";
    println!("Hello, {name}!"); // 注释
}
```

## 内嵌 HTML 与多媒体

支持行内样式：<span style="color: #e05d44; font-weight: bold;">红色加粗文字</span>、<font color="#2da44e" size="5">放大绿色字</font>，以及 <kbd>Ctrl+F</kbd> 按键样式。

<div align="center">

居中的容器段落：支持多块元素居中排版 ✨

</div>

<details open>
<summary>可折叠详情块（点击标题收起/展开）</summary>

这里可以包含任意 Markdown **富文本**、`行内代码` 与列表：
- 第一条：原生 DOM 解析 HTML
- 第二条：支持多层级嵌套块

</details>

<video src="assets/demo.mp4" title="示例演示视频" controls></video>

<audio src="assets/bgm.wav" title="示例背景音频"></audio>

[^1]: 2004 年发布，最初的目标是“易读易写”。
[^gfm]: GitHub Flavored Markdown，增加了表格、任务列表、删除线、自动链接等。
