# Reimagined Emailibrium: RuVector-Powered Intelligent Email Platform

> **Version**: 1.4 | **Date**: 2026-03-23 | **Status**: Technical Implementation Plan (Greenfield — no existing users)
> **Branch**: `feature/phase-2-cloud-llm-support` → next: `feature/ruvector-integration`
> **v1.1**: Corrected frontend migration — replacing Tauri 2.0 (Rust desktop shell) with pure React TypeScript web-first UI
> **v1.2**: All dependency versions verified against crates.io + npm registry (2026-03-23). Added Makefile targets. Confirmed pnpm-based workflow.
> **v1.3**: Rust 1.94.0 (latest stable). Docker Compose full-stack setup. Externalized config per OWASP/NIST/12-factor. Scrubbed migration/backward-compat language — greenfield architecture doc.
> **v1.4**: Full email client (view/reply/compose). Ingest→Tag→Archive zero-inbox strategy. Dynamic auto-grouping + continuous SONA learning. Image/attachment/hyperlink content features (FEAT-064–075).

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Vision & Goals](#2-vision--goals)
3. [Architecture Overview](#3-architecture-overview)
4. [RuVector Integration Deep Dive](#4-ruvector-integration-deep-dive)
5. [Tech Stack](#5-tech-stack-current--reimagined)
6. [Data Model & Schema](#6-data-model--schema-evolution)
7. [Backend Implementation Plan](#7-backend-implementation-plan)
8. [Frontend: React TypeScript Web](#8-frontend-tauri-desktop--react-typescript-web)
9. [User Journeys](#9-user-journeys)
10. [Wireframes](#10-wireframes)
11. [Feature Mapping (Current + Planned)](#11-feature-mapping-current--planned)
12. [Performance Targets](#12-performance-targets)
13. [Implementation Phases](#13-implementation-phases)
14. [Makefile Targets & Developer Workflow](#14-makefile-targets--developer-workflow)
15. [Risk Assessment](#15-risk-assessment)

- [Appendix A: Dependency Version Audit](#appendix-a-dependency-version-audit-2026-03-23)
- [Appendix B: Docker Compose Full-Stack Setup](#appendix-b-docker-compose-full-stack-setup)
- [Appendix C: Externalized Configuration](#appendix-c-externalized-configuration)
- [Appendix D: API Response Examples](#appendix-d-api-response-examples)

---

## 1. Executive Summary

Emailibrium is being reimagined as a **vector-native email intelligence platform**. By integrating [RuVector](https://github.com/ruvnet/ruvector) — a Rust-native vector database with HNSW indexing, SIMD-optimized distance calculations, quantization, self-learning (SONA), graph neural networks, and local LLM inference — we transform from keyword search + rule-based automation into a **semantic-first, sub-second intelligent email engine**.

**Key outcomes:**

- **10-minute inbox zero** for 10,000+ emails via semantic clustering + batch actions
- **Instant semantic search** across any attribute, any text, any pattern
- **Subscription & recurring mail intelligence** surfaced automatically
- **Pure React TypeScript web UI** replacing the Tauri 2.0 desktop app
- **Privacy-first**: all vector operations run locally, cloud optional

---

## 2. Vision & Goals

### 2.1 Core Promise

> "Connect your inbox. In under 10 minutes, Emailibrium understands every email you've ever received — clusters them by topic, surfaces subscriptions you forgot about, identifies what matters, and lets you clean house with one-click batch actions. Then it keeps learning."

### 2.2 Measurable Goals

| Goal                           | Target           | Mechanism                                  |
| ------------------------------ | ---------------- | ------------------------------------------ |
| Inbox zero (10K emails)        | < 10 minutes     | Semantic clustering → batch actions        |
| Email ingestion rate           | > 500 emails/sec | RuVector batch insert + parallel embedding |
| Search latency (semantic)      | < 50ms p95       | HNSW index, SIMD distance, quantization    |
| Search latency (hybrid)        | < 100ms p95      | FTS5 + HNSW fusion with reciprocal rank    |
| Categorization accuracy        | > 95%            | Vector similarity + SONA adaptive learning |
| Subscription detection         | > 98% recall     | Pattern clustering + header analysis       |
| Memory footprint (100K emails) | < 500MB          | Scalar quantization (4x compression)       |
| Cold start to usable           | < 30 seconds     | Streaming embed pipeline                   |

### 2.3 Design Principles

1. **Vector-first, keyword-fallback** — Semantic understanding is the default; FTS5 remains for exact-match and structured filters
2. **Stream, don't batch** — Users see results as they arrive, not after everything completes
3. **Learn from every interaction** — SONA self-learning improves search & categorization continuously
4. **Privacy by architecture** — All embeddings generated and stored locally; cloud LLM is opt-in
5. **10-minute promise** — Every UX decision optimizes for rapid inbox comprehension and action

---

## 3. Architecture Overview

### 3.1 System Architecture (Reimagined)

```text
┌────────────────────────────────────────────────────────────────────────┐
│              REACT TYPESCRIPT WEB APP (Pure SPA / PWA)                 │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
│  │ Command  │  │ Inbox    │  │ Insights │  │ Rules    │  │ Settings │  │
│  │ Center   │  │ Cleaner  │  │ Explorer │  │ Studio   │  │          │  │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  │
│       └─────────────┴─────────────┴─────────────┴─────────────┘        │
│  TanStack Router + Query + Zustand • shadcn/ui • SSE • cmdk            │
│  Vite 7 SPA • No Tauri • Browser-native (PWA optional)                 │
└──────────────────────────┼─────────────────────────────────────────────┘
                           │ REST + SSE (Server-Sent Events)
┌──────────────────────────┼──────────────────────────────────────────┐
│                     AXUM API GATEWAY                                │
│  ┌──────────┐  ┌──────────┐ ┌──────────┐ ┌──────────┐               │
│  │ Auth     │  │ Search   │ │ Actions  │ │ Insights │               │
│  │ (OAuth2) │  │ (Hybrid) │ │ (Bulk)   │ │ (Stream) │               │
│  └────┬─────┘  └────┬─────┘ └────┬─────┘ └────┬─────┘               │
└───────┼─────────────┼────────────┼────────────┼─────────────────────┘
        │             │            │            │
┌───────┼─────────────┼────────────┼────────────┼────────────────────┐
│       │      INTELLIGENCE LAYER (NEW)         │                    │
│  ┌────┴─────────────┴───────────┴─────────────┴──────────────────┐ │
│  │                    RuVector Engine                            │ │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐              │ │
│  │  │ HNSW    │ │ SONA    │ │ GNN     │ │ Quant   │              │ │
│  │  │ Index   │ │ Learning│ │ Cluster │ │ Engine  │              │ │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘              │ │
│  └───────────────────────────────────────────────────────────────┘ │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐                      │
│  │ Embedding  │ │ Category   │ │ Pattern    │                      │
│  │ Pipeline   │ │ Classifier │ │ Detector   │                      │
│  └────────────┘ └────────────┘ └────────────┘                      │
└────────────────────────────────┬───────────────────────────────────┘
                                 │
┌────────────────────────────────┼───────────────────────────────────┐
│                    DATA LAYER                                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │
│  │ SQLite/PG    │  │ RuVector     │  │ Moka + Redis │              │
│  │ (Structured) │  │ (Vectors)    │  │ (Cache)      │              │
│  │ FTS5 search  │  │ HNSW index   │  │ Embeddings   │              │
│  │ Email meta   │  │ Embeddings   │  │ LLM responses│              │
│  │ Rules/Config │  │ Clusters     │  │ Search cache │              │
│  └──────────────┘  └──────────────┘  └──────────────┘              │
└────────────────────────────────────────────────────────────────────┘
                                 │
┌────────────────────────────────┼───────────────────────────────────┐
│                EMAIL PROVIDERS                                     │
│  ┌────────┐ ┌─────────┐ ┌──────┐ ┌──────┐                          │
│  │ Gmail  │ │ Outlook │ │ IMAP │ │ POP3 │                          │
│  │ (OAuth)│ │ (OAuth) │ │      │ │      │                          │
│  └────────┘ └─────────┘ └──────┘ └──────┘                          │
└────────────────────────────────────────────────────────────────────┘
```

### 3.2 Data Flow: Email Ingestion Pipeline

```text
Email arrives (via provider sync)
  │
  ├─── 1. Parse & Extract metadata ──────── SQLite (structured data)
  │         subject, from, to, date,
  │         headers, labels, size
  │
  ├─── 2. Generate text embedding ─────────── RuVector "email_text"
  │         subject + from + body_preview          HNSW index update
  │         Model: all-MiniLM-L6-v2 (384D)         SONA learning feed
  │         Latency: ~5ms local
  │
  ├─── 3. Extract & vectorize assets ──────── RuVector (multi-collection)
  │         HTML body → clean text + URLs          "email_text" collection
  │         Inline images → OCR + CLIP             "image_text" + "image_visual"
  │         Attachments → extract text             "attachment_text"
  │         URLs → resolve + classify              Metadata filters
  │
  ├─── 4. Classify category ─────────────── Moka cache
  │         Vector similarity to category        Cache key: embedding hash
  │         centroids (< 1ms if cached)
  │         Fallback: LLM categorization
  │
  ├─── 5. Detect patterns ──────────────── Pattern store
  │         Subscription headers (List-Unsubscribe)
  │         Recurring sender frequency
  │         Tracking pixel detection
  │         Thread grouping via vector proximity
  │
  └─── 6. Apply rules ──────────────────── Action queue
            Match against rule conditions
            Execute: label, archive, delete, move
```

**Throughput targets**:

- Text-only emails: 500+ emails/sec (batch embedding amortizes to ~2ms/email)
- Full multi-asset extraction: ~50 emails/sec (images, attachments are the bottleneck)
- Configurable: fast mode (text-only) for initial sync, deep mode (all assets) for background processing

---

## 4. RuVector Integration Deep Dive

### 4.1 Why RuVector

RuVector is a **Rust-native vector database** — same language as our backend, zero FFI overhead, zero serialization cost. Key capabilities we leverage:

| RuVector Feature                         | Emailibrium Use                                                           |
| ---------------------------------------- | ------------------------------------------------------------------------- |
| **HNSW Index** (O(log n) search)         | Sub-millisecond similarity search across 100K+ emails                     |
| **SIMD Distance** (16M ops/sec)          | Cosine similarity for email matching at hardware speed                    |
| **Scalar Quantization** (4x compression) | 100K emails in ~100MB instead of 400MB                                    |
| **SONA Self-Learning**                   | Search quality improves with every query — recall@10 gains +12% over time |
| **GNN Clustering**                       | Automatic email topic discovery (GraphSAGE on HNSW neighbor graph)        |
| **RuvLLM** (local inference)             | Replace Ollama dependency with integrated Rust-native LLM                 |
| **REDB Persistence**                     | Crash-safe vector storage, no external database needed                    |
| **Batch Operations**                     | Insert hundreds of vectors in single call                                 |

### 4.2 RuVector Module Structure

New backend module: `backend/src/vectors/`

```text
backend/src/vectors/
├── mod.rs              # VectorService facade (init, shutdown, health)
├── embedding.rs        # EmbeddingPipeline — text → f32 vector
│                       #   - Local: sentence-transformers via RuvLLM
│                       #   - Fallback: Ollama /api/embed
│                       #   - Fallback: Cloud embedding API
├── store.rs            # VectorStore — RuVector wrapper
│                       #   - insert(), batch_insert()
│                       #   - search(), search_filtered()
│                       #   - delete(), update()
│                       #   - cluster(), get_neighbors()
├── search.rs           # HybridSearch — FTS5 + vector fusion
│                       #   - reciprocal_rank_fusion()
│                       #   - filter_post_search()
│                       #   - rerank_results()
├── clustering.rs       # ClusterEngine — GNN-based topic discovery
│                       #   - detect_topics()
│                       #   - subscription_patterns()
│                       #   - recurring_sender_analysis()
├── insights.rs         # InsightEngine — subscription & pattern detection
│                       #   - detect_subscriptions()
│                       #   - analyze_sender_frequency()
│                       #   - identify_recurring_patterns()
│                       #   - generate_inbox_report()
├── learning.rs         # SONABridge — self-learning integration
│                       #   - feed_interaction()
│                       #   - get_personalized_results()
│                       #   - export_learned_weights()
└── config.rs           # VectorConfig — settings & tuning
                        #   - embedding_model, dimensions
                        #   - hnsw_m, hnsw_ef_construction
                        #   - quantization_mode
                        #   - sona_enabled
```

### 4.3 Embedding Strategy

**Model**: `all-MiniLM-L6-v2` (384 dimensions, 22M params)

- Runs via RuvLLM (Metal/CUDA/CPU SIMD) — no Python, no external process
- Fallback chain: RuvLLM → Ollama → Cloud API

**What gets embedded** — multiple vectors per email across separate collections:

| Collection        | Content                                           | Model            | Dimensions |
| ----------------- | ------------------------------------------------- | ---------------- | ---------- |
| `email_text`      | subject + from + clean body text (max 512 tokens) | all-MiniLM-L6-v2 | 384        |
| `image_text`      | OCR'd text from inline images + image attachments | all-MiniLM-L6-v2 | 384        |
| `image_visual`    | Raw image pixels → visual embedding               | CLIP ViT-B-32    | 512        |
| `attachment_text` | Extracted text from PDF/DOCX/XLSX attachments     | all-MiniLM-L6-v2 | 384        |

**Primary text embedding** (always generated):

```text
{subject}\n
From: {from_name} <{from_addr}>\n
To: {to_addrs}\n
Date: {received_at}\n
Labels: {labels}\n
{clean_body_text[..400]}
```

**URL metadata** (stored as filterable fields on the email vector, not separate collection):

- Extracted URLs with resolved destinations
- Link categories: `tracking`, `unsubscribe`, `shopping`, `news`, `social`, `internal`
- Tracking pixel presence: boolean flag

**Embedding lifecycle**:

1. **On sync (fast mode)**: Text embedding generated immediately (~5ms) — emails are searchable within seconds
2. **On sync (deep mode, background)**: Images, attachments, URLs processed via Apalis job queue
3. **On search**: Query text embedded on-the-fly (~5ms), searched across all collections
4. **On feedback**: SONA updates internal weights (sub-millisecond)

### 4.4 Hybrid Search Algorithm

```text
HybridSearch(query_text, filters) → Vec<ScoredEmail>

  1. embed(query_text) → query_vec [384D]                        ~5ms
  2. vector_search(query_vec, k=100, ef=200) → vec_results       ~2ms
  3. fts5_search(query_text, filters) → text_results              ~10ms
  4. reciprocal_rank_fusion(vec_results, text_results) → fused    ~1ms
  5. apply_filters(fused, date_range, labels, sender) → filtered  ~1ms
  6. sona_rerank(filtered, user_history) → final                  ~1ms
                                                          Total: ~20ms
```

**Reciprocal Rank Fusion (RRF)**:

```text
score(doc) = Σ 1 / (k + rank_i(doc))
```

Where `k=60`, and `rank_i` is the document's rank in result set `i`. This balances semantic relevance (vector) with exact-match precision (FTS5).

### 4.5 Subscription & Recurring Mail Detection

```rust
// Insight pipeline — runs on initial sync and incrementally

pub struct SubscriptionInsight {
    pub sender: String,
    pub sender_domain: String,
    pub frequency: RecurrencePattern,     // Daily, Weekly, Monthly, Irregular
    pub email_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub has_unsubscribe_header: bool,
    pub unsubscribe_link: Option<String>,
    pub category: SubscriptionCategory,   // Newsletter, Marketing, Notification, Receipt, Social
    pub cluster_id: u64,                  // RuVector cluster assignment
    pub avg_open_rate: Option<f32>,       // If read status available
    pub suggested_action: SuggestedAction, // Keep, Unsubscribe, Archive, Digest
}

pub enum RecurrencePattern {
    Daily { avg_interval_hours: f32 },
    Weekly { day_of_week: Weekday },
    BiWeekly,
    Monthly { day_of_month: u8 },
    Quarterly,
    Irregular { avg_interval_days: f32 },
}
```

**Detection pipeline**:

1. **Header scan**: `List-Unsubscribe`, `Precedence: bulk`, `X-Mailer` headers
2. **Sender frequency**: Group by `from_domain`, compute inter-arrival times
3. **Content similarity**: RuVector cluster emails from same sender → detect templates
4. **Pattern matching**: Fit recurrence model (daily/weekly/monthly) via interval analysis
5. **Actionability scoring**: Combine read-rate, age, frequency → suggest keep/unsub/archive

---

## 5. Tech Stack (Current → Reimagined)

### 5.1 Backend

| Component            | Current                      | Reimagined                      | Rationale                                             |
| -------------------- | ---------------------------- | ------------------------------- | ----------------------------------------------------- |
| **Vector Store**     | — (none)                     | **RuVector** (ruvector-core)    | Native Rust, HNSW, SIMD, zero FFI                     |
| **Embeddings**       | — (none)                     | **RuvLLM** (local inference)    | Replace Ollama for embedding; keep Ollama as fallback |
| **Text Search**      | SQLite FTS5                  | SQLite FTS5 + RuVector hybrid   | FTS5 stays for exact match; vectors add semantics     |
| **Categorization**   | Ollama / Cloud LLM           | Vector centroids + LLM fallback | 10x faster: cosine distance vs LLM call               |
| **Clustering**       | — (none)                     | RuVector GNN (GraphSAGE)        | Topic discovery, subscription detection               |
| **Self-Learning**    | Adaptive categorizer (basic) | **SONA** (3-tier adaptive)      | Instant + session + long-term learning                |
| **Web Framework**    | Axum 0.8                     | Axum 0.8 (unchanged)            | Already excellent                                     |
| **Database**         | SQLite + SQLx                | SQLite + SQLx (unchanged)       | Structured data stays in SQL                          |
| **Auth**             | OAuth2 + JWT                 | OAuth2 + JWT (unchanged)        | Mature, working                                       |
| **Caching**          | Moka + Redis                 | Moka + Redis (unchanged)        | Add vector embedding cache to Moka                    |
| **Background Jobs**  | Apalis                       | Apalis (unchanged)              | Add embedding batch jobs                              |
| **Rules Engine**     | evalexpr-based               | evalexpr + vector-enhanced      | "Emails similar to X" as rule condition               |
| **LLM (generative)** | Ollama + Cloud               | Ollama + Cloud (unchanged)      | Generative stays LLM; classification moves to vectors |

### 5.2 Frontend: Tauri Desktop → React TypeScript Web

**The current Emailibrium frontend is a Tauri 2.0 desktop application** — React/TypeScript runs inside a native webview shell with a Rust-side bridge (`src-tauri/`). While this provides small binaries and direct Rust integration, it constrains distribution (requires desktop install), limits cross-platform reach, and couples the UI to Tauri's webview sandbox.

**The reimagined frontend drops Tauri entirely** in favor of a **pure React TypeScript web application** served as an SPA, with optional Electron or PWA wrapping for desktop if needed later.

#### Why Drop Tauri

| Concern               | Tauri (Current)               | Web-First (Reimagined)                |
| --------------------- | ----------------------------- | ------------------------------------- |
| **Distribution**      | Desktop install required      | URL — instant access, zero install    |
| **Cross-platform**    | macOS/Windows/Linux only      | Any browser + mobile + tablet         |
| **Updates**           | App store / auto-updater      | Deploy once, everyone gets it         |
| **Development speed** | Tauri CLI + Rust build + Vite | Vite only — 10x faster HMR cycle      |
| **Team skills**       | Requires Rust + TypeScript    | Pure TypeScript team                  |
| **Testing**           | Tauri-specific E2E needed     | Standard Playwright/Cypress           |
| **Mobile support**    | Tauri Mobile (beta)           | Responsive web — works today          |
| **Offline**           | Native (Tauri store)          | Service Worker + IndexedDB            |
| **Native features**   | System tray, global shortcuts | PWA install prompt, notifications API |

#### Technology Choices

| Layer                  | Technology                     | Rationale                                                 |
| ---------------------- | ------------------------------ | --------------------------------------------------------- |
| **Shell**              | **None — pure SPA**            | No native wrapper; browser-native, zero-install           |
| **Framework**          | **React 19** (standalone SPA)  | Mature, massive ecosystem, concurrent rendering           |
| **Build**              | **Vite 8**                     | Sub-second HMR, optimized production builds               |
| **Router**             | **TanStack Router**            | Type-safe routing, search params as state                 |
| **Server State**       | **TanStack Query 5**           | Caching, background refetch, optimistic updates           |
| **Client State**       | **Zustand 5**                  | Minimal, focused, no boilerplate                          |
| **Local storage**      | **IndexedDB** via `idb-keyval` | Browser-native, no framework dependency                   |
| **Secure storage**     | **Web Crypto API** + IndexedDB | AES-GCM encryption, no external dependency                |
| **Backend comms**      | **REST API** + **SSE**         | Standard HTTP, streaming via EventSource                  |
| **Styling**            | **Tailwind CSS 4**             | Utility-first, small bundles, design system ready         |
| **Components**         | **shadcn/ui** + Radix + Lucide | Accessible, customizable, copy-paste components           |
| **Animations**         | **Framer Motion 12**           | Declarative, layout animations, gestures                  |
| **Forms**              | **React Hook Form + Zod 4**    | Performant forms with runtime validation                  |
| **Charts**             | **Recharts 3**                 | React-native charting for insights                        |
| **Command palette**    | **cmdk**                       | Primary search interaction (⌘K)                           |
| **Virtual scroll**     | **@tanstack/react-virtual**    | Handle 10K+ email lists without DOM pressure              |
| **Monorepo**           | **Turborepo + pnpm**           | Parallel builds, smart caching, workspace isolation       |
| **Desktop (optional)** | **PWA**                        | Install prompt, standalone window, service worker offline |

#### Best-in-Class React TypeScript Stack

```text
RUNTIME & BUILD
├── React 19.2            # Concurrent rendering, Server Components ready
├── TypeScript 5.9+       # Satisfies, const type params, decorators
├── Vite 7.1              # Sub-second HMR, optimized builds
└── Turborepo 2.5         # Monorepo orchestration

ROUTING & DATA
├── TanStack Router 1.x   # Type-safe routes, search params as state
├── TanStack Query 5.x    # Server state, caching, optimistic updates
├── Zustand 5.x           # Client state (minimal, focused)
└── React Hook Form 7.x   # Form state with Zod validation

UI COMPONENTS
├── shadcn/ui (latest)    # Copy-paste Radix-based components
├── Radix UI Primitives   # Accessible headless components
├── Tailwind CSS 4.1      # Utility-first styling
├── Framer Motion 12.x    # Layout animations, gestures
├── Lucide React           # 1500+ tree-shakeable icons
├── cmdk 1.x              # Command palette (⌘K)
├── Recharts 2.15         # Chart library for insights
└── @tanstack/react-virtual 3.x  # Virtual scrolling

SEARCH & INTERACTION
├── cmdk                   # Command palette — primary search UX
├── @radix-ui/react-dialog # Modal overlays
├── @radix-ui/react-tooltip # Tooltips
├── @radix-ui/react-progress # Progress bars
├── @radix-ui/react-tabs   # Tab navigation
├── @radix-ui/react-dropdown-menu # Action menus
└── @radix-ui/react-popover # Popovers for previews

DATA & UTILITIES
├── ky 1.x                 # HTTP client (fetch wrapper)
├── Zod 3.x               # Runtime validation + type inference
├── date-fns 4.x          # Date formatting
├── idb-keyval 6.x        # IndexedDB key-value (replaces Tauri store)
└── nanoid 5.x            # Compact unique IDs

TESTING
├── Vitest 3.2             # Unit + integration (Vite-native)
├── @testing-library/react # User-centric component testing
├── Playwright 1.50        # E2E (no Tauri dependency)
├── MSW 2.x               # API mocking for tests
└── @vitest/coverage-v8    # Coverage reporting

DEV TOOLS
├── ESLint 9.x + flat config # Linting with TypeScript rules
├── Prettier 3.x           # Code formatting
├── Storybook 8.x          # Component development & docs
└── Lighthouse CI           # Performance budgets
```

#### Monorepo Structure (Reimagined)

```text
frontend/
├── apps/
│   ├── web/                     # PRIMARY: React SPA (replaces desktop/)
│   │   ├── src/
│   │   │   ├── app/             # App shell, providers, router
│   │   │   │   ├── App.tsx
│   │   │   │   ├── Router.tsx   # TanStack Router config
│   │   │   │   ├── Providers.tsx # Query + Store + Theme
│   │   │   │   └── Layout.tsx   # Shell layout with nav
│   │   │   ├── features/        # Feature-sliced design
│   │   │   │   ├── command-center/  # Search hub (NEW)
│   │   │   │   ├── inbox-cleaner/   # Cleanup wizard (NEW)
│   │   │   │   ├── insights/        # Analytics (NEW)
│   │   │   │   ├── chat/            # AI assistant
│   │   │   │   ├── dashboard/       # Overview
│   │   │   │   ├── rules/           # Rule management
│   │   │   │   ├── settings/        # Configuration
│   │   │   │   └── onboarding/      # OAuth flow
│   │   │   ├── shared/          # Shared hooks, utils, components
│   │   │   │   ├── hooks/
│   │   │   │   ├── utils/
│   │   │   │   └── components/
│   │   │   └── test/            # Test setup, fixtures
│   │   ├── public/
│   │   ├── index.html
│   │   ├── vite.config.ts       # NO Tauri plugin
│   │   ├── tailwind.config.ts
│   │   └── package.json
│   └── (no desktop/ — web-first only)
├── packages/
│   ├── api/                     # HTTP client → REST API
│   │   ├── src/
│   │   │   ├── client.ts        # ky-based HTTP client
│   │   │   ├── sse.ts           # SSE EventSource wrapper (NEW)
│   │   │   ├── searchApi.ts     # Semantic search endpoints (NEW)
│   │   │   ├── insightsApi.ts   # Insights endpoints (NEW)
│   │   │   ├── ingestionApi.ts  # Ingestion + progress (NEW)
│   │   │   ├── clusterApi.ts    # Cluster endpoints (NEW)
│   │   │   ├── actionsApi.ts    # Bulk actions (NEW)
│   │   │   ├── authApi.ts       # OAuth
│   │   │   ├── dashboardApi.ts  # Dashboard
│   │   │   ├── rulesApi.ts      # Rules CRUD
│   │   │   └── configApi.ts     # Config
│   │   └── package.json
│   ├── types/                   # Shared TypeScript types + Zod schemas
│   │   ├── src/
│   │   │   ├── search.ts        # SearchQuery, SearchResult (NEW)
│   │   │   ├── vectors.ts       # Cluster, Embedding types (NEW)
│   │   │   ├── insights.ts      # Subscription, Pattern types (NEW)
│   │   │   ├── ingestion.ts     # IngestionProgress types (NEW)
│   │   │   ├── email.ts         # Email types
│   │   │   ├── rules.ts         # Rule types
│   │   │   ├── auth.ts          # Auth types
│   │   │   └── config.ts        # Config types
│   │   └── package.json
│   ├── ui/                      # shadcn/ui component library
│   │   ├── src/
│   │   │   ├── components/      # shadcn/ui components
│   │   │   │   ├── button.tsx
│   │   │   │   ├── command.tsx  # cmdk wrapper
│   │   │   │   ├── dialog.tsx
│   │   │   │   ├── progress.tsx
│   │   │   │   ├── card.tsx
│   │   │   │   ├── badge.tsx
│   │   │   │   ├── tabs.tsx
│   │   │   │   └── ...
│   │   │   ├── hooks/
│   │   │   └── utils/
│   │   └── package.json
│   └── core/                    # Business logic utilities
│       ├── src/
│       └── package.json
├── turbo.json
├── pnpm-workspace.yaml
└── package.json
```

### 5.3 New Dependencies

**Backend (Cargo.toml additions)** — _verified against crates.io 2026-03-23_:

```toml
[dependencies]
# RuVector ecosystem (all at 2.x, MSRV 1.77, MIT licensed)
ruvector-core = { version = "2.0", features = ["hnsw", "storage", "simd", "parallel", "api-embeddings"] }
# Default features include: simd, storage, hnsw, api-embeddings, parallel
ruvector-gnn = { version = "2.0" }    # Latest: 2.0.5
ruvllm = { version = "2.0" }          # Latest: 2.0.6 — local LLM inference (Metal/CUDA/SIMD)

# NOTE: The "sona" crate on crates.io (0.0.0) is an ELF binary analyzer, NOT the
# RuVector self-learning system. SONA learning lives inside ruvector-core or must
# be imported from the ruvector monorepo directly:
# sona = { git = "https://github.com/ruvnet/ruvector", path = "crates/sona" }

# Local embedding (text + image CLIP):
fastembed = "5.13"  # Latest: 5.13.0 — ONNX text + CLIP image embeddings

# Content extraction (FEAT-064–068)
ocrs = "0.9"             # Pure Rust OCR — no system deps, Latin text
scraper = "0.22"         # HTML parsing + CSS selectors (html5ever)
html2text = "0.16"       # HTML → readable plain text
linkify = "0.10"         # URL extraction from plain text
ammonia = "4"            # HTML sanitization (strip scripts/tracking)
infer = "0.19"           # Magic byte file type detection (100+ types)
pdf-extract = "0.7"      # PDF text extraction (pure Rust)
calamine = "0.33"        # XLSX/XLS/ODS reading (pure Rust)
dotext = "0.2"           # DOCX text extraction
```

**Backend (core crate versions)** — _verified against crates.io 2026-03-23_:

```toml
# Rust 1.94.0 (latest stable, 2026-03-02)
# All crates pinned to latest stable versions:
axum = "0.8"              # Latest: 0.8.8
tokio = "1.50"            # Latest: 1.50.0
sqlx = "0.8"              # Latest stable: 0.8.x (0.9.0 is alpha)
oauth2 = "5.0"            # Latest: 5.0.0
jsonwebtoken = "10.3"     # Latest: 10.3.0
moka = "0.12"             # Latest: 0.12.15
redis = "1.1"             # Latest: 1.1.0
apalis = "0.7"            # Latest stable: 0.7.x (1.0 is RC)
genai = "0.4"             # Latest stable: 0.4.x (0.6 is beta)
ollama-rs = "0.3"         # Latest: 0.3.4
mail-parser = "0.11"      # Latest: 0.11.2
lettre = "0.11"           # Latest: 0.11.19
evalexpr = "13.1"         # Latest: 13.1.0
serde = "1.0"             # Latest: 1.0.228
chrono = "0.4"            # Latest: 0.4.44
uuid = "1.22"             # Latest: 1.22.0
thiserror = "2.0"         # Latest: 2.0.18

# Use serde_yml (not serde_yaml which is EOL)
serde_yml = "0.0.12"      # YAML support (replaces deprecated serde_yaml)
```

**Frontend (package.json — web app)** — _verified via npm registry 2026-03-23_:

```json
{
  "dependencies": {
    "react": "^19.2.4",
    "react-dom": "^19.2.4",
    "@tanstack/react-router": "^1.168.3",
    "@tanstack/react-query": "^5.95.2",
    "@tanstack/react-virtual": "^3.13.23",
    "zustand": "^5.0.12",
    "react-hook-form": "^7.72.0",
    "zod": "^4.3.6",
    "ky": "^1.14.3",
    "cmdk": "^1.1.1",
    "recharts": "^3.8.0",
    "framer-motion": "^12.38.0",
    "lucide-react": "^1.0.1",
    "date-fns": "^4.1.0",
    "idb-keyval": "^6.2.2",
    "nanoid": "^5.1.7",
    "react-markdown": "^10.1.0",
    "react-syntax-highlighter": "^16.1.1"
  },
  "devDependencies": {
    "@radix-ui/react-dialog": "^1.1.15",
    "@radix-ui/react-tooltip": "^1.2.8",
    "@radix-ui/react-progress": "^1.1.8",
    "@radix-ui/react-tabs": "^1.1.13",
    "@radix-ui/react-dropdown-menu": "^2.1.16",
    "@radix-ui/react-popover": "^1.1.15",
    "tailwindcss": "^4.2.2",
    "typescript": "^6.0.2",
    "vite": "^8.0.2",
    "vite-plugin-pwa": "^1.2.0",
    "vitest": "^4.1.1",
    "@testing-library/react": "^16.3.2",
    "playwright": "^1.58.2",
    "msw": "^2.12.14",
    "eslint": "^10.1.0",
    "prettier": "^3.8.1",
    "storybook": "^10.3.3",
    "turbo": "^2.8.20"
  }
}
```

---

## 6. Data Model & Schema Evolution

### 6.1 New SQLite Migration

```sql
-- Migration: 012_ruvector_integration.sql

-- Track embedding status per email
ALTER TABLE emails ADD COLUMN embedding_status TEXT DEFAULT 'pending'
  CHECK (embedding_status IN ('pending', 'embedded', 'failed', 'stale'));
ALTER TABLE emails ADD COLUMN embedded_at TIMESTAMP;
ALTER TABLE emails ADD COLUMN embedding_model TEXT;
ALTER TABLE emails ADD COLUMN vector_id TEXT;  -- RuVector document ID

-- Subscription detection results
CREATE TABLE subscriptions (
  id TEXT PRIMARY KEY,
  sender_domain TEXT NOT NULL,
  sender_address TEXT NOT NULL,
  sender_name TEXT,
  frequency TEXT NOT NULL,  -- 'daily', 'weekly', 'monthly', 'irregular'
  frequency_interval_hours REAL,
  email_count INTEGER NOT NULL DEFAULT 0,
  first_seen TIMESTAMP NOT NULL,
  last_seen TIMESTAMP NOT NULL,
  has_unsubscribe_header BOOLEAN DEFAULT FALSE,
  unsubscribe_link TEXT,
  category TEXT,  -- 'newsletter', 'marketing', 'notification', 'receipt', 'social'
  cluster_id TEXT,
  avg_read_rate REAL,
  suggested_action TEXT,  -- 'keep', 'unsubscribe', 'archive', 'digest'
  user_action TEXT,  -- What user actually chose
  user_action_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_subscriptions_domain ON subscriptions(sender_domain);
CREATE INDEX idx_subscriptions_category ON subscriptions(category);
CREATE INDEX idx_subscriptions_action ON subscriptions(suggested_action);

-- Cluster assignments (topic groups)
CREATE TABLE email_clusters (
  id TEXT PRIMARY KEY,
  name TEXT,  -- Auto-generated or user-defined
  description TEXT,
  centroid_vector_id TEXT,  -- RuVector ID for cluster centroid
  email_count INTEGER DEFAULT 0,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE email_cluster_members (
  email_id TEXT NOT NULL REFERENCES emails(id),
  cluster_id TEXT NOT NULL REFERENCES email_clusters(id),
  similarity_score REAL NOT NULL,
  PRIMARY KEY (email_id, cluster_id)
);
CREATE INDEX idx_cluster_members_cluster ON email_cluster_members(cluster_id);

-- Search interaction history (for SONA learning)
CREATE TABLE search_interactions (
  id TEXT PRIMARY KEY,
  query_text TEXT NOT NULL,
  query_vector_id TEXT,
  result_email_id TEXT REFERENCES emails(id),
  result_rank INTEGER,
  clicked BOOLEAN DEFAULT FALSE,
  feedback TEXT,  -- 'relevant', 'irrelevant', null
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_search_interactions_query ON search_interactions(query_text);

-- Ingestion progress tracking
CREATE TABLE ingestion_jobs (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  total_emails INTEGER,
  processed_emails INTEGER DEFAULT 0,
  embedded_emails INTEGER DEFAULT 0,
  failed_emails INTEGER DEFAULT 0,
  started_at TIMESTAMP,
  completed_at TIMESTAMP,
  error_message TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

### 6.2 RuVector Storage

RuVector manages its own persistence via REDB (embedded key-value store). Stored at:

```text
~/.emailibrium/vectors/
├── hnsw.redb          # HNSW index + vectors
├── metadata.redb      # Document metadata (email_id → vector mapping)
└── sona.redb          # Learned weights and session state
```

**Vector document schema** (stored in RuVector):

```rust
pub struct EmailVector {
    pub id: VectorId,           // UUID
    pub email_id: String,       // Links to SQLite emails.id
    pub vector: Vec<f32>,       // 384-dimensional embedding
    pub metadata: HashMap<String, String>,  // Filterable attributes
    // metadata includes: from_domain, category, received_at, account_id
}
```

---

## 7. Backend Implementation Plan

### 7.1 New API Endpoints

```text
POST   /api/v1/search                    # Hybrid semantic + keyword search
POST   /api/v1/search/semantic            # Pure vector search
POST   /api/v1/search/similar/:email_id   # Find similar emails
GET    /api/v1/search/suggestions          # Autocomplete / did-you-mean

GET    /api/v1/clusters                   # List topic clusters
GET    /api/v1/clusters/:id               # Cluster detail + members
POST   /api/v1/clusters/refresh           # Re-run clustering

GET    /api/v1/insights/subscriptions     # Detected subscriptions
GET    /api/v1/insights/recurring          # Recurring sender patterns
GET    /api/v1/insights/report             # Full inbox intelligence report
POST   /api/v1/insights/refresh            # Re-analyze patterns

POST   /api/v1/actions/bulk               # Execute bulk action on email set
POST   /api/v1/actions/unsubscribe/:id    # Unsubscribe from subscription
POST   /api/v1/actions/archive-cluster/:id # Archive entire cluster
POST   /api/v1/actions/apply-rule          # Apply rule to matched emails

GET    /api/v1/ingestion/status            # Current ingestion progress (SSE)
POST   /api/v1/ingestion/start             # Begin full inbox ingestion
POST   /api/v1/ingestion/pause             # Pause ingestion
POST   /api/v1/ingestion/resume            # Resume ingestion

GET    /api/v1/vectors/health              # RuVector health + stats
GET    /api/v1/vectors/stats               # Index size, query stats, SONA metrics
```

### 7.2 Embedding Pipeline Service

```rust
// backend/src/vectors/embedding.rs

pub struct EmbeddingPipeline {
    ruvllm: Option<RuvLlmClient>,      // Preferred: local Rust-native
    ollama: Option<OllamaClient>,       // Fallback 1: Ollama API
    cloud: Option<CloudEmbedClient>,    // Fallback 2: OpenAI/Cohere
    cache: Arc<moka::Cache<u64, Vec<f32>>>,
    config: EmbeddingConfig,
}

impl EmbeddingPipeline {
    /// Embed a single text. Tries providers in priority order.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> { ... }

    /// Batch embed multiple texts. Uses provider batch APIs.
    /// Returns results in same order as input.
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Chunk into groups of 64 for batch efficiency
        // Parallel execution across chunks
        // ~2ms amortized per text in batch mode
    }

    /// Embed an email's searchable content.
    pub fn prepare_email_text(email: &Email) -> String {
        format!(
            "{subject}\nFrom: {from}\nTo: {to}\nDate: {date}\n{body}",
            subject = email.subject,
            from = email.from_addr,
            to = email.to_addrs.join(", "),
            date = email.received_at.format("%Y-%m-%d"),
            body = &email.body_text[..min(400, email.body_text.len())]
        )
    }
}
```

### 7.3 Vector-Enhanced Categorization

```rust
// Replace LLM-first categorization with vector-first

pub struct VectorCategorizer {
    vector_store: Arc<VectorStore>,
    category_centroids: HashMap<EmailCategory, Vec<f32>>,
    confidence_threshold: f32,  // Below this, fall back to LLM
    llm_fallback: Arc<EmailCategorizer>,  // Existing LLM categorizer
}

impl VectorCategorizer {
    /// Categorize by nearest centroid. ~0.5ms vs ~200ms for LLM.
    pub async fn categorize(&self, email: &Email) -> CategoryResult {
        let embedding = self.embed(email).await?;

        // Find nearest category centroid
        let mut best_category = EmailCategory::Personal;
        let mut best_score = f32::MIN;

        for (category, centroid) in &self.category_centroids {
            let score = cosine_similarity(&embedding, centroid);
            if score > best_score {
                best_score = score;
                best_category = *category;
            }
        }

        if best_score >= self.confidence_threshold {
            CategoryResult {
                category: best_category,
                confidence: best_score,
                method: "vector_centroid",
            }
        } else {
            // Low confidence → fall back to LLM for nuanced classification
            self.llm_fallback.categorize(email).await
        }
    }

    /// Update centroids from user feedback (SONA micro-learning)
    pub fn learn_from_feedback(&mut self, email: &Email, correct_category: EmailCategory) {
        // Adjust centroid position toward this email's embedding
        // Uses exponential moving average
    }
}
```

### 7.4 Inbox Ingestion Pipeline

```rust
// backend/src/vectors/ingestion.rs

pub struct IngestionPipeline {
    embedding: Arc<EmbeddingPipeline>,
    vector_store: Arc<VectorStore>,
    insight_engine: Arc<InsightEngine>,
    db: Arc<Database>,
    progress: Arc<RwLock<IngestionProgress>>,
}

pub struct IngestionProgress {
    pub job_id: String,
    pub total: u64,
    pub processed: u64,
    pub embedded: u64,
    pub categorized: u64,
    pub failed: u64,
    pub phase: IngestionPhase,
    pub eta_seconds: Option<u64>,
    pub emails_per_second: f64,
}

pub enum IngestionPhase {
    Syncing,        // Fetching from provider
    Embedding,      // Generating vectors
    Categorizing,   // Assigning categories
    Clustering,     // Building topic groups
    Analyzing,      // Subscription/pattern detection
    Complete,
}

impl IngestionPipeline {
    /// Full ingestion with streaming progress via SSE.
    pub async fn ingest_account(
        &self,
        account_id: &str,
        tx: mpsc::Sender<IngestionProgress>,
    ) -> Result<IngestionReport> {
        // Phase 1: Sync emails from provider (existing HistoricalProcessor)
        // Phase 2: Batch embed (chunks of 500, parallel)
        // Phase 3: Categorize via vector centroids
        // Phase 4: Run GNN clustering
        // Phase 5: Detect subscriptions & patterns

        // Each phase streams progress via `tx`
        // Checkpoint after every 1000 emails for resume capability
    }
}
```

### 7.5 Multi-Asset Content Pipeline (FEAT-064–068)

Emails contain far more than plain text. The multi-asset pipeline extracts, analyzes, and vectorizes **every searchable asset** in an email:

```text
Email → Parse (mail-parser)
  │
  ├─── Body (text/plain)
  │     → embed text → RuVector "email_text" collection
  │
  ├─── Body (text/html)
  │     → ammonia (sanitize) → scraper (structured extraction)
  │     → html2text (clean readable text) → embed → RuVector "email_text"
  │     → linkify/scraper (extract URLs) → classify → RuVector metadata
  │     → detect tracking pixels (1x1 img, known domains)
  │
  ├─── Inline Images (Content-ID)
  │     → decode base64 → ocrs/leptess (OCR text) → embed → RuVector "image_text"
  │     → fastembed CLIP model → image vector → RuVector "image_visual"
  │     → (optional) multimodal LLM description → embed → RuVector "image_text"
  │
  ├─── Attachments
  │     → infer (magic byte detection → true file type)
  │     → Route by type:
  │         PDF  → pdf-extract → text → embed → RuVector "attachment_text"
  │         DOCX → dotext → text → embed → RuVector "attachment_text"
  │         XLSX → calamine → cell text → embed → RuVector "attachment_text"
  │         IMG  → OCR + CLIP (same as inline images)
  │         Other → store metadata only (filename, size, type)
  │
  └─── URLs (extracted from HTML body)
        → reqwest (resolve redirects, capture chain)
        → classify: tracking, unsubscribe, shopping, news, social, internal
        → store as filterable metadata on email vector
```

**New backend module**: `backend/src/content/`

```text
backend/src/content/
├── mod.rs               # ContentPipeline facade
├── html_extractor.rs    # HTML → clean text + structured content (scraper, html2text, ammonia)
├── link_analyzer.rs     # URL extraction, redirect resolution, classification (linkify, reqwest)
├── image_analyzer.rs    # OCR (ocrs), CLIP embedding (fastembed), multimodal LLM (genai/ollama)
├── attachment_extractor.rs  # File type detection (infer), text extraction (pdf-extract, calamine, dotext)
└── tracking_detector.rs # Tracking pixel detection, analytics beacon identification
```

**New Rust crates** (all verified on crates.io 2026-03-23):

```toml
# Content extraction
ocrs = "0.9"             # Pure Rust OCR (no system deps, Latin text)
scraper = "0.22"         # HTML parsing + CSS selectors (html5ever-based)
html2text = "0.16"       # HTML → readable plain text
linkify = "0.10"         # URL extraction from plain text
ammonia = "4"            # HTML sanitization

# Attachment processing
infer = "0.19"           # Magic byte file type detection (100+ types)
pdf-extract = "0.7"      # PDF text extraction (pure Rust)
calamine = "0.33"        # XLSX/XLS/ODS reading (pure Rust, fast)
dotext = "0.2"           # DOCX text extraction

# Image embeddings (in addition to text embeddings)
fastembed = "5.13"       # CLIP models for image → vector (already in plan)
```

**Multi-collection vector search**:

```rust
pub struct MultiAssetSearchResult {
    pub email_id: String,
    pub body_matches: Vec<ScoredResult>,       // Text body semantic matches
    pub image_matches: Vec<ScoredResult>,       // OCR text + CLIP visual matches
    pub attachment_matches: Vec<ScoredResult>,  // PDF/DOCX/XLSX content matches
    pub url_matches: Vec<UrlMatch>,             // URLs matching query domain/category
    pub combined_score: f32,                    // Weighted fusion across all collections
}

// Example: "find receipts from Amazon"
// → body_matches: emails mentioning "order", "receipt", "Amazon"
// → image_matches: OCR'd receipt images with Amazon logo (CLIP)
// → attachment_matches: PDF invoices with "Amazon" text
// → url_matches: URLs pointing to amazon.com/orders
// → combined_score: fused ranking across all asset types
```

**Performance budget** (per email during ingestion):

| Step                                  | Latency                  | Local?       |
| ------------------------------------- | ------------------------ | ------------ |
| HTML extraction (scraper + html2text) | < 1ms                    | Yes          |
| URL extraction (linkify + scraper)    | < 1ms                    | Yes          |
| Text embedding (fastembed MiniLM)     | ~5ms                     | Yes          |
| Image OCR (ocrs)                      | ~200ms per image         | Yes          |
| Image CLIP embedding (fastembed)      | ~50ms per image          | Yes          |
| PDF text extraction                   | ~100ms per page          | Yes          |
| XLSX reading (calamine)               | ~50ms per file           | Yes          |
| URL redirect resolution               | ~200ms per URL (network) | Network      |
| Multimodal LLM description            | ~500ms local, ~1s cloud  | Configurable |

For a typical email (no attachments, 2 inline images, 5 URLs): **~520ms total**.
For a heavy email (3 PDF attachments, 5 images): **~2s total**.
Parallelized across 4 workers: **10K emails in ~20 minutes** with full multi-asset extraction.

### 7.6 Ingest → Tag → Archive Pipeline (FEAT-070)

The core strategy: **Gmail becomes a dumb store. Emailibrium is the smart interface.**

```text
New email arrives in Gmail INBOX
  │
  ├─── Gmail watch() push notification ──── Pub/Sub → webhook
  │
  ├─── 1. Fetch via Gmail API ─────────── messages.get (format=full)
  │
  ├─── 2. Parse & embed ──────────────── mail-parser → RuVector
  │
  ├─── 3. Classify ───────────────────── VectorCategorizer → category
  │         confidence >= 0.7 → centroid
  │         confidence <  0.7 → LLM fallback
  │
  ├─── 4. Auto-group ─────────────────── HNSW nearest cluster assignment
  │
  ├─── 5. Apply Gmail labels ─────────── messages.modify
  │         addLabelIds: ["EM/{category}", "EM/{cluster_name}"]
  │
  ├─── 6. Archive in Gmail ───────────── messages.modify
  │         removeLabelIds: ["INBOX"]     (configurable: auto vs manual)
  │
  └─── 7. Store locally ──────────────── SQLite + RuVector + Moka cache
          Email is now searchable, classified, grouped, and archived
          User sees it ONLY in Emailibrium UI
```

**Archive timing options** (user-configurable):

- **Instant** (power users): Archive within 2 seconds of ingestion. True zero inbox.
- **Delayed** (default): Archive after 60 seconds. Allows mobile Gmail notification to be actionable.
- **Manual** (conservative): Don't auto-archive. User explicitly marks "Done" in Emailibrium.

**Batch archive on first sync**:

```rust
// Archive 10,000 emails in ~5 seconds
pub async fn batch_archive(&self, message_ids: &[String]) -> Result<()> {
    for chunk in message_ids.chunks(1000) {
        self.gmail_client.batch_modify(BatchModifyRequest {
            ids: chunk.to_vec(),
            add_label_ids: vec![],
            remove_label_ids: vec!["INBOX".to_string()],
        }).await?;
    }
    Ok(())
}
```

**Offline resilience**: If Emailibrium is offline, emails accumulate in Gmail INBOX. On reconnect, incremental sync via `history.list(startHistoryId=...)` catches all missed emails, classifies, and archives them.

### 7.7 Full Email Client Backend (FEAT-069, FEAT-075)

New API endpoints for email interaction:

```text
# Reading
GET    /api/v1/emails                     # List emails (with RuVector-powered sorting)
GET    /api/v1/emails/:id                 # Get full email (body, attachments, headers)
GET    /api/v1/emails/:id/thread          # Get full conversation thread
GET    /api/v1/emails/:id/attachments/:n  # Download specific attachment

# Actions
POST   /api/v1/emails/:id/archive        # Archive (remove from inbox)
POST   /api/v1/emails/:id/unarchive      # Move back to inbox
POST   /api/v1/emails/:id/star           # Star/flag
POST   /api/v1/emails/:id/read           # Mark read
POST   /api/v1/emails/:id/unread         # Mark unread
POST   /api/v1/emails/:id/label          # Apply label(s)
POST   /api/v1/emails/:id/reclassify     # User reclassifies → triggers learning
POST   /api/v1/emails/:id/move-group     # Move to different topic group → triggers learning
DELETE /api/v1/emails/:id                 # Delete (trash in Gmail)

# Composing
POST   /api/v1/emails/compose            # New email
POST   /api/v1/emails/:id/reply          # Reply (sets In-Reply-To, References, threadId)
POST   /api/v1/emails/:id/reply-all      # Reply all
POST   /api/v1/emails/:id/forward        # Forward
POST   /api/v1/emails/drafts             # Save draft
PUT    /api/v1/emails/drafts/:id         # Update draft
POST   /api/v1/emails/drafts/:id/send    # Send draft

# Groups & Classification
GET    /api/v1/groups                     # List auto-discovered topic groups
PUT    /api/v1/groups/:id                 # Rename/pin a group
POST   /api/v1/groups/:id/pin            # Pin group (prevents auto-merge/delete)
GET    /api/v1/groups/:id/emails          # Emails in a group
POST   /api/v1/classify/feedback          # Batch feedback submission
```

### 7.8 Continuous Learning Engine (FEAT-072, FEAT-073)

```rust
// backend/src/vectors/learning.rs

pub struct LearningEngine {
    sona: Arc<SonaRuntime>,
    categorizer: Arc<VectorCategorizer>,
    cluster_engine: Arc<ClusterEngine>,
    db: Arc<Database>,
}

impl LearningEngine {
    /// Process explicit user feedback (reclassification, group move)
    pub async fn on_user_feedback(&self, feedback: UserFeedback) -> Result<()> {
        let quality = match &feedback.action {
            FeedbackAction::Reclassify { from, to } => {
                // Update category centroid via EMA
                self.categorizer.update_centroid(*to, &feedback.embedding, 1.0);
                // Negative signal for old category (weaker)
                self.categorizer.update_centroid(*from, &feedback.embedding, -0.3);
                1.0 // Highest quality signal
            }
            FeedbackAction::MoveToGroup { group_id } => {
                self.cluster_engine.assign_to_cluster(&feedback.email_id, group_id);
                1.0
            }
            FeedbackAction::Star => 0.4,
            FeedbackAction::Reply { delay_secs } => {
                if *delay_secs < 300 { 0.5 } else { 0.3 }
            }
            FeedbackAction::Archive => 0.2,
            FeedbackAction::Delete => 0.4,
        };

        // Feed SONA trajectory
        let traj = self.sona.begin_trajectory(&feedback.embedding);
        self.sona.end_trajectory(traj, quality);
        self.sona.tick(); // Process immediately for instant learning

        Ok(())
    }

    /// Hourly background job: incremental re-clustering + centroid adjustment
    pub async fn hourly_consolidation(&self) -> Result<ConsolidationReport> {
        // 1. Mini-Batch K-Means on unassigned emails (~500ms / 100K)
        let new_clusters = self.cluster_engine.incremental_recluster().await?;

        // 2. Re-classify low-confidence emails with updated centroids
        let reclassified = self.categorizer.reclassify_low_confidence(0.6).await?;

        // 3. SONA session consolidation
        self.sona.consolidate_session();

        Ok(ConsolidationReport { new_clusters, reclassified })
    }

    /// Daily background job: full re-clustering + deep learning consolidation
    pub async fn daily_consolidation(&self) -> Result<DailyReport> {
        // 1. Full HDBSCAN re-clustering (~8s / 100K)
        let clusters = self.cluster_engine.full_recluster_with_stability().await?;

        // 2. Recompute category centroids from all feedback data
        self.categorizer.recompute_centroids_from_history().await?;

        // 3. SONA deep consolidation (BaseLoRA with EWC++)
        self.sona.consolidate_long_term();

        Ok(DailyReport { clusters })
    }
}
```

### 7.9 Insight Engine

```rust
// backend/src/vectors/insights.rs

pub struct InsightEngine {
    vector_store: Arc<VectorStore>,
    db: Arc<Database>,
}

impl InsightEngine {
    /// Detect all subscription-like patterns in the inbox.
    pub async fn detect_subscriptions(&self) -> Vec<SubscriptionInsight> {
        // 1. Group emails by sender_domain
        // 2. For each group with > 3 emails:
        //    a. Check List-Unsubscribe header presence
        //    b. Compute inter-arrival intervals
        //    c. Fit recurrence model (daily/weekly/monthly)
        //    d. Compute content similarity within group (RuVector)
        //    e. Classify: newsletter, marketing, notification, receipt
        //    f. Score actionability (read-rate, age, frequency)
        // 3. Sort by suggested_action priority
    }

    /// Analyze recurring sender patterns beyond subscriptions.
    pub async fn analyze_recurring_senders(&self) -> Vec<RecurringSenderInsight> {
        // Identify colleagues, clients, services by frequency + content
    }

    /// Generate full inbox intelligence report.
    pub async fn generate_report(&self) -> InboxReport {
        InboxReport {
            total_emails: ...,
            category_breakdown: ...,    // Pie chart data
            top_senders: ...,           // Bar chart data
            subscription_count: ...,
            estimated_time_spent: ...,  // Reading time estimate
            clusters: ...,             // Topic map data
            action_suggestions: ...,   // "You could unsubscribe from 47 newsletters"
        }
    }
}
```

---

## 8. Frontend: Tauri Desktop → React TypeScript Web

### 8.1 Architecture Decision: Web-First SPA

The frontend is a **pure React TypeScript web application** — no native desktop shell, no Tauri, no Electron:

- **Standalone SPA** served via Vite, deployable to any static host or run locally in Docker
- Communicates with backend exclusively via **REST API + SSE**
- Works in any modern browser — desktop, tablet, mobile
- Optional **PWA wrapper** for installed desktop experience (standalone window, offline, notifications)
- No native dependencies to install or update

### 8.2 Implementation Approach

**Phase 1: Foundation** — Build the web app shell

- Create `apps/web/` with Vite 8 + React 19 + TanStack Router
- Set up shadcn/ui component library (Radix-based, accessible)
- Implement REST API client in `packages/api/`
- Implement SSE client for real-time streaming
- Implement secure storage via Web Crypto API + IndexedDB
- Set up cmdk command palette as primary search interaction

**Phase 2: Core Screens** — Build RuVector-powered UI

- Command Center (search hub)
- Inbox Cleaner (guided cleanup wizard)
- Insights Explorer (subscription analytics)
- Rules Studio (AI-suggested rules)
- Chat interface (conversational rule building with cluster context)
- Dashboard (overview stats)

**Phase 3: Polish** — Quality and distribution

- Responsive design (mobile-friendly)
- Accessibility (ARIA, keyboard nav, focus management)
- PWA support (service worker, manifest, install prompt)
- Storybook component documentation
- E2E tests with Playwright

### 8.3 Route Structure (Web App)

```typescript
// frontend/apps/web/src/app/Router.tsx
// Using TanStack Router for type-safe routing

import { createRouter, createRoute, createRootRoute } from '@tanstack/react-router';

const rootRoute = createRootRoute({ component: AppLayout });

const routes = [
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/onboarding',
    component: OnboardingFlow,           // OAuth flow
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/command-center',
    component: CommandCenter,             // NEW: Primary hub + search
    validateSearch: (search) => searchSchema.parse(search),  // Type-safe search params
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/inbox-cleaner',
    component: InboxCleaner,              // NEW: Guided cleanup wizard
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/insights',
    component: InsightsExplorer,          // NEW: Subscription & pattern analytics
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/rules',
    component: RulesStudio,              // Enhanced with RuVector
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/dashboard',
    component: Dashboard,                 // Enhanced with RuVector
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/settings',
    component: Settings,                  // Configuration
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/chat',
    component: ChatInterface,             // Enhanced with RuVector with cluster context
  }),
  createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: () => <Navigate to="/command-center" />,
  }),
];
```

### 8.4 Key Frontend Architecture Patterns

#### SSE Client (replaces Tauri events + polling)

```typescript
// packages/api/src/sse.ts
export function createSSEStream<T>(
  url: string,
  options?: { onError?: (e: Event) => void },
): {
  subscribe: (handler: (data: T) => void) => () => void;
  close: () => void;
} {
  const source = new EventSource(url, { withCredentials: true });
  return {
    subscribe: (handler) => {
      const listener = (e: MessageEvent) => handler(JSON.parse(e.data));
      source.addEventListener('message', listener);
      return () => source.removeEventListener('message', listener);
    },
    close: () => source.close(),
  };
}

// Usage in React:
function useIngestionProgress(jobId: string) {
  const [progress, setProgress] = useState<IngestionProgress | null>(null);
  useEffect(() => {
    const stream = createSSEStream<IngestionProgress>(`/api/v1/ingestion/status?jobId=${jobId}`);
    const unsub = stream.subscribe(setProgress);
    return () => {
      unsub();
      stream.close();
    };
  }, [jobId]);
  return progress;
}
```

#### Secure Storage (replaces Tauri plugin-store)

```typescript
// packages/core/src/secureStorage.ts
import { get, set, del } from 'idb-keyval';

const ENCRYPTION_KEY_NAME = 'emailibrium-storage-key';

async function getOrCreateKey(): Promise<CryptoKey> {
  const stored = await get(ENCRYPTION_KEY_NAME);
  if (stored) return stored;
  const key = await crypto.subtle.generateKey(
    { name: 'AES-GCM', length: 256 },
    false, // not extractable
    ['encrypt', 'decrypt'],
  );
  await set(ENCRYPTION_KEY_NAME, key);
  return key;
}

export const secureStorage = {
  async setItem(key: string, value: string): Promise<void> {
    const cryptoKey = await getOrCreateKey();
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const encrypted = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, cryptoKey, new TextEncoder().encode(value));
    await set(`secure:${key}`, { iv, data: encrypted });
  },
  async getItem(key: string): Promise<string | null> {
    const cryptoKey = await getOrCreateKey();
    const stored = await get(`secure:${key}`);
    if (!stored) return null;
    const decrypted = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: stored.iv }, cryptoKey, stored.data);
    return new TextDecoder().decode(decrypted);
  },
  async removeItem(key: string): Promise<void> {
    await del(`secure:${key}`);
  },
};
```

#### Command Palette (primary search UX)

```typescript
// features/command-center/components/CommandPalette.tsx
import { Command } from 'cmdk';

export function CommandPalette() {
  const [query, setQuery] = useState('');
  const { data: results, isLoading } = useHybridSearch(query);

  return (
    <Command>
      <Command.Input
        placeholder="Search anything... (⌘K)"
        value={query}
        onValueChange={setQuery}
      />
      <Command.List>
        {isLoading && <Command.Loading>Searching...</Command.Loading>}
        <Command.Empty>No results found.</Command.Empty>

        {results?.emails?.length > 0 && (
          <Command.Group heading="Emails">
            {results.emails.map((r) => (
              <Command.Item key={r.email.id} value={r.email.subject}>
                <EmailResultRow result={r} />
              </Command.Item>
            ))}
          </Command.Group>
        )}

        {results?.clusters?.length > 0 && (
          <Command.Group heading="Related Topics">
            {results.clusters.map((c) => (
              <Command.Item key={c.id} value={c.name}>
                <ClusterBadge cluster={c} />
              </Command.Item>
            ))}
          </Command.Group>
        )}

        <Command.Group heading="Actions">
          <Command.Item onSelect={() => navigate('/inbox-cleaner')}>
            Clean Inbox
          </Command.Item>
          <Command.Item onSelect={() => navigate('/insights')}>
            View Insights
          </Command.Item>
          <Command.Item onSelect={() => navigate('/rules')}>
            Manage Rules
          </Command.Item>
        </Command.Group>
      </Command.List>
    </Command>
  );
}
```

### 8.5 New Type Definitions

```typescript
// frontend/packages/types/src/search.ts

export interface SearchQuery {
  text: string;
  mode: 'hybrid' | 'semantic' | 'keyword';
  filters?: {
    dateRange?: { from: string; to: string };
    senders?: string[];
    labels?: string[];
    categories?: EmailCategory[];
    hasAttachment?: boolean;
    isRead?: boolean;
    accounts?: string[];
  };
  limit?: number;
  offset?: number;
}

export interface SearchResult {
  email: Email;
  score: number;
  matchType: 'semantic' | 'keyword' | 'hybrid';
  highlights: { field: string; snippet: string }[];
}

// frontend/packages/types/src/vectors.ts

export interface Cluster {
  id: string;
  name: string;
  description: string;
  emailCount: number;
  topSenders: string[];
  dateRange: { from: string; to: string };
  previewEmails: Email[];
}

// frontend/packages/types/src/insights.ts

export interface SubscriptionInsight {
  id: string;
  senderDomain: string;
  senderAddress: string;
  senderName: string | null;
  frequency: RecurrencePattern;
  emailCount: number;
  firstSeen: string;
  lastSeen: string;
  hasUnsubscribeHeader: boolean;
  unsubscribeLink: string | null;
  category: SubscriptionCategory;
  avgReadRate: number | null;
  suggestedAction: 'keep' | 'unsubscribe' | 'archive' | 'digest';
}

// frontend/packages/types/src/ingestion.ts

export interface IngestionProgress {
  jobId: string;
  total: number;
  processed: number;
  embedded: number;
  categorized: number;
  failed: number;
  phase: 'syncing' | 'embedding' | 'categorizing' | 'clustering' | 'analyzing' | 'complete';
  etaSeconds: number | null;
  emailsPerSecond: number;
}

export interface InboxReport {
  totalEmails: number;
  categoryBreakdown: Record<EmailCategory, number>;
  topSenders: { sender: string; count: number }[];
  subscriptionCount: number;
  estimatedReadingHours: number;
  clusters: Cluster[];
  actionSuggestions: ActionSuggestion[];
}
```

### 8.6 PWA Support (Optional Desktop Experience)

For users who want an "installed" desktop feel without Tauri:

```typescript
// frontend/apps/web/vite.config.ts
import { VitePWA } from 'vite-plugin-pwa';

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: 'autoUpdate',
      manifest: {
        name: 'Emailibrium',
        short_name: 'Emailibrium',
        description: 'AI-powered email intelligence',
        theme_color: '#4F46E5',
        icons: [
          { src: '/icon-192.png', sizes: '192x192', type: 'image/png' },
          { src: '/icon-512.png', sizes: '512x512', type: 'image/png' },
        ],
        display: 'standalone', // Feels like native app
        start_url: '/command-center',
      },
      workbox: {
        runtimeCaching: [
          {
            urlPattern: /^\/api\//,
            handler: 'NetworkFirst', // API calls: network first, cache fallback
            options: { cacheName: 'api-cache', expiration: { maxEntries: 100 } },
          },
        ],
      },
    }),
  ],
});
```

This gives users:

- Install prompt in browser ("Add to Home Screen")
- Standalone window (no browser chrome)
- Offline capability via Service Worker
- Background sync for email operations
- Native-feeling on macOS, Windows, Linux, and mobile

---

## 9. User Journeys

### 9.1 Journey: First-Time Onboarding (One or More Accounts)

```text
┌─────────────────────────────────────────────────────────────────┐
│ STEP 1: WELCOME (0:00)                                          │
│                                                                 │
│ ◉ Emailibrium                                                   │
│                                                                 │
│ "Take control of your inbox."                                   │
│ Emailibrium replaces Gmail, Outlook, and Apple Mail with        │
│ AI-powered organization, instant search, and zero inbox.        │
│                                                                 │
│ Connect one or more email accounts to get started.              │
│                                                                 │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │                                                             │ │
│ │  ┌──────────────────┐  ┌──────────────────┐               │ │
│ │  │   G  Gmail       │  │   M  Outlook     │               │ │
│ │  │   Sign in with   │  │   Sign in with   │               │ │
│ │  │   Google OAuth   │  │   Microsoft OAuth│               │ │
│ │  └──────────────────┘  └──────────────────┘               │ │
│ │                                                             │ │
│ │  ┌──────────────────┐  ┌──────────────────┐               │ │
│ │  │   ✉  IMAP        │  │   ✉  Other       │               │ │
│ │  │   Manual server  │  │   Yahoo, iCloud, │               │ │
│ │  │   configuration  │  │   Fastmail, etc.  │               │ │
│ │  └──────────────────┘  └──────────────────┘               │ │
│ │                                                             │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                                                                 │
│ "Your emails never leave your device. All processing is local." │
│ [Learn about our privacy model →]                              │
└─────────────────────────────────────────────────────────────────┘
```

#### Provider-Specific Flows

**Gmail (OAuth 2.0 + PKCE):**

```text
User clicks [Gmail] →
  Browser opens Google OAuth consent screen
  Scopes requested:
    • gmail.modify (read, send, label, archive)
    • gmail.labels (manage labels)
    • userinfo.email (identify account)
  User grants access → redirect to Emailibrium callback
  Backend exchanges code for access + refresh tokens
  Tokens encrypted with OAUTH_ENCRYPTION_KEY → stored in DB
  "Gmail connected! user@gmail.com"
  [Connect Another Account]  [Start Analysis →]
```

**Outlook (OAuth 2.0 + PKCE via Microsoft Identity Platform):**

```text
User clicks [Outlook] →
  Browser opens Microsoft login
  Scopes requested:
    • Mail.ReadWrite (read + modify)
    • Mail.Send (compose + reply)
    • User.Read (identify account)
  User grants access → redirect to Emailibrium callback
  Backend exchanges code for tokens via /oauth2/v2.0/token
  Tokens encrypted → stored in DB
  "Outlook connected! user@outlook.com"
  [Connect Another Account]  [Start Analysis →]
```

**IMAP/Other (Manual Configuration):**

```text
User clicks [IMAP] or [Other] →
  ┌─────────────────────────────────────────────────────────────┐
  │ IMAP Server Configuration                                    │
  │                                                             │
  │ Provider: [Select or enter manually ▾]                       │
  │   Presets: Yahoo, iCloud, Fastmail, Zoho, ProtonMail Bridge │
  │   (presets auto-fill server/port/encryption)                 │
  │                                                             │
  │ Email:    [user@provider.com                    ]           │
  │ Password: [••••••••••••                         ]           │
  │   (or App Password — link to provider instructions)          │
  │                                                             │
  │ ── Advanced (auto-filled by preset, editable) ────────────  │
  │ IMAP Server: [imap.provider.com     ]  Port: [993  ]       │
  │ Encryption:  [SSL/TLS ▾]                                    │
  │ SMTP Server: [smtp.provider.com     ]  Port: [465  ]       │
  │ Encryption:  [SSL/TLS ▾]                                    │
  │                                                             │
  │ [Test Connection]                                            │
  │   ✓ IMAP: Connected (842 messages in INBOX)                  │
  │   ✓ SMTP: Connected (send test email succeeded)              │
  │                                                             │
  │ [Save & Connect →]                                           │
  └─────────────────────────────────────────────────────────────┘
```

**Connecting Multiple Accounts:**

```text
After first account connected:

┌─────────────────────────────────────────────────────────────────┐
│ CONNECTED ACCOUNTS                                              │
│                                                                 │
│ ✓ user@gmail.com           Gmail    [Syncing... 4,231 emails]  │
│                                                                 │
│ Want to add more accounts?                                      │
│ All accounts feed into one unified inbox.                      │
│                                                                 │
│ [+ Add Gmail]  [+ Add Outlook]  [+ Add IMAP]                  │
│                                                                 │
│ ── or ──                                                        │
│                                                                 │
│ [Skip — Start with 1 account →]                                │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘

User adds Outlook:

┌─────────────────────────────────────────────────────────────────┐
│ CONNECTED ACCOUNTS                                              │
│                                                                 │
│ ✓ user@gmail.com           Gmail    ✓ 12,847 emails synced     │
│ ✓ user@company.com         Outlook  [Syncing... 8,432 emails]  │
│                                                                 │
│ [+ Add Another Account]                                        │
│                                                                 │
│ [Continue to Analysis →]                                       │
└─────────────────────────────────────────────────────────────────┘
```

#### Step 2: Ingest & Analyze (All Accounts)

```text
┌─────────────────────────────────────────────────────────────────┐
│ STEP 2: ANALYZING YOUR INBOX (0:30 - 3:00)                     │
│                                                                 │
│ Processing 2 accounts • 21,279 total emails                    │
│                                                                 │
│ ┌─ user@gmail.com (12,847) ──────────────────────────────────┐ │
│ │ Syncing      ████████████████████████████████████████ 100%  │ │
│ │ Analyzing    ████████████████████████░░░░░░░░░░░░░░░  62%  │ │
│ └────────────────────────────────────────────────────────────┘ │
│ ┌─ user@company.com (8,432) ─────────────────────────────────┐ │
│ │ Syncing      ████████████████████████████░░░░░░░░░░░░  71%  │ │
│ │ Analyzing    ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   0%  │ │
│ └────────────────────────────────────────────────────────────┘ │
│                                                                 │
│ Live discoveries (across all accounts):                        │
│ ┌──────────────────────────────────────────────────────┐       │
│ │ 📊 72 subscriptions detected                         │       │
│ │ 📊 9 topic clusters forming                          │       │
│ │ 📊 3 accounts overlap: 47 senders appear in both     │       │
│ │ 📊 Top sender: notifications@github.com (1,340)      │       │
│ └──────────────────────────────────────────────────────┘       │
└─────────────────────────────────────────────────────────────────┘
```

#### Steps 3-5: Same as before (Inbox Cleaner → Rules → Command Center)

The cleanup wizard, rule suggestions, and command center all operate on the **unified inbox** — all accounts merged. Topic clusters span accounts (e.g., "Project Alpha" may contain emails from both Gmail and Outlook). Categories and groups are global.

```text
┌─────────────────────────────────────────────────────────────────┐
│ STEP 3: INBOX CLEANER WIZARD (3:00 - 8:00)                      │
│                                                                 │
│ "Your inbox is analyzed! Let's clean up."                       │
│ 21,279 emails across 2 accounts                                │
│                                                                 │
│ ── SUBSCRIPTIONS (72 found) ─────────────────────────────────── │
│ │ ☑ marketing@company.com  Weekly  142 emails  Unsubscribe  │   │
│ │ ☑ news@techsite.io       Daily   891 emails  Unsubscribe  │   │
│ │ ☐ updates@github.com     Daily   892 emails  Keep         │   │
│ │ ... 69 more                                               │   │
│ │ [Select All Unsubscribe Suggestions] [Review Individually]│   │
│ └───────────────────────────────────────────────────────────┘   │
│                                                                 │
│ ── TOPIC CLUSTERS (across all accounts) ────────────────────── │
│ │ Work / Project Alpha    2,456 emails  [Archive Old ▾]     │   │
│ │   ├ 1,234 from Gmail • 1,222 from Outlook                │   │
│ │ Shopping / Receipts       567 emails  [Archive All ▾]     │   │
│ │ Social / Event Invites    398 emails  [Review     ▾]      │   │
│ │ Finance / Statements      289 emails  [Keep       ▾]      │   │
│ │ Promotions / Deals      3,456 emails  [Delete All ▾]      │   │
│ │ Notifications / Alerts  5,891 emails  [Archive Old ▾]     │   │
│ └───────────────────────────────────────────────────────────┘   │
│                                                                 │
│ ── ARCHIVE STRATEGY ────────────────────────────────────────── │
│ │ How should Emailibrium handle new emails?                  │   │
│ │                                                           │   │
│ │ ● Instant archive                                         │   │
│ │   Tag + archive immediately. True zero inbox.             │   │
│ │   Best if Emailibrium is your only email app.             │   │
│ │                                                           │   │
│ │ ○ Delayed archive (60 sec)                                │   │
│ │   Archive after 1 min. Mobile Gmail notifications work.   │   │
│ │                                                           │   │
│ │ ○ Manual                                                  │   │
│ │   Never auto-archive. You mark "Done" yourself.           │   │
│ │                                                           │   │
│ │ (Can be changed per-account in Settings)                   │   │
│ └───────────────────────────────────────────────────────────┘   │
│                                                                 │
│ [Execute Cleanup →]                                             │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ STEP 4: EXECUTION & RULES (8:00 - 10:00)                        │
│                                                                 │
│ ┌── Cleanup Progress ──────────────────────────────────────┐    │
│ │ Unsubscribing... 54/72 ████████████████░░░░ 75%          │    │
│ │ Archiving...   4,200/5,200 ████████████████░ 81%         │    │
│ │ Deleting...    2,892/3,456 ██████████████░░░ 84%         │    │
│ └──────────────────────────────────────────────────────────┘    │
│                                                                 │
│ "Want to keep your inbox clean automatically?"                  │
│                                                                 │
│ Suggested rules (apply across all accounts):                    │
│ ☑ Auto-archive promotional emails older than 7 days             │
│ ☑ Auto-label GitHub notifications as "Dev"                      │
│ ☑ Move receipts to "Finance" group                              │
│ ☐ Auto-delete marketing emails after 30 days                    │
│                                                                 │
│ [Save Rules & Finish →]                                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ STEP 5: COMMAND CENTER (ongoing)                                │
│                                                                 │
│ "Inbox zero achieved! 21,279 → 2,103 actionable emails"        │
│                                                                 │
│ [Search anything...]  ← Searches across ALL accounts            │
│                                                                 │
│ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐                  │
│ │2,103 │ │ 72   │ │ 9    │ │ 4    │ │ 2    │                  │
│ │inbox │ │unsub'd│ │groups│ │rules │ │accts │                  │
│ └──────┘ └──────┘ └──────┘ └──────┘ └──────┘                  │
└─────────────────────────────────────────────────────────────────┘
```

### 9.1.1 Journey: Add Another Account (Post-Onboarding)

```text
User clicks [➕ Add Account] from Command Center or Settings
  │
  ▼
┌─────────────────────────────────────────────────────────────────┐
│ ADD EMAIL ACCOUNT                                               │
│                                                                 │
│ Connected accounts:                                             │
│ ✓ user@gmail.com        Gmail    12,847 emails  ● Active       │
│ ✓ user@company.com      Outlook   8,432 emails  ● Active       │
│                                                                 │
│ Add a new account:                                              │
│ [+ Gmail]  [+ Outlook]  [+ IMAP]  [+ Other]                   │
│                                                                 │
│ ── Options for new account ──────────────────────────────────  │
│ │ Archive strategy: [Same as other accounts ▾]               │  │
│ │ Sync depth:       [All emails ▾]   (or Last 30/90/365 days)│  │
│ │ Add to unified inbox: [Yes ▾]                              │  │
│ └────────────────────────────────────────────────────────────┘  │
│                                                                 │
│ After connecting, new account syncs in background.              │
│ Emails appear in unified inbox as they're processed.            │
│ No downtime — keep using Emailibrium while syncing.            │
└─────────────────────────────────────────────────────────────────┘
```

### 9.1.2 Unified Inbox Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│ HOW MULTI-ACCOUNT WORKS                                         │
│                                                                 │
│ ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│ │ Gmail       │  │ Outlook     │  │ IMAP        │             │
│ │ (OAuth)     │  │ (OAuth)     │  │ (Creds)     │             │
│ └──────┬──────┘  └──────┬──────┘  └──────┬──────┘             │
│        │                │                │                     │
│        └────────────────┴────────────────┘                     │
│                         │                                      │
│               ┌─────────┴──────────┐                           │
│               │ Unified Ingestion  │                           │
│               │ Pipeline           │                           │
│               │ (per-account sync  │                           │
│               │  threads, shared   │                           │
│               │  vector store)     │                           │
│               └─────────┬──────────┘                           │
│                         │                                      │
│            ┌────────────┴────────────┐                         │
│            │ Single RuVector Index   │                         │
│            │ (all accounts merged)   │                         │
│            │ account_id as metadata  │                         │
│            │ filter for cross-acct   │                         │
│            │ or per-acct search      │                         │
│            └────────────┬────────────┘                         │
│                         │                                      │
│         ┌───────────────┴───────────────┐                     │
│         │ Unified UI                     │                     │
│         │ ┌─────────────────────────┐   │                     │
│         │ │ All Accounts (default)  │   │                     │
│         │ │ • Merged inbox          │   │                     │
│         │ │ • Cross-account search  │   │                     │
│         │ │ • Shared topic groups   │   │                     │
│         │ │ • Account badge on each │   │                     │
│         │ │   email (G/M/I icon)    │   │                     │
│         │ └─────────────────────────┘   │                     │
│         │ ┌─────────────────────────┐   │                     │
│         │ │ Per-Account View        │   │                     │
│         │ │ • Filter to one account │   │                     │
│         │ │ • Account-specific stats│   │                     │
│         │ │ • Account-specific rules│   │                     │
│         │ └─────────────────────────┘   │                     │
│         └───────────────────────────────┘                     │
└─────────────────────────────────────────────────────────────────┘

Actions route to the correct provider automatically:
  Reply to Gmail email → Gmail API messages.send
  Reply to Outlook email → Graph API sendMail
  Reply to IMAP email → SMTP send
  Archive Gmail email → removeLabelIds: ["INBOX"]
  Archive Outlook email → move to Archive folder
  Archive IMAP email → MOVE to Archive mailbox
```

### 9.1.3 Account Switcher & Settings

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings      │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│ ⚙ SETTINGS → Accounts                                               │
│ [General] [Accounts] [AI/LLM] [Privacy] [Appearance]                │
│                                                                      │
│ ┌─ Connected Accounts ───────────────────────────────────────────┐  │
│ │                                                                 │  │
│ │  G  user@gmail.com                                    ● Active │  │
│ │     Gmail • OAuth • 12,847 emails • Last sync: 2 min ago       │  │
│ │     Archive: Instant • Labels: EM/* (14 labels created)        │  │
│ │     [Sync Now] [Edit] [Disconnect]                             │  │
│ │                                                                 │  │
│ │  M  user@company.com                                  ● Active │  │
│ │     Outlook • OAuth • 8,432 emails • Last sync: 5 min ago     │  │
│ │     Archive: Delayed (60s) • Categories: EM/* (14 created)     │  │
│ │     [Sync Now] [Edit] [Disconnect]                             │  │
│ │                                                                 │  │
│ │  ✉  admin@mysite.com                              ● Active     │  │
│ │     IMAP (imap.fastmail.com:993) • 2,341 emails                │  │
│ │     Archive: Manual • Last sync: 10 min ago                    │  │
│ │     [Sync Now] [Edit] [Disconnect]                             │  │
│ │                                                                 │  │
│ │  [+ Add Account]                                                │  │
│ │                                                                 │  │
│ └─────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│ ┌─ Per-Account Settings (user@gmail.com) ────────────────────────┐  │
│ │                                                                 │  │
│ │  Archive strategy:  [Instant ▾]                                │  │
│ │  Sync frequency:    [Real-time (push) ▾]                       │  │
│ │  Sync depth:        [All emails ▾]                             │  │
│ │  Label prefix:      [EM/ ▾]                                    │  │
│ │  Include in unified inbox: [Yes ▾]                             │  │
│ │  Send from this account by default: [No ▾]                    │  │
│ │                                                                 │  │
│ │  ── Danger Zone ──                                              │  │
│ │  [Remove all Emailibrium labels from Gmail]                    │  │
│ │  [Unarchive all emails (restore to INBOX)]                     │  │
│ │  [Disconnect & delete local data]                              │  │
│ │                                                                 │  │
│ └─────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│ ┌─ Default Compose Account ──────────────────────────────────────┐  │
│ │ When composing a new email, send from:                          │  │
│ │ ● user@gmail.com (Gmail)                                       │  │
│ │ ○ user@company.com (Outlook)                                   │  │
│ │ ○ admin@mysite.com (IMAP/Fastmail)                             │  │
│ │ (When replying, always use the account that received the email) │  │
│ └─────────────────────────────────────────────────────────────────┘  │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ Command Center   📧 Email   📥 Inbox Cleaner   📊 Insights      │
└──────────────────────────────────────────────────────────────────────┘
```

### 9.2 Journey: Power Search

```text
User: Types "quarterly budget discussions from finance team last year"

Command Center processes:
  1. Embeds query → 384D vector
  2. HNSW search → top 100 semantic matches
  3. FTS5 search → "quarterly" AND "budget" matches
  4. RRF fusion → combined ranking
  5. Filter: date_range=last_year, from_domain=company.com
  6. SONA rerank based on user's past search behavior

Results appear in < 100ms:
  ┌─────────────────────────────────────────────────────────┐
  │ "quarterly budget discussions from finance team..."     │
  │ 47 results (23 semantic, 18 keyword, 6 both)            │
  │                                                         │
  │ 1. Q4 Budget Review - Final Numbers                     │
  │    From: CFO <cfo@company.com> • Nov 2025               │
  │    "...approved the quarterly budget allocation..."     │
  │    Relevance: 0.94 (semantic match)                     │
  │                                                         │
  │ 2. RE: Budget Planning for Q1 2026                      │
  │    From: Finance <finance@company.com> • Dec 2025       │
  │    "...quarterly projections attached..."               │
  │    Relevance: 0.91 (hybrid match)                       │
  │                                                         │
  │ Related clusters: [Budget & Finance] [Q4 Planning]      │
  │ Similar searches: "expense reports", "annual review"    │
  └─────────────────────────────────────────────────────────┘
```

### 9.3 Journey: Subscription Management

```text
User navigates to Insights → Subscriptions tab

  ┌─────────────────────────────────────────────────────────┐
  │ SUBSCRIPTION INTELLIGENCE                               │
  │                                                         │
  │ 47 active subscriptions • 891 emails/month              │
  │ Estimated reading time: 14.2 hours/month                │
  │                                                         │
  │ ── Never Opened (12) ──────────────────────────────────│
  │ ☑ marketing@saas-tool.com     Weekly   0% opened   [×] │
  │ ☑ promo@retailer.com          Daily    0% opened   [×] │
  │ ...                                                     │
  │ [Unsubscribe All Never-Opened →]                        │
  │                                                         │
  │ ── Rarely Opened (<10%) (18) ──────────────────────────│
  │ ☐ digest@community.org        Weekly   8% opened   [▾] │
  │ ...                                                     │
  │                                                         │
  │ ── Regularly Opened (>50%) (17) ───────────────────────│
  │ ☐ newsletter@favorite.com     Weekly  89% opened   [✓] │
  │ ...                                                     │
  │                                                         │
  │ Trend: +3 new subscriptions this month                  │
  │ Suggestion: "You could save ~8 hours/month"             │
  └─────────────────────────────────────────────────────────┘
```

### 9.4 Journey: Conversational Rule Building (Existing, Enhanced)

````text
User opens Chat from Command Center

  User: "I keep getting pull request notifications from archived repos.
         Can you help me filter those out?"

  Assistant: "I can see you have 234 GitHub notification emails. Let me
             analyze them semantically...

             I found 3 clusters:
             • Active repos (your-org/main-app, your-org/api) — 156 emails
             • Archived repos (old-project, deprecated-lib) — 67 emails
             • Bot/CI notifications — 11 emails

             Here's a rule to archive notifications from archived repos:

             ```yaml
             name: Archive Old Repo Notifications
             conditions:
               and:
                 - field: from
                   operator: contains
                   value: notifications@github.com
                 - field: subject
                   operator: matches
                   value: (old-project|deprecated-lib)
             actions:
               - type: archive
               - type: label
                 value: github/archived
             ```

             This would catch 67 existing emails. Apply it?"

  User: [Apply Rule] [Edit First] [Find Similar Patterns]
````

---

## 10. Wireframes

### 10.1 Command Center (Primary Screen)

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings       │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │  🔍  Search anything... (⌘K)                              ⚡   │  │
│  │                                                                │  │
│  │  Try: "invoices from last quarter"                             │  │
│  │       "unread from alice@company.com"                          │  │
│  │       "emails about project launch"                            │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐    │
│  │  12,847  │ │  1,203   │ │    47    │ │    6     │ │    4     │    │
│  │  Total   │ │  Inbox   │ │  Subs    │ │  Topics  │ │  Rules   │    │
│  │  Emails  │ │  Active  │ │  Found   │ │  Clusters│ │  Active  │    │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘    │
│                                                                      │
│  ┌─ Quick Actions ───────────────────────────────────────────────┐   │
│  │ [🧹 Clean Inbox]  [📊 View Insights]  [💬 Chat with AI]      │   │
│  │ [📋 Manage Rules]  [➕ Add Account]   [🔄 Sync Now]          │   │
│  └───────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌─ Recent Activity ─────────────────────────────────────────────┐   │
│  │                                                               │   │
│  │  ● 3 new emails from alice@company.com         2 min ago      │   │
│  │  ● Rule "Archive Promos" matched 12 emails     15 min ago     │   │
│  │  ● Subscription detected: weekly@newsletter    1 hour ago     │   │
│  │  ● Ingestion complete: 500 new emails embedded  2 hours ago   │   │
│  │                                                               │   │
│  └───────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌─ Topic Clusters ──────────────────────────────────────────────┐   │
│  │                                                               │   │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐              │   │
│  │  │ Work        │ │ Shopping    │ │ Social      │              │   │
│  │  │ 3,456 emails│ │ 1,234 emails│ │ 567 emails  │              │   │
│  │  │ ██████████ │ │ ████████   │ │ █████         │              │   │
│  │  └─────────────┘ └─────────────┘ └─────────────┘              │   │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐              │   │
│  │  │ Finance     │ │ Promotions  │ │ Alerts      │              │   │
│  │  │ 189 emails  │ │ 2,456 emails│ │ 3,891 emails│              │   │
│  │  │ ███        │ │ █████████  │ │ ██████████    │              │   │
│  │  └─────────────┘ └─────────────┘ └─────────────┘              │   │
│  │                                                               │   │
│  └───────────────────────────────────────────────────────────────┘   │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ Command Center   📥 Inbox Cleaner   📊 Insights   📋 Rules       │
└──────────────────────────────────────────────────────────────────────┘
```

### 10.2 Semantic Search Results

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings       │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │  🔍  quarterly budget from finance team  ✕               ⚡    │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  47 results • 23ms • Mode: Hybrid (semantic + keyword)               │
│  [Semantic ●] [Keyword ○] [Hybrid ●]  Sort: [Relevance ▾]            │
│                                                                      │
│  ┌─ Filters ──┐  ┌─ Results ────────────────────────────────────┐   │
│  │            │  │                                               │   │
│  │ Date       │  │  ┌─────────────────────────────────────────┐ │   │
│  │ [Last Year]│  │  │ ★ Q4 Budget Review - Final Numbers      │ │   │
│  │            │  │  │ From: Sarah Chen <cfo@company.com>       │ │   │
│  │ Sender     │  │  │ Nov 15, 2025 • Work • ■■■■■■■■■■ 0.94  │ │   │
│  │ [Any    ▾] │  │  │ "...approved the quarterly budget        │ │   │
│  │            │  │  │  allocation for engineering..."           │ │   │
│  │ Category   │  │  │ [Open] [Archive] [Similar]               │ │   │
│  │ ☑ Work    │  │  └─────────────────────────────────────────┘ │   │
│  │ ☐ Personal│  │                                               │   │
│  │ ☐ Finance │  │  ┌─────────────────────────────────────────┐ │   │
│  │            │  │  │ RE: Budget Planning for Q1 2026          │ │   │
│  │ Labels     │  │  │ From: Finance Team <finance@co.com>      │ │   │
│  │ [Select ▾] │  │  │ Dec 3, 2025 • Work • ■■■■■■■■■░ 0.91   │ │   │
│  │            │  │  │ "...quarterly projections attached        │ │   │
│  │ Has Attach │  │  │  for board review..."                    │ │   │
│  │ ☐ Yes     │  │  │ [Open] [Archive] [Similar]  📎 2 files   │ │   │
│  │            │  │  └─────────────────────────────────────────┘ │   │
│  │ Read State │  │                                               │   │
│  │ ○ All     │  │  ┌─────────────────────────────────────────┐ │   │
│  │ ○ Unread  │  │  │ FY2025 Budget Summary                    │ │   │
│  │ ○ Read    │  │  │ From: Mike Torres <accounting@co.com>    │ │   │
│  │            │  │  │ Oct 28, 2025 • Finance • ■■■■■■■■░░ 0.87│ │   │
│  │ Attach Type│  │  │ "...annual budget performance vs         │ │   │
│  │ ☐ PDF    │  │  │  quarterly targets..."                   │ │   │
│  │ ☐ DOCX   │  │  │ [Open] [Archive] [Similar]  📎 1 file    │ │   │
│  │ ☐ XLSX   │  │  └─────────────────────────────────────────┘ │   │
│  │ ☐ Image  │  │                                               │   │
│  │            │  │                                               │   │
│  │ Match In   │  │  ┌─ Related ──────────────────────────────┐  │   │
│  │ ☑ Body    │  │  │ Clusters: [Budget & Finance] [Q4 Plan] │  │   │
│  │ ☑ Attach  │  │  │ Also try: "expense reports" "forecasts" │  │   │
│  │ ☐ Images  │  │  │ Matched in: body (23), attachments (8), │  │   │
│  │ ☐ URLs    │  │  │   images (3)                             │  │   │
│  │            │  │                                               │   │
│  │ Cluster    │  │                                               │   │
│  │ [Budget ▾] │  │                                               │   │
│                  │  └────────────────────────────────────────┘  │   │
│                  │                                               │   │
│                  │  Showing 1-10 of 47  [Load More ↓]           │   │
│                  └───────────────────────────────────────────────┘   │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ Command Center   📥 Inbox Cleaner   📊 Insights   📋 Rules      │
└──────────────────────────────────────────────────────────────────────┘
```

### 10.3 Inbox Cleaner Wizard

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings      │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  🧹 INBOX CLEANER                                    Step 2 of 4    │
│  ━━━━━━━━━━━━━━━━●━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━    │
│  Connect ✓    Review Subs    Clean Topics    Set Rules               │
│                                                                      │
│  ┌─ Subscriptions Review ────────────────────────────────────────┐  │
│  │                                                               │  │
│  │  47 subscriptions found • 891 emails/month                    │  │
│  │  "You spend ~14 hours/month on subscription emails"           │  │
│  │                                                               │  │
│  │  ┌─ NEVER OPENED (12) ─── Recommended: Unsubscribe ────────┐│  │
│  │  │                                                          ││  │
│  │  │ ☑ marketing@saas.com    Weekly  │ 142 emails │ 0% read  ││  │
│  │  │ ☑ promo@store.com       Daily   │ 365 emails │ 0% read  ││  │
│  │  │ ☑ news@oldsite.com      Monthly │  24 emails │ 0% read  ││  │
│  │  │ ☑ alerts@service.io     Daily   │ 730 emails │ 0% read  ││  │
│  │  │ ... 8 more                                               ││  │
│  │  │                                                          ││  │
│  │  │ [☑ Select All 12]                  Est. save: 6 hrs/mo  ││  │
│  │  └──────────────────────────────────────────────────────────┘│  │
│  │                                                               │  │
│  │  ┌─ RARELY OPENED <10% (18) ─── Review suggested ──────────┐│  │
│  │  │                                                          ││  │
│  │  │ ☐ digest@forum.org      Weekly  │  52 emails │ 4% read  ││  │
│  │  │ ☑ deals@retailer.com    Daily   │ 365 emails │ 2% read  ││  │
│  │  │ ☐ update@tool.dev       Weekly  │  48 emails │ 8% read  ││  │
│  │  │ ... 15 more                                              ││  │
│  │  │                                                          ││  │
│  │  │ [☑ Select Suggested (11)]          Est. save: 5 hrs/mo  ││  │
│  │  └──────────────────────────────────────────────────────────┘│  │
│  │                                                               │  │
│  │  ┌─ REGULARLY OPENED >50% (17) ─── Keep ───────────────────┐│  │
│  │  │                                                          ││  │
│  │  │ ✓ newsletter@fav.com    Weekly  │  52 emails │ 89% read ││  │
│  │  │ ✓ updates@work.com      Daily   │ 260 emails │ 72% read ││  │
│  │  │ ... 15 more                                              ││  │
│  │  └──────────────────────────────────────────────────────────┘│  │
│  │                                                               │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌─ Summary ─────────────────────────────────────────────────────┐  │
│  │ 23 subscriptions selected for unsubscribe                     │  │
│  │ Estimated time saved: 11 hours/month                          │  │
│  │ Emails to clean: 2,847                                        │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  [← Back]                                      [Next: Clean Topics →]│
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ Command Center   📥 Inbox Cleaner   📊 Insights   📋 Rules      │
└──────────────────────────────────────────────────────────────────────┘
```

### 10.4 Insights Explorer

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings      │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  📊 INSIGHTS EXPLORER                                                │
│  [Overview] [Subscriptions] [Senders] [Topics] [Trends]             │
│                                                                      │
│  ┌─ Inbox Health Score ──────────────────────────────────────────┐  │
│  │                                                               │  │
│  │       ┌──────┐                                                │  │
│  │       │  72  │  Good — but 47 unused subscriptions detected   │  │
│  │       │ /100 │  [Improve Score →]                             │  │
│  │       └──────┘                                                │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌─ Category Breakdown ──┐  ┌─ Email Volume (30 days) ───────────┐  │
│  │                       │  │                                     │  │
│  │    ┌─────┐            │  │  120│    ╱╲                         │  │
│  │   ╱ Work  ╲  27%      │  │     │   ╱  ╲  ╱╲                   │  │
│  │  ╱─────────╲          │  │   80│  ╱    ╲╱  ╲    ╱╲            │  │
│  │ │ Promos    │ 19%     │  │     │ ╱          ╲  ╱  ╲           │  │
│  │ │ Alerts    │ 30%     │  │   40│╱            ╲╱    ╲          │  │
│  │ │ Personal  │  4%     │  │     │                     ╲         │  │
│  │ │ Newsletter│ 12%     │  │    0└──────────────────────────     │  │
│  │ │ Finance   │  8%     │  │      Mar 1         Mar 15    Mar 23│  │
│  │                       │  │                                     │  │
│  │  12,847 total emails  │  │  ── Received  ── Processed         │  │
│  └───────────────────────┘  └─────────────────────────────────────┘  │
│                                                                      │
│  ┌─ Top Senders ─────────────────────────────────────────────────┐  │
│  │                                                               │  │
│  │  1. notifications@github.com    892 ██████████████████████   │  │
│  │  2. noreply@slack.com           567 ██████████████           │  │
│  │  3. updates@jira.atlassian.com  445 ██████████               │  │
│  │  4. calendar@google.com         312 ████████                 │  │
│  │  5. alice@company.com           234 ██████                   │  │
│  │  6. deals@amazon.com            198 █████                    │  │
│  │  7. team@linear.app             187 █████                    │  │
│  │  8. bob@company.com             156 ████                     │  │
│  │                                                               │  │
│  │  [View All Senders →]                                         │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌─ Recurring Patterns ──────────────────────────────────────────┐  │
│  │                                                               │  │
│  │  ⏰ Daily:    GitHub notifications, Slack digests, Calendar   │  │
│  │  📅 Weekly:   Team standup summary, Newsletter roundup        │  │
│  │  📆 Monthly:  Bank statements, SaaS invoices, Usage reports   │  │
│  │  🔄 Irregular: PR reviews (avg 3.2/day), Client emails       │  │
│  │                                                               │  │
│  │  Insight: "Notifications account for 42% of your inbox.       │  │
│  │   Consider batching GitHub + Slack into a daily digest."      │  │
│  │  [Create Digest Rule →]                                       │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ Command Center   📥 Inbox Cleaner   📊 Insights   📋 Rules      │
└──────────────────────────────────────────────────────────────────────┘
```

### 10.5 Ingestion Progress (Real-Time SSE)

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings      │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ⚡ ANALYZING YOUR INBOX                           ETA: 2m 34s      │
│                                                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                                                               │  │
│  │  Phase 1: Sync Emails                                         │  │
│  │  ████████████████████████████████████████ 100%  ✓ Complete    │  │
│  │  12,847 emails synced from Gmail                              │  │
│  │                                                               │  │
│  │  Phase 2: Understanding Content                               │  │
│  │  ████████████████████████████░░░░░░░░░░░  71%  ⟳ Running     │  │
│  │  9,121 / 12,847 embedded • 523 emails/sec                    │  │
│  │                                                               │  │
│  │  Phase 3: Categorizing                                        │  │
│  │  ████████████████████████░░░░░░░░░░░░░░░  59%  ⟳ Running     │  │
│  │  7,589 / 12,847 categorized • 0.4ms avg                     │  │
│  │                                                               │  │
│  │  Phase 4: Discovering Topics                                  │  │
│  │  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   0%  ◯ Pending     │  │
│  │                                                               │  │
│  │  Phase 5: Detecting Patterns                                  │  │
│  │  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░   0%  ◯ Pending     │  │
│  │                                                               │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ┌─ Live Discoveries ───────────────────────────────────────────┐   │
│  │                                                               │   │
│  │  ✨ 6 topic clusters forming...                               │   │
│  │     Work (3,456) • Shopping (1,234) • Social (567)            │   │
│  │     Finance (189) • Promotions (2,456) • Alerts (3,891)       │   │
│  │                                                               │   │
│  │  📬 47 subscriptions detected so far                          │   │
│  │     12 never opened • 18 rarely opened • 17 active            │   │
│  │                                                               │   │
│  │  🏷️  Top categories: Alerts 30% • Work 27% • Promos 19%      │   │
│  │                                                               │   │
│  └───────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  [⏸ Pause]  [Cancel]                                                │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ Command Center   📥 Inbox Cleaner   📊 Insights   📋 Rules      │
└──────────────────────────────────────────────────────────────────────┘
```

### 10.6 Email Client — Thread View

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings      │
├──────────┬───────────────────────────────────────────────────────────┤
│          │                                                           │
│ GROUPS   │  ← Inbox / Work                                          │
│          │                                                           │
│ ● Inbox  │  Q4 Budget Review - Final Numbers                        │
│   (12)   │  ─────────────────────────────────────────────────────── │
│          │                                                           │
│ Work     │  Sarah Chen <cfo@company.com>          Nov 15, 10:30 AM  │
│   (156)  │  To: team@company.com                                    │
│ Personal │  ┌───────────────────────────────────────────────────┐   │
│   (23)   │  │ Hi team,                                          │   │
│ Newsletters│  │                                                   │   │
│   (47)   │  │ I'm pleased to share that we've approved the      │   │
│ Finance  │  │ quarterly budget allocation for engineering.       │   │
│   (12)   │  │                                                   │   │
│ ── Topics──│  │ Key highlights:                                   │   │
│ Project  │  │ • Engineering: $2.4M (+15% YoY)                   │   │
│  Alpha   │  │ • Infrastructure: $800K                           │   │
│   (34)   │  │ • R&D: $600K                                      │   │
│ Q4       │  │                                                   │   │
│  Planning│  │ See attached spreadsheet for details.              │   │
│   (18)   │  │                                                   │   │
│ Client   │  │ Best,                                              │   │
│  Reviews │  │ Sarah                                              │   │
│   (9)    │  └───────────────────────────────────────────────────┘   │
│          │                                                           │
│ ── Subs ──│  📎 Attachments:                                        │
│ GitHub   │  ┌──────────────────┐ ┌──────────────────┐              │
│  (892)   │  │ 📊 Q4-Budget.xlsx │ │ 📄 Summary.pdf   │              │
│ Slack    │  │ 245 KB • XLSX    │ │ 89 KB • PDF      │              │
│  (234)   │  └──────────────────┘ └──────────────────┘              │
│ Jira     │                                                           │
│  (445)   │  🔗 Links: 3 found (2 internal, 1 external)             │
│          │                                                           │
│          │  ── Thread (4 messages) ──────────────────────────────── │
│          │                                                           │
│          │  You replied • Nov 15, 2:15 PM                           │
│          │  "Thanks Sarah, the engineering allocation looks..."      │
│          │                                                           │
│          │  Mike Torres • Nov 16, 9:00 AM                           │
│          │  "I've updated the tracking spreadsheet with..."          │
│          │                                                           │
│          │  ┌─────────────────────────────────────────────────────┐ │
│          │  │ Reply...                                 [Send ⌘↩] │ │
│          │  │                                                     │ │
│          │  │ [📎 Attach] [Bold] [Italic] [Link] [• List]       │ │
│          │  └─────────────────────────────────────────────────────┘ │
│          │                                                           │
│          │  [✓ Done]  [⭐ Star]  [🏷 Reclassify ▾]  [📋 Move ▾]  │
│          │                                                           │
├──────────┴───────────────────────────────────────────────────────────┤
│  ◉ Command Center   📧 Email   📥 Inbox Cleaner   📊 Insights      │
└──────────────────────────────────────────────────────────────────────┘
```

### 10.7 Rules Studio (Enhanced)

```text
┌──────────────────────────────────────────────────────────────────────┐
│ ◉ Emailibrium                    ☰ Menu    🔔 3    ⚙ Settings      │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  📋 RULES STUDIO                                                     │
│  [Active Rules] [Templates] [AI Suggestions] [Metrics]              │
│                                                                      │
│  ── AI-Suggested Rules (based on your cleanup patterns) ──────────  │
│  │                                                               │  │
│  │  💡 "Auto-archive GitHub notifications older than 3 days"     │  │
│  │     Based on: You archive 89% of GitHub emails after 3 days   │  │
│  │     Would match: ~267 emails/month                            │  │
│  │     [Accept] [Customize] [Dismiss]                            │  │
│  │                                                               │  │
│  │  💡 "Label finance emails from banks automatically"            │  │
│  │     Based on: You always label bank emails as "Finance"       │  │
│  │     Would match: ~24 emails/month                             │  │
│  │     [Accept] [Customize] [Dismiss]                            │  │
│  │                                                               │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ── Active Rules (6) ─────────────────────────────────────────────  │
│  │                                                               │  │
│  │  Rule                      Matches  Accuracy  Status          │  │
│  │  ─────────────────────────────────────────────────────────    │  │
│  │  Archive Old Promos          2,456    99.2%   ● Active        │  │
│  │  Label GitHub as Dev           892    98.7%   ● Active        │  │
│  │  Move Receipts to Finance      189    97.3%   ● Active        │  │
│  │  Auto-read CI notifications    445    99.8%   ● Active        │  │
│  │  Flag urgent from boss          34   100.0%   ● Active        │  │
│  │  Archive newsletters > 7d      312    96.1%   ● Active        │  │
│  │                                                               │  │
│  │  [+ New Rule]  [💬 Build with AI]  [📊 View Metrics]         │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  ── NEW: Semantic Rule Conditions ────────────────────────────────  │
│  │                                                               │  │
│  │  You can now create rules with semantic conditions:            │  │
│  │                                                               │  │
│  │  • "Emails similar to [selected email]"                       │  │
│  │  • "Emails about [topic/description]"                         │  │
│  │  • "Emails from [cluster name]"                               │  │
│  │                                                               │  │
│  │  These use RuVector to match by meaning, not just keywords.   │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ Command Center   📥 Inbox Cleaner   📊 Insights   📋 Rules      │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 11. Feature Mapping (Current + Planned)

### 11.1 Existing Features → Reimagined

| Feature ID | Feature                       | Current Status | Reimagined Enhancement                 |
| ---------- | ----------------------------- | -------------- | -------------------------------------- |
| FEAT-001   | Local Processing Architecture | ✅ Complete    | + RuVector local vector store          |
| FEAT-002   | Rule Definition & Validation  | ✅ Complete    | + Semantic rule conditions             |
| FEAT-003   | Email Attribute Extraction    | ✅ Complete    | + Vector embedding extraction          |
| FEAT-004   | Rule Processing Engine        | ✅ Complete    | + Vector-enhanced matching             |
| FEAT-005   | Gmail Integration             | ✅ Complete    | + Batch embedding on sync              |
| FEAT-006   | Outlook Integration           | ✅ Complete    | + Batch embedding on sync              |
| FEAT-007   | IMAP/POP3 Support             | ✅ Complete    | + Batch embedding on sync              |
| FEAT-008   | Rule Management UI            | ✅ Complete    | → Rules Studio with AI suggestions     |
| FEAT-009   | Email Dashboard               | ✅ Complete    | → Command Center + Insights Explorer   |
| FEAT-010   | Local LLM Rule Generation     | ✅ Complete    | + RuvLLM as additional local engine    |
| FEAT-011   | Conversational Rule Building  | ✅ Complete    | + Semantic context (clusters, similar) |
| FEAT-012   | Cloud LLM Integration         | 🔄 In Progress | + Cloud embedding fallback             |
| FEAT-013   | Intelligent Bulk Unsubscribe  | 🔄 In Progress | → Subscription Intelligence            |
| FEAT-014   | Historical Email Processing   | 🔄 In Progress | → Ingestion Pipeline                   |
| FEAT-015   | Smart Categorization          | 🔄 In Progress | → Vector centroid categorization       |
| FEAT-016   | Time-Based Rules              | 📋 Planned     | Unchanged                              |
| FEAT-017   | Rule Templates                | 📋 Planned     | + AI-generated templates from patterns |
| FEAT-030   | OAuth Infrastructure          | ✅ Complete    | Unchanged                              |

### 11.2 New Features (RuVector-Powered)

| Feature ID | Feature                             | Priority | Description                                                                                                                                                                                          |
| ---------- | ----------------------------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| FEAT-050   | Semantic Hybrid Search              | P0       | FTS5 + vector fusion search                                                                                                                                                                          |
| FEAT-051   | Vector Embedding Pipeline           | P0       | Batch + streaming email embedding                                                                                                                                                                    |
| FEAT-052   | Inbox Cleaner Wizard                | P0       | Guided 10-min cleanup flow                                                                                                                                                                           |
| FEAT-053   | Subscription Intelligence           | P0       | Auto-detect & manage subscriptions                                                                                                                                                                   |
| FEAT-054   | Topic Clustering                    | P0       | GNN-based email topic discovery                                                                                                                                                                      |
| FEAT-055   | Ingestion Pipeline (SSE)            | P0       | Real-time progress streaming                                                                                                                                                                         |
| FEAT-056   | Command Center                      | P1       | Unified search + action hub                                                                                                                                                                          |
| FEAT-057   | Insights Explorer                   | P1       | Analytics & pattern visualization                                                                                                                                                                    |
| FEAT-058   | SONA Self-Learning                  | P1       | Search & categorization improvement                                                                                                                                                                  |
| FEAT-059   | Semantic Rule Conditions            | P1       | "Emails similar to X" rules                                                                                                                                                                          |
| FEAT-060   | AI Rule Suggestions                 | P2       | Pattern-based rule recommendations                                                                                                                                                                   |
| FEAT-061   | Inbox Health Score                  | P2       | Gamified inbox quality metric                                                                                                                                                                        |
| FEAT-062   | Conversation Threading              | P2       | Vector proximity thread detection                                                                                                                                                                    |
| FEAT-063   | Smart Digest Creation               | P3       | Auto-bundle similar emails                                                                                                                                                                           |
| FEAT-064   | Image & Visual Content Analysis     | P1       | OCR + multimodal LLM on inline images and image attachments; CLIP embeddings for visual semantic search                                                                                              |
| FEAT-065   | Hyperlink Extraction & Intelligence | P1       | Extract all URLs from HTML bodies; resolve redirects; detect tracking pixels; classify link destinations; unsubscribe link discovery                                                                 |
| FEAT-066   | Attachment Content Extraction       | P1       | Magic-byte file type detection; extract text from PDF/DOCX/XLSX/PPTX; vectorize attachment content for search                                                                                        |
| FEAT-067   | Deep HTML Body Extraction           | P2       | Structured content extraction from newsletters/marketing; clean text from templated HTML; strip tracking/styling noise                                                                               |
| FEAT-068   | Multi-Asset Semantic Search         | P1       | Separate vector collections per asset type (body, image, attachment, URL); cross-collection search: "find receipts" matches image OCR + PDF text + subject line                                      |
| FEAT-069   | Full Email Client UI                | P0       | View, read, reply, forward, compose — complete Gmail/Outlook replacement in React. Thread view, rich text editor, attachment viewer                                                                  |
| FEAT-070   | Ingest → Tag → Archive Pipeline     | P0       | On sync: classify → apply Gmail labels → archive (remove INBOX). Instant zero inbox. Configurable: auto vs manual "Done"                                                                             |
| FEAT-071   | Dynamic Auto-Grouping               | P0       | Auto-group emails by topic/project using GNN clustering. Groups evolve as emails arrive. Stability guardrails (hysteresis, pinning, minimum age)                                                     |
| FEAT-072   | Continuous Learning System          | P0       | 3-tier SONA learning: instant (per-interaction), session (per-session patterns), long-term (hourly consolidation). Learns from explicit reclassification + implicit signals (opens, replies, stars)  |
| FEAT-073   | Periodic Re-Classification          | P1       | Background jobs (hourly + daily) re-evaluate classifications with updated models. Low-confidence emails re-classified. Category centroids recomputed from cumulative feedback                        |
| FEAT-074   | Gmail Push Notifications            | P1       | Gmail `watch()` + Pub/Sub for near-real-time new email notification. Incremental sync via `history.list()`. Eliminates polling                                                                       |
| FEAT-075   | Email Compose & Reply               | P0       | Send, reply, reply-all, forward via Gmail API `messages.send`. RFC 2822 MIME builder. Thread-aware with proper `In-Reply-To`/`References` headers                                                    |
| FEAT-076   | Multi-Account Onboarding            | P0       | Connect 1+ accounts (Gmail OAuth, Outlook OAuth, IMAP manual). Per-provider setup flows with presets. Unified inbox architecture.                                                                    |
| FEAT-077   | Account Management                  | P1       | Per-account settings (archive strategy, sync frequency, sync depth, label prefix). Add/remove accounts post-onboarding. Default compose account. Danger zone (disconnect, unarchive, remove labels). |
| FEAT-078   | Unified Inbox                       | P0       | All accounts merged into single searchable stream. Account badge on each email. Cross-account topic groups. Per-account filtering. Reply routes to correct provider automatically.                   |

### 11.3 Planned Features (Phase 3-4) — Updated

| Feature ID | Feature                     | Impact from RuVector                                   |
| ---------- | --------------------------- | ------------------------------------------------------ |
| FEAT-018   | Multi-Account Management    | Vector store is per-account, unified search across all |
| FEAT-019   | Cross-Account Sync          | Vector IDs enable cross-account similarity             |
| FEAT-020   | Email Pattern Analytics     | Replaced by Insights Explorer (FEAT-057)               |
| FEAT-021   | Automated Rule Optimization | Enhanced by SONA learning                              |
| FEAT-022   | Calendar Integration        | Semantic matching: "emails about meetings"             |
| FEAT-023   | API Integration             | Vector search exposed via REST API                     |
| FEAT-024   | iOS Mobile App              | React components shared via web build                  |
| FEAT-025   | Android Mobile App          | React components shared via web build                  |
| FEAT-026   | End-to-End Encryption       | Vector embeddings encrypted at rest                    |
| FEAT-027   | Compliance Features         | Audit trail for all vector operations                  |
| FEAT-028   | Email Content Understanding | Core RuVector capability                               |
| FEAT-029   | Predictive Email Management | SONA + GNN enables prediction                          |

---

## 12. Performance Targets

### 12.1 Benchmarks

| Operation                 | Target   | Mechanism                            |
| ------------------------- | -------- | ------------------------------------ |
| Single email embed        | < 5ms    | RuvLLM local inference (384D MiniLM) |
| Batch embed (500)         | < 1s     | Parallel batch with RuvLLM           |
| Vector search (100K)      | < 10ms   | HNSW index, ef_search=200            |
| Hybrid search             | < 50ms   | Parallel FTS5 + HNSW, RRF merge      |
| Full inbox ingest (10K)   | < 3 min  | 500 emails/sec pipeline              |
| Full inbox ingest (50K)   | < 10 min | With streaming progress              |
| Categorization (cached)   | < 1ms    | Vector centroid cosine distance      |
| Categorization (uncached) | < 10ms   | Embed + centroid lookup              |
| Clustering (10K)          | < 5s     | GNN on HNSW neighbor subgraph        |
| Subscription detection    | < 2s     | SQL aggregation + header scan        |

### 12.2 Memory Budget

| Component                             | 10K Emails | 100K Emails | 1M Emails  |
| ------------------------------------- | ---------- | ----------- | ---------- |
| SQLite DB                             | ~50MB      | ~500MB      | ~5GB       |
| Vector Store (384D, scalar quantized) | ~10MB      | ~100MB      | ~1GB       |
| HNSW Index                            | ~5MB       | ~50MB       | ~500MB     |
| Moka Cache                            | ~50MB      | ~50MB       | ~50MB      |
| **Total**                             | **~115MB** | **~700MB**  | **~6.5GB** |

### 12.3 Quantization Strategy

| Email Count | Quantization    | Memory | Search Quality                |
| ----------- | --------------- | ------ | ----------------------------- |
| < 50K       | None (fp32)     | ~75MB  | Best (baseline)               |
| 50K - 200K  | Scalar (int8)   | ~75MB  | 99.5% recall                  |
| 200K - 1M   | Product (PQ)    | ~75MB  | 98% recall                    |
| > 1M        | Binary + rerank | ~50MB  | 97% recall (top-100 reranked) |

---

## 13. Implementation Phases

### Phase 2.5: RuVector Foundation (Weeks 1-3)

**Week 1: Core Integration**

- [ ] Add ruvector-core to Cargo.toml
- [ ] Create `backend/src/vectors/` module structure
- [ ] Implement `VectorStore` wrapper (CRUD, persistence)
- [ ] Implement `EmbeddingPipeline` (RuvLLM → Ollama → Cloud fallback)
- [ ] SQLite migration: embedding status columns
- [ ] Basic unit tests for vector operations

**Week 2: Search & Ingestion**

- [ ] Implement `HybridSearch` (FTS5 + HNSW + RRF)
- [ ] Implement `IngestionPipeline` with batch embedding
- [ ] SSE endpoint for ingestion progress
- [ ] API endpoints: `/search`, `/search/semantic`, `/search/similar`
- [ ] Integration tests with real email data

**Week 3: Intelligence**

- [ ] Implement `InsightEngine` (subscription detection, patterns)
- [ ] Implement `ClusterEngine` (GNN topic discovery)
- [ ] Implement `VectorCategorizer` (centroid-based classification)
- [ ] API endpoints: `/insights/*`, `/clusters/*`
- [ ] Background job: batch embed existing emails

### Phase 2.6: React TypeScript Web UI (Weeks 4-6)

**Week 4: App Shell + Command Center**

- [ ] Scaffold `apps/web/` with Vite 8 + React 19 + TanStack Router
- [ ] Set up shadcn/ui + Radix primitives component library
- [ ] Implement REST API client + SSE EventSource streaming
- [ ] Implement secure storage (Web Crypto API + IndexedDB)
- [ ] Install cmdk, Recharts, date-fns
- [ ] Build Command Center (primary screen + command palette)
- [ ] Build semantic search UI with relevance scoring + filters
- [ ] Create types packages: search, vectors, insights, ingestion

**Week 5: Inbox Cleaner & Insights**

- [ ] Inbox Cleaner wizard (4-step flow with streaming progress)
- [ ] Subscription management UI (never-opened, rarely-opened, active)
- [ ] Insights Explorer with Recharts (category pie, volume line, sender bar)
- [ ] Ingestion progress screen with SSE real-time updates
- [ ] Bulk action execution with progress + undo
- [ ] Add PWA support (vite-plugin-pwa, manifest, service worker)

**Week 6: Rules Enhancement & Polish**

- [ ] Rules Studio with AI suggestions from vector patterns
- [ ] Semantic rule conditions UI ("emails similar to X")
- [ ] Enhanced chat with cluster/similarity context
- [ ] Responsive design pass (mobile-friendly)
- [ ] Accessibility audit (ARIA, keyboard nav, focus management)
- [ ] Performance optimization (virtualization, lazy loading, code splitting)
- [ ] Lighthouse CI performance budget gate

### Phase 2.7: Learning & Optimization (Weeks 7-8)

**Week 7: SONA & Feedback**

- [ ] SONA self-learning integration
- [ ] Search interaction tracking
- [ ] User feedback loop (relevant/irrelevant)
- [ ] Category centroid updates from feedback
- [ ] Quantization auto-selection based on email count

**Week 8: Testing & Hardening**

- [ ] End-to-end tests (ingestion → search → action)
- [ ] Performance benchmarks (criterion)
- [ ] Memory profiling under load
- [ ] Error handling & graceful degradation
- [ ] Documentation updates

---

## 14. Makefile Targets & Developer Workflow

The project already uses a **3-tier Makefile structure** (root → backend → frontend) with **pnpm** as the package manager. The reimagined plan preserves this pattern and extends it for the new RuVector + web app workflow.

### 14.1 Existing Makefile Structure (Preserved)

```text
Makefile              # Root orchestrator — delegates to backend/ and frontend/
├── backend/Makefile  # Rust: cargo build/test/lint/bench, sqlx migrations, coverage
└── frontend/Makefile # TypeScript: pnpm + turbo build/test/lint/typecheck, storybook
```

**Key existing targets** (all preserved):

- `make dev` / `make build` / `make test` — Full-stack orchestration
- `make ci` / `make lint` / `make fmt` — Quality gates
- `make test-backend` / `make test-frontend` — Component testing
- `make ci-run-*` — GitHub Actions workflow runners
- Backend: `make db-setup` / `make db-migrate` / `make sqlx-prepare`
- Frontend: `make dev-web` / `make build-web` / `make typecheck`
- Frontend already uses `PNPM := pnpm` and `TURBO := pnpm turbo`

### 14.2 New Makefile Targets (Added)

**Root Makefile additions:**

```makefile
# RuVector Development
dev-vectors:
 @cd $(BACKEND_DIR) && $(MAKE) dev-vectors

# Web app (replaces desktop as primary)
dev-web:
 @cd $(FRONTEND_DIR) && $(MAKE) dev-web

# Full stack: backend + web app + vectors
dev-full:
 @echo "$(GREEN)Starting full development environment...$(NC)"
 @$(DOCKER_COMPOSE) up -d redis
 @cd $(BACKEND_DIR) && $(MAKE) dev &
 @cd $(FRONTEND_DIR) && $(MAKE) dev-web &
 @echo "$(GREEN)Backend: http://localhost:8080  Frontend: http://localhost:3000$(NC)"

# Ingestion test (end-to-end vector pipeline)
test-ingestion:
 @cd $(BACKEND_DIR) && $(MAKE) test-vectors

# Verify all dependency versions
verify-deps:
 @echo "$(GREEN)Verifying dependency versions...$(NC)"
 @cd $(BACKEND_DIR) && $(MAKE) outdated-deps-direct
 @cd $(FRONTEND_DIR) && $(PNPM) outdated -r
 @echo "$(GREEN)✅ Dependency check complete$(NC)"
```

**Backend Makefile additions:**

```makefile
# RuVector-specific targets
dev-vectors:
 @echo "Starting backend with vector features enabled..."
 @$(CARGO) watch -x 'run --features vectors' -w src

test-vectors:
 @echo "Running vector integration tests..."
 @$(CARGO) test --features test-vectors -- vectors::

bench-vectors:
 @echo "Running vector performance benchmarks..."
 @$(CARGO) bench --features vectors -- vectors

# Embedding pipeline
embed-existing:
 @echo "Batch embedding existing emails..."
 @$(CARGO) run --features vectors -- embed --batch-size 500

# Database with vector schema
db-migrate-vectors:
 @$(SQLX) migrate run
 @echo "✅ Vector schema migration complete"
```

**Frontend Makefile additions:**

```makefile
# Web app targets (primary — replaces desktop)
dev-web:
 @$(TURBO) dev --filter=@emailibrium/web

build-web:
 @$(TURBO) build --filter=@emailibrium/web

test-web:
 @$(TURBO) test --filter=@emailibrium/web

test-web-e2e:
 @$(PNPM) --filter=@emailibrium/web test:e2e

# PWA build
build-pwa:
 @NODE_ENV=production $(TURBO) build --filter=@emailibrium/web
 @echo "✅ PWA build ready in apps/web/dist"

# Storybook for new shadcn/ui components
storybook-web:
 @$(PNPM) --filter=@emailibrium/web storybook

# PWA build
build-pwa:
 @NODE_ENV=production $(TURBO) build --filter=@emailibrium/web
 @echo "✅ PWA build ready in apps/web/dist"
```

### 14.3 Developer Workflow Quick Reference

```bash
# Daily development
make dev-full              # Backend + Web app + Redis
make test                  # All tests (backend + frontend)
make ci                    # Full CI pipeline locally

# Backend-focused work
cd backend && make dev     # Watch mode
cd backend && make test    # Fast tests
cd backend && make test-vectors  # Vector-specific tests
cd backend && make bench-vectors # Performance benchmarks

# Frontend-focused work
cd frontend && make dev-web      # Vite dev server (port 3000)
cd frontend && make test-web     # Web app tests
cd frontend && make build-web    # Production build
cd frontend && make storybook-web # Component explorer

# Dependency management (pnpm throughout)
make verify-deps                 # Check all deps for updates
cd frontend && pnpm outdated -r  # Frontend-specific
cd frontend && pnpm update -r    # Update all frontend deps
cd backend && make outdated-deps # Backend-specific

# Database & vectors
cd backend && make db-migrate-vectors  # Run vector schema migration
cd backend && make embed-existing      # Batch embed historical emails
```

---

## 15. Risk Assessment

| Risk                                         | Likelihood | Impact | Mitigation                                                                           |
| -------------------------------------------- | ---------- | ------ | ------------------------------------------------------------------------------------ |
| RuVector API instability (pre-1.0)           | Medium     | High   | Pin exact version; wrap in facade; integration tests                                 |
| Embedding model size (MiniLM = 80MB)         | Low        | Medium | Ship pre-downloaded; lazy-load on first use                                          |
| Memory pressure on large inboxes             | Medium     | Medium | Quantization auto-scaling; configurable limits                                       |
| RuvLLM hardware compatibility                | Medium     | Low    | Fallback chain: RuvLLM → Ollama → Cloud                                              |
| Slow initial ingestion (>10 min)             | Low        | High   | Streaming progress; resume capability; user expectations set                         |
| Vector storage corruption                    | Low        | High   | REDB WAL; periodic backup; rebuild from SQL if needed                                |
| Search quality regression                    | Low        | Medium | A/B test vector vs FTS5; user feedback loop; SONA learning                           |
| Privacy concerns (embeddings = derived data) | Medium     | High   | Local-only default; clear consent UX; encryption at rest                             |
| Browser secure storage limitations           | Medium     | Medium | Web Crypto API (AES-GCM) + IndexedDB; tokens encrypted at rest; non-extractable keys |
| Offline capability in web app                | Low        | Medium | PWA Service Worker + IndexedDB; background sync for queued operations                |
| Breaking changes in existing features        | Medium     | High   | Feature flags for gradual rollout; existing tests as guardrails                      |

---

## Appendix A: Dependency Version Audit (2026-03-23)

All versions verified against live registries. Issues flagged with warnings.

### Backend (Rust) — crates.io

| Crate                       | In Plan | Latest on crates.io              | Status                                                                                                              |
| --------------------------- | ------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| **ruvector-core**           | 2.0     | **2.0.6**                        | Valid. Default features: simd, storage, hnsw, api-embeddings, parallel. MSRV 1.77.                                  |
| **ruvector-gnn**            | 2.0     | **2.0.5**                        | Valid. MIT. MSRV 1.77.                                                                                              |
| **ruvllm**                  | 2.0     | **2.0.6**                        | Valid. Local LLM inference. MIT. MSRV 1.77.                                                                         |
| **sona** (self-learning)    | git dep | **0.0.0** (different crate!)     | The `sona` on crates.io is an ELF binary tool, NOT RuVector's SONA. Must use git dependency from ruvector monorepo. |
| **fastembed**               | 5.13    | **5.13.0**                       | Valid. Alternative ONNX embedding engine.                                                                           |
| **redb** (used by ruvector) | —       | **3.1.1**                        | Transitive dependency via ruvector-core.                                                                            |
| axum                        | 0.8     | **0.8.8**                        | Current.                                                                                                            |
| tokio                       | 1.50    | **1.50.0**                       | Current.                                                                                                            |
| sqlx                        | 0.8     | 0.8.x stable / **0.9.0-alpha.1** | Stay on 0.8.x — 0.9 is alpha.                                                                                       |
| oauth2                      | 5.0     | **5.0.0**                        | Current.                                                                                                            |
| jsonwebtoken                | 10.3    | **10.3.0**                       | Current.                                                                                                            |
| moka                        | 0.12    | **0.12.15**                      | Current.                                                                                                            |
| redis                       | 1.1     | **1.1.0**                        | Latest stable.                                                                                                      |
| apalis                      | 0.7     | 0.7.x / **1.0.0-rc.6**           | Stay on 0.7.x — 1.0 is RC.                                                                                          |
| genai                       | 0.4     | 0.4.x / **0.6.0-beta.10**        | Stay on 0.4.x — 0.6 is beta.                                                                                        |
| ollama-rs                   | 0.3     | **0.3.4**                        | Current.                                                                                                            |
| mail-parser                 | 0.11    | **0.11.2**                       | Current.                                                                                                            |
| lettre                      | 0.11    | **0.11.19**                      | Current.                                                                                                            |
| evalexpr                    | 13.1    | **13.1.0**                       | Bump from current 12.0 — minor breaking changes possible.                                                           |
| serde                       | 1.0     | **1.0.228**                      | Current.                                                                                                            |
| chrono                      | 0.4     | **0.4.44**                       | Current.                                                                                                            |
| uuid                        | 1.22    | **1.22.0**                       | Current.                                                                                                            |
| thiserror                   | 2.0     | **2.0.18**                       | Major bump from 1.x — derive macro changes.                                                                         |
| ring                        | 0.17    | **0.17.14**                      | Current.                                                                                                            |
| secrecy                     | 0.10    | **0.10.3**                       | Current.                                                                                                            |
| **serde_yml**               | 0.0.12  | **0.0.12**                       | YAML support. Replaces EOL serde_yaml.                                                                              |

### Frontend (npm) — npm registry

| Package                 | In Plan  | Latest on npm | Status                          |
| ----------------------- | -------- | ------------- | ------------------------------- |
| react                   | ^19.2.4  | **19.2.4**    | Current.                        |
| react-dom               | ^19.2.4  | **19.2.4**    | Current.                        |
| typescript              | ^6.0.2   | **6.0.2**     | Current. Latest stable.         |
| vite                    | ^8.0.2   | **8.0.2**     | Current. Latest stable.         |
| @tanstack/react-router  | ^1.168.3 | **1.168.3**   | Current.                        |
| @tanstack/react-query   | ^5.95.2  | **5.95.2**    | Current.                        |
| @tanstack/react-virtual | ^3.13.23 | **3.13.23**   | Current.                        |
| zustand                 | ^5.0.12  | **5.0.12**    | Current. Latest stable.         |
| react-hook-form         | ^7.72.0  | **7.72.0**    | Current.                        |
| zod                     | ^4.3.6   | **4.3.6**     | Latest stable. New schema API.  |
| ky                      | ^1.14.3  | **1.14.3**    | Current.                        |
| cmdk                    | ^1.1.1   | **1.1.1**     | Current. New dependency.        |
| recharts                | ^3.8.0   | **3.8.0**     | Latest stable.                  |
| framer-motion           | ^12.38.0 | **12.38.0**   | Current.                        |
| lucide-react            | ^1.0.1   | **1.0.1**     | Latest stable.                  |
| date-fns                | ^4.1.0   | **4.1.0**     | Current.                        |
| idb-keyval              | ^6.2.2   | **6.2.2**     | IndexedDB key-value store.      |
| tailwindcss             | ^4.2.2   | **4.2.2**     | Current.                        |
| vitest                  | ^4.1.1   | **4.1.1**     | Latest stable.                  |
| playwright              | ^1.58.2  | **1.58.2**    | Current.                        |
| eslint                  | ^10.1.0  | **10.1.0**    | Latest stable.                  |
| storybook               | ^10.3.3  | **10.3.3**    | Latest stable.                  |
| turbo                   | ^2.8.20  | **2.8.20**    | Current.                        |
| pnpm                    | —        | **10.32.1**   | Package manager.                |
| vite-plugin-pwa         | ^1.2.0   | **1.2.0**     | New dependency for PWA support. |

### Implementation Notes

1. **`sona` crate name collision** — The crates.io `sona` is an ELF binary tool, NOT RuVector's self-learning system. Use a git dependency: `sona = { git = "https://github.com/ruvnet/ruvector", path = "crates/sona" }`
2. **`serde_yaml` is EOL** — Use `serde_yml` (0.0.12) for YAML support. Alternatively, standardize on TOML via `figment`.
3. **RuVector MSRV 1.77** — Project targets Rust 1.94.0, well above RuVector's minimum.
4. **Rust 1.94.0** (stable, 2026-03-02) — Use `rustup default stable` to install. All crate versions verified compatible.
5. **pnpm 10.32+** — Use `corepack enable && corepack prepare pnpm@latest --activate` to install.

---

## Appendix B: Docker Compose Full-Stack Setup

### Production Docker Compose

```yaml
# docker-compose.yml — Full-stack Emailibrium
# Usage: docker compose up -d
# Secrets: place in secrets/ directory (gitignored), see secrets.example/

services:
  # ─── Backend API (Rust) ───────────────────────────────────
  backend:
    build:
      context: ./backend
      dockerfile: Dockerfile
      target: runtime
      args:
        RUST_VERSION: '1.94'
    container_name: emailibrium-backend
    user: '1000:1000'
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    tmpfs:
      - /tmp:size=100M,mode=1777
    ports:
      - '${BACKEND_PORT:-8080}:8080'
    environment:
      # Non-secret config via env vars (12-factor)
      APP_ENV: ${APP_ENV:-development}
      RUST_LOG: ${RUST_LOG:-emailibrium=info,tower_http=info}
      REDIS_URL: redis://redis:6379
      VECTOR_STORE_PATH: /app/data/vectors
      CONFIG_PATH: /app/config.yaml
    volumes:
      - ./configs/config.${APP_ENV:-development}.yaml:/app/config.yaml:ro
      - backend_data:/app/data
    secrets:
      - jwt_secret
      - oauth_encryption_key
      - database_url
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_healthy
    healthcheck:
      test: ['CMD', '/app/healthcheck']
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 45s
    networks:
      - db-internal
      - cache-internal
      - frontend-proxy
    logging:
      driver: 'json-file'
      options:
        max-size: '10m'
        max-file: '5'

  # ─── Frontend Web App (React SPA) ────────────────────────
  frontend:
    build:
      context: ./frontend
      dockerfile: Dockerfile
      target: runtime
    container_name: emailibrium-frontend
    user: '1000:1000'
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    tmpfs:
      - /tmp:size=50M
      - /var/cache/nginx:size=50M
      - /var/run:size=10M
    ports:
      - '${FRONTEND_PORT:-3000}:80'
    environment:
      VITE_API_URL: ${VITE_API_URL:-http://localhost:8080}
    healthcheck:
      test: ['CMD', 'wget', '--spider', '-q', 'http://localhost:80/']
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 15s
    networks:
      - frontend-proxy
    logging:
      driver: 'json-file'
      options:
        max-size: '5m'
        max-file: '3'

  # ─── PostgreSQL ───────────────────────────────────────────
  postgres:
    image: postgres:16-alpine
    container_name: emailibrium-postgres
    user: '999:999'
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    cap_add:
      - SETUID
      - SETGID
      - FOWNER
      - DAC_READ_SEARCH
    tmpfs:
      - /tmp:size=100M
      - /var/run/postgresql:size=10M
    environment:
      POSTGRES_DB: emailibrium
      POSTGRES_USER: emailibrium
      POSTGRES_PASSWORD_FILE: /run/secrets/db_password
    volumes:
      - postgres_data:/var/lib/postgresql/data
    secrets:
      - db_password
    healthcheck:
      test: ['CMD-SHELL', 'pg_isready -U emailibrium -d emailibrium']
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 30s
    networks:
      - db-internal
    # NO ports exposed — only backend accesses via internal network

  # ─── Redis Cache ──────────────────────────────────────────
  redis:
    image: redis:7-alpine
    container_name: emailibrium-redis
    user: '999:999'
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    tmpfs:
      - /tmp:size=50M
    volumes:
      - redis_data:/data
    command: >
      redis-server
      --appendonly yes
      --maxmemory 256mb
      --maxmemory-policy allkeys-lru
    healthcheck:
      test: ['CMD', 'redis-cli', 'ping']
      interval: 30s
      timeout: 5s
      retries: 3
      start_period: 10s
    networks:
      - cache-internal
    # NO ports exposed — only backend accesses via internal network

# ─── Secrets (file-based, per OWASP recommendation) ──────────
secrets:
  jwt_secret:
    file: ./secrets/${APP_ENV:-dev}/jwt_secret
  oauth_encryption_key:
    file: ./secrets/${APP_ENV:-dev}/oauth_encryption_key
  database_url:
    file: ./secrets/${APP_ENV:-dev}/database_url
  db_password:
    file: ./secrets/${APP_ENV:-dev}/db_password

# ─── Volumes ─────────────────────────────────────────────────
volumes:
  postgres_data:
  redis_data:
  backend_data:

# ─── Networks (segmented per NIST SP 800-190) ───────────────
networks:
  db-internal:
    internal: true # No external access — only backend ↔ postgres
  cache-internal:
    internal: true # No external access — only backend ↔ redis
  frontend-proxy:
    driver: bridge # Frontend ↔ backend communication
```

### Development Override

```yaml
# docker-compose.dev.yml — Development overrides
# Usage: docker compose -f docker-compose.yml -f docker-compose.dev.yml up

services:
  backend:
    build:
      target: development
    read_only: false # Need writable for cargo watch
    environment:
      APP_ENV: development
      RUST_LOG: emailibrium=debug,tower_http=debug
    volumes:
      - ./backend:/app
      - cargo_cache:/usr/local/cargo/registry
    command: cargo watch -x run

  frontend:
    build:
      target: development
    read_only: false
    ports:
      - '3000:3000' # Vite dev server
    volumes:
      - ./frontend:/app
      - /app/node_modules
    command: pnpm dev-web

  postgres:
    ports:
      - '5432:5432' # Exposed for local tools (DBeaver, psql)

  redis:
    ports:
      - '6379:6379' # Exposed for redis-cli

volumes:
  cargo_cache:
```

### Secrets Directory Structure

```text
secrets/
├── dev/                     # Development secrets (gitignored)
│   ├── jwt_secret           # openssl rand -base64 32
│   ├── oauth_encryption_key # openssl rand -base64 32
│   ├── database_url         # postgres://emailibrium:devpass@postgres:5432/emailibrium
│   └── db_password          # devpass
├── dev.example/             # Template (committed to git)
│   ├── jwt_secret           # REPLACE_ME_jwt_secret_32_chars_minimum
│   ├── oauth_encryption_key # REPLACE_ME_encryption_key_32_chars
│   ├── database_url         # postgres://emailibrium:REPLACE@postgres:5432/emailibrium
│   └── db_password          # REPLACE_ME
└── .gitignore               # */!*.example/*
```

Generate dev secrets:

```bash
mkdir -p secrets/dev
openssl rand -base64 32 > secrets/dev/jwt_secret
openssl rand -base64 32 > secrets/dev/oauth_encryption_key
echo "postgres://emailibrium:devpass@postgres:5432/emailibrium" > secrets/dev/database_url
echo "devpass" > secrets/dev/db_password
chmod 600 secrets/dev/*
```

---

## Appendix C: Externalized Configuration

### Design Principles (per OWASP, NIST SP 800-190, 12-Factor)

1. **Secrets via file mounts, never env vars** — `docker inspect` and `/proc/1/environ` expose env vars; `/run/secrets/` is tmpfs-backed and only visible inside the container
2. **Non-secret config via YAML files, mounted read-only** — structured, validated at startup, environment-specific overlays
3. **Environment variables for non-secret runtime knobs** — `APP_ENV`, `RUST_LOG`, port numbers, feature flags
4. **Startup validation** — fail fast if required secrets are missing or config is invalid
5. **No hardcoded defaults for secrets** — no `${DB_PASSWORD:-development}` fallback patterns

### Backend Entrypoint (Secret Resolution)

```bash
#!/bin/sh
# /app/entrypoint.sh — resolve /run/secrets/ into env vars
# Per OWASP: secrets delivered via file mounts, read at startup, not persisted in env

for secret_file in /run/secrets/*; do
  if [ -f "$secret_file" ]; then
    var_name=$(basename "$secret_file" | tr '[:lower:]' '[:upper:]')
    export "$var_name"="$(cat "$secret_file")"
  fi
done

# Validate required secrets
for required in JWT_SECRET OAUTH_ENCRYPTION_KEY DATABASE_URL; do
  if [ -z "$(eval echo \$$required)" ]; then
    echo "FATAL: Required secret $required not found in /run/secrets/" >&2
    exit 1
  fi
done

exec "$@"
```

### Config File Hierarchy

```text
configs/
├── config.yaml              # Base config — all settings with sane defaults
├── config.development.yaml  # Dev overrides (debug, relaxed limits, SQLite OK)
├── config.staging.yaml      # Staging overrides (production-like, PostgreSQL)
├── config.production.yaml   # Production hardening (strict CORS, CSRF, audit logging)
└── config.local.yaml        # Local overrides (gitignored, per-developer)
```

**Loading order** (later overrides earlier, via `figment`):

```text
config.yaml → config.{APP_ENV}.yaml → config.local.yaml → env vars → /run/secrets/
```

### Vector Config Section (added to config.yaml)

```yaml
vectors:
  enabled: true
  store_path: '~/.emailibrium/vectors'

  embedding:
    provider: 'ruvllm' # ruvllm | ollama | cloud
    model: 'all-MiniLM-L6-v2' # Sentence transformer model
    dimensions: 384
    batch_size: 64 # Texts per batch call
    cache_size: 10000 # Moka cache entries

  index:
    type: 'hnsw'
    m: 16 # HNSW connections per node
    ef_construction: 200 # Build quality (higher = better, slower)
    ef_search: 100 # Search quality (higher = better, slower)

  quantization:
    mode: 'auto' # auto | none | scalar | product | binary
    auto_thresholds:
      scalar: 50000 # Switch to scalar above this count
      product: 200000
      binary: 1000000

  search:
    hybrid_weight_semantic: 0.6 # 60% semantic, 40% keyword
    hybrid_weight_keyword: 0.4
    rrf_k: 60 # Reciprocal rank fusion constant
    default_limit: 20
    max_limit: 100
    similarity_threshold: 0.5 # Min cosine similarity to include

  clustering:
    enabled: true
    algorithm: 'graphsage' # graphsage | gcn | gat
    min_cluster_size: 5
    refresh_interval_hours: 24

  learning:
    sona_enabled: true
    feedback_weight: 0.1 # How much each feedback shifts centroids
    session_memory: true

  ingestion:
    batch_size: 500
    num_workers: 4
    checkpoint_interval: 1000
    auto_embed_on_sync: true
```

## Appendix D: API Response Examples

### Hybrid Search Response

```json
{
  "query": "quarterly budget from finance",
  "mode": "hybrid",
  "totalResults": 47,
  "latencyMs": 23,
  "results": [
    {
      "email": {
        "id": "em_abc123",
        "subject": "Q4 Budget Review - Final Numbers",
        "from": "Sarah Chen <cfo@company.com>",
        "receivedAt": "2025-11-15T09:30:00Z",
        "bodyPreview": "Hi team, I'm pleased to share that we've approved the quarterly budget allocation for engineering..."
      },
      "score": 0.94,
      "matchType": "semantic",
      "highlights": [
        { "field": "subject", "snippet": "<mark>Q4 Budget</mark> Review - Final Numbers" },
        { "field": "body", "snippet": "approved the <mark>quarterly budget</mark> allocation" }
      ]
    }
  ],
  "relatedClusters": ["Budget & Finance", "Q4 Planning"],
  "suggestedQueries": ["expense reports Q4", "annual budget review"]
}
```

### Subscription Intelligence Response

```json
{
  "totalSubscriptions": 47,
  "monthlyEmailVolume": 891,
  "estimatedReadingHoursPerMonth": 14.2,
  "subscriptions": [
    {
      "id": "sub_xyz789",
      "senderDomain": "marketing.saas.com",
      "senderAddress": "marketing@saas.com",
      "senderName": "SaaS Marketing",
      "frequency": { "type": "weekly", "dayOfWeek": "Tuesday" },
      "emailCount": 142,
      "firstSeen": "2024-03-12T00:00:00Z",
      "lastSeen": "2026-03-18T00:00:00Z",
      "hasUnsubscribeHeader": true,
      "unsubscribeLink": "https://saas.com/unsubscribe?token=...",
      "category": "marketing",
      "avgReadRate": 0.0,
      "suggestedAction": "unsubscribe"
    }
  ],
  "summary": {
    "neverOpened": 12,
    "rarelyOpened": 18,
    "regularlyOpened": 17,
    "potentialTimeSaved": 11.3
  }
}
```

### Ingestion Progress SSE Stream

```text
event: progress
data: {"jobId":"ing_001","total":12847,"processed":5432,"embedded":5432,"categorized":5100,"failed":3,"phase":"embedding","etaSeconds":142,"emailsPerSecond":523}

event: discovery
data: {"type":"subscription","senderDomain":"newsletter.com","emailCount":52}

event: discovery
data: {"type":"cluster","name":"Work / Project Alpha","emailCount":1234}

event: progress
data: {"jobId":"ing_001","total":12847,"processed":12847,"embedded":12847,"categorized":12847,"failed":7,"phase":"complete","etaSeconds":0,"emailsPerSecond":0}

event: complete
data: {"jobId":"ing_001","report":{"totalEmails":12847,"clusters":6,"subscriptions":47,"duration_seconds":178}}
```

---

_This document serves as the technical implementation plan for the reimagined Emailibrium platform. It should be updated as implementation progresses and architectural decisions evolve._
