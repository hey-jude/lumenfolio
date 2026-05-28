# Lumenfolio 

[中文文档 (Chinese README)](./README.zh-CN.md)

Local-first desktop PDF AI reading workspace built with Tauri 2 + Vue 3.

Lumenfolio is designed for focused paper reading and evidence-grounded Q&A on local PDFs. 

## Highlights

- Three-pane reading workflow:
  - Left: workspace roots and PDF list
  - Center: PDF reader, selection, translation controls
  - Right: document chat, evidence chain, folded agent trace
- Local indexing pipeline for PDF pages/blocks/chunks with SQLite
- Current-document RAG (structure tree + FTS + page/block evidence)
- Citation-aware answers with page/bbox jump support
- Provider-based chat and translation (OpenAI-compatible + translation options)

## Current Scope

Implemented today:

- Workspace folder selection and recursive PDF discovery
- Local PDF read/index with SQLite persistence
- Reader-side selection and translation flow
- Agentic retrieval loop for **single-document** Q&A
- Evidence chain + trace metadata in chat

## Tech Stack

- Frontend: Vue 3 + Vite
- Desktop runtime: Tauri 2 (Rust backend)
- Storage: SQLite (local app data)
- PDF rendering: `pdfjs-dist` (renderer)

## Prerequisites

- Node.js 18+ (LTS recommended)
- npm 9+
- Rust stable toolchain
- Platform requirements for Tauri 2 (macOS/Linux/Windows build deps)

## Quick Start

```bash
npm install
npm run tauri:dev
```

For browser-only UI iteration:

```bash
npm run dev
```

## Build & Verification

Frontend build:

```bash
npm run build
```

Rust checks:

```bash
cd src-tauri
cargo check
cargo test
```

Suggested smoke sequence for most changes:

```bash
npm run build
cd src-tauri && cargo test
```

## Project Structure (Key Paths)

- `src/App.vue`: top-level app orchestration
- `src/components/`: workspace, reader, chat, markdown UI
- `src-tauri/src/lib.rs`: Tauri commands and runtime entry points
- `src-tauri/src/runtime/rag/`: retrieval and evidence assembly
- `src-tauri/src/runtime/agent/`: intent, finalize policy, session memory, trace
- `docs/`: product/architecture/runtime plans

## Runtime Flow (High Level)

```text
Question
-> Retrieval (tree/section/FTS/page/table/visual tools)
-> Finalize gate (answerable / needs more / insufficient)
-> Answer + citations + evidence chain + trace
```

## Data & Privacy Notes

- Data is local-first; indexing artifacts are stored in local SQLite.
- API keys are currently stored locally (keychain migration is planned later).
- If cloud model/translation providers are configured, selected text/questions may be sent to those providers.

## Important Docs

- `AGENTS.md`
- `docs/lumenfolio_desktop_plan.md`
- `docs/lumenfolio_desktop_ia.md`
- `docs/lumenfolio_local_rag_plan.md`
- `docs/lumenfolio_agentic_rag_runtime_plan.md`
- `docs/lumenfolio_pdf_render_translation_plan.md`

## License

Internal project status (add explicit license text here when finalized).

