# Lumenfolio → 个人知识库（"第二大脑"）转型 — 方案与计划

**状态：** P0 / P1 / P2 / P2.5 / P3 已闭环（已测/已构建/已提交，待 live 验证）；剩 P4（信息架构收尾）
**分支：** `feat/knowledge-base-pivot`
**日期：** 2026-06-26（更新：2026-06-27）
**作者：** （规划讨论）

---

## 0. 一句话主张

把产品重心**倒过来**：从 **"带知识图谱的 AI PDF 阅读器"** 转为 **"以个人知识库（大脑）为主体、由多种来源（PDF、Office 文档、网页剪藏、笔记、Markdown）喂养"**。PDF 退化为众多输入源之一；阅读器退化为"按类型自适应的来源预览"；**"问我的整个知识库"** 成为默认主循环。

---

## 1. 愿景（已锁定）

- **形态：** 第二大脑 / 多源汇聚（Obsidian + AI 方向）。
- **核心用户任务：** 把从任何地方学到的东西都收进来、**在应用内编写/整理**，并能向积累的知识提问。
- **核心能力（不只是"收集+阅读"）：** 多源摄入 + **在线编写知识（Markdown 编辑器 + 双向链接 + AI 辅助写作，见 §7）** + 全库问答。
- **要支持的输入源：**
  - PDF（已有）
  - Office：Word（`.docx`）、Excel（`.xlsx`/`.xls`）、PowerPoint（`.pptx`）—— **要可预览**
  - 网页 / 文章剪藏（URL → 可读正文）
  - 独立笔记 / 想法（不依附任何文档）
  - Markdown / 纯文本
- **主交互：** "问我的知识库"（全库检索）为默认；"聚焦某个来源"为可选。

---

## 2. 现状分析（基于代码实证）

### 2.1 后端已经 ~60% 是知识库 —— 直接复用

跨文档的知识层**已经存在**，且并非文档作用域：

| 能力 | 位置 | 是否已 KB 化 |
|---|---|---|
| 跨库统一的实体/概念/关键词（经 `normalize_key` 别名归一） | `document_artifacts` 表；`runtime/precipitation.rs` | ✅ 已跨文档 |
| 文档—文档关系图（共引、共概念） | `document_links` 表；`runtime/knowledge_graph.rs` | ✅ 已全库 |
| 每次对话沉淀出的主张（带跨文档引用） | `knowledge_claims` 表；`precipitate_turn()` | ✅ 已多文档感知 |
| 会话与文档解耦 | `chat_sessions.focus_document_id` 可空且可变 | ✅ 已与文档无关 |
| 库级检索工具 | `runtime/rag/mod.rs` 中的 `search_library_knowledge`、`query_knowledge_graph` | ✅ 已全局 |
| 通用 chunk 存储 + FTS | `document_chunks` + `document_chunks_fts` | ✅ 来源无关的文本 |
| 知识沉淀（LLM 抽取） | `run_precipitation_job()` | ✅ 在 LLM 边界处格式无关；只有"取输入"那段是 PDF 专属 |

### 2.2 三处焊死在"你正在读的那篇 PDF"上

1. **摄入只支持 PDF** —— `pdfium_render` → 页/bbox 抽取（`src-tauri/src/pdf_index/`、`document_index.rs`）。知识只能通过"索引一篇 PDF"进入系统。
2. **引用是页 + bbox** —— `Citation { page, block_id, bbox_list, ... }` 是承重结构；非分页内容（网页、笔记、Markdown）无法表达。
3. **检索假定有焦点文档** —— `build_retrieval_run()` 在单个 `document_id` 上做种子检索；`chat_turns.document_id` 是 `NOT NULL`。

### 2.3 前端 ~50–60% 是 PDF 阅读器

- 信息架构围绕"打开的那篇 PDF"：`ReaderPane.vue`（~1719 行）+ `PdfViewer.vue` 占据中央（约 60–70% 宽度）。
- 知识图谱 / 知识卡是**挂在 PDF 上的浮层/工具**，不是"家"。
- **笔记绑死在 文档 + 页 上** —— 无法记一条独立笔记/想法，也无法剪藏网页。
- 唯一入口是"加文件夹 → 扫 PDF → 阅读"。

### 2.4 复用 vs 改造一览

**可直接复用：** 知识图谱（`document_artifacts`、`document_links`、`normalize_key`）、沉淀逻辑、会话、库级 RAG 工具、chunk 存储 + FTS、`MarkdownText.vue`、`KnowledgeGraphView.vue`、`KnowledgeMiniGraph.vue`、`ChatPane.vue`（焦点 → 全库）、`PdfViewer.vue`（PDF 仍需要）。

**需改造：** 摄入（PDF → 按类型可插拔）、引用（页+bbox → 位置无关的锚点）、检索入口（焦点可选 + 全库默认）、数据模型（页/bbox 可空 + `content_type`）、信息架构（阅读器 → 按类型自适应详情视图；侧栏 → 知识导航器；图谱前移接近"家"）、笔记（支持独立存在）。

---

## 3. 关键架构决策（已锁定）

1. **来源（source）抽象。** 把 `documents` 泛化为"来源/条目"，加 `content_type` 鉴别符（`pdf | docx | xlsx | pptx | web | markdown | text | note`）。`page_count`、`page_no`、`bbox_*` 改为**可选**。
2. **引用锚点抽象。** 引用携带带类型的锚点：
   - 分页来源（PDF、若将来用 Office→PDF）：`page + bbox`（不变）。
   - 非分页来源（网页、Markdown、笔记、Office 转文本）：`chunk_id + 章节/字符偏移`。
   - 实现：`page` / `bbox_list` 改可空；加锚点 `kind`，前端据此渲染"章节：…"而非"第 5 页"。
3. **检索默认转为全库。** 检索请求与 `chat_turns` 的 `document_id` 改为可选。无焦点时用 `search_library_knowledge` / `query_knowledge_graph` 做种子，而非单文档种子链。
4. **笔记升为一等知识项。** 独立笔记本身即一种来源（可被切块、沉淀、检索），与现有"在某来源上做高亮批注"那种笔记并存。
5. **Office = 客户端开源预览 + Rust 文本解析（不用 LibreOffice）。** 见 §5。
6. **可插拔摄入。** 引入 `ContentIngestor` trait；`PdfIngestor`（现有），外加 `MarkdownIngestor`、`PlaintextIngestor`、`WebClipIngestor`、`NoteIngestor`，以及后续的 Office 摄入器。`upsert_document_index()` 按 `content_type` 分发。

---

## 4. 分阶段路线

节奏决策：**先 P0 → P1**（用现有 PDF 知识重构体验、验证方向），再做多源/Office 的重活。

### P0 — 地基（仅后端、不可见、向后兼容）✅ 已闭环

**已落地（commit 25a006d / f67e52d / f8f0572 / + P0-d）：**
- **P0-a** `documents.content_type`（默认 `'pdf'` + 幂等迁移 backfill）。
- **P0-b** 摄入按 `content_type` **分发接缝**（pdf→现有路径；其它→明确报错，待 P2/P3 接入）。
- **P0-c** `CitationAnchor{Paged,Reference}` + `Citation::anchor()`，**形式化 `page==0` 锚点约定**。
- **P0-d** `build_retrieval_run` **焦点可选**：空 `document_id` → 返回空的有效 run，交给库级工具（含单测）。

**实施中的三处工程取舍（替代原措辞，理由=避免无消费者的过早抽象/高风险迁移）：**
- **`ContentIngestor` trait → 延后到 P2**：现仅 PDF 一个实现，trait 会过早固化 pdfium/OCR/progress 形状；先落分发接缝，待第二个 ingestor 出现再抽。
- **引用锚点不 `Option` 化 → 延后到 P2**：`page==0` 哨兵已能表达非分页源（trending/web 在用、证据 chip 过滤已认），Option 化波及几乎所有构造点且更难用；P0 先形式化约定，精确 chunk-id/偏移定位留 P2。
- **`chat_turns.document_id` 改可空 → 延后到 P1**：该列 `NOT NULL` 且带 FK，需"建新表→拷数据→改名→重建 2 个索引"的**数据承载型重建**；当前无消费者（无焦点首页对话是 P1 才有），speculative 重建用户聊天历史风险高收益零。检索接缝（P0-d）已就位，P1 接首页时一起做该迁移。
- **风险：** 低。无 UI 变化，保持现有 PDF 行为。`page_no/bbox/page_count` 的 `Option` 化亦随各自消费者（P2/P3）增量进行。

### P1 — "问我的知识库" + 知识库首页（重构体验，几乎全复用）

**功能内核 ✅ 已落地（可测/已构建/已提交）：**
- **P1-a** `chat_turns.document_id` 改可空（数据承载型表重建,幂等/动态列拷贝/孤儿安全,单测)。
- **P1-b** `run_ask_document` 接受**无焦点**全库问答turn(空 doc + session),持久化存 NULL document_id。增量,焦点路径不变。
- **P1-c** 前端 `handleSend` 无焦点时**走全库**(空 documentId,"问我的知识库");并打上库级模式标签。后端 P1-b 配套。

**视觉 IA ✅ 已落地(已构建,待你 live 验证/微调)：**
- **P1-d 知识库首页 `KnowledgeHome.vue`**:无文档时中央显示"问我的知识库"落地页(库级提问框 → 右栏对话、概念 chips、最近来源)。增量——reader/trending/graph 不变,仅空文档中央换成首页。
- **P1-d 侧栏图标栏**:`WorkspaceSidebar` 展开态左侧加 Obsidian 式图标栏(来源=树/默认、图谱、趋势、设置),复用现有 open-graph/open-trending/open-settings;树/搜索/footer 进面板,折叠态不动。
- **留作后续(非阻塞)**:概念/会话/搜索独立面板、完整"对话撑满中央"的自适应舞台、文档标签页 —— 视觉增强,按 mockup live 迭代。
- **风险：** 低(全部增量、向后兼容)。需你在 app 里 live 验证视觉细节。

### P2 — 独立摄入（第二大脑内核）✅ 已闭环（已测/已构建/已提交，待你 live 验证）

- **P2-a 数据模型 + CRUD**：`documents` 加可空 `body_md`（可编辑正文）+ `source_url`（剪藏来源）；虚拟「知识库」根（`root-knowledge-base`）承载脱离磁盘的来源，目录重扫只回收 `content_type='pdf'`，笔记永不被误删。命令 `create_note_source` / `update_note_source` / `load_note_source`（与既有"PDF 页内批注"`create_note` 区分命名）。
- **P2-b 文本回流管道**：`run_document_reindex_job` 把 note/markdown/text/web 分发到**专用、无几何**的索引路径（PDF 块归一化是论文/几何专属，会破坏零几何文本块）。`upsert_text_document_index`：markdown → 标题/正文块（代码围栏整体保留）→ `document_chunks` + FTS + 结构树 + 沉淀队列，page=0（Reference 锚点），无视觉/TSR 层。**保存即从 `body_md` 重新切块**。
- **P2-c Markdown 编辑器**：`NoteEditor.vue` —— CodeMirror 6（markdown/行号/历史/软换行/暗色主题）+ 实时 `MarkdownText` 预览；标题、保存（Ctrl/⌘-S）、剪藏来源链接、索引/已存状态。可编辑来源在中央以编辑器打开而非阅读器；新建入口=首页按钮 + 侧栏 📝。
- **P2-d 网页剪藏**：`clip_web_page` 复用 `web_fetch` 抽正文 → 存为可编辑 `web` 来源（带 front-matter 原文链接）→ 走文本管道索引。首页「剪藏网页」表单。
- **P2-e Markdown/txt 导入**：`import_workspace_paths` 按扩展名分发（.md→markdown、.txt→text），内容拷入 `body_md` 成为可继续编辑的知识库来源；文件选择器 + 拖拽均支持。
- **P2.5 `[[ ]]` 双向链接 + 反链**：`note_links` 表（索引时重建出链）；`extract_wikilinks` 解析 `[[Title]]`/`[[Title|alias]]`；`load_note_links`（出链按当前标题动态解析 + 反链）；编辑器预览内 `[[ ]]` 可点击——已解析→跳转，未解析→新建同名笔记；编辑器底部反链/出链栏。
- **风险：** 中（已落地）。`ContentIngestor` trait 仍未抽——文本路径就是"第二个 ingestor"，但它与 PDF 路径差异足够大（无几何/无视觉），共享的是下游（`upsert` 的结构树/沉淀/FTS 接缝）而非上游接口；待第三个 ingestor（Office, P3）出现再决定是否抽象。

### P3 — Office 格式 + 预览 ✅ 已闭环（已测/已构建/已提交，待你 live 验证）

- **P3-a 摄入(file-backed)**：`build_document_for_path` 泛化——按扩展名设 `content_type`(pdf/docx/xlsx/pptx);Office 文件留磁盘(随 PDF 进父目录根)、注册到 registry 以供预览。文件选择器 + 拖拽 + import 接受三种格式。
- **P3-b 文本抽取 + 索引**：`office.rs` —— docx(`word/document.xml` → `w:p`/`w:t`,Heading 样式)、pptx(`ppt/slides/*` → `a:p`/`a:t`,每页一个标题块)、xlsx(`calamine` → 每表标题 + 逐行块);ZIP+XML 用 `zip`(仅 deflate)+`quick-xml`,预定义实体/字符引用已解析。`run_document_reindex_job` 分发 office → `run_office_document_reindex_job`;原 `upsert_text_document_index` 重构为共享 `upsert_block_document_index`(markdown 传 wikilink 源,office 传 None)。三格式可问、page=0(Reference 锚点)、无视觉层。
- **P3-c docx 预览**：`read_document_bytes` 命令(registry→ArrayBuffer);`OfficeViewer.vue`(懒加载)用 **docx-preview**(Apache-2.0)高保真渲染。
- **P3-d xlsx 预览 + pptx 延后**：xlsx 用 **exceljs** 解析为逐表 HTML 表格;**pptx 仅文本索引**,预览显示"已索引、预览暂不可用"提示(无干净开源渲染器,许可待决——§5/§9)。`App.vue` 按 `content_type` 路由 docx/xlsx/pptx 到 `OfficeViewer`;视觉(TSR)索引收紧为仅 `pdf`。
- **风险：** 中(已落地)。**pptx 保真预览仍是唯一待决项**(许可)——文本照常可问,渲染器待清晰方案。
- **后续(非阻塞)**：docx/pptx 标题 → 大纲/结构树细化;xlsx 大表分页/虚拟滚动;office 文件目录扫描自动纳入(当前仅单文件导入)。

### P4 — 信息架构收尾

- 阅读器 → 按类型自适应详情视图（PDF 查看 / 电子表格 / 幻灯片 / 网页阅读 / Markdown / 笔记编辑）。
- 侧栏 → 统一知识导航器（来源 + 概念/主题，不只是文件夹）。
- 知识图谱前移接近"家"。

---

## 5. Office 预览与摄入 —— 调研结论

**决策：预览用客户端开源渲染 + 索引用 Rust 解析。不捆绑 LibreOffice/OnlyOffice。**（Tauri 是 web 前端，webview 可直接渲染 Office 文档。）

### 预览（前端，Vue 3）

- **`@vue-office`** 组件集（[501351981/vue-office](https://github.com/501351981/vue-office)）—— 一站式 Vue 2/3 预览：
  - `docx` → 底层 [docx-preview](https://github.com/VolodymyrBaydalka/docxjs)（Apache-2.0；保真度好 —— 远胜 mammoth，mammoth 只是 docx→HTML 文本）。
  - `xlsx` → exceljs + x-data-spreadsheet。需要更强时可换 [Univer](https://github.com/dream-num/Luckysheet)（Apache-2.0，现代、功能全）。
  - `pdf` → pdf.js（已有 `PdfViewer.vue`，保留）。
- **PPTX 是 JS 生态最弱的一环。** `@vue-office` 的 pptx 渲染器**部分收费/闭源**。开源替代（[PptxViewJS](https://gptsci.com/pptxviewjs/)、pptx-preview.js、PPTXjs）保真度参差。

### 索引（后端，Rust —— 与预览解耦）

- `xlsx` → `calamine`。
- `docx` → `docx-rs` / `dotext`。
- `pptx` → 解压 + 解析 slide XML。
- 输出：纯文本 → 现有 chunk 管道；引用按 章节/工作表/幻灯片/单元格，而非页+bbox。

### P3 前需解决的待决项

- **与 AGPL-3.0 的许可证兼容**：每个要捆绑的 JS 库都要核实。docx-preview（Apache-2.0）、exceljs（MIT）、Univer（Apache-2.0）没问题；**PPTX 渲染器需逐个核实**（@vue-office 的 pptx 部分可能不可自由分发）。
- **PPTX 保真度**：定一个可接受的预览质量，或先推迟 pptx 预览（仅索引文本、显示"暂不支持预览"）直到选定合适的开源渲染器。

---

## 6. 前端信息架构 / 侧栏重构（Obsidian 式：图标栏 + 可切换面板）

内容不再只有 PDF 后，现在的"单一文件树侧栏"（`WorkspaceSidebar.vue`：工作区根 → 文件夹 → 文档）撑不住。改为 **Obsidian 式结构**：最左一条 **图标栏（activity bar，~48px）** 切换 **面板（~260px，导航）**，**中央舞台**放当前打开的文档，**对话**为常驻助手。另加一个醒目的全局 **「+ 捕获」**（新建笔记 / 导入文件 / 剪藏网页），取代现在只有"加文件夹"的单一入口。

### 6.1 整体布局：单一对话 + 自适应中央舞台（已确认）

布局四件套：`[图标栏 ~48px][面板 ~260px][中央舞台][对话]`。关键是**只有一个对话，永不出现两个**：

- **左栏（图标栏 + 面板）** = 常驻导航（来源 / 搜索 / 概念 / 图谱 / 会话 / 笔记）。
- **中央舞台** = 只放当前打开的**文档**；文档只有两类：**来源（只读预览）** 或 **笔记（可编辑）**，多个打开用文档标签页切换。
- **对话 = 唯一、常驻的助手**，随上下文改变大小/位置（**它不是中央的一个标签**，否则会出现两个对话）：
  - **无文档打开（首页）** → 对话**撑满中央**（全宽"问我的知识库"）。
  - **打开来源/笔记** → 文档占中央，**同一对话缩回右栏**（自动聚焦本文，也可切回全库）。
- 由此**首页就是对话**：提问框 + 建议问题 + 进行中的线程；浏览/最近/概念都在左栏，中央**不放仪表盘、不放收件箱**。

（视觉稿见 §6.6。）

### 6.2 面板模式（图标栏从上到下）

| 模式 | 图标 | 内容 | 接的现成后端 | 上线阶段 |
|---|---|---|---|---|
| 来源 Sources | `ti-files` | **文件夹树**（泛化到所有类型）+ 顶部类型筛选 + 最近 | `documents`（加 `content_type`） | **P1**（复用现有树） |
| 搜索 Search | `ti-search` | 全库全文 + 语义检索 | 库级 FTS / `search_library_knowledge` | P1 |
| 概念 Concepts | `ti-hash` | 浏览沉淀的概念/实体，点一个 → 提到它的所有来源 | `document_artifacts` | P1 |
| 图谱 Graph | `ti-affiliate` | 知识图谱前移 | `KnowledgeGraphView` | P1 |
| 会话 Chats | `ti-message` | 会话历史（从 ChatPane 历史拎出来） | `chat_sessions` | P1 |
| 笔记 Notes | `ti-note` | 独立笔记捕获 + 列表 | 新建（见 P2） | P2 |
| 设置 Settings | `ti-settings` | 底部 | 现有设置 | P1 |

v1（P1）图标栏先上 5 个：来源 / 搜索 / 概念 / 图谱 / 会话；笔记留到 P2。

### 6.3 核心设计原则：两条正交的轴，都是一等公民

**文件夹树 ≠ 概念，二者并存、互补**（Obsidian 本身也是 文件夹 + 标签 + 图谱并存）：

| 轴 | 性质 | 解决什么 | 面板 |
|---|---|---|---|
| **文件夹 / 树** | 人工、显式、稳定（"我把它放哪"） | **归档**：项目分类、刻意结构、符合心智习惯 | 来源 Sources（就是文件夹树） |
| **概念 / 图谱** | AI 涌现、隐式（"它讲的是什么"） | **发现**：跨文件夹串联、意外关联 | 概念 / 图谱 |

取向：**保留树作为骨干**，但 **不强制"先归档才能用"** —— 丢进来即可搜/问/被概念串到，归档可以慢慢补。

### 6.4 文件夹要从"磁盘目录镜像"升级为"KB 逻辑集合"

现状：文件夹 = `add-folder` 扫盘扫出来的**文件系统目录**。问题：第二大脑里的**笔记、网页剪藏没有磁盘路径**，放不进文件系统目录。

升级为 **KB 自己的逻辑集合（collection/folder）**：
- 与磁盘解耦，**任意类型混放**（PDF / 笔记 / 剪藏 / Office）；
- 可嵌套（真正的树）、可拖拽移动；
- **不做独立的"收件箱"概念**（Obsidian 也没有内置收件箱——它只是"默认新建位置"设置 + 用户自建文件夹约定）。改为一个**可配置的「默认归入集合」**（默认 = 根/「全部」）：新捕获落默认集合、立刻出现在「最近」并可搜可问，慢慢再归档。想要 Inbox 工作流的人，自建一个文件夹设为默认即可。
- 导入磁盘文件时可自动镜像出对应文件夹，但 **KB 树是组织的真相来源**，不是磁盘。

（第三轴：用户自定义 **标签 tag**，比文件夹更轻、可多归属。v1 暂不做，树 + 概念两轴够用。）

### 6.5 落点

- 图标栏 + 面板**骨架** + 自适应中央舞台（首页即对话 / 打开文档则文档占中央、对话缩右栏）+ 「来源」面板复用现有树 → **P1**（让"知识库导航 + 对话首页"成立，即使内容暂时只有 PDF）。
- 「+ 捕获」+ 逻辑集合（脱离磁盘、任意类型、可配置默认归入集合）→ **P2**（随独立笔记/剪藏一起做数据模型升级）。
- 各面板丰富化 + 中央舞台按类型自适应预览（来源各类型 / 笔记编辑器）→ **P4**。

### 6.6 视觉稿（mockups）

> 设计稿见 `docs/mockups/`（自包含 SVG）。

- ① 首页 · 单一对话 + 自适应中央舞台 — `mockups/kb_pivot_01_home_adaptive_stage.svg`
- ② 侧栏 · 图标栏 + 面板 + 捕获（无独立收件箱） — `mockups/kb_pivot_02_sidebar_rail_panels.svg`
- ③ Markdown 笔记编辑器（源+预览 / `[[ ]]` / AI 辅助 / 反链） — `mockups/kb_pivot_03_markdown_note_editor.svg`
- ④ 全库问答（混合来源引用 / 答案存为笔记） — `mockups/kb_pivot_04_ask_my_knowledge_base.svg`

---

## 7. 知识编辑 / 写作（第二大脑的核心，不只是"收集+阅读"）

第二大脑首先是个**编辑器**（Obsidian/Notion 皆然）。本产品要支持在应用内**在线编写个人知识**，并让写下的内容立刻成为"可被检索/被概念串联/进图谱"的知识。

### 7.1 四层能力

1. **笔记写作（核心）** —— 用编辑器写独立笔记/想法。
2. **双向链接 `[[ ]]` + 反向链接（backlinks）** —— 笔记↔笔记、笔记↔来源、笔记↔概念互链；反链面板（图谱已有骨架可承接）。
3. **AI 辅助写作（差异化）** —— **「把对话答案存为笔记」**、选中文字让 AI 扩写/总结/改写、写作中 `/` 调 AI。chat 已在产出答案，"答案→笔记"几乎免费。
4. **可编辑 vs 只读（按类型）** —— 笔记 / Markdown / 纯文本 **可编辑**；PDF / Office / 网页剪藏 **只读预览 + 批注**（高亮/旁注，不改原文）。

### 7.2 编辑器范式（已锁定）

**Markdown 源 + 实时预览（Obsidian 式）。**
- 技术：**CodeMirror 6** 编辑 + **复用现有 markdown-it / KaTeX** 渲染（`MarkdownText.vue`）。
- 存储：**纯 Markdown 文本**（可移植、可导出、与现有渲染栈无缝、`[[ ]]` 天然）。
- 不引入 ProseMirror/Tiptap 整套（Notion 式更富但重很多，存 JSON 块、几乎全新栈）。

### 7.3 编辑 → 回流

保存即把笔记正文重新切块 → 进同一套 chunk → artifact → 图谱 → claims 管道。**你写下的笔记立刻可被问、被概念串联、进图谱**。

### 7.4 落点

- **「对话答案存为笔记」可在 P1 先做**（chat 已在产出答案，低成本、立刻体现"写"的价值）。
- **Markdown 编辑器 = P2 核心**（随独立笔记一起）。
- **`[[ ]]` 链接 + 反链 = P2.5**。
- AI 辅助写作（扩写/改写/`/` 命令）贯穿 P2–P4。

---

## 8. 数据模型改动（高层）

- `documents`：加 `content_type TEXT`、`source_url TEXT NULL`；`page_count` 等改可空。（概念上视为"来源"，初期不改表名以免破坏。）
- `document_pages` / `document_blocks` / `document_lines` / `document_chunks`：`page_no`、`bbox_json` 改可空；为非分页内容加通用定位符。
- `structure_tree_nodes`：`page_start` / `page_end` 改可空；无页码时用 `block_start/end_index` 作主排序。
- `notes`：支持独立类型（`document_id` / `page` / `bbox` 可空），或用一张并列表存独立笔记并注册为 `source`。
- **独立笔记 = 可编辑来源**：笔记正文存 **Markdown 文本**（`body_md`），保存即重新切块回流（§7.3）。md/txt 导入的来源同样可编辑。
- **链接（`[[ ]]` / backlinks）**：新增 `links` 表（`source_id` → `target_kind`(note|source|concept) + `target_id`），驱动反链面板。（P2.5）
- **逻辑集合（folders/collections）**：新增表（如 `collections` + `collection_items`），与磁盘解耦、可嵌套、任意 `source` 可多/单归属；默认「收件箱」集合。取代"文件夹=扫描目录"的旧假设（旧的扫描目录可作为导入时的自动归类来源）。（P2）
- `chat_turns`：`document_id` 改可空（**P1**：NOT NULL+FK，需数据承载型表重建 + 重建 2 个索引；随首页无焦点对话的消费者一起做）。
- `Citation`：`page` / `block_id` / `bbox_list` 可选 + 一个锚点 `kind` 鉴别符。

所有改动都力求**增量 / 向后兼容**（新增可空列 + 默认值），使现有 PDF 数据与行为在 P0–P1 期间继续可用。

---

## 9. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 范围蔓延 —— "第二大脑"很大 | 严格分阶段；P1 必须先落地并验证，再做 P2/P3。 |
| 引用改造波及 prompt/trace/UI | P0 先引入锚点抽象，初期只有 PDF 一种 kind；其余 kind 增量加入。 |
| PPTX 预览保真/许可证 | 若无干净开源方案，先推迟 pptx *预览*；文本照常索引；P3 时再定。 |
| 非工具 / M3-M4 模型用不好库级工具 | 全库默认依赖工具调用 + agentic 路径（已接好）；文档化此限制。 |
| 知识去重只是表层（`normalize_key`） | MVP 可接受；图谱变嘈杂时再做语义去重。 |

---

## 10. 待决问题（边做边定）

- 首页具体形态：统一收件箱 + 图谱 + 检索？默认落地布局是什么？
- 笔记模型：独立笔记作为自己的 `source` 行，还是用一张专表并注册为来源。
- 网页剪藏：是否存原始 HTML 快照？可重新抓取？离线副本？
- Office 文档是否需要页+bbox 引用（即对某些子集是否值得转 PDF），还是章节/单元格引用永远够用？
- 现有用户的迁移/运行时体验（当前数据全是 PDF）—— 因改动为增量，预计无感。

---

## 11. 起步第一刀（开工时）

从风险最低的地基开始做 **P0**：`content_type` 鉴别符 + 页/bbox 可空 + 引用锚点抽象 + 检索焦点可选。纯后端、向后兼容、不动 UI。随后做 P1 的知识库首页 + 全库聊天默认。
