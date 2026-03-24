# Architecture Overview

> **Repository**: [github.com/pacphi/emailibrium](https://github.com/pacphi/emailibrium)

## System Architecture

Emailibrium is a **vector-native email intelligence platform** organized as a four-tier architecture (docs/plan/inception.md Section 3):

```
+------------------------------------------------------------------+
|                    PRESENTATION TIER                               |
|  React TypeScript SPA (Vite 8 + TanStack Router + shadcn/ui)     |
|  Components: Command Center, Inbox Cleaner, Insights Explorer,    |
|              Email Client, Rules Studio, Settings                 |
+-------------------------------+----------------------------------+
                                | REST + SSE
+-------------------------------+----------------------------------+
|                    API GATEWAY TIER                                |
|  Axum web framework with tower-http middleware                    |
|  Routes: /vectors, /ingestion, /insights                         |
|  CORS, tracing, CSP headers (ADR-008)                            |
+-------------------------------+----------------------------------+
                                |
+-------------------------------+----------------------------------+
|                    INTELLIGENCE TIER                               |
|  Embedding Pipeline (ADR-002): ONNX (default) -> Ollama -> cloud |
|  Vector Store (ADR-003): InMemoryVectorStore / EncryptedStore    |
|  Categorizer (ADR-004): centroid classification + LLM fallback   |
|  Clustering (ADR-009): GraphSAGE on HNSW neighbor graph          |
|  Quantization (ADR-007): adaptive scalar quantization            |
|  Insight Engine: subscription detection, recurring sender analysis|
|  Content Pipeline (ADR-006): HTML, images, attachments, links    |
+-------------------------------+----------------------------------+
                                |
+-------------------------------+----------------------------------+
|                    DATA TIER                                       |
|  SQLite (structured data, FTS5 search, email metadata, config)   |
|  Vector Store (embeddings, HNSW index, clusters)                 |
|  Moka Cache (embedding cache, LLM response cache)                |
+------------------------------------------------------------------+
```

## Bounded Contexts

Emailibrium follows Domain-Driven Design with five bounded contexts (DDD-000 through DDD-005):

| Context                | Type       | Document | Responsibility                                                            |
| ---------------------- | ---------- | -------- | ------------------------------------------------------------------------- |
| **Email Intelligence** | Core       | DDD-001  | Embedding generation, vector storage, classification, clustering          |
| **Search**             | Core       | DDD-002  | Query execution, result fusion (FTS5 + HNSW), SONA re-ranking             |
| **Ingestion**          | Supporting | DDD-003  | Email sync from providers, multi-asset extraction, pipeline orchestration |
| **Learning**           | Supporting | DDD-004  | SONA adaptive learning, centroid updates, feedback processing             |
| **Account Management** | Supporting | DDD-005  | Provider connections (OAuth), sync state, archive strategy                |

### Context Map

```
Account Management --[Published Language]--> Ingestion
                                               |
                                    [Customer/Supplier]
                                               |
                                               v
                   Email Intelligence <--[Conformist]-- Ingestion
                          |
                   [Open Host Service]
                          |
                          v
                        Search <--[Shared Kernel]--> Learning
```

Integration patterns:

- **Published Language**: Account Management emits `AccountConnected` and `SyncCompleted` events consumed by Ingestion
- **Customer/Supplier**: Ingestion produces `ContentExtracted` events consumed by Email Intelligence
- **Open Host Service**: Email Intelligence exposes embedding and classification APIs consumed by Search
- **Shared Kernel**: Search and Learning share the `SearchInteraction` aggregate for SONA feedback

## Data Flow

The email ingestion pipeline processes emails through six stages (docs/plan/inception.md Section 3.2):

```
Email arrives (via provider sync)
  |
  +--- 1. Parse & Extract metadata ----------> SQLite (structured data)
  |        subject, from, to, date,
  |        headers, labels, size
  |
  +--- 2. Generate text embedding -----------> Vector Store "email_text"
  |        subject + from + body_preview          HNSW index update
  |        Model: all-MiniLM-L6-v2 (384D)
  |
  +--- 3. Extract & vectorize assets --------> Vector Store (multi-collection)
  |        HTML body -> clean text + URLs          "email_text"
  |        Inline images -> OCR + CLIP             "image_text" + "image_visual"
  |        Attachments -> extract text             "attachment_text"
  |
  +--- 4. Classify category -----------------> Moka cache
  |        Vector similarity to centroids        Cache key: embedding hash
  |        Fallback: LLM categorization
  |
  +--- 5. Detect patterns ------------------> Pattern store
  |        Subscription headers, recurring
  |        senders, tracking pixels, threads
  |
  +--- 6. Apply rules ----------------------> Action queue
           Match rule conditions
           Execute: label, archive, delete, move
```

Throughput targets:

- Text-only emails: 500+ emails/sec
- Full multi-asset extraction: ~50 emails/sec
- Configurable: fast mode (text-only) for initial sync, deep mode (all assets) for background processing

## Key Decisions

| ADR     | Title                      | Decision                                                                            | Key Trade-off                                                      |
| ------- | -------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| ADR-001 | Hybrid Search Architecture | FTS5 + HNSW + Reciprocal Rank Fusion                                                | Complexity vs. search quality across exact and semantic queries    |
| ADR-002 | Embedding Model Selection  | Pluggable pipeline with fallback chain (local -> Ollama -> cloud)                   | Latency vs. quality; mock fallback ensures the system always works |
| ADR-003 | Vector Database            | RuVector as primary store; SQLite backup for persistence                            | Rust-native performance vs. ecosystem maturity                     |
| ADR-004 | Adaptive Learning          | SONA self-learning with centroid-based classification + LLM fallback                | Continuous improvement vs. classification stability                |
| ADR-005 | Frontend Architecture      | Pure React TypeScript SPA replacing Tauri 2.0 desktop app                           | Web accessibility vs. native desktop integration                   |
| ADR-006 | Content Extraction         | Multi-asset pipeline (HTML, images, attachments, links)                             | Extraction breadth vs. reliability across input types              |
| ADR-007 | Quantization               | Adaptive scalar quantization based on corpus size                                   | 4x memory reduction vs. slight recall degradation                  |
| ADR-008 | Privacy & Security         | AES-256-GCM encryption at rest, Argon2id key derivation, embedding noise            | ~5-10% performance overhead vs. data protection                    |
| ADR-009 | Clustering                 | GraphSAGE on HNSW neighbor graph for topic discovery                                | Novel approach (needs empirical validation) vs. proven methods     |
| ADR-010 | Inbox Strategy             | Ingest-Tag-Archive pipeline ("Gmail is dumb store, Emailibrium is smart interface") | Aggressive automation vs. user safety and undo capability          |

## Module Structure

### Backend (`backend/src/`)

```
src/
  main.rs              # Axum server setup, middleware, startup
  lib.rs               # Public module re-exports for tests
  api/
    mod.rs             # Route composition
    vectors.rs         # Search, classify, health, stats endpoints
    ingestion.rs       # SSE streaming, start/pause/resume jobs
    insights.rs        # Subscription, recurring sender, report endpoints
    accounts.rs        # OAuth account management (DDD-005)
    ai.rs              # Chat and generative AI endpoints (ADR-012)
    backup.rs          # Backup management endpoints
    clustering.rs      # Cluster discovery endpoints
    consent.rs         # Privacy consent management
    evaluation.rs      # Search quality evaluation endpoints
    interactions.rs    # Search interaction tracking (SONA)
    learning.rs        # SONA learning engine endpoints
  db/
    mod.rs             # SQLite connection pool (sqlx)
  content/
    mod.rs             # ContentPipeline facade (ADR-006)
    html_extractor.rs  # HTML -> clean text
    image_analyzer.rs  # OCR + CLIP embedding
    link_analyzer.rs   # URL extraction and classification
    attachment_extractor.rs  # PDF/DOCX/XLSX text extraction
    tracking_detector.rs     # Tracking pixel detection
    types.rs           # Shared content types
  vectors/
    mod.rs             # VectorService facade
    embedding.rs       # EmbeddingPipeline with provider fallback (ADR-002)
    store.rs           # VectorStoreBackend trait + InMemoryVectorStore
    ruvector_store.rs  # RuVector HNSW backend (ADR-003)
    encryption.rs      # AES-256-GCM encryption at rest (ADR-008)
    config.rs          # Layered configuration (figment)
    types.rs           # Core value objects (VectorDocument, etc.)
    models.rs          # Data model types
    error.rs           # Error types (thiserror)
    search.rs          # Search execution logic
    categorizer.rs     # Centroid-based classification (ADR-004)
    backup.rs          # SQLite vector backup/restore (ADR-003)
    clustering.rs      # GraphSAGE clustering (ADR-009)
    ingestion.rs       # Ingestion pipeline orchestration
    insights.rs        # Subscription and recurring sender detection
    interactions.rs    # Search interaction tracking (SONA)
    learning.rs        # SONA adaptive learning engine (ADR-004)
    quantization.rs    # Scalar quantization (ADR-007)
    generative.rs      # Generative AI integration (ADR-012)
    consent.rs         # Privacy consent management
    metrics.rs         # Vector service metrics and telemetry
    reindex.rs         # Reindex operations
```

### Frontend (`frontend/`)

React TypeScript SPA built with Vite, TanStack Router, Zustand state management, and shadcn/ui components.

#### Turborepo Monorepo

The frontend is organized as a **Turborepo monorepo** (`frontend/turbo.json`) using pnpm workspaces (`frontend/pnpm-workspace.yaml`). Turborepo orchestrates `dev`, `build`, `test`, `lint`, and `typecheck` tasks across all workspace packages with dependency-aware caching and parallel execution.

#### Workspace Packages

| Package              | Path                      | Purpose                                            |
| -------------------- | ------------------------- | -------------------------------------------------- |
| `@emailibrium/api`   | `frontend/packages/api`   | HTTP client layer wrapping backend REST endpoints  |
| `@emailibrium/core`  | `frontend/packages/core`  | Shared business logic and utilities                |
| `@emailibrium/types` | `frontend/packages/types` | TypeScript type definitions shared across packages |
| `@emailibrium/ui`    | `frontend/packages/ui`    | Reusable UI component library (shadcn/ui based)    |
| `@emailibrium/web`   | `frontend/apps/web`       | Main web application (Vite + React)                |

#### Storybook

Component development and visual testing use **Storybook 8** (`frontend/apps/web/.storybook/`). There are currently **9 stories** covering shared components (`ErrorFallback`, `OfflineIndicator`, `SkipToContent`, `Toast`) and feature components (`ProgressBar`, `PhaseIndicator`, `SubscriptionRow`, `HealthScoreGauge`, `FrequencyBadge`). Run via `pnpm storybook` in the web app.

#### E2E Testing (Playwright)

End-to-end tests live in `frontend/apps/web/e2e/` and cover 6 test suites: `email`, `inbox-cleaner`, `navigation`, `onboarding`, `rules`, and `search`. Run with `pnpm test:e2e` (which invokes `playwright test`).

#### Chat Feature

An AI-powered chat interface resides in `frontend/apps/web/src/features/chat/`. It includes `ChatInterface.tsx` (main container), `ChatInput.tsx` (message input), `ChatMessage.tsx` (message rendering), and a `useChat` hook for managing conversation state against the backend `/api/ai/chat` endpoint (ADR-012).

#### Onboarding Flow

A guided onboarding wizard at `frontend/apps/web/src/features/onboarding/` walks users through initial setup. Components include `OnboardingFlow.tsx` (step orchestrator), `ProviderSelector.tsx` (email provider choice), `GmailConnect.tsx` / `OutlookConnect.tsx` / `ImapConnect.tsx` (OAuth and credential flows per DDD-005), `ConnectedAccounts.tsx` (account summary), `ArchiveStrategyPicker.tsx` (inbox strategy selection per ADR-010), `AISetup.tsx` (AI tier configuration), and `SetupComplete.tsx` (completion confirmation).

#### TanStack React Query

Data fetching and server-state management use **TanStack React Query v5** (`@tanstack/react-query`). This provides automatic caching, background refetching, and optimistic updates for all backend API interactions.

#### TanStack React Virtual

Large email lists are rendered efficiently using **TanStack React Virtual v3** (`@tanstack/react-virtual`), which virtualizes DOM nodes so only visible rows are mounted. This is critical for inboxes with thousands of messages.

#### Framer Motion

UI animations and transitions use **Framer Motion v11** (`framer-motion`). Used for page transitions, component mount/unmount animations, and interactive feedback throughout the application.

#### MSW (Mock Service Worker)

API mocking in tests uses **MSW v2** (`msw`). Request handlers intercept `fetch` calls during Vitest runs, enabling isolated component and integration tests without a running backend.

#### PWA Support

The app ships as a **Progressive Web App**. A custom service worker (`frontend/apps/web/src/pwa/sw.ts`) pre-caches static assets under the `emailibrium-v1` cache name and serves them offline. Registration logic in `frontend/apps/web/src/pwa/register.ts` handles install and update lifecycle events.

## Security Architecture

Refer to ADR-008 for the full security design. Key points:

- **Encryption at rest**: AES-256-GCM for vector store persistence
- **Key derivation**: Argon2id from master password; key held in memory only, zeroed on drop (`zeroize` crate)
- **Content Security Policy**: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'`
- **CORS**: Restricted to specific origins (not wildcard)
- **OAuth tokens**: Stored via Web Crypto API in browser (not localStorage)
- **Embedding privacy**: Calibrated Gaussian noise injection (differential privacy lite)
