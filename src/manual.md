<!--
ThunderMD 语法说明书数据文件（编译时通过 include_str!("manual.md") 嵌入程序，首次打开 F1 时解析）。

====================== 格式说明（新增条目只改本文件即可） ======================
1. `# 分类名称`         定义一个一级分类（按文件出现顺序显示）。
2. `## 语法名称`        定义一条语法卡片，归属上方最近的分类。
3. 紧跟在 `##` 下方的可选元数据行（每行一个 `key: value`，顺序不限）：
   - `id:`        全局唯一标识符（仅英文/数字/连字符/下划线）
   - `keywords:`  逗号分隔的中英文检索词与同义词（辅助搜索）
   - `tags:`      竖线 `|` 分隔的提醒标签（如：需在设置中开启 | ThunderMD 扩展语法，其他编辑器可能不支持 | HTML 写法）
   - `setting:`   关联设置项开关，可选 `sup_sub` 或 `front_matter`（卡片会显示实时开关按钮）
   - `mode:`      插入行为覆盖，可选 `inline`（光标处直接插入）或 `block`（必要时自动补齐前后空行）
   - `note:`      补充注意事项（显示在说明下方）
4. 元数据之后、代码围栏之前的非空文本行作为该语法的「一句话说明」。
5. 使用 4 个波浪线围栏 `~~~~markdown` ... `~~~~` 包裹示例源码（支持内部嵌套 ``` 代码块，保留缩进与行尾空格）。
=======================================================================
-->

# 标题

## 一级至六级标题
id: heading-atx
keywords: 标题, 井号, heading, title, h1, h2, h3, h4, h5, h6
mode: block
note: `#` 与标题文字之间至少保留一个空格；一、二级标题下方自带分割细线。
在行首使用 1 到 6 个 `#` 号表示不同层级的标题，支持目录与大纲提取。
~~~~markdown
# 一级标题
## 二级标题
### 三级标题
#### 四级标题
##### 五级标题
###### 六级标题
~~~~

## 下划线式标题（Setext）
id: heading-setext
keywords: 标题, 下划线标题, setext, heading, ===, ---
mode: block
note: 写 `---` 分割线时若上方没有空行，会误把上一行文字变成二级标题。
在正文下一行使用至少一个 `=` 或 `-` 可分别生成一级或二级标题。
~~~~markdown
这是一级标题
============

这是二级标题
------------
~~~~

## 自定义标题锚点
id: heading-custom-id
keywords: 标题, 锚点, 自定义id, anchor, heading attributes, {#id}
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
在标题末尾添加 `{#自定义id}`，即可用固定锚点名称做文内跳转。
~~~~markdown
### 接口规范 {#api-spec}

查看上方的 [接口规范](#api-spec)。
~~~~

# 段落与换行

## 段落与软换行
id: paragraph-soft-break
keywords: 段落, 换行, 软换行, 回车, paragraph, soft break
mode: block
note: 中文连续行之间软换行不额外加空格；英文单词行间自动补一个空格。
空一行划分独立段落；普通单次回车只算同一段落内的软换行。
~~~~markdown
第一段：相邻两行中文在软换行时
会自动连接成连续句子。

第二段：English lines joined by
a single newline get a space.
~~~~

## 行尾两个空格换行
id: hard-break-spaces
keywords: 换行, 硬换行, 空格, 两个空格, line break, hard break
mode: block
note: 源码框中用淡色 `·` 标出了行尾两个空格，方便一眼看清。
在行尾输入至少两个空格再回车，可在同一段落内强制换行。
~~~~markdown
北京市海淀区中关村大街 1 号  
收件人：张三
~~~~

## 行尾反斜杠换行
id: hard-break-backslash
keywords: 换行, 硬换行, 反斜杠, backslash, line break
mode: block
note: 比行尾双空格更直观，不容易在编辑时被忽略。
在行尾直接输入反斜杠 `\` 再回车，实现段落内强制换行。
~~~~markdown
第一行内容\
第二行内容
~~~~

## HTML 换行标签
id: hard-break-br
keywords: 换行, br, html, 表格换行, line break
tags: HTML 写法
mode: inline
在段落、引用或表格单元格中插入 `<br>`，强制另起一行。
~~~~markdown
第一行文字<br>第二行文字
~~~~

# 文字样式

## 粗体
id: style-bold
keywords: 加粗, 粗体, 强调, bold, strong, b
mode: inline
用成对的两个星号 `**` 或两个下划线 `__` 包裹需要加粗的文字。
~~~~markdown
这是 **重要结论**，也可以写成 __同样加粗__。
~~~~

## 斜体
id: style-italic
keywords: 斜体, 倾斜, italic, emphasis, em
mode: inline
note: 单词内部的 `snake_case` 下划线不会被误识别为斜体。
用单个星号 `*` 或下划线 `_` 包裹文字使其倾斜。
~~~~markdown
这是 *斜体强调*，以及 _英文 italic 写法_。
~~~~

## 粗斜体
id: style-bold-italic
keywords: 粗斜体, 加粗斜体, bold italic
mode: inline
用三个星号 `***` 或三个下划线 `___` 同时启用粗体与斜体。
~~~~markdown
这是 ***非常重要*** 的提示，或 ___同样效果___。
~~~~

## 删除线
id: style-strikethrough
keywords: 删除线, 划线, 作废, strikethrough, strike, del
mode: inline
note: 推荐始终写两个波浪线 `~~`，不受上标/下标设置影响。
用成对的两个波浪线 `~~` 为文字添加删除线。
~~~~markdown
原价 ~~199 元~~，现价 99 元。
~~~~

## 文字高亮
id: style-highlight
keywords: 高亮, 荧光笔, 标记, highlight, mark
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: inline
用成对的双等号 `==` 包裹文字，添加醒目的高亮底色。
~~~~markdown
请重点核对 ==截止日期与版本号==。
~~~~

## 上标
id: style-superscript
keywords: 上标, 幂, 平方, superscript, sup, ^
tags: 需在设置中开启 | ThunderMD 扩展语法，其他编辑器可能不支持
setting: sup_sub
mode: inline
note: 支持在单词内直接书写，例如 `E=mc^2^`、`x^n^`。
开启「上标 / 下标」设置后，用成对的 `^` 包裹内容生成上标。
~~~~markdown
质能方程：E = mc^2^，面积单位：100 m^2^。
~~~~

## 下标
id: style-subscript
keywords: 下标, 化学式, subscript, sub, ~
tags: 需在设置中开启 | ThunderMD 扩展语法，其他编辑器可能不支持
setting: sup_sub
mode: inline
note: 开启此设置后，单个 `~` 表示下标，删除线请改用 `~~文字~~`。
开启「上标 / 下标」设置后，用成对的单个 `~` 包裹内容生成下标。
~~~~markdown
水的化学式是 H~2~O，二氧化碳是 CO~2~。
~~~~

## 下划线
id: style-underline
keywords: 下划线, underline, u, ins
tags: HTML 写法
mode: inline
标准 Markdown 没有原生下划线语法，可使用 HTML 的 `<u>` 或 `<ins>` 标签。
~~~~markdown
请仔细阅读<u>用户协议</u>和<ins>补充条款</ins>。
~~~~

# 列表

## 无序列表
id: list-unordered
keywords: 无序列表, 项目符号, 圆点, bullet, unordered list, ul
mode: block
note: 符号后面必须保留至少一个空格。
在行首使用 `-`、`*` 或 `+` 加空格创建无序列表。
~~~~markdown
- 需求分析
* 方案设计
+ 编码实现
~~~~

## 有序列表
id: list-ordered
keywords: 有序列表, 编号, 数字列表, ordered list, ol
mode: block
在行首使用 `数字.` 加空格创建有序列表，渲染时会自动顺延编号。
~~~~markdown
1. 拉取最新代码
2. 运行单元测试
3. 提交变更记录
~~~~

## 指定起始编号
id: list-start-number
keywords: 有序列表, 起始编号, 续号, start, ordered list
mode: block
第一项写任意起始数字，后续列表项会从该数字开始递增编号。
~~~~markdown
5. 第五步：准备发布包
6. 第六步：验收截图
7. 第七步：归档文档
~~~~

## 多级嵌套列表
id: list-nested
keywords: 嵌套列表, 子列表, 缩进, nested list
mode: block
note: 通常每层缩进 2 个（无序）或 3 个（有序）空格，使子级标记对齐上级文字起始列。
在列表项下方缩进书写，可无限层级混合嵌套无序与有序列表。
~~~~markdown
- 第一阶段：规划
  1. 梳理语法清单
  2. 设计数据格式
     - 支持关键词检索
     - 支持一键插入
- 第二阶段：验收
~~~~

## 列表项包含段落或代码块
id: list-multiline-content
keywords: 列表, 多段, 代码块, 缩进, 续行, list item
mode: block
列表项内部续写段落或围栏代码块时，只需保持与该列表项正文起始列相同的缩进。
~~~~markdown
1. 安装依赖并运行检查：

   这一段依然属于第 1 项的正文内容。

   ```bash
   cargo test
   ```

2. 检查通过后提交代码。
~~~~

## 任务列表
id: list-task
keywords: 任务列表, 待办, 复选框, checklist, todo, task list
mode: block
note: 在普通文档的预览窗格中直接点击复选框，可同步回写源码状态。
在无序列表标记后写 `[ ]`（未完成）或 `[x]`（已完成）生成可交互待办清单。
~~~~markdown
- [x] 完成需求调研
- [x] 编写核心解析逻辑
- [ ] 完成上线前回归测试
~~~~

# 引用与提示块

## 普通引用
id: quote-basic
keywords: 引用, 块引用, blockquote, quote
mode: block
在行首使用 `>` 加空格表示引用块，内部可混排加粗、链接、列表等格式。
~~~~markdown
> 好的工具应当减少记忆负担，让用户专注内容本身。
~~~~

## 嵌套引用
id: quote-nested
keywords: 嵌套引用, 多层引用, nested blockquote
mode: block
在引用块内部再加一个 `>`，用于引用对话或层次批注。
~~~~markdown
> 第一层引用内容。
>
> > 第二层嵌套引用内容。
~~~~

## 提示块：备注（NOTE）
id: alert-note
keywords: 提示块, 警告框, 备注, note, callout, alert, admonition
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
GFM 提示块语法，在引用块首行写 `[!NOTE]` 展示蓝色信息提醒。
~~~~markdown
> [!NOTE]
> 这是补充背景信息，有助于读者理解上下文。
~~~~

## 提示块：提示（TIP）
id: alert-tip
keywords: 提示块, 技巧, 建议, tip, callout, alert
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
在引用块首行写 `[!TIP]` 展示绿色实用技巧或最佳实践卡片。
~~~~markdown
> [!TIP]
> 在编辑框按 `Ctrl+/` 可以快速切换源代码模式。
~~~~

## 提示块：重要（IMPORTANT）
id: alert-important
keywords: 提示块, 重要, 关键, important, callout, alert
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
在引用块首行写 `[!IMPORTANT]` 展示紫色重点强调卡片。
~~~~markdown
> [!IMPORTANT]
> 导出单文件 HTML 前，请确认图片路径可正常访问。
~~~~

## 提示块：警告（WARNING）
id: alert-warning
keywords: 提示块, 警告, 风险, warning, callout, alert
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
在引用块首行写 `[!WARNING]` 展示琥珀色潜在风险提醒。
~~~~markdown
> [!WARNING]
> 修改快捷键覆盖前建议先导出 JSON 备份。
~~~~

## 提示块：注意（CAUTION）
id: alert-caution
keywords: 提示块, 注意, 危险, 破坏性操作, caution, alert
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
在引用块首行写 `[!CAUTION]` 展示红色高危或不可逆操作提醒。
~~~~markdown
> [!CAUTION]
> 清空未保存的草稿后无法恢复，请谨慎确认。
~~~~

# 代码

## 行内代码
id: code-inline
keywords: 行内代码, 反引号, 等宽, inline code, code
mode: inline
用单个反引号 ` 包裹变量名、文件名或简短命令。
~~~~markdown
使用 `cargo test` 运行全部单元测试。
~~~~

## 包含反引号的行内代码
id: code-inline-escaped-tick
keywords: 行内代码, 双反引号, 反引号嵌套, inline code, backtick
mode: inline
note: 若内容首尾正好紧贴反引号，在起止双反引号内侧各留一个空格，渲染时会自动剥除。
代码内容本身包含反引号时，改用双反引号（或更多）从外层包裹。
~~~~markdown
在 Markdown 中写 `` `行内代码` `` 需要用双反引号包裹。
~~~~

## 带语言的围栏代码块
id: code-fenced-lang
keywords: 代码块, 围栏代码块, 语法高亮, code block, fence, rust, python, javascript, json, bash, sql
mode: block
note: 目前内置语法高亮支持的语言标识符包括：`c`/`cpp`/`h`/`hpp`、`js`/`jsx`/`mjs`、`ts`/`tsx`、`py`/`python`、`rs`/`rust`、`json`/`jsonc`/`json5`、`bash`/`sh`/`shell`/`zsh`、`sql`、`toml`、`yaml`/`yml`。
用三个反引号 ``` 起止并在开头标明语言名，预览区会按语言着色并附带行号与复制按钮。
~~~~markdown
```rust
fn greet(name: &str) {
    println!("你好，{name}！");
}
```
~~~~

## 波浪线围栏代码块
id: code-fenced-tilde
keywords: 代码块, 波浪线, 围栏, tilde fence, code block
mode: block
当代码块内容本身包含 ``` 时，使用三个或更多波浪线 ~~~ 作为外层围栏。
~~~~markdown
~~~markdown
```python
print("Hello ThunderMD")
```
~~~
~~~~

## 缩进代码块
id: code-indented
keywords: 代码块, 缩进代码块, 四个空格, indented code
mode: block
note: 缩进代码块无法指定语言名称，通用场景建议优先使用围栏代码块。
空一行后将每行统一缩进 4 个空格（或 1 个制表符）即可形成代码块。
~~~~markdown
下面是缩进代码块示例：

    let total = 10 + 20;
    println!("total = {total}");
~~~~

# 链接

## 行内链接
id: link-inline
keywords: 链接, 超链接, 网址, link, url, href
mode: inline
使用 `[显示文字](目标地址)` 创建可点击的超链接。
~~~~markdown
欢迎访问 [Rust 官方网站](https://www.rust-lang.org)。
~~~~

## 带标题的链接
id: link-with-title
keywords: 链接, 悬停提示, 标题, link title
mode: inline
在网址后用双引号附带说明文字，鼠标悬停在链接上时会显示该标题。
~~~~markdown
前往 [ThunderMD 代码仓库](https://github.com/czh26/ThunderMD "查看源码与说明")。
~~~~

## 引用式链接
id: link-reference
keywords: 链接, 引用式链接, 参考链接, reference link
mode: block
note: 链接定义 `[id]: ...` 可放在文末任意位置，渲染时不会直接显示。
将文字与 URL 分离，适合正文多次引用同一网址或保持段落整洁。
~~~~markdown
推荐阅读 [CommonMark 规范][cm] 与 [GFM 指南][gfm]。

[cm]: https://spec.commonmark.org "CommonMark 标准"
[gfm]: https://github.github.com/gfm/
~~~~

## 尖括号自动链接
id: link-autolink-angle
keywords: 自动链接, 尖括号, 网址, autolink
mode: inline
用尖括号 `<...>` 包裹完整网址，直接显示链接本身并可点击打开。
~~~~markdown
项目主页：<https://www.rust-lang.org>
~~~~

## GFM 裸网址自动识别
id: link-gfm-bare-url
keywords: 裸链接, 自动链接, 网址, bare url, autolink, gfm
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: inline
直接写出以 `https://`、`http://` 或 `www.` 开头的网址，无需方括号也会自动转为可点击链接。
~~~~markdown
无需包裹直接识别：https://github.com 以及 www.example.com。
~~~~

## 文内锚点跳转
id: link-anchor
keywords: 锚点, 页内跳转, 目录跳转, anchor, fragment
mode: inline
note: 中文和英文标题会自动生成全小写、空格转 `-` 的锚点；点击下方链接可在卡片内测试跳转。
使用 `#目标标题`（或自定义 `{#id}`）跳转到同一文档中的指定章节。
~~~~markdown
点击跳转：[前往目标小节](#锚点目标测试)

#### 锚点目标测试
成功定位到此处！
~~~~

## 邮箱链接
id: link-email
keywords: 邮箱, 邮件, mailto, email
mode: inline
使用 `<邮箱>` 自动链接或 `[文字](mailto:邮箱)` 呼出系统默认邮件客户端。
~~~~markdown
联系作者：<support@example.com> 或 [发送邮件](mailto:support@example.com)。
~~~~

# 图片

## 基本图片
id: image-basic
keywords: 图片, 插图, 图像, image, img, picture
mode: block
note: 本例使用内置演示资源保证离线预览；实际写作时请换成 `https://...` 网络图或本地路径。
使用 `![替代文本](图片地址)` 插入图片，替代文本用于图片未加载时的悬停说明。
~~~~markdown
![山川风景示意图](bytes://thundermd/landscape.png)
~~~~

## 带标题的图片
id: image-with-title
keywords: 图片, 图片标题, 悬停提示, image title
mode: block
在图片地址后用双引号写标题，鼠标悬停在图片上时会显示提示。
~~~~markdown
![功能徽章图](bytes://thundermd/badge.png "鼠标悬停时显示的图片标题")
~~~~

## 引用式图片
id: image-reference
keywords: 图片, 引用式图片, reference image
mode: block
与引用式链接类似，把较长的图片 URL 集中放到文档末尾管理。
~~~~markdown
![展示图][demo-pic]

[demo-pic]: bytes://thundermd/badge.png "引用式图片演示"
~~~~

## 本地相对路径图片
id: image-local-relative
keywords: 本地图片, 相对路径, assets, local image
mode: block
note: 请替换成你自己的文件路径（如 `./assets/photo.png`）；找不到本地文件时说明书会显示占位示意图。
引用当前 Markdown 文件所在目录或子目录中的本地图片。
~~~~markdown
![项目结构示意图](./images/diagram.png)
~~~~

## 用 HTML 指定图片宽高
id: image-html-size
keywords: 图片尺寸, 宽高, 缩放, width, height, img
tags: HTML 写法
mode: inline
note: 仅写 `width` 或 `height` 其中一个属性时，按原图纵横比等比缩放；同时指定两者则按指定像素拉伸。
标准 Markdown 不支持设置宽高，可用 HTML `<img>` 标签精确控制显示尺寸。
~~~~markdown
<img src="bytes://thundermd/badge.png" width="120" height="72" alt="自定义尺寸示例" title="固定宽高 120x72">
~~~~

## 图片链接（点击图片跳转）
id: image-clickable-link
keywords: 图片链接, 点击图片, 徽章, clickable image, badge
mode: block
用链接语法 `[...](跳转地址)` 包裹图片语法 `![...](图片地址)`，点击图片即可打开网页。
~~~~markdown
[![点击访问 Rust 官网](bytes://thundermd/badge.png "前往 Rust 官网")](https://www.rust-lang.org)
~~~~

# 表格

## 基本表格
id: table-basic
keywords: 表格, 列表格, table, grid
mode: block
用竖线 `|` 分隔列，表头下方必须有一行由三个以上 `-` 组成的分隔行。
~~~~markdown
| 功能 | 状态 | 说明 |
|---|---|---|
| 多标签页 | 已支持 | 支持拖拽排序 |
| 语法说明书 | 已支持 | 按 F1 打开 |
~~~~

## 表格对齐方式
id: table-alignment
keywords: 表格, 对齐, 左对齐, 居中, 右对齐, alignment
mode: block
在分隔行使用 `:---`（左对齐）、`:---:`（居中）、`---:`（右对齐）控制列对齐。
~~~~markdown
| 商品 | 数量 | 单价 |
|:---|:---:|---:|
| 笔记本 | 2 | 18.00 |
| 钢笔 | 1 | 45.50 |
~~~~

## 单元格内的行内格式
id: table-inline-format
keywords: 表格, 粗体, 高亮, 代码, 链接, 单元格格式
mode: block
表格单元格内支持粗体、斜体、删除线、高亮、行内代码与链接。
~~~~markdown
| 写法 | 效果 |
|---|---|
| `**重要**` | **重要** |
| `==重点==` | ==重点== |
| `[官网](https://www.rust-lang.org)` | [官网](https://www.rust-lang.org) |
~~~~

## 单元格内写竖线
id: table-escape-pipe
keywords: 表格, 竖线, 转义, pipe, |
mode: block
表格列分隔符也是 `|`，若要在单元格里显示字面竖线字符，请写成 `\|`。
~~~~markdown
| 运算符 | 含义 |
|---|---|
| a \| b | 按位或 |
| `x \| y` | 代码中的竖线 |
~~~~

# 脚注与目录

## 数字脚注
id: footnote-numeric
keywords: 脚注, 注释, 引用, footnote
mode: block
note: 点击正文中的上标 `[1]` 跳转到卡片底部脚注，点击 `↩ 返回正文` 可跳回原处。
在正文中使用 `[^数字]` 插入脚注标记，并在文档任意位置用 `[^数字]: 解释` 定义内容。
~~~~markdown
Markdown 由 John Gruber 于 2004 年提出[^1]。

[^1]: 设计初衷是让纯文本本身具备良好的可读性。
~~~~

## 命名脚注
id: footnote-named
keywords: 脚注, 命名脚注, 标签脚注, named footnote
mode: block
note: 无论名称叫什么，渲染时都会按正文出现的先后顺序自动编号。
脚注标识符可以使用语义化的英文或拼音名称，方便维护长文档。
~~~~markdown
ThunderMD 基于 eframe 与 egui[^egui-note] 构建。

[^egui-note]: egui 是纯 Rust 实现的跨平台即时模式 GUI 库。
~~~~

## 自动目录
id: toc-block
keywords: 目录, 自动目录, 大纲, toc, table of contents
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
note: 也兼容 `[[TOC]]` 与 `[[_TOC_]]` 写法，目录项支持点击跳转。
单独一行写 `[TOC]`，自动汇总当前文档全部标题生成可点击目录。
~~~~markdown
[TOC]

### 第一节：概述
说明背景与目标。

### 第二节：快速上手
介绍基础操作。
~~~~

# 分割线与 Emoji

## 分割线
id: horizontal-rule
keywords: 分割线, 分隔线, 横线, hr, horizontal rule, divider
mode: block
note: 如果使用 `---`，请务必在上方留一行空行，否则会把上一行文字识别为二级标题。
单独一行输入至少三个连续的 `*`、`-` 或 `_`（中间可加空格）生成水平分割线。
~~~~markdown
第一部分内容。

---

第二部分内容。

***
~~~~

## Emoji 短码
id: emoji-shortcode
keywords: emoji, 表情, 短码, 图标, smile, rocket, heart
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
note: 完整词库基于 GitHub / Unicode 标准短码，下面列出写作中最常用的部分。
用成对的冒号 `:短码:` 插入原生彩色或单色 Emoji 表情。
~~~~markdown
常用短码：:smile: :rocket: :+1: :heart: :tada: :sparkles:

| 短码 | 效果 | 短码 | 效果 |
|---|---|---|---|
| `:smile:` | :smile: | `:rocket:` | :rocket: |
| `:+1:` | :+1: | `:heart:` | :heart: |
| `:tada:` | :tada: | `:sparkles:` | :sparkles: |
| `:bulb:` | :bulb: | `:warning:` | :warning: |
| `:white_check_mark:` | :white_check_mark: | `:fire:` | :fire: |
~~~~

# 元数据

## YAML Front Matter
id: meta-yaml
keywords: 元数据, front matter, yaml, 头部信息, metadata
tags: 需在设置中开启
setting: front_matter
mode: block
note: 必须位于文档第 1 行；关闭「预览中显示 YAML Front Matter」后预览区会隐藏该卡片。
在文档开头用两行 `---` 包裹 YAML 键值对，用于记录标题、作者与标签（导出 HTML 时也会读取其中的 `title`）。
~~~~markdown
---
title: 产品发布说明
author: ThunderMD
tags: [rust, markdown]
---

正文从这里开始。
~~~~

## TOML Front Matter
id: meta-toml
keywords: 元数据, front matter, toml, 头部信息, hugo, +++
tags: 需在设置中开启
setting: front_matter
mode: block
note: 同样受「预览中显示 YAML Front Matter」开关控制，且必须位于文档第 1 行。
在文档开头用两行 `+++` 包裹 TOML 键值对与 `[section]` 配置。
~~~~markdown
+++
title = "TOML 元数据示例"
draft = false
[author]
name = "ThunderMD"
+++

正文从这里开始。
~~~~

# 内嵌 HTML

## 按键样式
id: html-kbd
keywords: 按键, 快捷键, 键盘, kbd, keyboard
tags: HTML 写法
mode: inline
使用 `<kbd>` 标签渲染具有键盘按键背景框的快捷键组合。
~~~~markdown
按 <kbd>F1</kbd> 打开语法说明书，按 <kbd>Ctrl</kbd>+<kbd>S</kbd> 保存文档。
~~~~

## HTML 高亮标记
id: html-mark
keywords: 高亮, mark, html, 荧光笔
tags: HTML 写法
mode: inline
标准 HTML 高亮标签 `<mark>`，效果与 ThunderMD 扩展 `==高亮==` 一致，兼容更多编辑器。
~~~~markdown
请优先检查 <mark>关键修改行</mark>。
~~~~

## HTML 上标与下标
id: html-sup-sub
keywords: 上标, 下标, sup, sub, 化学式, 幂, html
tags: HTML 写法
mode: inline
note: 如果不想修改全局设置，或者需要导出给其他 Markdown 工具，推荐使用 `<sup>` / `<sub>`。
使用 `<sup>` 和 `<sub>` 表示上标与下标，无需开启额外设置，也不影响单个 `~`。
~~~~markdown
平方和：a<sup>2</sup> + b<sup>2</sup> = c<sup>2</sup>；硫酸：H<sub>2</sub>SO<sub>4</sub>。
~~~~

## 行内样式（颜色与背景）
id: html-span-style
keywords: 颜色, 字体颜色, 背景色, 高亮颜色, span, style, css
tags: HTML 写法
mode: inline
note: `style` 支持 `color`、`background-color`、`font-weight`、`font-style`、`text-decoration`、`font-size`；颜色支持十六进制（`#f00` / `#e05d44`）、`rgb()` 和英文色名。
使用 `<span style="...">` 为文字自定义前景色、背景色、粗斜体与字号。
~~~~markdown
<span style="color: #e05d44; font-weight: bold;">红色加粗</span>、<span style="background-color: #2ea043; color: white;">绿色底纹</span> 与 <span style="color: dodgerblue; text-decoration: underline;">蓝色下划线</span>。
~~~~

## 字体标签
id: html-font
keywords: 字体颜色, 字号, font, color, size, html
tags: HTML 写法
mode: inline
使用传统 HTML `<font color="..." size="1..7">` 快速设置文字颜色和大小。
~~~~markdown
<font color="#2da44e" size="5">放大绿色字</font> 与 <font color="crimson" size="2">缩小深红字</font>。
~~~~

## 居中容器
id: html-center
keywords: 居中, 居中对齐, center, align, html
tags: HTML 写法
mode: block
note: 当 `<center>` 内包含 Markdown 段落或图片时，首尾标签与内容之间请保留空行。
使用 `<center>` 容器使其中的段落、标题或图片水平居中显示。
~~~~markdown
<center>

居中的标题或**加粗文字**

![居中示意图](bytes://thundermd/badge.png)

</center>
~~~~

## 段落与容器对齐
id: html-align-div-p
keywords: 对齐, 居中, 右对齐, 左对齐, div align, p align, text-align
tags: HTML 写法
mode: block
使用 `<p align="...">` 或 `<div align="...">`（支持 `left` / `center` / `right`，也支持 `style="text-align: center"`）控制整块内容对齐。
~~~~markdown
<p align="center">居中对齐的单行段落</p>

<div align="right">

右对齐的署名：ThunderMD 团队

</div>
~~~~

## 可折叠详情块
id: html-details
keywords: 折叠, 展开, 详情, 手风琴, details, summary, collapse
tags: HTML 写法
mode: block
note: 添加 `open` 属性可默认展开；标签与内部 Markdown 块之间保留空行可混排任意列表和代码块。
使用 `<details>` 和 `<summary>` 创建交互式折叠面板，点击标题可展开或收起。
~~~~markdown
<details open>
<summary>点击收起或展开详细步骤</summary>

这里支持任意 **Markdown** 语法：
- 第一步：编写内容
- 第二步：预览核对

</details>
~~~~

## 其他常用行内 HTML 标签
id: html-inline-misc
keywords: 粗体, 斜体, 删除线, 链接, 代码, b, strong, i, em, del, s, a, code
tags: HTML 写法
mode: inline
在表格或复杂混排中，可直接写 `<b>`、`<i>`、`<s>`、`<del>`、`<code>` 与 `<a href="..." title="...">`。
~~~~markdown
<b>粗体</b>、<i>斜体</i>、<del>删除</del>、<code>行内代码</code> 与 <a href="https://www.rust-lang.org" title="Rust 官网">HTML 链接</a>。
~~~~

## HTML 注释
id: html-comment
keywords: 注释, 隐藏, 备注, comment, <!--
tags: HTML 写法
mode: block
使用 `<!-- ... -->` 编写隐藏注释，仅在编辑器源码中可见，预览和导出 HTML 时自动隐藏。
~~~~markdown
这一行会正常显示。

<!-- 这是一条隐藏注释，预览区不可见 -->

注释下方的正文继续正常显示。
~~~~

# 音视频

## 视频卡片
id: media-video
keywords: 视频, 播放器, 封面, poster, video, mp4, mov, mkv, webm
tags: HTML 写法
mode: block
note: 本地相对路径（如 `assets/demo.mp4`）会自动解析 mp4/mov 文件头时长；本例使用网络视频地址与内置封面图演示，点击卡片将在外部默认程序中打开。
使用 `<video>` 标签生成媒体信息卡片，支持 `poster` 封面图、`title` 标题及内部 `<source>` 子标签。
~~~~markdown
<video src="https://example.com/media/demo.mp4" poster="bytes://thundermd/landscape.png" title="ThunderMD 功能演示视频" controls></video>
~~~~

## 音频卡片
id: media-audio
keywords: 音频, 音乐, 播客, audio, mp3, wav, flac, m4a, ogg
tags: HTML 写法
mode: block
note: 本地 wav / flac / m4a 文件会自动读取并展示音频时长；请将 `src` 替换为你自己的音频文件或链接。
使用 `<audio>` 标签渲染音频卡片，展示标题与地址并支持点击打开。
~~~~markdown
<audio src="https://example.com/media/bgm.wav" title="示例背景音频"></audio>
~~~~

## 图片语法写媒体链接
id: media-image-syntax
keywords: 视频, 音频, 媒体链接, mp4, mp3, wav, mov, flac
tags: ThunderMD 扩展语法，其他编辑器可能不支持
mode: block
note: 识别扩展名包括视频 `.mp4` `.mov` `.webm` `.mkv` 与音频 `.mp3` `.m4a` `.wav` `.flac` `.ogg`。
直接把常见音视频文件名放进 Markdown 图片语法 `![标题](地址)` 中，自动升级为媒体卡片。
~~~~markdown
![产品宣讲音频](https://example.com/media/intro.mp3)
~~~~

# 转义

## 反斜杠转义
id: escape-backslash
keywords: 转义, 反斜杠, 字面字符, 星号, 井号, escape, backslash
mode: inline
在 Markdown 标记符号前加反斜杠 `\`，让系统按普通字面字符显示，不触发格式。
~~~~markdown
字面星号：\*不是斜体\*；字面井号：\# 不是标题；字面扩展：\==不是高亮==、\:smile:。
~~~~

## HTML 字符实体
id: escape-html-entities
keywords: 实体, 字符实体, 转义, 版权, 空格, entity, &lt, &copy, &nbsp
mode: inline
使用 `&实体名;` 输出尖括号、版权号、不换行空格或特殊排版符号。
~~~~markdown
小于号 &lt; 与大于号 &gt;，&amp; 符号，版权 &copy; 2026，破折号 &mdash; 说明。
~~~~

# 常见问题

## 为什么按一次回车换行不生效？
id: faq-line-break
keywords: 常见问题, 换行不生效, 回车, 软换行, 硬换行, faq
mode: block
CommonMark 规定普通单次回车只是「软换行」，会在同一段内合并显示。需要段内换行请在行尾加两个空格、反斜杠 \ 或 <br>；需要分新段落请中间空一行。
~~~~markdown
错误示范（直接回车不会换行）：
第一行
第二行

正确做法（任选其一）：
第一行  
第二行

或者使用反斜杠\
下一行
~~~~

## 为什么单个 ~ 不是删除线？
id: faq-single-tilde
keywords: 常见问题, 删除线, 波浪线, 下标, tilde, strikethrough, faq
setting: sup_sub
mode: inline
note: 保持书写双波浪线 `~~` 最稳妥，无论是否开启上标/下标设置都能稳定显示为删除线。
在 ThunderMD 中，当开启「上标 / 下标」设置后，成对的单个 `~` 会优先解析为下标（如 `H~2~O`），不再表示删除线；只有关闭该设置时单个 `~` 才会被当作删除线。
~~~~markdown
总是删除线：~~已过期内容~~；单波浪线随设置变化：~文本~、H~2~O。
~~~~

## 怎么在表格里换行？
id: faq-table-linebreak
keywords: 常见问题, 表格换行, br, 多行单元格, table newline, faq
mode: block
GFM 表格要求一行对应表格中的一条记录，不能在源码里直接回车换行，请改用 `<br>` 标签。
~~~~markdown
| 模块 | 职责说明 |
|---|---|
| 解析器 | 1. 词法切分<br>2. 构建结构树 |
| 渲染器 | 视口裁剪<br>高亮绘制 |
~~~~

## 怎么让图片居中？
id: faq-center-image
keywords: 常见问题, 图片居中, 居中, align, center, image, faq
mode: block
note: 记得在 `<div>` / `<center>` 标签与 Markdown 图片行之间各留一个空行，这样 Markdown 图片语法才会被正常解析。
使用 `<div align="center">` 或 `<center>` 包裹图片即可居中，也可以直接写居中 `<img>`。
~~~~markdown
<div align="center">

![居中演示图](bytes://thundermd/badge.png)

</div>
~~~~

## 怎么输入 * # [ 等字面字符？
id: faq-literal-chars
keywords: 常见问题, 字面字符, 逃逸, 反斜杠, 行内代码, literal, escape, faq
mode: inline
在容易被识别成语法标记的字符前加反斜杠 \，或者直接用反引号 ` 把整段代码/符号包起来。
~~~~markdown
使用反斜杠：\*星号\*、\[方括号\]、\#井号；或用行内代码：`* # == [1]`。
~~~~

## 为什么写 --- 分割线把上一行文字变成了大标题？
id: faq-hr-became-heading
keywords: 常见问题, 分割线, 标题, setext, 误判, ---, faq
mode: block
在 Markdown 中，如果紧贴着一行正文下面写 ---（中间没有空行），会被优先识别成「下划线式二级标题」。在 --- 上方空一行即可恢复为分割线。
~~~~markdown
错误示范（紧贴上一行会变成大标题）
---

正确示范（上方留空行才是分割线）

---
~~~~
