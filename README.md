<div align="center">

<img src="assets/icon.png" width="96" alt="ThunderMD">

# ThunderMD

**Windows 原生桌面 Markdown 编辑器 · Rust + egui**

启动快 · 体积小 · 中文排版友好

</div>

---

## ✨ 简介

ThunderMD 是一款面向 Windows 的原生桌面 Markdown 编辑器，使用 Rust（2024 edition）编写，基于 [eframe / egui](https://github.com/emilk/egui) 即时模式 GUI，并采用 OpenGL（glow）后端渲染，Markdown 解析由 [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) 完成。

- **启动快**：选用 glow 而非 wgpu 后端，冷启动更快；开发构建也会对依赖做优化，调试时界面不卡顿。
- **体积小**：发布版开启 `lto = "fat"`、`codegen-units = 1`、`panic = "abort"`、`strip = true`，产物为单个 exe，运行时不弹出控制台窗口。
- **中文友好**：中文连续行软换行时不额外插入空格、英文行间自动补空格；字体扫描与中英混排度量单独处理，默认字体组合为 Segoe UI / Consolas + 微软雅黑。

## 🚀 功能特性

### 编辑与预览

- **三种视图**：编辑、分屏（默认）、预览，可随时切换；另有**源代码模式**切换。
- 编辑区内置轻量 **Markdown 语法高亮**，行尾两个空格的硬换行以淡色 `·` 标出。
- **自动闭合**括号、引号与代码围栏，可单独设置是否在代码块内生效。
- **查找与替换**：支持区分大小写、全词匹配和正则表达式。
- **任务列表可交互**：在预览中点击复选框会同步回写源码。
- **字数统计**：支持选区统计，可选择排除代码块 / 行内代码、忽略 Markdown 标记符号。
- **剪贴板增强**：纯文本粘贴；粘贴网页 / 富文本时可自动转为 Markdown（默认关闭）；粘贴剪贴板图片时自动保存为 PNG 到文档同级的 `assets/` 目录并插入图片链接（未保存的新文档存放在系统临时目录）。
- 关闭标签或退出时，如有未保存的修改会弹窗确认。

### 多文档管理

- **多标签页**：支持拖拽重排、右键菜单、同名文件自动消歧，可重新打开刚关闭的标签。
- **会话恢复**：启动时可恢复上次打开的标签（可在设置中关闭）。
- **侧边栏**，包含三个面板：
  - 📁 **文件树**：打开文件夹作为工作区，懒加载目录、自然排序（`2.md` < `10.md`），支持新建文件 / 文件夹、行内重命名、移入回收站，并监听文件变化；
  - 📰 **文章列表**：后台扫描工作区内的 Markdown 文件，提取标题（Front Matter `title` → 首个 H1 → 文件名）与摘要；
  - 🧭 **大纲**：由当前文档标题生成，支持折叠与当前章节高亮，点击跳转。
- 支持将文件拖入窗口打开，也支持通过命令行参数打开：`thundermd.exe a.md b.md`。

### 语法支持

在 CommonMark 基础上支持 GFM、脚注、上下标、Front Matter，并提供若干 ThunderMD 扩展语法。完整清单见内置语法说明书（数据源：[`src/manual.md`](src/manual.md)）。

| 类别 | 支持内容 |
|---|---|
| 标题 | ATX 标题（`#` ~ `######`）、Setext 下划线式标题、自定义锚点 `{#id}` ¹ |
| 段落与换行 | 段落、软换行、行尾两个空格 / 反斜杠 `\` 硬换行、`<br>` |
| 文字样式 | 粗体、斜体、粗斜体、删除线 `~~`、高亮 `==文字==` ¹、上标 `^x^` / 下标 `~x~` ¹ ²、`<u>` / `<ins>` 下划线 |
| 列表 | 无序、有序、指定起始编号、多级嵌套、列表项内段落与代码块、任务列表 |
| 引用与提示块 | 普通 / 嵌套引用，GFM 提示块 `[!NOTE]` `[!TIP]` `[!IMPORTANT]` `[!WARNING]` `[!CAUTION]` |
| 代码 | 行内代码（含双反引号）、反引号 / 波浪线围栏代码块、缩进代码块；围栏代码块支持语法高亮、行号与一键复制 |
| 链接 | 行内、带标题、引用式、尖括号自动链接、裸网址自动识别、文内锚点、`mailto:` 邮箱链接 |
| 图片 | 行内 / 带标题 / 引用式图片、本地相对路径、`<img width height>` 指定尺寸、可点击图片链接；支持本地与网络图片（PNG、JPEG、GIF、WebP、BMP） |
| 表格 | GFM 表格、列对齐、单元格内行内格式、`\\|` 转义竖线 |
| 脚注与目录 | 数字 / 命名脚注（点击可跳转与返回）、`[TOC]` / `[[TOC]]` / `[[_TOC_]]` 自动目录 ¹ |
| 分割线与 Emoji | `---` / `***` / `___` 分割线、`:emoji:` 短码 ¹ |
| 元数据 | YAML（`---`）与 TOML（`+++`）Front Matter ² |
| 内嵌 HTML | `<kbd>`、`<mark>`、`<sup>` / `<sub>`、`<span style>`、`<font>`、`<center>`、`<p align>` / `<div align>`、`<details>` / `<summary>`、`<b>` `<i>` `<s>` `<del>` `<code>` `<a>`、HTML 注释 |
| 音视频 | `<video>` / `<audio>` 媒体卡片；图片语法中写音视频链接自动升级为卡片 ¹；本地 mp4 / mov / m4a / wav / flac 可读取时长 |
| 转义 | 反斜杠转义、HTML 字符实体 |

¹ ThunderMD 扩展语法，其他编辑器可能不支持。
² 受设置项控制：上标 / 下标默认关闭，Front Matter 预览显示默认开启。

围栏代码块语法高亮支持的语言标识符：`c` / `cpp` / `h` / `hpp`、`js` / `jsx` / `mjs`、`ts` / `tsx`、`py` / `python`、`rs` / `rust`、`json` / `jsonc` / `json5`、`bash` / `sh` / `shell` / `zsh`、`sql`、`toml`、`yaml` / `yml`。

> [!TIP]
> 按 <kbd>F1</kbd> 打开内置**语法说明书**：按分类浏览所有语法卡片，支持中英文关键词搜索、实时预览、一键复制与插入当前文档，相关卡片上还可直接切换对应设置项。

> [!WARNING]
> 开启「上标 / 下标」后，成对的单个 `~` 会被解析为下标而不再表示删除线。删除线请始终使用 `~~文字~~`。

### 导入与导出

- **导出 HTML**：导出带主题样式的完整单文件文档，可选择将本地图片以 Base64 内嵌；文档标题优先读取 Front Matter 中的 `title`。
- **导出 HTML 片段**：仅导出正文部分，便于嵌入其他页面。
- **导入 HTML**：按 BOM → `<meta charset>` → UTF-8 → GBK 的顺序识别编码，优先提取 `<article>` / `<main>` 正文并转换为 Markdown，同时换算相对路径。
- **复制为 HTML / 复制为 Markdown**：便于粘贴到邮件、富文本编辑器等场景。
- 导入导出在后台异步执行，完成后显示提示条。

### 外观与个性化

内置 5 套主题，支持日 / 夜一键热切换，日间、夜间各自使用的主题可在设置中指定：

| 编号 | 名称 | 英文名 | 色调 | 说明 |
|---|---|---|---|---|
| 01 | 无界疾风 | Zen Horizon | 浅色 | 默认主题，默认日间主题 |
| 02 | 白昼疾影 | Scarlet Streak | 浅色 | 可设为日间主题 |
| 03 | 半山书房 | Atelier Contrast | 撞色 | 可设为日间主题 |
| 04 | 黑金疾电 | Carbon & Gold | 深色 | 默认夜间主题 |
| 05 | 钛晶流光 | Titanium Glass | 深色 | 可设为夜间主题 |

「外观与编辑器设置」中还可调整：

- **字体**：后台扫描系统字体，分别设置预览正文、编辑器、代码块字体（代码块优先列出等宽字体），支持按名称筛选；
- **排版与缩放**：编辑器 / 预览字号（10–32 pt）、预览行高（1.2–2.2）、预览段距（0.3–1.5 em）、界面缩放（75%–200%），以及可选的最大行宽限制（560–1400 px，居中显示）；
- **Markdown 与编辑器**：上标 / 下标、Front Matter 显示、代码块行号、自动闭合、富文本粘贴转换、启动恢复标签；
- **字数统计**：排除代码、忽略标记符号；
- **窗口**：默认使用自绘无边框窗口，也可改用 Windows 系统原生标题栏（重启后生效）。

**快捷键可自定义**：快捷键设置窗口支持搜索、分组浏览、按键录制、冲突检测，以及以 JSON 格式导入 / 导出自定义配置。

## ⌨️ 常用快捷键

以下为默认快捷键，均可在快捷键设置（<kbd>Ctrl</kbd>+<kbd>K</kbd>）中修改或解绑。

| 功能 | 快捷键 | 功能 | 快捷键 |
|---|---|---|---|
| 新建文档 | `Ctrl+N` | 新建标签页 | `Ctrl+T` |
| 打开文件 | `Ctrl+O` | 打开文件夹 | `Ctrl+Shift+O` |
| 保存 | `Ctrl+S` | 另存为 | `Ctrl+Shift+S` |
| 关闭标签页 | `Ctrl+W` | 重新打开关闭的标签 | `Ctrl+Shift+T` |
| 下一个 / 上一个标签页 | `Ctrl+Tab` / `Ctrl+Shift+Tab` | 退出 | `Ctrl+Q` |
| 编辑 / 分屏 / 预览视图 | `Ctrl+1` / `Ctrl+2` / `Ctrl+3` | 切换源代码模式 | `Ctrl+/` |
| 查找 | `Ctrl+F` | 替换 | `Ctrl+H` |
| 查找下一个 / 上一个 | `F3` / `Shift+F3` | 显示 / 隐藏侧边栏 | `Ctrl+B` |
| 侧边栏：文件树 / 文章列表 / 大纲 | `Ctrl+Shift+1` / `2` / `3` | 切换日 / 夜主题 | `Ctrl+Shift+D` |
| 行内代码 | ``Ctrl+` `` | 代码块 | ``Ctrl+Shift+` `` |
| 复制为 HTML | `Ctrl+Shift+C` | 纯文本粘贴 | `Ctrl+Shift+V` |
| 增大 / 减小 / 重置字号 | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | 外观与编辑器设置 | `Ctrl+,` |
| 快捷键设置 | `Ctrl+K` | 语法说明书 | `F1` |

> [!NOTE]
> 「导入 HTML」「导出 HTML」「导出 HTML 片段」「复制为 Markdown」以及「选择主题 01–05」默认未绑定快捷键，可按需自行设置。

## 🛠️ 构建与测试

### 环境要求

- Windows 10 / 11
- [Rust](https://www.rust-lang.org/tools/install) **1.95 及以上**：项目使用 2024 edition，且依赖的 eframe / egui 0.37 声明的最低 Rust 版本为 1.95

### 编译运行

```powershell
git clone https://github.com/czh26/ThunderMD.git
cd ThunderMD

# 开发模式运行（依赖以 opt-level = 2 编译，仅保留行号级调试信息）
cargo run

# 也可以直接打开文件
cargo run -- path\to\note.md

# 构建发布版
cargo build --release
```

发布版产物位于 `target\release\thundermd.exe`。构建脚本 `build.rs` 会将 `assets/icon.ico` 及产品名称写入 exe 资源。

### 运行测试

```powershell
cargo test
```

### 清理旧编译产物

修改依赖或 feature、升级 rustc 之后，Cargo 会在 `target` 中留下旧哈希目录，且不会自动清理。`clean-target.ps1` 用于清理这些过期产物：

- 同一编译单元的多个版本只保留最近编译的那份；
- 每个 crate 的增量编译缓存只保留最近 3 个；
- 通过 `cargo metadata` 找出已不在依赖图中的包（例如已删除的依赖、项目改名后的旧 exe / pdb），一并删除。

当前在用的产物会保留，不会触发全量重编译；即使误删，下次编译也只会重建对应单元。

```powershell
.\clean-target.ps1 -DryRun   # 仅列出将要删除的内容及可释放空间
.\clean-target.ps1           # 实际删除
```

> [!WARNING]
> 请勿在 `cargo` 编译过程中运行该脚本。被占用而无法删除的文件会被跳过并给出提示；若 `cargo metadata` 执行失败，脚本会跳过“已不存在的包”这项检查。

## 📂 项目结构

```text
ThunderMD/
├── assets/                # 应用图标（icon.png 运行时窗口图标 / icon.ico exe 资源图标）
├── build.rs               # 嵌入 Windows 图标与产品信息资源
├── clean-target.ps1       # target 目录旧产物清理脚本
├── Cargo.toml
└── src/
    ├── main.rs            # 程序入口、窗口初始化
    ├── app.rs             # 顶层应用状态、布局与统一命令分发
    ├── commands.rs        # 统一命令定义与默认快捷键
    ├── keymap.rs          # 快捷键解析、格式化、冲突检测与匹配
    ├── shortcuts_ui.rs    # 自定义快捷键窗口
    ├── settings.rs        # 持久化设置
    ├── theme.rs           # 主题内核与 5 套主题色板
    ├── dialog.rs          # 通用弹窗外壳与主题化 UI 组件
    ├── window_frame.rs    # 自绘无边框窗口框架
    ├── fonts.rs           # 系统字体扫描与按需加载
    ├── editor.rs          # 编辑器面板、自动补全
    ├── highlight.rs       # 编辑区 Markdown 语法高亮与查找匹配叠加
    ├── find.rs            # 查找与替换
    ├── markdown.rs        # Markdown 解析与排版（pulldown-cmark → Doc）
    ├── preview.rs         # 预览渲染
    ├── textproc.rs        # 预览与导出共用的行内文本扫描器
    ├── html_inline.rs     # 行内 HTML / CSS 样式解析、媒体链接识别
    ├── media.rs           # 音视频卡片与时长读取
    ├── export.rs          # Markdown → HTML 导出
    ├── import.rs          # HTML → Markdown 导入
    ├── convert.rs         # HTML ↔ Markdown 清理与转换
    ├── clipboard.rs       # 剪贴板读写与图片粘贴落盘
    ├── jobs.rs            # 后台任务调度（导入 / 导出）
    ├── document.rs        # 单文档状态与解析 / 统计缓存
    ├── tabs.rs            # 多标签页栏
    ├── stats.rs           # 字数统计
    ├── winutil.rs         # Windows 平台工具（资源管理器定位、默认程序打开、回收站）
    ├── manual.rs          # 语法说明书界面
    ├── manual.md          # 语法说明书数据（编译时嵌入）
    └── sidebar/           # 侧边栏：容器与文件监听、文件树、文章列表、大纲
```

> [!NOTE]
> 语法说明书的内容全部来自 [`src/manual.md`](src/manual.md)，编译时通过 `include_str!` 嵌入程序。新增或修改语法条目只需编辑该文件，格式说明见文件头部注释。

## 🧱 技术栈

| 用途 | 依赖 |
|---|---|
| 语言 | Rust（2024 edition） |
| GUI 框架 | `eframe` / `egui` 0.37（glow / OpenGL 后端） |
| Markdown 解析 | `pulldown-cmark` 0.13 |
| 图片加载 | `egui_extras`、`image` |
| HTML → Markdown | `htmd` |
| 正则 | `regex` |
| 编码识别 | `encoding_rs` |
| Emoji 短码 | `emojis` |
| 字体解析 | `read-fonts` |
| 文件监听 | `notify` |
| 文件对话框 | `rfd` |
| 剪贴板 | `arboard` |
| 配置序列化 | `serde`、`serde_json` |
| Windows API | `windows-sys`、`raw-window-handle` |
| exe 资源嵌入 | `winresource`（构建依赖） |

## 🤝 贡献

欢迎通过 [Issue](https://github.com/czh26/ThunderMD/issues) 反馈问题或提出建议，也欢迎提交 Pull Request。提交代码前建议先运行：

```powershell
cargo fmt
cargo clippy
cargo test
```
