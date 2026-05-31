<p align="center">
  <img src="./src/assets/lumenfolio-logo-transparent.png" alt="Lumenfolio logo" width="96">
</p>

# Lumenfolio

[中文文档 (Chinese README)](./README.zh-CN.md)

Lumenfolio is a local-first desktop AI reading workspace for academic PDFs. It combines a focused PDF reader, vectorless agentic RAG, layout-aware translation, and evidence-anchored notes into one reading environment.

It is not just "chat over a PDF". Lumenfolio is built around local document evidence: pages, blocks, chunks, structure, tables, visual regions, citations, and bounding boxes that can point back to the original PDF.

![Lumenfolio demo](./docs/assets/lumenfolio-demo.gif)

## Screenshots

**Side-by-side PDF translation**

![Lumenfolio side-by-side PDF translation](./docs/assets/lumenfolio-translation-split.png)

**Agentic RAG with evidence trace**

![Lumenfolio agentic RAG chat with evidence trace](./docs/assets/lumenfolio-rag-chat.png)

## Why Lumenfolio

Most PDF AI tools optimize for quick answers. Lumenfolio is designed for deep reading: following claims, checking citations, translating difficult sections, and keeping notes tied to the exact place where an idea appears.

The core product principles are:

- **Local-first by default**: PDFs, indexes, chat history, notes, provider settings, and API keys stay on the user's machine.
- **Evidence-grounded answers**: answers are expected to cite page-level and bbox-level evidence from the current PDF.
- **Vectorless agentic RAG**: retrieval does not require embeddings or a vector database.
- **Layout-aware translation**: PDF translation is handled as a document-layout problem, not just plain text translation.
- **Anchored notes**: highlights and comments are attached to PDF coordinates and quotes, so they can jump back to the source.

## Vision

Lumenfolio aims to become a Cursor-like AI workspace for academic papers: a local-first environment where readers can translate, question, annotate, compare, and eventually analyze papers with an agent that understands document structure and stays grounded in verifiable evidence.

The long-term direction is not generic PDF chat. It is a paper analysis workspace with:

- citation-grounded reasoning over local PDFs
- structure-aware navigation across sections, figures, tables, and references
- translation and note-taking as part of the same reading loop
- multi-turn research memory for a paper or reading collection
- agentic workflows for summarizing claims, comparing methods, extracting experiments, and checking evidence

## Vectorless Agentic RAG

Lumenfolio's RAG pipeline is vectorless by design. It does not require an embedding model, vector database, or external retrieval service.

Instead, each PDF is indexed into a local, inspectable evidence layer:

- PDF pages, text blocks, lines, and chunks
- deterministic document structure tree
- SQLite FTS5 text search
- page and block bounding boxes
- table and visual evidence
- citation records with quote, page, and bbox metadata

At question time, the document agent uses retrieval tools rather than a single opaque similarity lookup:

```text
Question
-> inspect document structure
-> open relevant sections
-> search local FTS chunks
-> open pages, neighbors, tables, and visual evidence
-> run an answerability / finalize gate
-> answer with citations and evidence trace
```

This makes retrieval cheap to run locally, independent from embedding model quality, and easier to audit. The goal is not to replace every vector-search use case; it is to optimize for single-document scholarly reading where structure, page context, and verifiable citations matter.

## Translation

Lumenfolio supports both quick selection translation and document-level PDF translation.

For full-document translation, Lumenfolio orchestrates a bundled PDFMathTranslate sidecar. The goal is to preserve academic PDF layout as much as possible, including formulas, figures, tables, double-column structure, pagination, and bilingual output.

Reader workflows include:

- selected-text translation while reading
- page/document translation jobs with progress and cancellation
- translated PDF and bilingual PDF outputs
- original / translated / side-by-side reading modes
- linked original and translated blocks for easier comparison

## Notes

Notes are evidence-anchored, not detached text snippets.

Each note stores the selected quote, page number, normalized PDF bounding boxes, optional user commentary, and local timestamps. This lets Lumenfolio render persistent highlights, list notes by document, and jump back to the exact source location.

The notes workflow is designed for paper reading:

- highlight a passage from the PDF
- add an optional Markdown note
- keep the note in local SQLite
- click a note to return to the original page and highlight
- keep notes alongside chat and translation, not in a separate app

## Features

- Three-pane reading workspace:
  - left: workspace roots and recursive PDF list
  - center: PDF reader, selection tools, translation controls
  - right: document chat, evidence chain, agent trace, and notes
- Local PDF indexing into SQLite
- Single-document agentic Q&A with citations
- Evidence chain and foldable agent trace in chat
- Provider-based chat and translation configuration
- Visual/table-aware retrieval path for richer PDF evidence
- PDFMathTranslate-based sidecar for layout-aware translation

## Architecture

Lumenfolio is a Tauri 2 + Vue 3 desktop app.

![Lumenfolio technical architecture](./src/assets/lumenfolio-technical-architecture.png)

- Frontend: Vue 3 + Vite
- Desktop runtime: Tauri 2
- Backend: Rust
- Storage: SQLite in the local app data directory
- PDF rendering: `pdfjs-dist`
- Translation sidecar: bundled PDFMathTranslate runtime

Key paths:

- `src/App.vue`: top-level app orchestration
- `src/components/`: workspace, reader, chat, notes, and markdown UI
- `src/components/pdf/selection/`: geometry-driven PDF text selection
- `src/translationLinking.js`: original/translated block linking
- `src-tauri/src/lib.rs`: Tauri command surface and runtime setup
- `src-tauri/src/runtime/rag/`: retrieval and evidence assembly
- `src-tauri/src/runtime/agent/`: turn runner, policy gate, session memory, ledger, trace
- `src-tauri/src/pdf2zh_sidecar/`: PDF translation sidecar manager
- `docs/`: product, architecture, and runtime plans

## Current Scope

Implemented today:

- Workspace folder selection and recursive PDF discovery
- Local PDF reading and indexing with SQLite persistence
- Reader-side selection, highlighting, and translation flow
- Single-document agentic retrieval loop
- Citation-aware answers with page/bbox jump support
- Evidence chain and agent trace metadata in chat
- Local notes with PDF anchors
- PDFMathTranslate sidecar integration for document translation

## Prerequisites

- Node.js 18+ (LTS recommended)
- npm 9+
- Rust stable toolchain
- Platform requirements for Tauri 2 (macOS/Linux/Windows build dependencies)

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

Additional project checks:

```bash
npm run check:translation-linking
npm run check:prod-no-testids
```

## Data & Privacy

- Lumenfolio is local-first. PDF indexes, notes, chat history, and translation metadata are stored locally.
- API keys are currently stored locally; migration to the system keychain is planned.
- If a cloud chat or translation provider is configured, selected text, questions, page context, or translation content may be sent to that provider.

## Acknowledgements

- Thanks to [`PDFMathTranslate`](https://github.com/PDFMathTranslate/PDFMathTranslate) for its translation capabilities and related engineering inspiration.

## License

This project is licensed under the PolyForm Noncommercial License 1.0.0.

Commercial use is not permitted under this license. If you need commercial licensing, contact the copyright holder at `tanghui315@126.com`.
