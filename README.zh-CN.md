# Lumenfolio 

[English README](./README.md)

一个基于 Tauri 2 + Vue 3 的本地优先（local-first）桌面 PDF AI 阅读工作区。

Lumenfolio 面向本地论文阅读与证据可追溯问答场景。

## 功能亮点

- 三栏阅读工作流：
  - 左侧：工作区目录与 PDF 列表
  - 中间：PDF 阅读器、选择与翻译控制
  - 右侧：当前文档聊天、证据链、可折叠 Agent Trace
- 本地 PDF 索引（pages/blocks/chunks）+ SQLite 存储
- 当前文档 RAG（structure tree + FTS + page/block evidence）
- 带 citation 的回答，支持 page/bbox 跳转
- 基于 Provider 的聊天与翻译能力（OpenAI-compatible + 翻译路径）

## 当前范围

已实现：

- 工作区目录选择与递归 PDF 发现
- 本地 PDF 读取、索引与 SQLite 持久化
- 阅读态选区翻译流程
- 面向**单文档**的 Agentic 检索问答链路
- Chat 侧 evidence chain 与 trace 展示

## 技术栈

- 前端：Vue 3 + Vite
- 桌面运行时：Tauri 2（Rust 后端）
- 存储：SQLite（本地应用数据）
- PDF 渲染：`pdfjs-dist`（Renderer 侧）

## 环境要求

- Node.js 18+（建议 LTS）
- npm 9+
- Rust stable toolchain
- Tauri 2 对应平台依赖（macOS/Linux/Windows）

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

## 目录结构（核心路径）

- `src/App.vue`：应用顶层状态与流程编排
- `src/components/`：workspace / reader / chat / markdown 组件
- `src-tauri/src/lib.rs`：Tauri 命令与运行时入口
- `src-tauri/src/runtime/rag/`：检索与证据组装
- `src-tauri/src/runtime/agent/`：intent、finalize policy、session memory、trace
- `docs/`：产品、架构与 runtime 方案文档

## 运行时链路（高层）

```text
Question
-> Retrieval (tree/section/FTS/page/table/visual tools)
-> Finalize gate (answerable / needs more / insufficient)
-> Answer + citations + evidence chain + trace
```

## 数据与隐私说明

- 项目是 local-first，索引产物默认存本地 SQLite。
- API Key 当前仍是本地存储（后续再迁移到系统 keychain）。
- 若配置云端模型/翻译 Provider，选中文本与问题可能会发送到对应服务商。

## License

本项目采用 PolyForm Noncommercial License 1.0.0。

该许可证禁止商业化使用。如需商用授权，请联系版权所有者：`tanghui315@126.com`。

