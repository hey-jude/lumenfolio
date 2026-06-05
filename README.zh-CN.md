<div align="center">
  <img src="./src/assets/lumenfolio-logo-transparent.png" alt="Lumenfolio logo" width="96">
  <h1>Lumenfolio</h1>
  <p><strong>本地优先的桌面 PDF AI 阅读工作区</strong></p>
  <p>
    <a href="https://github.com/tanghui315/lumenfolio/releases/latest"><strong>下载应用</strong></a>
    ·
    <a href="./docs/assets/lumenfolio-demo.gif"><strong>观看演示</strong></a>
    ·
    <a href="https://github.com/tanghui315/lumenfolio/issues"><strong>反馈问题</strong></a>
  </p>
  <p>支持 macOS Intel、macOS Apple Silicon 和 Windows x86_64。</p>
  <p><a href="./README.md">English README</a></p>
</div>

Lumenfolio 是一个本地优先的桌面 PDF AI 阅读工作区，面向论文精读、证据可追溯问答、版面级翻译和原文锚定笔记。

它不是简单的“PDF + 聊天框”。Lumenfolio 的核心是围绕本地 PDF 证据构建阅读工作流：页面、文本块、chunk、结构树、表格、视觉区域、citation 和 bbox 坐标都可以回到原始 PDF。

![Lumenfolio 演示](./docs/assets/lumenfolio-demo.gif)

## 界面截图

**PDF 原文 / 译文对照阅读**

![Lumenfolio PDF 原文和译文对照阅读](./docs/assets/lumenfolio-translation-split.png)

**Agentic RAG 问答与证据链**

![Lumenfolio Agentic RAG 问答与证据链](./docs/assets/lumenfolio-rag-chat.png)

## 为什么做 Lumenfolio

很多 PDF AI 工具更偏向快速问答。Lumenfolio 更关注深度阅读：追踪论文论点、核对证据、翻译困难段落、记录读书笔记，并且让每一次回答和每一条笔记都能回到原文位置。

核心产品原则：

- **本地优先**：PDF、索引、聊天历史、笔记、Provider 设置和 API Key 都保存在用户本机。
- **证据优先**：回答应能回到当前 PDF 的页码、bbox 和原文 quote。
- **无向量 Agentic RAG**：默认不依赖 embedding 模型和向量数据库。
- **版面级翻译**：把 PDF 翻译当成文档版面任务，而不是普通纯文本翻译。
- **原文锚定笔记**：高亮和评注绑定到 PDF 坐标和原文片段，可跳回来源。

## 愿景

Lumenfolio 的长期目标，是成为面向论文阅读与分析的 Cursor-like AI workspace：一个本地优先的研究工作区，让用户可以在同一个环境里翻译、提问、批注、对比，并最终让 agent 基于可验证证据分析论文。

它的方向不是泛化的 PDF 聊天，而是面向论文的分析工具：

- 基于本地 PDF citation 的证据推理
- 理解章节、图表、表格和参考文献的结构化导航
- 把翻译和笔记纳入同一个阅读循环
- 面向单篇论文或文献集合的多轮研究记忆
- 支持总结贡献、比较方法、抽取实验、核验证据等 agentic workflow

## 无向量 Agentic RAG

Lumenfolio 的 RAG 默认不依赖向量数据库，也不要求 embedding 模型或外部检索服务。

它不会把论文切碎后丢进一个难以解释的向量索引，而是把每个 PDF 解析成可检查的本地证据层：

- PDF 页面、文本块、行和 chunk
- 确定性的文档结构树
- SQLite FTS5 全文检索
- 页面和文本块 bbox 坐标
- 表格与视觉证据
- 带 quote、page、bbox 元数据的 citation

用户提问时，文档 agent 会通过工具分步取证，而不是只做一次黑盒相似度查询：

```text
Question
-> 检查文档结构
-> 打开相关章节
-> 搜索本地 FTS chunk
-> 展开页面 / 邻近页 / 表格 / 视觉证据
-> 经过 answerability / finalize gate
-> 生成带 citation 和 evidence trace 的回答
```

这种设计让检索可以在本地低成本运行，不依赖 embedding 模型质量，也更容易审计。它并不是要替代所有向量检索场景，而是专门优化单篇论文精读：结构、页内上下文和可验证 citation 比模糊语义召回更重要。

对于支持原生工具调用（native tool calling）的模型，整个过程是一个统一的 agent loop：检索与回答共享同一段不断增长的上下文，因此 agent 能始终“记得”自己检索过什么。不支持工具调用的模型会回退到规则驱动的检索路径，弱模型 / 本地模型照样可用。Agent 还具备工作区感知：它能看到你整个已索引的文献库、回答关于库本身的问题（例如“我哪篇论文是关于 X 的”）、并把检索路由到正确的文档；库很大时按需发现文档，而不是把它们全部塞进 prompt。

## Agent 会话

Agent 区是一个独立的多会话工作区，而不是绑在某一篇 PDF 上的聊天框。会话与文档解耦：

- 可同时打开多个独立会话，用标签页切换。
- 会话不绑定单篇 PDF —— 可以设置 / 切换它的焦点文档，并用 `@` 拉入其它论文。
- 对话记忆按会话隔离，每条研究线索保留自己的上下文。
- 笔记以浮层抽屉的形式与任意会话并存。

阅读器的文档切换由左侧栏驱动：你在左侧选哪篇，阅读器就跟到哪篇。

## 跨文档对话（@ 引用）

阅读往往不止于一篇论文。在 Lumenfolio 中，只需在输入框里输入 `@`，就能把其它已索引的论文拉进当前对话。

![Lumenfolio 跨文档 @ 引用对话](./resources/screenshot/s_1.png)

- 输入 `@` 打开论文选择器，按标题搜索并选中要引用的论文。
- 一次提问最多可引用 4 篇其它论文，每个引用都会变成一个可移除的标签。
- Agent 会同时从被引用论文和当前论文取证，因此回答可以跨文档比较方法、对比结果并给出 citation。
- Citation 依然可追溯：每条引用段落都带有来源文档、页码和 bbox，点击即可跳回精确的来源位置。

这样，比较与综合就留在同一个证据可追溯的循环里，而不必在多个独立聊天之间来回复制文本。

## 视觉证据与 TSR

论文里的关键证据并不总在正文段落里，很多实验结果、指标对比和方法细节都藏在图、表格和图表中。Lumenfolio 内置了视觉证据链路：识别 PDF 中的视觉区域，渲染表格 / 图像裁剪，并把这些视觉资产纳入 agent 可追溯的证据范围。

对于表格，运行时包含 Table Structure Recognition（TSR）路径：在配置本地 TSR 模型时，可以把表格区域转换成结构化单元格和可检索的 table facts。这样，用户追问某一行、某一列、实验指标或 benchmark 结果时，agent 不只依赖表格附近的正文描述。

当前发布包已经包含视觉 / 表格证据工作流。扫描 / 图片型 PDF 的本地 OCR 已内置于 macOS Apple Silicon 和 Windows 版本；可选的 ONNX TSR 模型尚未默认随发布包内置。

## 翻译

Lumenfolio 支持阅读中的快速选区翻译，也支持整篇 PDF 的文档级翻译。

对于整篇 PDF 翻译，Lumenfolio 通过内置 PDFMathTranslate sidecar 处理任务。目标是尽可能保留学术 PDF 的版面结构，包括公式、图表、表格、双栏排版、分页和双语输出。

阅读器里的翻译流程包括：

- 阅读时选中文本即时翻译
- 页级 / 文档级翻译任务，支持进度、取消和重试
- 译文 PDF 与双语 PDF 输出
- 原文 / 译文 / 左右对照阅读模式
- 原文块与译文块联动，方便对照阅读

## 笔记

Lumenfolio 的笔记不是脱离原文的普通文本片段，而是绑定到 PDF 证据位置。

每条笔记会保存选中的原文 quote、页码、归一化 PDF bbox、用户评注和本地时间戳。因此它可以在 PDF 上常驻高亮，在笔记列表中展示，并一键跳回原始阅读位置。

笔记工作流面向论文阅读：

- 在 PDF 中高亮一段原文
- 添加可选 Markdown 评注
- 笔记保存在本地 SQLite
- 点击笔记跳回对应页面和高亮
- 笔记、聊天和翻译在同一个阅读工作区中协同

## 功能亮点

- 三栏阅读工作流：
  - 左侧：工作区文件夹与每个文件夹内的 PDF
  - 中间：PDF 阅读器、选区工具、翻译控制
  - 右侧：独立 Agent 会话、证据链、Agent Trace 和浮层笔记抽屉
- 独立的多会话 Agent 工作区（会话与文档解耦）
- 本地 PDF 索引，持久化到 SQLite
- 带 citation 的 agentic Q&A，支持单文档与跨文档
- 工作区感知检索：agent 能看到并回答关于整个已索引库的问题，库大时按需发现文档
- 面向支持工具调用模型的 native tool-calling agent loop，弱 / 本地模型自动回退到规则路径
- 跨文档对话：一次提问可 `@` 引用至多 4 篇其它已索引论文
- 带 citation 的回答，支持 page/bbox 跳转
- Chat 侧 evidence chain 与可折叠 agent trace
- Provider 化聊天和翻译配置；每个模型的上下文窗口自动从 provider 探测（可手动覆盖）
- 表格 / 视觉证据参与检索，支持视觉裁剪和 TSR-ready 表格证据
- macOS Apple Silicon 和 Windows 版本支持扫描 / 图片型 PDF 本地 OCR
- 基于 PDFMathTranslate sidecar 的版面级 PDF 翻译

## 架构

Lumenfolio 是一个 Tauri 2 + Vue 3 桌面应用。

![Lumenfolio 技术架构](./src/assets/lumenfolio-technical-architecture.png)

- 前端：Vue 3 + Vite
- 桌面运行时：Tauri 2
- 后端：Rust
- 存储：SQLite，本地应用数据目录
- PDF 渲染：`pdfjs-dist`
- 翻译 Sidecar：内置 PDFMathTranslate runtime

核心路径：

- `src/App.vue`：应用顶层状态与流程编排
- `src/components/`：workspace、reader、chat、notes、markdown UI
- `src/components/pdf/selection/`：几何驱动的 PDF 文本选择引擎
- `src/translationLinking.js`：原文 / 译文块联动逻辑
- `src-tauri/src/lib.rs`：Tauri 命令与运行时入口
- `src-tauri/src/runtime/rag/`：检索与证据组装
- `src-tauri/src/runtime/agent/`：turn runner、policy gate、session memory、ledger、trace
- `src-tauri/src/llm/agent_loop.rs`：统一的 native tool-calling agent loop
- `src-tauri/src/pdf2zh_sidecar/`：PDF 翻译 sidecar 管理
- `docs/`：产品、架构与 runtime 方案文档

## 当前范围

已实现：

- 工作区文件夹选择与单层 PDF 发现（子目录需单独添加）
- 本地 PDF 读取、索引与 SQLite 持久化
- 阅读态选区、高亮与翻译流程
- 独立的多会话 Agent 工作区
- Agentic 检索问答链路：native tool-calling 路径 + 规则回退
- 面向整个已索引库的工作区感知检索，库大时按需发现文档
- 跨多篇已索引论文的 `@` 引用对话
- 按模型识别上下文窗口并支持手动覆盖
- 带页码 / bbox 的 citation 跳转
- Chat 侧 evidence chain 与 trace 展示
- 本地笔记与 PDF 锚点
- 图、图表、图片和表格区域的视觉证据索引
- macOS Apple Silicon 和 Windows 发布包支持扫描 / 图片型 PDF OCR
- PDFMathTranslate sidecar 文档翻译集成

## 环境要求

- Node.js 18+（建议 LTS）
- npm 9+
- Rust stable toolchain
- Tauri 2 对应平台依赖（macOS / Linux / Windows 构建依赖）

## 快速开始

```bash
npm install
npm run tauri:dev
```

仅前端 UI 联调：

```bash
npm run dev
```

## 构建与校验

前端构建：

```bash
npm run build
```

Rust 检查：

```bash
cd src-tauri
cargo check
cargo test
```

常用 smoke 流程：

```bash
npm run build
cd src-tauri && cargo test
```

项目专项检查：

```bash
npm run check:translation-linking
npm run check:prod-no-testids
```

## 数据与隐私

- Lumenfolio 是 local-first，PDF 索引、笔记、聊天历史和翻译元数据默认保存在本地。
- API Key 当前仍是本地存储，后续计划迁移到系统 keychain。
- 如果配置云端模型或翻译 Provider，选中文本、问题、页面上下文或翻译内容可能会发送到对应服务商。

## 致谢

- 感谢 [`PDFMathTranslate`](https://github.com/PDFMathTranslate/PDFMathTranslate) 在翻译能力上的支持与相关工程启发。

## License

本项目采用 PolyForm Noncommercial License 1.0.0。

该许可证禁止商业化使用。如需商用授权，请联系版权所有者：`tanghui315@126.com`。
