# ThunderMD 功能扩展实施计划（PLAN.md）

> 状态：**已确认（2026-11-26）**，全部按推荐方案执行，最终决定见第 4 节开头的「最终决定」。执行顺序：阶段 1 → 8。

## 0. 现状摘要（通读代码后的结论）

| 文件 | 行数 | 现状 | 本次计划中的去向 |
|---|---|---|---|
| `main.rs` | ~480 | `MdApp` 单文档 + 菜单/状态栏/编辑器/对话框/自测 | 拆成 `app.rs`、`document.rs`、`commands.rs`、`editor.rs`、`settings.rs` 等，`main.rs` 只保留入口 |
| `markdown.rs` | ~900 | pulldown-cmark → `Doc`，主题、行内构建器（`==`、emoji、自动链接、词内上下标、行内 HTML） | 增加块/标题的**源码字节范围**、HTML 扩展、`<details>` 容器；行内文本处理抽到 `textproc.rs` 与导出共用 |
| `preview.rs` | ~600 | 视口裁剪渲染、锚点跳转、任务勾选回写 | 增加代码块行号/横向滚动、媒体卡片、HTML 块、行高/段距/行宽；解析结果改由 `Document` 持有 |
| `highlight.rs` | ~300 | 逐行高亮，按全文哈希缓存 | 缓存键改用文档版本号；叠加查找高亮 |
| `fonts.rs` | ~80 | 硬编码 Segoe UI / 雅黑 / Consolas | 扫描系统字体 + 按用户选择加载 |

基线：release exe **9.6 MB**；项目**还不是 git 仓库**（git 2.55、clippy 已安装）。

---

## 1. 模块拆分与核心数据结构

```
src/
├─ main.rs            入口：NativeOptions + run_native（约 40 行）
├─ app.rs             MdApp：顶层布局（菜单/标签栏/侧边栏/中央区/状态栏）、对话框、后台任务轮询
├─ settings.rs        Settings（#[serde(default)]，全部新字段带默认值）
├─ document.rs        Document：单文档状态 + 解析/统计缓存
├─ tabs.rs            标签栏 UI 与标签操作（打开/复用/关闭/重排/最近关闭栈）
├─ commands.rs        Command 枚举、元数据（id/中文名/默认快捷键）、统一分发
├─ keymap.rs          Shortcut 解析/格式化/匹配、绑定表、冲突检测（纯逻辑，有单测）
├─ editor.rs          编辑器面板：TextEdit 包装、EditOps（编程式编辑+撤销点）、括号补全、围栏补全、跳转到偏移
├─ find.rs            查找替换栏 + 匹配计算（防抖）+ 替换（纯逻辑有单测）
├─ stats.rs           字数统计（纯函数，有单测）
├─ sidebar/{mod,files,articles,outline}.rs
├─ fonts.rs           系统字体扫描（read-fonts 读 name/OS2 表）+ 按选择加载
├─ highlight.rs       编辑器高亮 + 查找高亮叠加
├─ markdown.rs        解析（+ 源码范围、HTML 扩展、details）
├─ textproc.rs        共享行内处理：== / :emoji: / 自动链接 / 词内上下标（预览与导出共用，有单测）
├─ html_inline.rs     HTML 标签与属性解析、CSS 颜色解析、媒体识别（有单测）
├─ media.rs           媒体卡片 + 时长读取（mp4/mov/m4a/wav/flac 头解析）
├─ preview.rs         渲染
├─ clipboard.rs       arboard：读写 CF_HTML、图片
├─ convert.rs         HTML → Markdown（htmd + 清理，有单测）
├─ export.rs          Markdown → HTML（完整文件 / 正文片段，有往返测试）
├─ import.rs          HTML 文件导入（编码识别、正文提取、相对路径换算）
├─ jobs.rs            后台线程任务封装（mpsc + request_repaint）
├─ shortcuts_ui.rs    快捷键设置窗口
└─ winutil.rs         资源管理器定位、回收站、默认程序打开
```

### 1.1 Document

```rust
pub struct Document {
    pub uid: u64,                 // 稳定 id：egui 的 TextEdit 状态 / 滚动位置按它区分
    pub text: String,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub version: u64,             // 每次修改 +1，作为各缓存的键（不再每帧哈希全文）
    pub parsed: ParseCache,       // 解析结果（Doc），预览 / 大纲 / 字数共享；按 version 失效
    pub stats: StatsCache,        // 防抖后计算
    pub preview: PreviewState,    // 视口裁剪高度表、待滚动锚点
    pub hl: Highlighter,
    pub find: FindState,          // 每个标签独立的查找结果缓存
    pub pending_cursor: Option<usize>,   // 待跳转的字节偏移（大纲/查找/源码模式切换用）
    pub view_before_source: Option<ViewMode>,
}
```
- 光标、选区、撤销栈保存在 egui 自己的 `TextEditState` 里（id 由 `uid` 生成），编辑器和预览的滚动位置也按 `uid` 区分，**切换标签时天然保留**，不需要手动存取。
- 解析结果的所有权从 `Preview` 移到 `Document`。这样「源代码模式」下预览没有绘制，大纲和字数统计也能拿到最新的标题列表（输入停顿 200 ms 后再解析）。

### 1.2 命令系统

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Command {
    NewTab, CloseTab, ReopenClosedTab, NextTab, PrevTab,
    New, Open, OpenFolder, Save, SaveAs, ImportHtml, ExportHtml, ExportHtmlFragment, Quit,
    ViewEdit, ViewSplit, ViewPreview, ToggleSourceMode,
    Find, Replace, FindNext, FindPrev,
    ToggleSidebar, SidebarFiles, SidebarArticles, SidebarOutline,
    InlineCode, CodeBlock, CopyAsMarkdown, CopyAsHtml, PastePlain,
    FontBigger, FontSmaller, FontReset, OpenShortcuts, OpenSettings,
}
impl Command {
    pub const ALL: &[Command] = &[/* … */];
    pub fn id(self) -> &'static str;          // "close_tab"，用于持久化
    pub fn label(self) -> &'static str;       // "关闭标签"
    pub fn default_shortcut(self) -> Option<Shortcut>;
}
pub struct Shortcut { pub ctrl: bool, pub shift: bool, pub alt: bool, pub key: egui::Key }
// "Ctrl+Shift+K" ⇄ Shortcut；Display 用于菜单右侧文字
```
- **统一匹配**：每帧开始时遍历 `input.events` 里的按键事件，只做**修饰键精确匹配**（Ctrl+S 不会被 Ctrl+Shift+S 误触发，自然解决了“Shift 组合要先检测”的问题）。按键同时比较 `physical_key` 和逻辑 `key`，所以 `Ctrl+Shift+1`（逻辑键可能是 `!`）和 `` Ctrl+Shift+` `` 都能识别。匹配到的事件会**从队列中移除**，`TextEdit` 收不到（例如 `Ctrl+Tab` 不会插入制表符）。
- 菜单项、右键菜单、快捷键都调用同一个 `app.run(cmd)`；菜单右侧的快捷键文字通过 `keymap.label(cmd)` 动态生成。
- 自测扩展：新增 `MDPAD_TEST_COMMANDS="toggle_sidebar;find"` 环境变量，启动后依次执行命令再截图，用来对新界面做截图自检。

---

## 2. 新增依赖清单

| 依赖 | 用途 | features | exe 体积影响（估算） |
|---|---|---|---|
| `regex` 1.13 | 查找替换的正则模式 | 默认（需要 unicode，中文大小写/单词边界） | +0.7 ~ 1.0 MB |
| `htmd` 0.6 | HTML → Markdown（粘贴 HTML、导入 HTML），附带的 html5ever 也用于解析 `Block::Html` | 默认 | +1.0 ~ 1.5 MB |
| `encoding_rs` 0.8 | 导入 HTML 时识别 GBK 等编码 | 默认 | +0.3 ~ 0.6 MB |
| `notify` 9 | 监听文件夹变化，刷新文件树和文章列表 | `default-features = false` | +0.1 ~ 0.2 MB |
| `serde_json` 1 | 快捷键配置导入/导出 JSON | 默认 | +0.1 MB |
| `pulldown-cmark`（已有） | 开启 `html` feature 用于导出 | `html` | ≈ +30 KB |
| `arboard` 3.7（**已在依赖树中**） | CF_HTML 读写、剪贴板图片 | 默认 | ≈ 0 |
| `read-fonts`/`skrifa`（**已在依赖树中**） | 读取字体文件的族名称 | — | ≈ 0 |
| `windows-sys` 0.61（**已在依赖树中**） | 回收站（`SHFileOperationW` + `FOF_ALLOWUNDO`） | `Win32_UI_Shell` 等 | ≈ 0 |

预计 release exe：**9.6 MB → 约 12 ~ 13.5 MB**。base64 编码（导出时内嵌图片）、自然排序、媒体时长读取都自己写，不再引入依赖。

---

## 3. 各阶段实现思路与风险

### 阶段 1：架构重构 + 多标签页 + 命令系统
- 先 `git init`（`.gitignore` 忽略 `/target`），把当前代码作为“阶段 0 基线”提交；顺便清理掉现有代码的 clippy 警告。
- 拆分模块（见上节），`MdApp` 改为 `docs: Vec<Document>` + `active: usize` + `closed_stack: Vec<PathBuf>`（`Ctrl+Shift+T` 用）。
- 标签栏放在 `ScrollArea::horizontal` 里；拖拽排序用 `Response::dragged()` 加指针 x 坐标计算插入位置；中键点击用 `middle_clicked()` 关闭；右键菜单用 `context_menu`。
- 同名文件消歧：统计重名的文件名，重名时显示 `name — 父目录`。
- 关闭窗口时如果有多个未保存的文档：弹窗列出全部未保存文件，提供“全部保存 / 全部不保存 / 取消”。全部保存时，遇到未命名文档会依次弹出另存为对话框，中途取消则中止关闭。
- 恢复标签：`Settings.restore_tabs`（默认开启）+ `session: { paths, active }`，启动时跳过已不存在的文件。
- **风险**：`Ctrl+Tab` 在 Windows 上可能被 IME 或系统占用，需要实测；如果收不到事件，在 `PLAN.md` 中记录并改用 `Ctrl+PageDown` / `Ctrl+PageUp` 作为后备。

### 阶段 2：编辑器增强
**EditOps（括号补全、粘贴、替换都依赖它）**
```rust
/// 编程式修改文本：一次调用 = 一个撤销点
fn apply_edit(ctx, id, doc, edit: impl FnOnce(&mut String, CCursorRange) -> CCursorRange) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    let before = state.cursor.char_range().unwrap_or_default();
    let mut undoer = state.undoer();
    undoer.add_undo(&(before, doc.text.clone()));        // 先把修改前的状态存为撤销点
    let after = edit(&mut doc.text, before);             // 内部按「字符索引 ↔ 字节索引」换算
    undoer.add_undo(&(after, doc.text.clone()));         // 立即提交修改后的状态，不和后续输入合并
    state.set_undoer(undoer);
    state.cursor.set_char_range(Some(after));
    state.store(ctx, id);
    doc.bump_version();
}
```
- **括号补全**：在 `TextEdit::show` **之前**，当编辑器有焦点时，从 `input_mut().events` 里拦截：
  - `Event::Text`：英文括号和引号
  - `Event::Ime(ImeEvent::Commit(..))`：**中文输入法输入的 `（【“‘《` 走这条路径**，必须一起处理
  - `Event::Key { key: Backspace }`：在空括号中间时两侧一起删除
  
  拦截后用 `apply_edit` 修改，并从队列中移除原事件。补全规则写成纯函数 `fn autopair(text, sel, input) -> Option<Edit>`，配单元测试，覆盖：有选区时包裹、跳过已有的右括号、`don't` 不补全、代码块内的开关。
- **撤销**：egui 的 Undoer 保存的是“整篇文本 + 光标”快照，上面两次 `add_undo` 保证一次 `Ctrl+Z` 恰好回到修改前。代价是大文档的撤销栈占内存：10 万字约 300 KB × 最多 100 步 ≈ 30 MB，可以接受。
- **围栏补全**：拦截 Enter。如果光标所在行是 ```` ```lang ```` 并且下方没有配对的结束围栏（统计光标前的围栏数为奇数），插入 `\n\n```` 并把光标放到中间的空行。
- **源代码模式**：`Ctrl+/` 记住 `view_before_source`。切换时，根据光标字节偏移找到所在的顶层块（依赖 `Doc.blocks[i].src` 源码范围），让预览 `scroll_to_block(i)`；反向切换时同理。
- **跳转到偏移**（查找、大纲、源码切换都用）：从 `TextEditOutput.galley` 用 `pos_from_cursor` 算出这一行的矩形，下一帧调用 `ui.scroll_to_rect`。
- **代码块预览**：用 `ScrollArea::horizontal` 包住、不自动换行；可选的行号列；未知语言按纯文本显示（egui_extras 的回退高亮对未知语言本来就近似纯文本，我会改成显式判断）。
- **查找替换**（`find.rs`）：
  - 匹配计算在**输入停顿 150 ms 后**执行，结果 `Vec<Range<usize>>` 按 `(doc.version, 查询条件)` 缓存。
  - 编辑器高亮：`highlight.rs` 的缓存键扩展为 `(version, 暗色, 字号, 查找叠加层 key, 当前匹配序号)`。先生成语法高亮的 LayoutJob，再用一遍 O(sections + matches) 的处理按匹配边界切分 section、设置背景色。匹配超过 1 万处时只高亮前 1 万处，计数照常显示。
  - 替换 / 全部替换走 `apply_edit`，所以一次 `Ctrl+Z` 就能撤销。正则替换直接使用 `regex::Regex::replace_all` 的 `$1` / `${name}` 语法。正则无效时输入框变红并显示错误，不会 panic。
  - 在预览模式下按 `Ctrl+F`：自动切到分屏（见待决定问题 6）。

### 阶段 3：外观与字数统计
- **字体扫描**：在后台线程里扫描 `C:\Windows\Fonts` 和 `%LOCALAPPDATA%\Microsoft\Windows\Fonts`。每个文件**只读取表目录以及 `name` / `OS/2` 表的字节**（按偏移 seek，不把几十 MB 的中文字体整个读进内存），用 `read-fonts` 解析出族名称（优先 name id 16/17）、字重和是否斜体，支持 TTC。得到“族名 → 常规文件 + 粗体文件（含 TTC 索引）”的映射，本次运行内缓存。
- **加载字体**：`fonts::setup(ctx, &FontChoice)` 按用户选择组装字体族，中文字体始终作为回退保留。修改设置后调用 `ctx.set_fonts` 立即生效。
- **行宽**：编辑器和预览外面加一层“最大内容宽度”的居中容器；预览的行高、段距加入 `Theme` 和解析缓存键。
- **字数统计**（`stats.rs`，纯函数）：
  - 中日韩字符每个算 1 字、英文按单词计；字符数分含/不含空格；还统计行数、段落数、阅读时长（中文 300 字/分钟 + 英文 200 词/分钟，结果合计）。
  - 排除 Front Matter；可选是否排除代码块和 Markdown 标记（复用解析结果中的纯文本）。
  - 输入停顿 300 ms 后计算。选区统计按 `(version, 选区范围)` 缓存，只统计选中的那一段。
  - 数字使用千分位分隔。状态栏点击后弹出包含全部统计项的浮层。

### 阶段 4：侧边栏
- `Panel::left("sidebar").resizable(true)`：宽度由 egui 的持久化自动记住，显示状态和当前页签存在 `Settings`。
- **文件树**：节点表 `HashMap<PathBuf, DirState { children: Option<Vec<Entry>>, expanded }>`，展开目录时才读取子项；自然排序自己实现（有单测）。重命名使用行内 `TextEdit`；删除调用 `SHFileOperationW` 移入回收站，需要二次确认。重命名或删除已打开的文件时，同步更新对应标签的路径或状态。
- **文件监听**：`notify` 递归监听根目录，事件通过 channel 传回，标记受影响的目录为“需刷新”，下一帧只重新读取已展开的目录。窗口重新获得焦点时也会刷新一次，作为后备。
- **文章列表**：后台线程遍历目录（跳过 `.` 开头的文件、`node_modules`、`target` 等），每个 `.md` 只读前 8 KB，提取标题（Front Matter `title` → 第一个一级标题 → 文件名）和约 80 字摘要。结果按 `(路径, 修改时间)` 缓存，文件变化时只重新读取变化的文件。
- **大纲**：复用 `Document.parsed` 中的 `headings`（新增每个标题的源码偏移），可折叠子级。点击标题时预览执行 `scroll_to`，同时编辑器执行“跳转到偏移”并移动光标。当前章节：分屏和编辑模式下取编辑器视口顶部的字符偏移，预览模式下取预览视口顶部的块，然后找出不超过该位置的最后一个标题。

### 阶段 5：内嵌 HTML 与音视频
- **行内 HTML**：现有 `html()` 改为带属性的标签栈（`html_inline.rs`：属性解析，CSS 颜色支持命名颜色、`#rgb`、`#rrggbb`、`rgb()`），支持 `span style` / `font color` / `a href` / `img width height` 等。
- **块级 HTML**：用 html5ever（htmd 自带）把 `Block::Html` 解析成 DOM，再转换为现有的 Block（居中或右对齐的段落、图片、媒体卡片等）。不认识的标签只保留其中的文字；`<script>`、`<style>`、事件属性一律丢弃，注释不显示。
- **`<details>`**：pulldown-cmark 会把 `<details>` 和 `</details>` 拆成两个 HTML 块，中间夹着普通 Markdown 块。我在解析器里把它当作一个**容器帧**（和引用块一样），所以 details 里面也能写任意 Markdown；折叠状态存在 egui memory 中。
- **媒体卡片**：显示类型图标、标题、时长、封面。时长由 `media.rs` 读取文件头：mp4/mov/m4a 读 `mvhd`，wav 读 `fmt`/`data` 块，flac 读 `STREAMINFO`；mp3/ogg/webm 不显示时长。点击卡片时用系统默认程序打开（本地路径相对于当前文档目录解析）。
- 在 `welcome.md` 中补充以上语法的示例。

### 阶段 6：剪贴板
- `clipboard.rs` 通过 arboard 实现：`get().html()`、`set().html(html, Some(纯文本))`、`get().image()`。需要实测 arboard 读取时返回的是完整的 CF_HTML（带头部偏移信息）还是已经截取好的片段，如果是前者就自己截取 `StartFragment` 到 `EndFragment` 之间的内容。
- **粘贴拦截**：egui-winit 只把 `Ctrl+V` 转成 `Event::Paste`（不带 Shift），所以：
  - `Ctrl+V`：在 TextEdit 之前拦截 `Event::Paste`。剪贴板有 HTML 并且设置开启时，转换成 Markdown 后通过 `apply_edit` 插入；只有图片时保存为 `assets/时间戳.png` 并插入图片链接；其余情况插入纯文本。三种情况都走 `apply_edit`，撤销行为一致。
  - `Ctrl+Shift+V`：不会产生 Paste 事件，作为命令处理，直接读取纯文本插入。
  - `Ctrl+Shift+C`：同样不会产生 Copy 事件，不会与 TextEdit 自带的复制冲突。
- **HTML 清理**（`convert.rs`，有单测）：转换前删除 `<style>`、`<script>`、注释、Word 的条件注释、`<o:p>` 和 `mso-*` 样式；转换后合并 3 个以上的连续空行、把 `&nbsp;`/`\u{a0}` 换成普通空格、去掉行尾空格。
- 编辑器右键菜单包含：剪切/复制/粘贴、复制为 Markdown、复制为 HTML、纯文本粘贴。

### 阶段 7：导入 / 导出 HTML
- **导出**：在 pulldown-cmark 事件流上做一层转换，再交给 `push_html` 输出。
  - 文本事件经过 `textproc` 处理成 `<mark>`、emoji、链接、`<sup>`/`<sub>`（与预览共用同一套规则）
  - 标题加上与 `slugify` 规则一致的 id；`[TOC]` 段落替换为目录 HTML
  - 代码块：用 egui_extras 的高亮结果生成带内联颜色样式的 `<span>`
  - GFM 提示块输出为 `<div class="alert note">`
  - 媒体输出为 `<video controls>` / `<audio controls>`
  - Front Matter 不输出到正文，其中的 `title` 用作 `<title>`
  - CSS 内嵌，配色跟随当前明/暗主题或固定为浅色；图片可选“保留相对路径”或“base64 内嵌”
  - 另提供“仅正文片段”导出
- **导入**：
  - 编码识别：BOM → 前 1 KB 中的 `<meta charset>` → 尝试 UTF-8 → 回退 GBK（encoding_rs）
  - 正文提取：用 html5ever 优先取 `<article>` 或 `<main>`，否则取 `<body>` 并去掉 nav、aside、footer、script、style、form
  - 相对图片路径按原 HTML 文件的位置换算成相对于新文档的路径；结果在新标签中打开，作为未保存文档
- 导入导出都在 `jobs.rs` 的后台线程中执行。完成后显示提示条，带“用浏览器打开”和“打开所在文件夹”按钮；失败时显示明确的错误信息。
- **往返测试**：Markdown → HTML 片段 → Markdown 之后，比较两份 pulldown 事件“骨架”（标签类型、层级、文本归一化），确认标题、列表、表格、代码块、链接保持一致。

### 阶段 8：自定义快捷键
- 设置窗口：搜索框 + 表格（命令、当前快捷键、默认快捷键、恢复默认）。
- 录制状态：读取下一帧的按键事件并吞掉，`Esc` 取消、`Backspace` 清除绑定。
- 冲突检测：新快捷键已被其他命令占用时提示，可选择“覆盖（清除原命令的绑定）”或“取消”。
- 持久化：`Settings.keybindings: BTreeMap<String, String>` 只保存与默认值不同的项，`""` 表示解除绑定；未知的命令 id 或解析失败的快捷键直接忽略。JSON 导入/导出使用同一格式。

---

## 4. 待你决定的问题（每项附推荐方案）

### ✅ 最终决定（2026-11-26，用户回复“按推荐”）
| # | 问题 | 决定 |
|---|---|---|
| Q1 | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | 调整焦点面板（编辑器 / 预览，分屏时取鼠标所在一侧）的字号；关闭 egui 键盘缩放，界面缩放移到设置滑块 |
| Q2 | 超大文档输入性能 | 方案 A：继续用 `TextEdit` 并尽量优化，阶段 2 加 10 万字输入基准测试并记录结果；不重写编辑器控件 |
| Q3 | HTML → Markdown | 使用 `htmd`（html5ever 同时用于阶段 5 的 HTML 块解析） |
| Q4 | 文件监听 | `notify` + 窗口重新获得焦点时刷新作为后备 |
| Q5 | 回收站 | 直接调用 `windows-sys` 的 `SHFileOperationW`（`FOF_ALLOWUNDO`） |
| Q6 | 预览模式下 `Ctrl+F` | 自动切换到分屏，查找后不自动切回 |
| Q7 | 恢复标签 | 只恢复有路径的文件，不保存未命名草稿 |
| Q8 | Git 提交身份 | 本仓库已由用户初始化并关联远程 `https://github.com/czh26/SpeedMd.git`（`main` 分支，首个提交「初始化」）。仅在本仓库设置 `user.name "czh26"`、`user.email "2535747780@qq.com"`，全局配置不动。阶段 1 不再需要 `git init` |
| Q9 | 媒体时长 | 只支持 mp4/mov/m4a/wav/flac（自己解析文件头），mp3/ogg/webm 不显示时长 |

> [!IMPORTANT]
> **Q1：`Ctrl+=` / `Ctrl+-` / `Ctrl+0` 的用途**
> **推荐：用来调字号**，调整焦点所在的面板（编辑器或预览；分屏时取鼠标所在的一侧）。同时关闭 egui 自带的键盘缩放（`zoom_with_keyboard = false`），界面缩放改到设置里用滑块调节。

> [!WARNING]
> **Q2：超大文档的输入性能**
> egui 的 `TextEdit` 每次修改都会对**整篇文本**重新排版。之前实测 27 万字符时每帧约 100 ms，据此估计 10 万字时**每次按键会有 30 ~ 50 ms 的延迟**（滚动不受影响，因为排版结果有缓存）。
> - **推荐 A**：继续使用 `TextEdit`，把能控制的部分都优化掉（用版本号做缓存键、高亮按行增量生成、避免每帧克隆），在阶段 2 加入 10 万字的输入基准测试并如实记录结果。
> - B：增加一个“阶段 2.5”，自己写一个虚拟化的编辑器控件（光标、选区、输入法、撤销、剪贴板全部重写）。工作量很大、风险高，**不推荐**放在这一轮。

> [!IMPORTANT]
> **Q3：HTML → Markdown 转换器** — **推荐 `htmd`**（体积约 +1.2 MB，效果稳定，附带的 html5ever 还能用于阶段 5 的 HTML 块解析）。也可以自己写一个轻量转换器，体积更小，但对 Word 和复杂网页的兼容性明显差一些。

> **Q4：文件监听** — **推荐 `notify` + 窗口重新获得焦点时刷新作为后备**。也可以只靠获得焦点时刷新，不引入依赖，但应用在前台时外部改动不会立即反映出来。

> **Q5：删除文件到回收站** — **推荐直接调用 `windows-sys` 的 `SHFileOperationW`**（这个库已经在依赖树里，体积约为 0）；`trash` crate 会额外引入 `windows` 大库，编译更慢。

> **Q6：在预览模式下按 `Ctrl+F`** — **推荐自动切换到分屏**，查找完成后不自动切回。另一种做法是在预览里也高亮匹配，但预览由多个独立的文本块组成，实现起来更复杂。

> **Q7：「恢复上次的标签」是否包含未保存的未命名文档内容** — **推荐只恢复有路径的文件**（关闭窗口时本来就会确认是否保存）。也可以把草稿自动保存到应用数据目录，再在下次启动时恢复。

> [!IMPORTANT]
> **Q8：Git 提交身份** — 全局 git 配置里**只有邮箱 `2535747780@qq.com`，没有用户名**，提交会失败。推荐我在本仓库内单独设置：`git config user.name "<你的名字>"`，请告诉我用什么名字（全局配置不会改动）。

> **Q9：媒体时长** — **推荐只支持 mp4/mov/m4a/wav/flac**（自己解析文件头，零依赖）；mp3/ogg/webm 不显示时长。

### 我自行决定的小细节（不同意可以提出来）
- 代码块内也补全括号和引号（设置项「代码块内补全」默认开启）；`*` `_` `~` `=` 只在**有选区时**包裹选区，没有选区时不补全，以免干扰列表、粗体等写法。
- 查找栏输入停顿 150 ms 后执行搜索，字数统计输入停顿 300 ms 后执行，解析（供大纲使用）输入停顿 200 ms 后执行。
- `Ctrl+Tab` 收不到事件时，后备为 `Ctrl+PageDown` / `Ctrl+PageUp`。
- 字体列表只在第一次打开设置时扫描一次，本次运行内缓存，不持久化。

---

## 5. 验证方式

### 每个阶段
```powershell
cargo build --release          # 零警告
cargo clippy --all-targets     # 零警告
cargo test                     # stats / keymap / find / textproc / html_inline / convert / export 往返测试
```
- **截图自检**：编辑、分屏、预览三种视图 × 亮/暗两种主题，再加上本阶段的新界面（通过 `MDPAD_TEST_COMMANDS` 打开侧边栏、查找栏、快捷键窗口等）。
- **性能基准**：新增 `MDPAD_BENCH=typing`，在 10 万字文档中模拟连续输入 200 个字符，把平均帧时间写入文件，并记录到 `PLAN.md`。

### 收尾
完整执行一次上述流程，输出每个阶段的完成情况、依赖清单、exe 体积前后对比，以及手动验收清单。

---

## 6. 进度清单
- [x] 阶段 1：架构重构 + 多标签页 + 命令系统
- [x] 阶段 2：编辑器增强（源代码模式、括号补全、代码块、查找替换）
- [x] 阶段 3：外观与字数统计
- [x] 阶段 4：侧边栏（文件树、文章列表、大纲）
- [x] 阶段 5：内嵌 HTML 与音视频
- [x] 阶段 6：剪贴板
- [x] 阶段 7：导入 / 导出 HTML
- [x] 阶段 8：自定义快捷键

## 7. 实施中的决定
- **Git 推送**（2026-11-26 用户决定）：每个阶段只在本地 `git commit`，**不推送**到 GitHub；等用户明确要求时再执行 `git push origin main`。

## 8. 实测记录
- **阶段 2 性能基准（`MDPAD_BENCH=typing`，release 版，100,300 字符中英文混排文档，模拟连续输入 200 个字符）**：
  - `cpu_ms`（主线程高亮 + `TextEdit` 排版与 UI 帧耗时）：平均 `5.24 ms`，P50 `5.09 ms`，P95 `6.59 ms`
  - `wall_ms`（端到端帧间隔，含垂直同步）：平均 `16.92 ms`，P50 `16.67 ms`，P95 `17.51 ms`
  - `release exe` 体积：`11,569,152` 字节（约 `11.03 MB`）
- **阶段 8 全量收尾验收（release 终版）**：
  - `cargo clippy --all-targets -- -D warnings`：0 警告通过；`cargo test`：40 项单元测试全部通过；`cargo build --release`：0 警告通过
  - `release exe` 最终体积：`13,170,176` 字节（约 `12.56 MB`，低于 15 MB 上限；较基线 `10,072,064` 字节增加 `3,098,112` 字节 / 约 `+2.95 MB`）
  - 终版 `MDPAD_BENCH=typing`（100,300 字符，200 样本）：`cpu_ms` 平均 `6.46 ms`（P50 `6.63 ms`，P95 `8.16 ms`），`wall_ms` 平均 `16.65 ms`（P50 `16.62 ms`，P95 `17.63 ms`）

---

## 9. 语法说明书

### 入口与模块位置
- **命令与快捷键**：在 `src/commands.rs` 新增 `Command::OpenManual`（命令 id：`open_manual`，默认快捷键 `F1`），纳入统一命令与快捷键系统，可在「设置 → 快捷键」中自定义；顶部菜单栏新增「帮助 → 语法说明书」。
- **特殊标签页（`📖 语法说明书`）**：在 `src/document.rs` / `src/tabs.rs` / `src/app.rs` 中以全局单例特殊标签页管理——再次按 `F1` 切换到已有标签页；只读且不计入未保存与最近关闭列表；不保存为 `session_paths` 文件路径，仅通过 `Settings` 的 `session_manual_open` / `session_manual_active` 记忆恢复；忽略 Edit/Split/Preview 视图切换及编辑/保存/查找/导出等不适用的命令。
- **数据文件**：`src/manual.md` 在编译期通过 `include_str!("manual.md")` 嵌入程序，首次打开说明书时才在后台/按需解析并缓存于 `OnceLock`，不拖慢程序启动。
- **界面与逻辑模块**：`src/manual.rs` 负责解析 `manual.md`、中英文同义词搜索（`Esc` 清空、显示命中条数）、左侧分类目录（点击跳转 + 滚动高亮）、右侧视口裁剪语法卡片（卡片主体采用**左源码、右效果预览**等高左右对比排版：左栏采用 `highlight::highlight_manual_source` 渲染高亮并以淡色 `·` 标示行尾空格、支持横向滚动；右栏复用 `markdown::parse` + `preview::render_doc_inline` 渲染实时效果；卡片顶部含「复制」与「插入到文档」按钮，及 `sup_sub` / `front_matter` 设置联动与「立即开启」按钮）。内置无网络依赖演示图片 `bytes://mdpad/*` 供示例渲染。

### `src/manual.md` 数据格式与新增条目方法
新增语法条目**只需编辑 `src/manual.md` 并重新编译，无需修改 Rust 代码**，格式约定如下：
1. `# 分类名称`：定义一个一级分类（按出现顺序展示在左侧目录）。
2. `## 语法名称`：定义一张语法卡片，归属上方最近的分类。
3. 紧跟在 `##` 下方的可选 `key: value` 元数据行（顺序不限）：
   - `id:` 全局唯一英文/数字/连字符标识符（单元测试校验非空且不重复）
   - `keywords:` 逗号分隔的中英文检索词与同义词（辅助搜索匹配）
   - `tags:` 竖线 `|` 分隔的标签（如 `需在设置中开启`、`MdPad 扩展语法，其他编辑器可能不支持`、`HTML 写法`）
   - `setting:` 关联实时设置项（支持 `sup_sub` 或 `front_matter`，卡片自动显示当前开关状态与「立即开启」按钮）
   - `mode:` 插入到文档时的换行补全策略（`inline` 直接插入光标处；`block` 在前后必要时自动补齐空行）
   - `note:` 补充注意事项（显示在说明文字下方）
4. 元数据之后、围栏之前的文本行作为该语法的「一句话说明」。
5. 使用 4 个波浪线围栏 `~~~~markdown` ... `~~~~` 包裹示例源码（支持内部嵌套 ```` ``` ```` 代码块，保留缩进与行尾空格）。

### 收录规模与实测指标
- **收录范围**：共 **16 个分类、78 张语法卡片**（含 6 条常见问题）；`<iframe>` 网页嵌入因 MdPad 基于纯原生 `egui` 绘制、无内嵌 WebView 内核且 `markdown.rs` 未支持，故按「不收录实际不支持写法」原则排除。
- **质量验证**：`cargo clippy --all-targets -- -D warnings` 零警告、`cargo build --release` 零警告、`cargo test` 46 项单元测试全部通过；`target/release/mdpad.exe` 体积为 `13,342,208` 字节（约 `12.72 MB`）。

---

## 10. 主题系统与统一空间骨架

### 5 套定稿主题与默认策略
- **主题内核（`src/theme.rs`）**：集中定义 `ThemeId`、`ThemeSpec`、全部颜色与形态 Token、派生色公式及 `LazyLock` 预构建的 `Visuals` / `Spacing` / 侧栏 `Visuals`（每帧零重建开销，并通过 `Context::all_styles_mut` 同时覆盖 Dark 与 Light 两套全局样式，杜绝弹窗/菜单/提示框残留 egui 默认灰蓝）。
- **5 套定稿主题**：
  1. `ZenHorizon`（**01 · 无界疾风**，浅色，**默认主题**：全新安装、缺少 `theme` 字段或配置无法识别时一律容错回落到 01）
  2. `ScarletStreak`（**02 · 白昼疾影**，浅色，绯红圆盾金电徽章 + 1.5px 红→金渐变顶栏线 + 绯红实色金电标签 + 悬浮绯红底栏胶囊）
  3. `AtelierContrast`（**03 · 半山书房**，撞色，左侧 `sidebar_w` 深胡桃木顶栏与侧栏无缝一体 + 向下连通米白书页激活标签 + 底栏左侧路径不跨栏截断并支持全路径悬停提示）
  4. `CarbonGold`（**04 · 黑金疾电**，深色，暗曜石底 + 鎏金激活标签底线 + 贴底收口遥测条）
  5. `TitaniumGlass`（**05 · 钛晶流光**，深色，暗钛底壳内嵌 `6px` 缩进磨砂玻璃板与 `1px` 珍珠银上缘高光 + 玻璃胶囊标签）
- **切换入口与缓存协同**：顶栏最右 ⚡ 主题钮支持左键日/夜配对热切换（`ToggleDayNight`，默认 `Ctrl+Shift+D`，配对默认 01 ↔ 04 且可在「设置 → 外观与编辑器设置」独立指定）、右键弹出 5 主题色块列表；新增命令 `theme_01..05` 接入快捷键与冲突检测；兼容 `MDPAD_DARK=1`（映射 `CarbonGold`）并新增 `MDPAD_THEME=1..5` 或英文名自检入口；切换主题时仅刷新 `Visuals` 与失效 `document.rs` / `highlight.rs` / `manual.rs` 渲染缓存，保留滚动位置、光标、选区与侧栏宽度。

### 统一单行顶栏与零跳动骨架
- **单行一体化顶栏（`TOP_BAR_H = 42.0`）**：左区等宽于侧栏（默认 `220px`）放置矢量徽标、`MdPad` 字标与 `≡` 主菜单（完整收纳原 `文件 / 编辑 / 视图 / 主题 / 设置 / 帮助` 全部条目与快捷键提示）；中区自适应多文档标签栏（`TAB_H = 28.0`）与 `+` 按钮；右区定宽 `编辑 | 分屏 | 预览` 分段胶囊（`144px`）与 ⚡ 主题钮（`28px`）。
- **统一空间常量与零跳动策略**：`STATUS_BAR_H = 28.0`、`SIDEBAR_DEFAULT_W = 220.0`（范围 `170.0..=500.0`）、`SIDEBAR_HEADER_H = 34.0`、`PANE_INSET = 6.0`（5 套主题共用 `6px` 外框内缩，01–04 下无缝隐形、05 下呈现磨砂玻璃板边缘，保障换行宽度与文字位置 1:1 一致）；后台导入进度条 `jobs::floating_banner_ui` 改为右下角悬浮 `Area`，避免中心编辑区高度跳动；分屏严格 50/50（`mid_x = avail.center().x.round()`）。
- **全仓零残留覆盖**：文件树 / 文章列表 / 大纲（`2px` 左侧强调竖条）、编辑器（当前行背景 `current_line_bg` 与语法高亮）、查找替换（含正则错误态）、预览区（`[!NOTE..CAUTION]` 提示块、代码块与矢量复制图标、任务复选框、表格斑马纹；内联 HTML 显式 `style="color/background"` 保持原样）、语法说明书（`ManualPalette`）、全部模态弹窗与 HTML 导出（“跟随当前主题” / “固定浅色 01”）全部统一接入 `ThemeSpec`。

### 派生色推导规则汇总
- 通用线性混合工具 `mix(a, b, num, den)` 与 `alpha(c, a)` 在编译期常量求值：
  - `hover_bg`：surface/chrome/btn 底色向文字/强调色混合 `6%~8%`；
  - `active_bg`：surface/btn 底色向主强调色混合 `10%~18%`；
  - `selection_bg`：画布/玻璃底向主强调色混合 `16%~26%`（侧栏在 `03` 撞色下使用 `mix(left_bg, accent, 32, 100)`）；
  - `disabled_fg`：底色与次文字按 `50%` 混合；
  - `current_line_bg`：callout/code/chrome 卡片底色带 `130..160` 透明度；
  - `widgets.inactive.bg_stroke`：边框色向正文色混合 `18%`（保障浅色主题未勾选复选框与控件描边清晰可读）。

### 实测 WCAG 2.1 对比度、零跳动与性能指标
- **WCAG 2.1 实测对比度（`cargo test theme::tests -- --nocapture --test-threads=1`）**：
  - `ZenHorizon`：正文/画布 `16.60`、正文/代码块 `15.22`、次字/画布 `4.55`、选区 `13.61`、侧栏正文 `16.60`、侧栏次字 `4.55`
  - `ScarletStreak`：正文/画布 `17.72`、正文/代码块 `16.93`、次字/画布 `4.76`、选区 `13.75`、侧栏正文 `16.63`、侧栏次字 `4.47`、红底白字 `4.83`
  - `AtelierContrast`：正文/画布 `15.42`、正文/代码块 `13.92`、次字/画布 `4.91`、选区 `12.45`、侧栏正文 `14.20`、侧栏次字 `5.50`
  - `CarbonGold`：正文/画布 `16.49`、正文/代码块 `15.39`、次字/画布 `6.12`、选区 `9.48`、侧栏正文 `15.39`、侧栏次字 `5.72`
  - `TitaniumGlass`：正文/画布 `14.20`、正文/代码块 `12.76`、次字/画布 `4.74`、选区 `7.55`、侧栏正文 `15.61`、侧栏次字 `5.21`
- **像素级零跳动核验（`1200x828` 同态分屏对比 01–05）**：编辑器文字首像素坐标 `X=243..244`（<=1px 抗锯齿）、`Y=64`；预览区 `X=738`、`Y=75`、可见正文行总数精确同为 `108` 行；侧栏分界线 `X=219`、顶栏底缘 `Y=41`（02–05）。
- **性能与体积回归**：
  - `MDPAD_BENCH=typing`（`100,300` 字符、`200` 样本）：`cpu_ms` 平均 `5.55 ms`（P50 `5.42 ms`，P95 `6.79 ms`），`wall_ms` 平均 `16.63 ms`（P50 `16.66 ms`，P95 `17.21 ms`），未较第 8 节基线（`6.46 ms` / `16.65 ms`）退化。
  - **质量验证**：`cargo clippy --all-targets -- -D warnings` 零警告、`cargo build --release` 零警告、`cargo test` 49 项单元测试全部通过；`target/release/mdpad.exe` 体积为 `13,438,976` 字节（约 `12.82 MB`）。

---

## 11. 自定义标题栏（无边框集成顶栏 + 全主题窗口控件）

- **无边框窗口与安全回退**：
  - `src/main.rs` 依据启动标记录 `use_system_titlebar`（`src/window_frame.rs` 的 `load_startup_use_system_titlebar` / `save_startup_use_system_titlebar` 与 `use_system_titlebar.flag`）设置 `.with_decorations(use_system_titlebar)` 与 `.with_resizable(true)`，并保留最小尺寸约束及 `ViewportCommand::Title` 同步任务栏/Alt+Tab 标题；在自动截图或指定 `MDPAD_WIDTH` / `MDPAD_HEIGHT` / `MDPAD_BENCH=typing` 时隔离 `persistence_path` 并返回 `persist_egui_memory() -> false`，避免读取或污染用户的持久化窗口几何。
  - `src/settings.rs` 新增 `pub use_system_titlebar: bool`（默认 `false`），设置窗口提供「使用系统标题栏」勾选框与「重启后生效」提示；启用后下次启动恢复原生系统标题栏，顶栏不再绘制自定义三键与空白区窗口热区。
- **全主题窗口控件 `ThemeSpec::window_controls`（`src/theme.rs`）**：
  - 新增 `WindowControlStyle` 结构体，5 套主题色值均由现有主题 Token 派生：常态保持克制通透，悬停/按下提供 100 ms `ctx.animate_bool_with_time` 平滑过渡；关闭按钮采用各主题专属危险色并在悬停时切换高对比度白色图标；`AtelierContrast` 针对左深右浅顶栏分别派生左右两侧背景与前景色；失焦时图标柔化弱化。
  - **实测 WCAG 2.1 对比度（常态 / 悬停 / 关闭悬停）**：
    - `ZenHorizon`：`11.02` / `19.12` / `4.83`（左区常态 `11.02`）
    - `ScarletStreak`：`10.28` / `19.90` / `4.83`（左区常态 `10.28`）
    - `AtelierContrast`：`9.77` / `14.49` / `5.31`（左区常态 `11.05`）
    - `CarbonGold`：`11.30` / `12.40` / `6.87`（左区常态 `11.30`）
    - `TitaniumGlass`：`11.49` / `11.30` / `7.11`（左区常态 `11.49`）
- **布局计算、交互热区与 Win11 DWM 圆角（`src/window_frame.rs`、`src/winutil.rs`、`src/app.rs`、`src/tabs.rs`、`src/sidebar/mod.rs`）**：
  - `compute_top_bar_layout` 将「编辑 | 分屏 | 预览」、⚡ 主题按钮与「最小化 / 最大化-还原 / 关闭」纯函数化排布于顶栏右侧；最小宽 `480px` 下自动收敛侧栏上限（`effective_sidebar_bounds` 与 `clamp_sidebar_width`）并压缩标签最小宽度（禁止标签区左键拖拽滚动以便空白区响应拖动），确保右侧控件不重叠、不裁切且 `AtelierContrast` 上下深木色分界 `0.0px` 对齐。
  - `paint_window_controls` 使用 `Painter` 绘制物理像素对齐矢量图标（最小化横线、最大化单框、还原双错位方框、关闭 `×`），在 100%–200% DPI 缩放下保持清晰；最大化状态下关闭按钮真实可点击热区延伸至窗口右上角（符合 Fitts 定律），同时支持 `MDPAD_TEST_HOVER` 与 `MDPAD_TEST_UNFOCUSED` 自动化截图覆盖悬停与失焦态。
  - `handle_window_frame_chrome` 补齐顶栏空白区拖动（`ViewportCommand::StartDrag`，最大化拖动触发系统还原并移动，顶部预留 `RESIZE_EDGE_PX` 避免抢占上边缩放）、双击空白区切换最大化/还原、仿系统右键菜单（还原 / 最小化 / 最大化 / 关闭）以及 8 向窗口边框缩放（常规 `5px` 边 / `10px` L 形角，`is_manual` 模式下右侧缩放边收窄为 `3px` 以保护贴边滚动条；最大化时禁用缩放）。关闭按钮触发原有 `ViewportCommand::Close` 流程，保留未保存标签确认框与 `Alt+F4`。
  - `src/winutil.rs` 新增 `try_enable_rounded_corners`，通过 `DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND)` 在初始化与首帧尝试启用 Windows 11 原生圆角与阴影，Win10 等不支持环境静默降级为 1px 主题色外边框。
- **回归验证与体积基线**：
  - `cargo clippy --all-targets -- -D warnings` 零警告、`cargo test` 53 项单元测试全部通过。
  - `MDPAD_BENCH=typing`（`100,300` 字符、`200` 样本）：`cpu_ms` 平均 `6.37 ms`（P50 `6.08 ms`，P95 `9.09 ms`），`wall_ms` 平均 `16.64 ms`（P50 `16.67 ms`，P95 `17.30 ms`），与原基线（`6.46 ms` / `16.65 ms`）持平。
  - `cargo build --release` 零警告，`target/release/mdpad.exe` 体积为 `13,469,696` 字节（约 `12.85 MB`）。
- **已知限制**：Windows 11 下鼠标悬停自定义「最大化」按钮不会弹出系统原生的 Snap Layouts（贴靠布局）浮层（因无边框客户端自绘按钮未向系统暴露 `WM_NCHITTEST -> HTMAXBUTTON` 热区）；`Win + ↑/↓/←/→` 快捷键贴靠与拖动窗口至屏幕边缘贴靠均正常可用。

---

## 12. 品牌重命名为 ThunderMD 与仓库迁移

- **品牌与构建产物**：应用名称、顶栏字标、窗口标题后缀（`● 文件名 - ThunderMD`）、HTML 导出默认标题、快捷键导出默认文件名（`thundermd-shortcuts.json`）、内置语法说明书文案及内部资源 URI（`bytes://thundermd/*`）全面更新为 **ThunderMD** / **thundermd**；`Cargo.toml` 包名更新为 `thundermd`，Release 构建产物为 `target/release/thundermd.exe`。
- **仓库迁移**：Git `origin` 远程地址更新为 `https://github.com/czh26/ThunderMD.git`，并在 `Cargo.toml` 补充 `repository = "https://github.com/czh26/ThunderMD"`。
- **平滑兼容与窄窗适配**：新增 `winutil::app_env` / `app_env_os` 优先读取 `THUNDERMD_*` 并兼容回退 `MDPAD_*`；非自动化模式首次启动调用 `window_frame::migrate_legacy_storage_if_needed(APP_NAME, "MdPad")` 自动迁移既有 `app.ron` 与 `use_system_titlebar.flag`；顶栏 9 字母字标在 `< 620px` 窄宽下结合 `min_left_w = 136.0`、自适应字号与裁剪矩形，确保与 `≡` 按钮互不重叠。

---

## 13. 顶栏与底栏尺寸体系（ChromeMetrics）与像素级居中

- **集中化尺寸管理（`src/theme.rs`）**：
  - 顶栏、底栏、多文档标签栏、模式切换胶囊、`⚡` 主题按钮、品牌区域、`≡` 按钮、窗口控制三键以及紧凑布局阈值的全部几何尺寸统一收口于 `pub struct ChromeMetrics` 与常量 `pub const CHROME: ChromeMetrics`（[src/theme.rs](file:///c:/Users/25357/Desktop/markdown/src/theme.rs)），`TOP_BAR_H` 与 `STATUS_BAR_H` 直接派生自 `CHROME`；`ThemeSpec` 与 `WindowControlStyle` 仅保留各主题的颜色与造型定义，`app.rs`、`tabs.rs`、`window_frame.rs` 中不再散落界面尺寸魔法数字。
  - **今后修改组件尺寸的方法**：只需调整 `src/theme.rs` 中的 `pub const CHROME: ChromeMetrics` 对应字段，无需修改绘制逻辑；改动后运行 `cargo test` 中的 `chrome_centering_math_within_1px_across_dpi_scales` 与 `--ignored measure_screenshot_chrome_margins` 即可验证所有主题和缩放比例下的对齐不变量。
- **整体放大（约 +15%）与窄窗适配**：
  - 顶栏高度 `42 -> 48 pt`、顶栏胶囊/标签/窗口按钮高度 `28 -> 32 pt`、标签与模式切换字号 `14.0`、品牌字标 `17.5` / 徽标 `26.0`、窗口控制按钮宽 `34.0`（图标描边 `1.55`）；底栏高度 `28 -> 32 pt`、底栏状态胶囊高度 `24.0`（圆角 `12`）、底栏字号 `13.0`；紧凑布局阈值由 `620 -> 700 pt`（`compact_sidebar_min_w = 150.0`），确保 `480 px` 最小窗口宽度下顶栏与侧栏不重叠、不裁切。
  - `src/sidebar/files.rs` 文件树顶栏采用右对齐先预留 `📄+` / `📁+` / `↻` 按钮、左侧目录名自动省略号截断，防止 `480 px` 窄窗下目录名与按钮碰触。
- **像素级垂直居中机制**：
  - `compute_top_bar_layout`（[src/window_frame.rs](file:///c:/Users/25357/Desktop/markdown/src/window_frame.rs)）与 `status_bar`（[src/app.rs](file:///c:/Users/25357/Desktop/markdown/src/app.rs)）先扣除顶部外边框与结构底/顶分割线的物理像素，再在净内容区间 `[content_top_px, content_bot_px]` / `[sb_top_px, sb_bot_px]` 内通过 `snap_centered_span_px` 计算整像素对齐的胶囊边界。
  - **Windows 无边框底边裁剪补偿（`window_frame::bottom_frame_occluded_px`）**：`winit` 无边框还原态窗口在 Windows 下通过 `WM_NCCALCSIZE`（`rgrc[0].top += 1; rgrc[0].bottom += 1`）将客户区相对 `WindowRect` 下移 `1` 物理像素，使最底 `1 px` 落在 `WindowRect` 外被系统裁剪；同时 Windows 11 DWM 圆角底边或 Windows 10 `1pt` 外边框再占用 `(1.0 * ppp).round().max(1.0)` 物理像素。`Panel::bottom("status_bar")` 因此分配 `STATUS_BAR_H + bot_occluded_px / ppp` 并按 `[sb_top_px, bar_bot_px - bot_occluded_px]` 居中（Win10 外边框亦上收 `1 px` 绘制于可见底边内侧），使屏幕真实可见底栏稳定维持 `32 pt` 且上下视觉完全居中。
  - `galley_ink_centered_pos_y` 使用字形真实墨迹包围盒 `Galley::mesh_bounds` 居中文本，消除 Windows 下 `Segoe UI` 与 `msyh.ttc` 行距不对称导致的视觉下沉；所有胶囊（含状态栏统计胶囊、模式切换、品牌字标与标签）围绕同一净内容物理中线对齐。
  - 顶/底分割线采用物理像素对齐的 `rect_filled`，`≡`、`+`、`×` 及脏标圆点改用矢量几何绘制，状态栏悬浮胶囊阴影改为对称 `offset: [0, 0], blur: CHROME.status_pill_shadow_blur (4)`，经自动化像素扫描验证 5 套主题 × `{100%, 150%}` 全部 60 项实测上下留白差均满足 `<= 1 px`。


