# Documentation Audit Report — Emailibrium

**Date:** 2026-03-24
**Scope:** All 32 project docs vs actual implementation
**Method:** 5-agent parallel swarm audit (architecture/ADRs, DDDs, guides, plans, code inventory)

---

## Executive Summary

**Overall doc-to-code alignment: ~45%.** The docs describe an ambitious architecture that was faithfully scaffolded but only partially implemented. Structural alignment is high (module names, file layout, trait interfaces match docs), but functional depth is low — several core dependencies and integrations described in docs do not exist in the codebase.

The project has a well-organized scaffold that maps to the plan, with the LLM tier and frontend being the most complete layers. The two critical missing pieces are (a) a real vector store backend (replacing or implementing RuVector) and (b) email provider connectivity. Without these, the platform is a UI shell with AI configuration but no ability to process actual email.

---

## Critical Misalignments (Fix First)

### 1. RuVector Does Not Exist

- **Docs affected:** ADR-003, ADR-001, ADR-007, ADR-009, architecture.md, INCEPTION.md, implementation plan
- **Reality:** No `ruvector` crate in Cargo.toml. No REDB. No HNSW library. Vector store is a custom `InMemoryVectorStore` with brute-force cosine similarity.
- **Impact:** This is the foundational dependency referenced across 5+ ADRs. Every doc that says "HNSW" or "RuVector" is misleading.

> **🎯 DESIRED STATE:** RuVector is now available as a git submodule (`ruvector/`, pinned at v0.5.0+). The workspace includes `ruvector-core`, `ruvector-gnn`, `ruvector-graph`, `ruvector-cluster`, `ruvector-collections`, `ruvector-snapshot`, `ruvector-raft`, `ruvector-replication`, and more. Backend `Cargo.toml` should add path dependencies to relevant ruvector crates (minimally `ruvector-core` for HNSW, `ruvector-gnn` for GraphSAGE). `InMemoryVectorStore` should be replaced with a `RuVectorStore` backend. Redis 7 serves as cache/pub-sub layer; PostgreSQL 16 for structured data if SQLite proves insufficient at scale.

### 2. No Email Provider Connectivity

- **Docs affected:** ADR-010, DDD-005, INCEPTION.md, user-guide, deployment-guide
- **Reality:** OAuth config scaffolding exists (DDD-005), and frontend has `GmailConnect.tsx`/`OutlookConnect.tsx` UI shells, but **zero** Gmail API, Microsoft Graph API, IMAP, or POP3 client code exists in the backend. The platform cannot connect to any email account.

### 3. Docker Compose vs Backend Code Mismatch

- **Docs affected:** deployment-guide, architecture.md, README
- **Reality:** `docker-compose.yml` defines PostgreSQL 16 + Redis 7. Backend code uses **only SQLite**. No `redis` or `postgres` crate in Cargo.toml. The `configs/` directory mounted by docker-compose doesn't exist. Docker Rust version is `1.85` but MSRV is `1.94`.

> **🎯 DESIRED STATE:** Redis 7 is the target cache/session/pub-sub layer. PostgreSQL 16 is the target structured data store if/when SQLite limitations are hit. Docker Compose services for both are correct and should stay. Backend needs `redis` and (conditionally) `sqlx` with `postgres` feature added to Cargo.toml. Fix Docker `RUST_VERSION` to 1.94 and create missing `configs/` directory.

### 4. Embedding Provider Default Wrong Everywhere

- **Docs affected:** deployment-guide, configuration-reference, user-guide
- **Reality:** Every guide says the default embedding provider is `"mock"`. The actual default is `"onnx"` (fastembed). ONNX is the most important provider and is **not mentioned** in any user-facing guide.

> **🎯 DESIRED STATE:** ONNX (fastembed) is the correct and intended default embedding provider. All docs must be updated to reflect `"onnx"` as default. This is already correct in code — the docs are wrong.

---

## Cross-Document Inconsistencies

| Topic             | deployment-guide | config-reference | user-guide        | architecture.md | README    | Actual Code                                        |
| ----------------- | ---------------- | ---------------- | ----------------- | --------------- | --------- | -------------------------------------------------- | --------------------------- |
| Embedding default | `mock`           | `mock`           | mock/Ollama/cloud | —               | —         | **`onnx`**                                         |
| Database          | SQLite           | SQLite           | —                 | SQLite          | SQLite    | SQLite (code) / **PG+Redis (docker)**              |
| Vite version      | —                | —                | —                 | Vite 7          | Vite 6    | **Vite 6.4**                                       | **🎯 Vite 8.0.2**           |
| Router            | —                | —                | —                 | TanStack Router | —         | TanStack Router / **ADR-005 says React Router v6** |                             |
| Vector modules    | —                | —                | —                 | —               | 16        | maintainer says 17 / **actual: 21**                |                             |
| API handlers      | —                | —                | —                 | 3 listed        | —         | **11 actual**                                      |                             |
| Clustering algo   | —                | —                | —                 | GraphSAGE       | K-means++ | **Mini-batch K-Means**                             | **🎯 GraphSAGE + KMeans++** |
| Config layers     | 4                | 5                | —                 | —               | —         | **3 implemented**                                  |                             |
| pnpm version      | —                | —                | —                 | —               | —         | project: 10.32 / **release CI: 9**                 | **🎯 pnpm 10.32+**          |

---

## Architecture Doc Audit

### Aligned (Correct)

- **Axum web framework**: Architecture doc says "Axum web framework with tower-http middleware." Cargo.toml confirms `axum = "0.8"` and `tower-http = "0.6"`. `main.rs` uses `axum::Router` with `tower_http::trace::TraceLayer`.
- **React TypeScript SPA**: Architecture doc says React TypeScript SPA. `frontend/apps/web/package.json` confirms `react: ^19.2.4` and `typescript: ^5.9.3`.
- **TanStack Router**: Architecture doc says "TanStack Router." `Router.tsx` uses `@tanstack/react-router` with lazy-loaded routes.
- **Zustand state management**: Architecture doc mentions Zustand. Zustand is in `package.json` (`zustand: ^5.0.12`) and used in store files (`toastStore.ts`, `useSettings.ts`, `useCommandPalette.ts`).
- **shadcn/ui components**: Architecture doc mentions shadcn/ui. `package.json` has 10 `@radix-ui/*` packages (the primitive layer shadcn/ui builds on).
- **Frontend components match**: Architecture doc lists Command Center, Inbox Cleaner, Insights Explorer, Email Client, Rules Studio, Settings. `Router.tsx` confirms all six routes plus Onboarding.
- **SQLite database**: Architecture doc says "SQLite (structured data, FTS5 search)." `db/mod.rs` uses `sqlx::sqlite::SqlitePool`, and `Cargo.toml` has `sqlx` with `sqlite` feature.
- **Moka cache**: Architecture doc mentions "Moka Cache." Cargo.toml has `moka = "0.12"` with `future` feature.
- **Figment configuration**: Architecture doc mentions `config.rs` for "Layered configuration (figment)." `config.rs` uses `figment` with YAML, env providers.
- **Embedding pipeline with fallback chain**: Architecture doc says "EmbeddingPipeline with provider fallback (ADR-002)." `embedding.rs` implements `EmbeddingPipeline` with `MockEmbeddingModel`, `OllamaEmbeddingModel`, and `OnnxEmbeddingModel` providers.
- **VectorStoreBackend trait (ADR-003)**: Architecture doc lists `store.rs` as "VectorStoreBackend trait + InMemoryVectorStore." `store.rs` confirms this exact trait and implementation.
- **AES-256-GCM encryption (ADR-008)**: Architecture doc lists `encryption.rs` for "AES-256-GCM encryption at rest." Cargo.toml has `aes-gcm = "0.10"`, `argon2 = "0.5"`, `zeroize`.
- **Categorizer (ADR-004)**: Architecture doc lists `categorizer.rs` for "Centroid-based classification." Config includes `CategorizerConfig` with `confidence_threshold`, `max_centroid_shift`, `min_feedback_events` — matching ADR-004 specifications.
- **Content extraction pipeline (ADR-006)**: Architecture doc lists `html_extractor.rs`, `image_analyzer.rs`, `link_analyzer.rs`, `attachment_extractor.rs`, `tracking_detector.rs`, `types.rs` under `content/`. All six files exist in `backend/src/content/`.
- **Clustering module (ADR-009)**: Architecture doc lists `clustering.rs`. Both `backend/src/vectors/clustering.rs` and `backend/src/api/clustering.rs` exist.
- **Learning module (ADR-004)**: Architecture doc lists `learning.rs` for "SONA adaptive learning engine." Both `backend/src/vectors/learning.rs` and `backend/src/api/learning.rs` exist.
- **Quantization (ADR-007)**: Architecture doc lists `quantization.rs` for "Scalar quantization." `store.rs` imports `QuantizationEngine` and `QuantizationTier` from `quantization` module.
- **ONNX default provider (ADR-011)**: `default_provider()` returns `"onnx"`. `fastembed = "5.13.0"` in Cargo.toml. `OnnxEmbeddingModel` implemented in `embedding.rs`.
- **Tiered AI providers (ADR-012)**: `generative.rs` implements the three-tier model: `RuleBasedClassifier` (Tier 0), `OllamaGenerativeModel` (Tier 1), `CloudGenerativeModel` (Tier 2).
- **API routes**: Architecture doc lists `/vectors`, `/ingestion`, `/insights`. `api/mod.rs` confirms these plus additional routes.

### Misaligned (Inconsistent)

- **Vite version**: Architecture doc says "Vite 7" in the presentation tier diagram. Actual `package.json` has `vite: ^6.4.1` (Vite 6, not 7).
  > **🎯 DESIRED STATE:** Vite 8.0.2 (Rolldown-based, 10-30x faster builds). Requires Node.js 20.19+ / 22.12+. Update `package.json` to `vite: ^8.0.2`. Review esbuild/rollupOptions for Rolldown compatibility.
- **React version**: ADR-005 says "React 18+ with TypeScript." Actual `package.json` has `react: ^19.2.4` (React 19).
- **Routing library**: ADR-005 says "React Router v6 with lazy-loaded routes." Actual code uses `@tanstack/react-router` (not React Router). The architecture doc correctly says TanStack Router, but ADR-005 contradicts it.
- **Backend framework**: ADR-005 says "actix-web or axum" in the architecture diagram. The actual code uses only Axum. The ambiguity in the ADR is outdated.
- **Docker-compose uses PostgreSQL + Redis**: `docker-compose.yml` defines `postgres:16-alpine` and `redis:7-alpine` as services. The architecture doc says "SQLite" for the data tier and makes no mention of PostgreSQL or Redis. The backend code itself uses only SQLite.
- **CORS not implemented in main.rs**: Architecture doc says "CORS" middleware is present. `main.rs` applies `TraceLayer` but no CORS layer is visible.
- **CSP headers not implemented**: Architecture doc and ADR-008 specify Content Security Policy headers. No CSP middleware is visible in `main.rs`.
- **No RuVector dependency**: Architecture doc and ADR-003 describe "RuVector" as the primary vector database. There is no `ruvector` crate in `Cargo.toml`.
  > **🎯 DESIRED STATE:** Add path dependencies to `ruvector-core`, `ruvector-gnn`, `ruvector-graph`, and `ruvector-collections` from the `ruvector/` submodule.
- **REDB not present**: ADR-003 says "REDB as its storage backend." There is no `redb` dependency in `Cargo.toml`.

### Missing from Docs (Implemented but Undocumented)

- **7 API routes not in architecture doc**: `/api/v1/clustering`, `/api/v1/learning`, `/api/v1/interactions`, `/api/v1/evaluation`, `/api/v1/backup`, `/api/v1/ai`, `/api/v1/consent`
- **Undocumented backend modules**: `generative.rs`, `consent.rs`, `models.rs`, `metrics.rs`, `reindex.rs`, `ai.rs` (API), `evaluation.rs` (API)
- **Turbo monorepo structure**: The frontend uses `turbo` (Turborepo) for orchestration (`frontend/package.json` has `turbo: ^2.8.20`). Not mentioned anywhere in docs.
- **Workspace packages**: `@emailibrium/api`, `@emailibrium/core`, `@emailibrium/types`, `@emailibrium/ui`. Undocumented.
- **Storybook**: 11 story files and `.storybook/` config exist. Not mentioned in docs.
- **Chat feature**: `frontend/apps/web/src/features/chat/` exists with `ChatInterface.tsx`, `ChatInput.tsx`, `ChatMessage.tsx`, `useChat.ts`. Not mentioned in docs.
- **Onboarding flow**: `OnboardingFlow.tsx`, `GmailConnect.tsx`, `OutlookConnect.tsx`, `ConnectedAccounts.tsx`, `ArchiveStrategyPicker.tsx`. Not in architecture doc.
- **E2E test suite**: 6 Playwright E2E test files under `frontend/apps/web/e2e/`. Not documented.
- **TanStack React Query**: Used for data fetching. Not in architecture doc or ADR-005.
- **TanStack React Virtual**: Used for virtualized lists. Not documented.
- **Framer Motion**: `framer-motion: ^11.18.2`. Not in docs.
- **MSW (Mock Service Worker)**: `msw: ^2.12.14` in devDependencies. Not documented.
- **PWA support**: `register.ts` and `sw.ts` exist. Not in architecture doc module structure.

### Missing from Code (Documented but Not Implemented)

- **RuVector vector database** (ADR-003)
- **Qdrant fallback** and **SqliteVectorStore emergency fallback** (ADR-003)
- **HNSW index implementation** — brute-force cosine similarity used instead
- **FTS5 full-text search integration** (ADR-001)
- **Cloud embedding provider** — returns error "Cloud embedding provider is unsupported"
- **GraphSAGE GNN clustering** (ADR-009) — no GNN library
  > **🎯 DESIRED STATE:** Use `ruvector-gnn` crate (available in submodule) for GraphSAGE. Combine with KMeans++ for the clustering pipeline: GraphSAGE produces learned node embeddings from the email similarity graph, KMeans++ clusters those embeddings. This is the intended hybrid approach per ADR-009.
- **CLIP image embedding** (ADR-002, ADR-006)
- **OCR capability** — `ocrs` crate described in ADR-006, not in dependencies
- **PDF extraction** — `pdf-extract` described in ADR-006, not in dependencies
- **SQLCipher** (ADR-008)
- **Apalis background jobs** (ADR-006)
- **HDBSCAN clustering** (ADR-009)
- **EWC++ implementation** (ADR-004)
- **A/B evaluation infrastructure** (ADR-004)
- **Model integrity verification** with SHA-256 (ADR-013)
- **`emailibrium --download-models` CLI** (ADR-013, ADR-011)

---

## ADR Alignment Scorecard

### ADR-001: Hybrid Search Architecture (FTS5 + HNSW + RRF)

- **Status: NOT IMPLEMENTED**
- The ADR describes a sophisticated hybrid search pipeline with FTS5 full-text search, HNSW vector search, Reciprocal Rank Fusion, and SONA re-ranking. The codebase has a `search.rs` module, but there is no HNSW library dependency, no FTS5 index setup in visible code, and no RRF fusion implementation. The actual search appears to be brute-force cosine similarity on in-memory vectors. LIKE-based queries are used instead of FTS5 (noted as a TODO in code).

### ADR-002: Embedding Model Selection Strategy

- **Status: PARTIALLY ALIGNED**
- The pluggable `EmbeddingModel` trait is implemented exactly as described. The `all-MiniLM-L6-v2` model is the default. The ONNX provider (from ADR-011) is implemented. However, the CLIP image model (ViT-B-32) is not implemented (no CLIP dependency). Short query augmentation is referenced in config (`min_query_tokens`) but implementation depth is unclear. The model evaluation harness does not appear to exist.

### ADR-003: RuVector as Primary Vector Database

- **Status: NOT IMPLEMENTED**
- RuVector is not a dependency. REDB is not a dependency. The `VectorStoreBackend` facade trait exists (aligned), but the actual implementation is a custom `InMemoryVectorStore`, not RuVector. Neither the Qdrant fallback nor the SQLite emergency fallback stores are implemented.

> **🎯 DESIRED STATE:** RuVector submodule (`ruvector/`) is now available with `ruvector-core` (HNSW), `ruvector-collections`, `ruvector-snapshot`, and storage backends. Implement `RuVectorStore` behind the existing `VectorStoreBackend` trait using path dependencies to the submodule crates.

### ADR-004: SONA Adaptive Learning Model Specification

- **Status: PARTIALLY ALIGNED**
- The `learning.rs` module exists with `LearningConfig`. The `CategorizerConfig` includes `max_centroid_shift` and `min_feedback_events` parameters matching the ADR. The `interactions.rs` module tracks search interactions. However, the formal EWC++ implementation, A/B evaluation, centroid snapshots, position bias mitigation, and degenerate collapse detection are not evidenced by any dependencies or visible infrastructure.

### ADR-005: Tauri 2.0 to Web SPA Migration

- **Status: PARTIALLY ALIGNED**
- The migration to a web SPA is complete: React + TypeScript + Vite confirmed. However: (a) the ADR says "React Router v6" but the code uses TanStack Router; (b) the ADR says "React 18+" but the code uses React 19; (c) the ADR says "actix-web or axum" but only Axum is used; (d) PWA is partially implemented; (e) Lighthouse CI gates and bundlewatch are not visible in CI config.

### ADR-006: Multi-Asset Content Extraction Pipeline

- **Status: PARTIALLY ALIGNED**
- The content module structure matches the ADR exactly (`html_extractor.rs`, `image_analyzer.rs`, `link_analyzer.rs`, `attachment_extractor.rs`, `tracking_detector.rs`, `types.rs`). However, the actual extraction dependencies are missing: no OCR crate (`ocrs`), no PDF extraction crate (`pdf-extract`), no `ammonia` or `scraper` crate, no CLIP model. The files exist as code scaffolding, but the heavy extraction capabilities described lack their underlying libraries.

### ADR-007: Adaptive Quantization Strategy

- **Status: PARTIALLY ALIGNED**
- The `quantization.rs` module exists and is imported by `store.rs` (including `QuantizationEngine`, `QuantizationTier`, `quantize_vector`, `dequantize_vector`). Config includes `QuantizationConfig`. However, the full 4-tier quantization strategy (fp32 -> int8 -> PQ -> binary+rerank) likely requires additional libraries for product quantization and binary codes that are not in Cargo.toml.

### ADR-008: Privacy Architecture and Embedding Security

- **Status: PARTIALLY ALIGNED**
- Encryption dependencies are present: `aes-gcm = "0.10"`, `argon2 = "0.5"`, `zeroize`. `encryption.rs` exists. However: (a) CORS is not wired in `main.rs` despite the ADR requiring it; (b) CSP headers are not applied; (c) SQLCipher is not a dependency; (d) audit logging infrastructure is not visible; (e) remote wipe capability is not evidenced.

### ADR-009: GNN Clustering Architecture (GraphSAGE on HNSW Graphs)

- **Status: NOT IMPLEMENTED**
- No GraphSAGE or GNN library in Cargo.toml. No HDBSCAN library. The `clustering.rs` module exists, and `ClusterConfig` is in the config, but the code uses Mini-batch K-Means with a cosine-similarity graph approach that is "inspired by" GraphSAGE but is not the actual algorithm.

> **🎯 DESIRED STATE:** Use `ruvector-gnn` (GraphSAGE) + `ruvector-graph` from the submodule combined with KMeans++ as the production clustering algorithm. Pipeline: (1) build email similarity graph via HNSW neighbors, (2) run GraphSAGE to produce learned node embeddings that capture graph structure, (3) cluster embeddings with KMeans++ for stable, interpretable topic clusters. Drop Mini-batch K-Means and the "inspired by" cosine-similarity graph.

### ADR-010: Ingest-Tag-Archive Pipeline Strategy

- **Status: NOT IMPLEMENTED**
- No Gmail API client library in Cargo.toml. No Microsoft Graph API client. The `ingestion.rs` API module exists with SSE streaming (`IngestionBroadcast`), but the actual provider integration has no backing dependencies. The `ArchiveStrategyPicker.tsx` component exists in the frontend, suggesting UI scaffolding is ahead of backend implementation.

### ADR-011: ONNX Runtime as Default Embedding Provider

- **Status: ALIGNED**
- `fastembed = "5.13.0"` in Cargo.toml (ADR says 5.12.0, minor version difference). `OnnxEmbeddingModel` is fully implemented using `fastembed::TextEmbedding`. Default provider is `"onnx"`. Supports `all-MiniLM-L6-v2`, `bge-small-en-v1.5`, `bge-base-en-v1.5`. Uses `tokio::task::block_in_place` for async wrapping. Cache directory configurable. Fallback to mock on ONNX init failure is implemented.

### ADR-012: Tiered AI Provider Architecture

- **Status: PARTIALLY ALIGNED**
- The three-tier structure is implemented: `RuleBasedClassifier` (Tier 0), `OllamaGenerativeModel` (Tier 1), `CloudGenerativeModel` (Tier 2) in `generative.rs`. Config has separate `embedding` and `generative` sections. Consent API exists. However: (a) cloud embedding provider returns an error ("unsupported"), so Tier 2 embedding is not functional; (b) Google/Gemini documented but not implemented; (c) Cohere documented but not implemented; (d) audit logging for cloud API calls is not evidenced.

### ADR-013: AI Model Lifecycle Management

- **Status: PARTIALLY ALIGNED**
- `reindex.rs` module exists for re-embedding support. `models.rs` exists for model manifests. `OnnxConfig` includes `cache_dir`, `show_download_progress`, `model` selection. However: (a) SHA-256 compile-time checksum verification is not evidenced; (b) the `emailibrium --download-models` CLI command does not exist; (c) model-switch detection and stale-marking logic is unclear.

---

## DDD Alignment Scorecard

### DDD-000: Context Map

- **Alignment: 6/10 (Partially Aligned)**
- The context map describes 5 bounded contexts (Email Intelligence, Search, Ingestion, Learning, Account Management). The codebase does NOT organize code into separate bounded-context modules — everything lives under a single `backend/src/vectors/` module with sub-files.
- The context map describes an asynchronous Event Bus with domain events flowing between contexts. NO event bus exists. Cross-context communication happens via direct function calls and shared `Arc` references.
- The "AI Providers" context (DDD-006) is NOT shown in DDD-000's context map.
- Per-context Email projections (SyncedEmail, RawEmail, EmbeddedEmail, SearchableEmail, FeedbackEmail) do not exist — there is a single `emails` table and `Email` type.
- The "no shared databases" rule is violated — all contexts share a single SQLite database.

### DDD-001: Email Intelligence — 7/10

**Entities/Aggregates:**

| Entity/Aggregate                              | Found in Code                                                                                               |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| EmbeddingAggregate / EmailEmbedding           | Partial — `VectorDocument` covers most fields; `EmbeddingStatus` enum exists. No formal aggregate boundary. |
| ClassificationAggregate / EmailClassification | Partial — `CategoryResult` covers it. No separate aggregate.                                                |
| ClusterAggregate / TopicCluster               | Partial — `Cluster` type exists. No `ClusterId` or formal aggregate.                                        |

**Services:**

| Service           | Found in Code                                                                 |
| ----------------- | ----------------------------------------------------------------------------- |
| EmbeddingPipeline | Yes — `embedding.rs` with `EmbeddingModel` trait, fallback chain, caching     |
| VectorCategorizer | Yes — `categorizer.rs` with centroid-based classification + LLM/rule fallback |
| ClusterEngine     | Yes — `clustering.rs` (K-Means, not HDBSCAN as DDD says)                      |
| HybridSearch      | Yes — `search.rs` with RRF fusion, FTS + vector search                        |

**Domain Events:** 0 of 5 implemented (EmailEmbedded, EmailClassified, ClusterDiscovered, ClusterMerged, ClassificationCorrected — none exist).

**Key gaps:** No domain events. HDBSCAN described but K-Means used. GraphSAGE described but cosine-similarity graph used. "RuvLLM" provider in DDD replaced by ONNX/fastembed.

### DDD-002: Search — 6/10

**Services:**

| Service                 | Found in Code                                     |
| ----------------------- | ------------------------------------------------- |
| QueryEmbedder           | Yes — embedded in `HybridSearch::semantic_search` |
| ResultFuser             | Yes — `reciprocal_rank_fusion()` with k=60        |
| SONAReranker            | No — not implemented                              |
| MultiCollectionSearcher | No — only `VectorCollection::EmailText` searched  |

**Key gaps:** SONA re-ranking not implemented. Multi-collection search not implemented. FTS5 not used (LIKE-based queries instead). No A/B evaluation control group. No domain events.

### DDD-003: Ingestion — 7/10

**Services:**

| Service             | Found in Code                            |
| ------------------- | ---------------------------------------- |
| IngestionPipeline   | Yes — 6-stage pipeline with pause/resume |
| HtmlExtractor       | Yes                                      |
| ImageAnalyzer       | Yes                                      |
| AttachmentExtractor | Yes                                      |
| LinkAnalyzer        | Yes                                      |
| ProgressStreamer    | Yes — SSE via broadcast channel          |

**Key gaps:** No formal domain events (progress broadcast via Tokio channels instead). Clustering and Analyzing phases are placeholder (`// placeholder phases, see ADR-006`). No checkpoint/resume from failure. TrackingDetector exists but not mentioned in DDD.

### DDD-004: Learning (SONA) — 6/10

**Services:**

| Service                       | Found in Code                                     |
| ----------------------------- | ------------------------------------------------- |
| InstantLearner (Tier 1)       | Yes — `process_feedback` with EMA updates         |
| SessionAccumulator (Tier 2)   | Yes — session state tracking                      |
| LongTermConsolidator (Tier 3) | Partial — method exists                           |
| DegenerateDetector            | Partial — position bias and drift detection exist |
| CentroidSnapshotManager       | Partial — snapshot and rollback exist             |

**Key gaps:** No domain events. No per-user `LearningModel` (single shared engine). No A/B control group. No timer-based session consolidation. Asymmetric learning rates (alpha=0.05, beta=0.02) are implemented as described.

### DDD-005: Account Management — 2/10

**This is the least implemented bounded context.**

| Service              | Found in Code                                                                                                                   |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| OAuthManager         | Partial — `OAuthConfig` structs exist for Gmail/Outlook. No actual OAuth flow (no token exchange, refresh, credential storage). |
| ProviderSync         | No                                                                                                                              |
| ArchiveExecutor      | No                                                                                                                              |
| LabelManager         | No                                                                                                                              |
| AccountHealthMonitor | No                                                                                                                              |

Frontend has `GmailConnect.tsx`, `OutlookConnect.tsx`, `ImapConnect.tsx` and `authApi.ts` — these are UI shells only; the backend endpoints they would call do not exist. No `/api/v1/accounts` route. No `EmailProvider` trait. No `SyncState` entity.

### DDD-006: AI Providers — 4/10

**Services:**

| Service             | Found in Code                                                |
| ------------------- | ------------------------------------------------------------ |
| ModelDownloader     | No — fastembed handles downloads internally                  |
| EmbeddingRouter     | Partial — `EmbeddingPipeline` routes based on config         |
| GenerativeRouter    | Partial — provider selected at startup, no runtime routing   |
| ConsentManager      | Yes — `consent.rs` with grant/revoke/check and audit logging |
| ReindexOrchestrator | Yes — `reindex.rs` with model change detection               |
| AuditLogger         | Yes — audit entries stored in `cloud_ai_audit` table         |

**Key gaps:** InferenceSession aggregate not implemented. ModelRegistry lifecycle (download/verify/quarantine) not implemented. Model manifest is a static hardcoded catalog of 3 models. ProviderType is a string, not a proper enum. 17 described domain events — zero implemented.

### Domain Concepts in Code NOT in DDDs

1. **VectorBackupService** (`backup.rs`) — SQLite-based vector persistence/restore
2. **QuantizationEngine** (`quantization.rs`) — scalar, binary, and product quantization
3. **InsightEngine** (`insights.rs`) — analytics and insight generation
4. **EncryptedVectorStore** (`encryption.rs`) — at-rest encryption for vectors
5. **TrackingDetector** (`tracking_detector.rs`) — tracking pixel detection
6. **RuleBasedClassifier** (`generative.rs`) — keyword/domain heuristic classification
7. **EvaluationAPI** (`evaluation.rs`) — evaluation endpoints
8. **Chat feature** (`frontend/apps/web/src/features/chat/`) — chat interface with generative AI
9. **Rules Studio** (`frontend/apps/web/src/features/rules/`) — rule management
10. **Inbox Cleaner** (`frontend/apps/web/src/features/inbox-cleaner/`) — guided inbox cleanup

### Cross-Cutting DDD Gaps

1. **Zero domain events implemented anywhere.** Every DDD describes an event-driven architecture with published language between contexts. All communication is via direct `Arc` function calls.
2. **No bounded context separation.** All backend code lives in `backend/src/vectors/` and `backend/src/content/`.
3. **No per-context data stores.** A single SQLite database is shared, violating the DDD-000 principle "each context owns its data store."
4. **DDD-000 context map is missing DDD-006** (AI Providers).

---

## Guide Audit

### deployment-guide.md

**Accurate:**

- Prerequisites (Rust 1.94+, Node.js 24+, pnpm 10.32+, Docker optional, Make) match Cargo.toml and package.json
- Quick Start commands (`make install`, `make dev`, ports 8080/3000) match the root Makefile
- Figment configuration hierarchy (config.yaml, config.local.yaml, env vars) matches `VectorConfig::load()`
- OAuth setup instructions consistent with config.yaml oauth section

**Inaccurate / Outdated:**

- Embedding provider default: doc says `"mock"`, actual is `"onnx"`
- Embedding provider options: doc lists `"mock" | "ollama"`, actual has `"onnx" | "mock" | "ollama" | "cloud"`
- ONNX sub-config (`embedding.onnx.*`) missing entirely
- Docker Compose services: doc says only backend and frontend; actual includes postgres and redis
- Database: doc implies SQLite throughout; docker-compose uses PostgreSQL
- Docker build `RUST_VERSION: "1.85"` but Cargo.toml requires `rust-version = "1.94"`
- `configs/` directory: docker-compose mounts from `./configs/` but directory doesn't exist
- Ollama fallback: doc says fallback to mock; actual default is ONNX (no Ollama needed)

**Missing from doc:**

- OAuth config section in config.yaml
- Generative AI config (`generative.*`)
- Full Docker secrets list (jwt_secret, oauth_encryption_key, database_url, db_password, google_client_id, google_client_secret, microsoft_client_id, microsoft_client_secret)
- `make docker-secrets` generates PostgreSQL URL (contradicts SQLite docs)
- Redis as required Docker service
- Network segmentation (db-internal, cache-internal, frontend-proxy)

### configuration-reference.md

**Accurate:**

- All config keys in tables match `backend/config.yaml` and Rust struct definitions
- Default values match config.yaml
- Environment variable naming convention (EMAILIBRIUM\_ prefix) matches figment config
- Config file table correctly notes config.yaml and config.local.yaml.example

**Inaccurate / Outdated:**

- Embedding provider options: lists `mock`, `ollama`, `cloud` — missing `onnx`
- Loading order item 2 (`config.{APP_ENV}.yaml`) listed as if implemented — it is NOT
- Loading order item 5 (`/run/secrets/*`) listed as if implemented — it requires code additions
- `config.development.yaml` and `config.production.yaml` listed at repo root — they are at `backend/`

**Missing from doc:**

- `oauth.*` configuration block
- `generative.*` configuration block
- `embedding.onnx.model`, `embedding.onnx.show_download_progress`, `embedding.onnx.dimensions`

### user-guide.md

**Accurate:**

- Feature list matches frontend feature directories
- UI component descriptions align with implemented features

**Inaccurate / Outdated:**

- Embedding provider options in Settings lists "mock, Ollama, cloud" — should include ONNX as primary

**Missing from doc:**

- Chat feature (fully implemented)
- Onboarding flow (fully implemented)
- ONNX as default embedding provider

### maintainer-guide.md

**Accurate:**

- Repository layout matches actual directory structure
- Component library list matches exactly the 10 `.tsx` files in `frontend/packages/ui/src/components/`
- Testing strategy table accurate

**Inaccurate / Outdated:**

- Backend vectors module: doc says "17 files", actual count is 21
- API handlers: doc says 3 (`vectors.rs`, `ingestion.rs`, `insights.rs`), actual is 11 files
- Docker RUST_VERSION (1.85) below the MSRV (1.94)
- Branch strategy says feature branches from `develop` — CI triggers on `main` and `develop`, but current branch is `main`
- "78 features across 7 sprints" — memory file says "10 sprints, 449 tests"

**Missing from doc:**

- 8 additional API modules
- 6 additional vector modules
- ONNX/fastembed as significant backend dependency
- `content/types.rs`

### releasing.md

**Accurate:**

- Tag-based release workflow matches `.github/workflows/release.yml`
- Docker image naming convention matches workflow
- Makefile targets (release-check, release-tag, release-push, release, changelog) all exist

**Inaccurate / Outdated:**

- Release CI uses pnpm 9; project declares `pnpm@10.32.1`
  > **🎯 DESIRED STATE:** All CI workflows must use pnpm 10.32+. No pnpm 9 anywhere.
- Makefile help text prints `VER=x.y.z` but actual variable is `VERSION`

**Missing from doc:**

- No frontend tests in release CI gate
- pnpm version mismatch warning

### README.md

**Accurate:**

- Feature descriptions match frontend features
- Quick Start commands match Makefile
- Architecture diagram components mostly accurate
- Tech stack table mostly confirmed

**Inaccurate / Outdated:**

- Architecture diagram lists "Redis" and "REDB" — neither used in backend code
- "16 vector intelligence modules" — actual is 21 files
- "K-means++ clustering" in tech stack — ADR-009 says GraphSAGE, code uses Mini-batch K-Means

**Missing from doc:**

- ONNX/fastembed (the default embedding provider)
- Chat feature
- PostgreSQL (used in Docker deployment)

### CHANGELOG.md

- **Empty** — contains only header and auto-maintenance marker. No entries despite multiple conventional commits. Expected for pre-release, but will need population when v0.1.0 is tagged.

---

## Plan & Research Audit

### INCEPTION.md — Vision Alignment: 7/10

The original vision describes a RuVector-powered platform with HNSW indexing, SONA learning, GNN clustering, local LLM inference, and a React TypeScript web UI. The codebase structurally follows this vision, but the intelligence layer is substantially simplified.

**Key divergences:**

1. No RuVector integration (uses `InMemoryVectorStore`)
   > **🎯** RuVector submodule now available — integrate `ruvector-core` + `ruvector-collections`
2. No real GNN/GraphSAGE (uses Mini-batch K-Means)
   > **🎯** Use `ruvector-gnn` (GraphSAGE) + KMeans++ hybrid pipeline
3. No real SONA (3-tier referenced but simplified)
4. Vite 6.4 (inception says Vite 7, plan says Vite 8)
   > **🎯** Target Vite 8.0.2 (Rolldown-based)
5. Tailwind 3.4 (inception says Tailwind 4)
   > **🎯** Target Tailwind CSS 4
6. No PostgreSQL or Redis in backend code
   > **🎯** Redis 7 for cache/pub-sub; PostgreSQL 16 if needed for structured data at scale
7. No actual email provider integration

### PRIMARY-IMPLEMENTATION-PLAN.md

**Sprints documented:** 8 (Sprint 0 through Sprint 7)
**Sprints completed per code:** approximately 0 fully, with partial work across several

| Sprint                      | Goal                                                           | Completion                                                         |
| --------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------ |
| S0: Foundation              | RuVector benchmark, scaffolding, Docker, SQLite migration      | ~40% — Docker exists, backend scaffolded, no RuVector benchmark    |
| S1: Vector Foundation       | VectorStore facade, EmbeddingPipeline, encryption, categorizer | ~70% — most complete sprint, but no RuVectorStore                  |
| S2: Search & Ingestion      | Hybrid search, multi-asset extraction, ingestion pipeline, SSE | ~50% — structurally present but unproven without real vector store |
| S3: Intelligence & Learning | GraphSAGE clustering, SONA tiers, quantization                 | ~30% — K-Means not GraphSAGE, SONA simplified                      |
| S4: Frontend Foundation     | App shell, API client, onboarding, command center              | ~80% — structurally present                                        |
| S5: Frontend Features       | Inbox cleaner, insights, email client, rules studio            | ~75% — all feature directories exist                               |
| S6: Polish & Hardening      | Responsive, a11y, PWA, Storybook, E2E                          | ~60% — partial                                                     |
| S7: Evaluation & Validation | Formal evaluation, benchmarks, documentation                   | ~40% — test files exist but untested against real data             |

**All sprint tasks are marked `[ ]` (unchecked) in the plan itself**, confirming no sprint was formally completed.

**Notable gaps:**

- Plan references `ruvector-core 2.0`, `ruvector-gnn 2.0`, `ruvllm 2.0` — none in Cargo.toml
- Plan references `apalis` for background jobs — not in Cargo.toml
- Plan references PostgreSQL — not in dependencies

### LLM Implementation

| Provider                 | Documented     | Implemented                                        |
| ------------------------ | -------------- | -------------------------------------------------- |
| ONNX (fastembed)         | Tier 0 default | Yes — fully integrated                             |
| Ollama embedding         | Tier 1         | Yes — fallback chain                               |
| Ollama generative        | Tier 1         | Yes — `OllamaGenerativeModel`                      |
| OpenAI generative        | Tier 2         | Yes — `CloudGenerativeModel`                       |
| Anthropic generative     | Tier 2         | Yes — `CloudGenerativeModel`                       |
| Google/Gemini generative | Tier 2         | **No — documented but not implemented**            |
| Cohere embedding         | Tier 2         | **No — documented but not implemented**            |
| OpenAI cloud embedding   | Tier 2         | **Frontend offers it; backend rejects with error** |
| Rule-based classifier    | Tier 0         | Yes — `RuleBasedClassifier` in `generative.rs`     |

**Model identifier mismatches:** LLM plan references `claude-haiku-4-5-20251001`, `claude-sonnet-4-20250514`. Frontend uses different identifiers. Backend defaults to `gpt-4o-mini`.

### Evaluation Docs

| Protocol                                | Implemented as Code                               |
| --------------------------------------- | ------------------------------------------------- |
| Inbox Zero Protocol (user study design) | No — research methodology doc, not automatable    |
| Domain Adaptation Evaluation            | Partially — `domain_evaluation.rs` test exists    |
| Search Quality Evaluation               | Partially — `search_evaluation.rs` exists         |
| Classification Accuracy Evaluation      | Partially — `classification_evaluation.rs` exists |
| Clustering Quality Evaluation           | Partially — `clustering_evaluation.rs` exists     |
| Security Audit                          | Partially — `security_audit.rs` exists            |

### Technology Cross-Check

| Technology                    | Documented         | In Dependencies                  | Status                        |
| ----------------------------- | ------------------ | -------------------------------- | ----------------------------- | ------------------------------------------------- |
| React 19                      | Yes                | `react: ^19.2.4`                 | Match                         |
| TanStack Router               | Yes                | `@tanstack/react-router`         | Match                         |
| TanStack Query                | Yes                | `@tanstack/react-query: ^5.95.2` | Match                         |
| Zustand                       | Yes                | `zustand: ^5.0.12`               | Match                         |
| cmdk                          | Yes                | `cmdk: ^1.1.1`                   | Match                         |
| Tailwind CSS 4                | INCEPTION says 4   | `tailwindcss: ^3.4.19`           | **Mismatch — v3**             | **🎯 Tailwind 4**                                 |
| Vite 8 (plan) / 7 (inception) | Both docs          | `vite: ^6.4.1`                   | **Mismatch — v6**             | **🎯 Vite 8.0.2**                                 |
| Axum 0.8                      | Yes                | `axum = "0.8"`                   | Match                         |
| SQLite                        | Yes                | `sqlx` with sqlite               | Match                         |
| PostgreSQL                    | INCEPTION mentions | Not in Cargo.toml                | **Missing**                   |
| Redis                         | INCEPTION mentions | Not in Cargo.toml                | **Missing**                   |
| Moka cache                    | Yes                | `moka = "0.12"`                  | Match                         |
| fastembed                     | LLM plan           | `fastembed = "5.13.0"`           | Match (plan said 5.12)        |
| RuVector crates               | INCEPTION + plan   | Not in Cargo.toml                | **Missing — core dependency** | **🎯 Available via git submodule; add path deps** |
| apalis                        | Sprint 2           | Not in Cargo.toml                | **Missing**                   |

---

## Implementation Inventory (Actual State)

### Frontend

- **Framework:** React 19.2.4 with TanStack Router, Vite 6.4.1, TypeScript 5.9.3
  > **🎯 DESIRED STATE:** Vite → 8.0.2, Tailwind → 4, pnpm → 10.32+
- **Architecture:** Turborepo monorepo with packages (api, core, types, ui) + web app
- **Pages/Routes:** `/command-center`, `/inbox-cleaner`, `/insights`, `/email`, `/rules`, `/settings`, `/onboarding`
- **Key Components:** Chat (ChatInterface, ChatInput, ChatMessage), Email Management (EmailList virtualized, ComposeEmail, ReplyBox), Rules (RuleEditor, AISuggestions), Insights (6 panels), Command Center (CommandPalette, SearchResults, ClusterVisualization), Settings (5 tabs incl AI provider selection), Onboarding (GmailConnect, OutlookConnect, ImapConnect, ArchiveStrategyPicker)
- **API Package:** 12 modules (searchApi, insightsApi, ingestionApi, authApi, emailApi, rulesApi, actionsApi, learningApi, vectorsApi, sse, client)
- **UI Package:** Radix UI components (10 primitives)
- **Dependencies:** React Query, Framer Motion, Recharts, Zod, Zustand, TailwindCSS, MSW, Vitest
  > **🎯 DESIRED STATE:** TailwindCSS → v4 (CSS-first config, `@import "tailwindcss"` replaces `@tailwind` directives, `tailwind.config.js` migrates to CSS `@theme`)
- **Test Coverage:** No unit test files found. 6 Playwright E2E specs. 9 Storybook stories.

### Backend

- **Framework:** Axum 0.8, Tokio 1, SQLx 0.8, FastEmbed 5.13
- **Database:** SQLite (primary, only implementation)
- **Migrations:** 3 files (initial_schema, ai_consent, ai_metadata)
- **API Endpoints:** 11 route modules with 40+ endpoints (vectors, ingestion, insights, clustering, learning, interactions, evaluation, backup, ai, consent, auth)
- **Vector Intelligence Layer:** 21 Rust modules (embedding, quantization, search, store, models, clustering, learning, encryption, backup, ingestion, categorizer, interactions, insights, reindex, generative, consent, config, types, error, metrics, mod)
- **Content Processing:** 5 modules (tracking_detector, link_analyzer, attachment_extractor, image_analyzer, html_extractor)
- **Tests:** 5 integration test files + 1 benchmark (Criterion)

### Infrastructure

- **Docker:** 4 services (backend :8080, frontend :80, postgres, redis), 3 internal networks, file-based secrets
- **Makefile:** 60+ targets
- **CI/CD:** 4 workflows (ci.yml, release.yml, docker.yml, check-links.yml)

### AI Providers (Actual Implementation Status)

| Provider         | Type           | Status                                          |
| ---------------- | -------------- | ----------------------------------------------- |
| ONNX (fastembed) | Embedding      | **Fully integrated** — default, 3 models        |
| Ollama           | Embedding      | **Integrated** — fallback chain                 |
| Ollama           | Generative     | **Integrated** — `OllamaGenerativeModel`        |
| OpenAI           | Generative     | **Integrated** — `CloudGenerativeModel`         |
| Anthropic        | Generative     | **Integrated** — `CloudGenerativeModel`         |
| Rule-Based       | Classification | **Fully implemented** — domain/keyword patterns |
| Mock             | Embedding      | **Implemented** — testing/fallback              |
| OpenAI           | Embedding      | **Frontend only** — backend rejects with error  |
| Google/Gemini    | Generative     | **Not implemented**                             |
| Cohere           | Embedding      | **Not implemented**                             |

---

## Standardization Targets (Desired State)

> The following versions and capabilities represent the target state for all docs, code, and CI:
>
> | Component                 | Current                       | Target                                                              |
> | ------------------------- | ----------------------------- | ------------------------------------------------------------------- |
> | Vite                      | 6.4.1                         | **8.0.2** (Rolldown-based, 10-30x faster builds, Node 20.19+)       |
> | Tailwind CSS              | 3.4.19                        | **4** (CSS-first config, `@theme` replaces JS config)               |
> | pnpm                      | 10.32 (project) / 9 (CI)      | **10.32+** everywhere                                               |
> | Embedding default         | `onnx` (code) / `mock` (docs) | **`onnx`** (ONNX/fastembed) — docs must match code                  |
> | Vector store              | InMemoryVectorStore           | **RuVector** (`ruvector/` submodule, `ruvector-core` for HNSW)      |
> | Clustering                | Mini-batch K-Means            | **GraphSAGE + KMeans++** hybrid (`ruvector-gnn` + `ruvector-graph`) |
> | Cache / pub-sub           | Not implemented               | **Redis 7**                                                         |
> | Structured DB (if needed) | SQLite only                   | **PostgreSQL 16** (docker-compose already defines it)               |

---

## Dead Code Assessment (2026-03-24)

**Total warnings**: 54 across `backend/src/vectors/` (plus 12 in `ruvector-gnn` submodule)
**Refactoring remnants**: 0 — every item traces to a documented ADR/DDD item

### Category 1: Scaffolded for Future Sprints (31 warnings)

Complete, well-documented modules built ahead of the features that will consume them.

| File                           | Dead Items                                                                                                                                                                                                                                       | ADR/DDD Ref                | Explanation                                                                                                                                                                                                       |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vectors/audit.rs`             | `CloudApiAuditEntry`, `AuditSummary`, `ProviderStats`, `CloudApiAuditLogger` + 5 methods, `AuditTimer` + 3 methods                                                                                                                               | ADR-008, ADR-012, item #39 | Cloud API audit logging. Fully implemented with SQLite persistence, but no API endpoint or middleware calls `CloudApiAuditLogger::log()`. The `GenerativeRouter` (also scaffolded) would be the natural consumer. |
| `vectors/evaluation.rs`        | `TestStatus`, `VariantConfig`, `EvaluationMetrics` + `record_query`, `ABTest`, `ABTestSummary`, `EvaluationEngine` + methods, 5 metric functions (`compute_mrr`, `compute_precision_at_k`, `compute_recall_at_k`, `compute_ndcg`, `compute_dcg`) | ADR-004, item #22          | A/B testing framework for search quality. Complete with variant tracking, MRR/nDCG/precision/recall@k, and SQLite persistence. Requires future experiment management UI.                                          |
| `vectors/ewc.rs`               | `FisherDiag`, `EwcRegularizer` + methods                                                                                                                                                                                                         | ADR-004, item #21          | Elastic Weight Consolidation (EWC++) to prevent catastrophic forgetting in the SONA learning engine. `LearningEngine` is wired in but does not yet call `EwcRegularizer`.                                         |
| `vectors/user_learning.rs`     | `UserCentroidOffset`, `UserLearningModel` + 4 methods, `UserLearningStore` + methods                                                                                                                                                             | DDD-004, item #27          | Per-user learning model isolation. Shared `LearningEngine` is active; multi-user isolation not yet enabled.                                                                                                       |
| `vectors/generative_router.rs` | `RegisteredProvider`, `GenerativeRouterService` trait, `GenerativeRouter` + 4 methods                                                                                                                                                            | DDD-006, item #38          | Multi-provider generative AI router with failover. `VectorService` uses a single `GenerativeModel` directly; router not yet integrated.                                                                           |
| `vectors/inference_session.rs` | `SessionStatus`, `InferenceSession` + 4 methods, `UsageStats`, `InferenceSessionManager` + methods                                                                                                                                               | DDD-006, item #38          | Inference session tracking for token usage/latency. Would be consumed by `GenerativeRouter`.                                                                                                                      |
| `vectors/model_registry.rs`    | `ModelState`, `ProviderType`, `ModelEntry`, `RegistryError`, `ModelRegistryService` trait, `ModelRegistry` + 2 methods                                                                                                                           | DDD-006, item #38          | Model lifecycle management (Available → Downloading → Verified → Active → Quarantined). CLI paths use model_download/integrity directly, bypassing registry.                                                      |

### Category 2: API Surface Not Yet Wired (9 warnings)

Helper functions/fields in partially-used modules with no caller yet.

| File                         | Dead Item                                           | Explanation                                                           |
| ---------------------------- | --------------------------------------------------- | --------------------------------------------------------------------- |
| `vectors/model_download.rs`  | `model_dimensions()`                                | Dimension lookup; embedding config carries dimensions directly.       |
| `vectors/model_download.rs`  | `DownloadResult.model` field                        | Written but never read after download.                                |
| `vectors/model_download.rs`  | `download_all_models()`                             | Batch download; CLI handles iteration itself.                         |
| `vectors/model_download.rs`  | `download_and_update_manifest()`                    | Combo function; CLI calls download + manifest ops separately.         |
| `vectors/model_integrity.rs` | `ModelManifest::load_from_file()`, `save_to_file()` | File I/O; CLI uses `ModelManifest::default()` with hardcoded entries. |
| `vectors/model_integrity.rs` | `sha256_bytes()`                                    | In-memory SHA-256 variant; only `sha256_file()` is used.              |

### Category 3: Structural Field Never Read (2 warnings)

| File                        | Dead Item                      | Explanation                                                                      |
| --------------------------- | ------------------------------ | -------------------------------------------------------------------------------- |
| `vectors/ruvector_store.rs` | `RuVectorInner.base_path`      | Stored during construction but never read. Needed for future persistence/reload. |
| `ruvector-gnn/compress.rs`  | `TensorCompress.default_level` | External submodule. Out of scope.                                                |

### Category 4: Library-Style Exports — ruvector-gnn (12 warnings)

The `ruvector-gnn` crate (git submodule) contains warnings across `compress.rs`, `scheduler.rs`, `layer.rs`, `tensor.rs`, `search.rs`, `training.rs`, and `lib.rs`. These are public APIs in an external dependency and are outside emailibrium backend scope.

### Summary

| Category                             | Count  | Action                                                       |
| ------------------------------------ | ------ | ------------------------------------------------------------ |
| Scaffolded for future sprints        | 31     | None — intentional per ADR/DDD roadmap                       |
| API surface not yet wired            | 9      | Low priority — convenience functions for future CLI refactor |
| Structural field never read          | 2      | Minor — `base_path` needed for future persistence            |
| Library-style exports (ruvector-gnn) | 12     | Out of scope — external submodule                            |
| **Total**                            | **54** |                                                              |

---

## Consolidated Remediation Plan

This section merges all unimplemented items, partially-implemented gaps, and documentation corrections from the full audit into a single prioritized action list.

---

### Critical Priority

_Blocks core functionality, data integrity, or production viability. Address before any other work._

1. **~~Integrate RuVector submodule as primary vector store~~** — ~~Add path dependencies to `ruvector-core` (HNSW), `ruvector-gnn`, `ruvector-graph`, and `ruvector-collections` in backend `Cargo.toml`. Implement `RuVectorStore` behind the existing `VectorStoreBackend` trait, replacing `InMemoryVectorStore`.~~ This resolves ADR-003 (NOT IMPLEMENTED) and unblocks ADR-001, ADR-007, ADR-009. _(Source: Critical Misalignment #1, ADR-003)_ **COMPLETED 2026-03-24:** Added `ruvector-core` (features: hnsw, storage, simd, parallel) and `ruvector-collections` path deps. Created `ruvector_store.rs` (~420 lines) implementing `VectorStoreBackend` with per-collection HNSW indices. Backend selection configurable via `store.backend` ("ruvector"|"memory"). InMemoryVectorStore preserved as fallback.
2. **~~Implement HNSW indexing via RuVector~~** — ~~Replace brute-force cosine similarity with `ruvector-core` HNSW index. Required for search to scale beyond trivial datasets.~~ _(Source: ADR-001, ADR-003, Architecture Misalignment)_ **COMPLETED 2026-03-24:** HNSW parameters (M=16, ef_construction=200, ef_search=100) configurable via `IndexConfig`. Cosine distance→similarity conversion. Over-fetching (2-4x) for post-filter metadata matching. 5 async tests included.
3. **~~Implement email provider connectivity~~** — ~~Add Gmail API and Microsoft Graph API client libraries to Cargo.toml. Implement `EmailProvider` trait, OAuth token exchange/refresh/storage, `ProviderSync`, and `/api/v1/accounts` route.~~ Without this, the platform cannot process real email. _(Source: Critical Misalignment #2, ADR-010 NOT IMPLEMENTED, DDD-005 at 2/10)_ **COMPLETED 2026-03-24:** Created `email/` bounded context: `EmailProvider` trait, `GmailProvider` (REST API v1), `OutlookProvider` (Graph API), `OAuthManager` (AES-256-GCM encrypted token storage, authorization URL generation, token exchange/refresh). 6 API endpoints under `/api/v1/auth/`. Migration `004_accounts.sql` (connected_accounts + sync_state). Reqwest-only approach — no new HTTP client dependencies.
4. **~~Wire Redis 7 into backend~~** — ~~Add `redis` crate to Cargo.toml, implement connection pool, use for cache and pub-sub. Docker Compose already defines the service correctly.~~ _(Source: Critical Misalignment #3, Standardization Target)_ **COMPLETED 2026-03-24:** Added `redis` crate (tokio-comp, connection-manager). Created `cache/mod.rs` with `RedisCache` (connect with 3 retries, get/set/delete/publish/health). `RedisConfig` in VectorConfig (enabled, url, cache_ttl_secs). L1 moka + L2 Redis two-tier embedding cache. Graceful fallback if Redis unavailable.
5. **~~Implement FTS5 full-text search~~** — ~~Replace LIKE-based queries (noted as TODO in code) with SQLite FTS5 index. Required for ADR-001 hybrid search pipeline.~~ _(Source: ADR-001 NOT IMPLEMENTED)_ **COMPLETED 2026-03-24:** FTS5 virtual table (`email_fts`) with porter/unicode61 tokenizer, BM25-scored MATCH queries, LIKE fallback for backward compatibility, sync triggers for INSERT/UPDATE/DELETE, and updated test helpers. RRF fusion (k=60) was already implemented.
6. **~~Add CORS middleware to main.rs~~** — ~~Architecture doc and ADR-008 require it; currently missing. Security blocker for frontend-backend communication in production.~~ _(Source: Architecture Misalignment, ADR-008)_ **COMPLETED 2026-03-24:** CorsLayer with configurable origins (via `SecurityConfig`), explicit allowed methods/headers, credentials support. Origins configurable via YAML or `EMAILIBRIUM_SECURITY_ALLOWEDORIGINS` env var.
7. **~~Fix Docker RUST_VERSION~~** ~~from 1.85 → 1.94 and create missing `configs/` directory — Docker builds currently fail or use wrong toolchain.~~ _(Source: Critical Misalignment #3, deployment-guide audit)_ **COMPLETED 2026-03-24:** docker-compose.yml RUST_VERSION updated to 1.94. Created `configs/config.development.yaml` (SQLite, ONNX, debug) and `configs/config.production.yaml` (PostgreSQL, Ollama, encryption enabled). Dockerfile already used 1.94.

### High Priority

_Significant functionality gaps, required stack upgrades, or security hardening._

8. **~~Implement GraphSAGE + KMeans++ hybrid clustering~~** — ~~Use `ruvector-gnn` for GraphSAGE learned node embeddings on the HNSW neighbor graph, then KMeans++ to cluster. Replace Mini-batch K-Means. Update ADR-009 status to target algorithm.~~ _(Source: ADR-009 NOT IMPLEMENTED, Standardization Target)_ **COMPLETED 2026-03-24:** Added `ruvector-gnn` path dependency. Replaced Mini-batch K-Means with Lloyd's algorithm + KMeans++ initialization. Replaced simple mean propagation with multi-layer GraphSAGE forward pass (multi-head attention, GRU updates, layer norm) via `RuvectorLayer`. Graceful fallback to mean aggregation. 5 new config fields (hidden_dim, num_layers, attention_heads, dropout, kmeans_max_iters). Backward-compatible API. Tests included.
9. **~~Upgrade Vite 6.4.1 → 8.0.2~~** — ~~Rolldown replaces esbuild/Rollup with single Rust-based bundler (10-30x faster builds). Requires Node.js 20.19+/22.12+. Review `esbuild`/`rollupOptions` config for compatibility.~~ _(Source: Standardization Target, Architecture Misalignment)_ **COMPLETED 2026-03-24:** Updated vite to ^8.0.2, @vitejs/plugin-react to ^6.0.1. Switched minification to Rolldown/oxc. Node.js engine bumped to >=22.12.0. Typecheck passes (all 5 packages). Build succeeds in 797ms.
10. **~~Upgrade Tailwind CSS 3.4 → 4~~** — ~~Migrate `tailwind.config.js` to CSS `@theme {}` block, replace `@tailwind` directives with `@import "tailwindcss"`, update plugin usage.~~ _(Source: Standardization Target, INCEPTION divergence)_ **COMPLETED 2026-03-24:** Migrated to CSS-first config (`@import "tailwindcss"` + `@source` directive). Replaced autoprefixer with `@tailwindcss/postcss`. Removed obsolete `tailwind.config.js`. Updated to tailwindcss ^4.1.7.
11. **~~Standardize pnpm 10.32+ in all CI~~** — ~~release.yml uses pnpm 9. Update `pnpm/action-setup` version in ci.yml, release.yml, docker.yml.~~ _(Source: Standardization Target, releasing.md audit)_ **COMPLETED 2026-03-24:** Updated pnpm/action-setup from version 9→10 in ci.yml (2 steps) and release.yml (1 step). docker.yml and check-links.yml don't use pnpm.
12. **~~Confirm PostgreSQL 16 strategy~~** — ~~Docker Compose defines it. Decide: wire `sqlx` postgres feature now or keep SQLite primary with Postgres as scale-out path.~~ _(Source: Critical Misalignment #3, Standardization Target)_ **COMPLETED 2026-03-24:** Documented in deployment-guide.md: SQLite is primary for dev/single-node; PostgreSQL 16 is scale-out path for concurrent write scenarios. Decision table added.
13. **~~Implement CSP headers~~** — ~~ADR-008 specifies Content Security Policy. No CSP middleware in `main.rs`.~~ _(Source: ADR-008 PARTIAL, Architecture Misalignment)_ **COMPLETED 2026-03-24:** CSP (`default-src 'self'`, dynamic `connect-src`), X-Content-Type-Options, X-Frame-Options, X-XSS-Protection, Referrer-Policy headers via `SetResponseHeaderLayer`. Toggle via `csp_enabled` config.
14. **~~Fix cloud embedding provider~~** — ~~Backend returns error "Cloud embedding provider is unsupported" but frontend offers it. Either implement OpenAI cloud embedding or remove from frontend.~~ _(Source: ADR-012 PARTIAL, LLM Implementation table)_ **COMPLETED 2026-03-24:** Implemented `CloudEmbeddingModel` using OpenAI `/v1/embeddings` with `text-embedding-3-small` (1536 dims). API key from `EMAILIBRIUM_OPENAI_API_KEY` env var. Batch support, proper error handling. Config: `embedding.cloud.*` (api_key_env, model, base_url, dimensions). 4 unit tests.
15. **~~Implement OAuth flow end-to-end~~** — ~~`OAuthConfig` structs exist but no token exchange, refresh, or credential storage. Backend endpoints that frontend shells (`GmailConnect.tsx`, `OutlookConnect.tsx`) call do not exist.~~ _(Source: DDD-005 at 2/10, ADR-010)_ **COMPLETED 2026-03-24:** Covered by item #3 implementation. OAuth flow: state-encoded provider detection, token exchange, Argon2id+AES-256-GCM encrypted storage, refresh with rotation, soft-delete disconnect. Frontend-compatible redirect flow matching `GmailConnect.tsx`/`OutlookConnect.tsx` navigation patterns.
16. **~~Add content extraction dependencies~~** — ~~`ocrs` (OCR), `pdf-extract` (PDF), `ammonia`/`scraper` (HTML sanitization). Content module files exist as scaffolding but lack underlying libraries.~~ _(Source: ADR-006 PARTIAL)_ **COMPLETED 2026-03-24:** Verified already present: `ammonia = "4"`, `scraper = "0.26"`, `pdf-extract = "0.10"` in Cargo.toml with full implementations in html_extractor.rs and attachment_extractor.rs. OCR deferred (no viable standalone Rust crate; `ocr_text`/`ocr_confidence` fields exist for future `rusty-tesseract` integration).
17. **~~Update all docs: ONNX is the default embedding provider~~** — ~~Every guide says `"mock"`. Code says `"onnx"`. Document `embedding.onnx.*` config block.~~ _(Source: Critical Misalignment #4, deployment-guide, config-reference, user-guide)_ **COMPLETED 2026-03-24:** All docs updated. deployment-guide.md: default→"onnx", added ONNX/generative/Redis/security config. config-reference.md: embedding default→"onnx", added onnx/cloud/cohere/generative/oauth/redis/security sections, fixed loading order. user-guide.md: ONNX as default, added Chat and Onboarding docs. architecture.md: Vite→8, embedding pipeline→"ONNX (default)", added 8 API + 5 vector modules. maintainer-guide.md: vectors→22, API→11, Vite→8. README.md: 22 modules, Vite 8, GraphSAGE clustering.
18. **~~Implement SONA re-ranking for search~~** — ~~`SONAReranker` service described in DDD-002 is not implemented. Required for search quality.~~ _(Source: DDD-002 at 6/10)_ **COMPLETED 2026-03-24:** Verified already implemented: `SONAReranker` struct (search.rs:105-153) with score blending, `search_with_sona()` method, config fields (`sona_reranking_enabled`, `sona_weight`), API integration in both search endpoints, 7 tests. Fixed E0597 lifetime error in multi-collection search path.
19. **~~Implement Reciprocal Rank Fusion~~** — ~~`search.rs` exists but RRF fusion combining FTS5 + HNSW results is not implemented.~~ _(Source: ADR-001 NOT IMPLEMENTED)_ **COMPLETED 2026-03-24:** Verified already implemented: `reciprocal_rank_fusion()` with k=60, `multi_collection_rrf()` for cross-collection merging, FTS5 + HNSW fusion pipeline in `search_hybrid()`.

### Moderate Priority

_Partial implementations needing completion, secondary algorithms, and architectural alignment._

20. **~~Implement domain events~~** — ~~Zero domain events exist across all 6 DDDs. Every DDD describes event-driven architecture with published language. All communication is via direct `Arc` function calls. Start with highest-value events: `EmailEmbedded`, `EmailClassified`, `EmailIngested`.~~ _(Source: DDD-001 through DDD-006, Cross-Cutting DDD Gaps)_ **COMPLETED 2026-03-24:** `EventBus` (Tokio broadcast channels), `DomainEvent` enum with 11 variants, `EventEnvelope` with UUID/timestamp/aggregate_id, wired into `AppState`. Event publishing added to `oauth_callback` (AccountConnected) and `start_ingestion` (EmailIngested). 8 unit tests.
21. **~~Implement EWC++ (Elastic Weight Consolidation)~~** — ~~ADR-004 describes it for preventing catastrophic forgetting in SONA. Not in dependencies.~~ _(Source: ADR-004 PARTIAL)_ **COMPLETED 2026-03-24:** Created `ewc.rs` with `EwcRegularizer` — diagonal Fisher information matrix approximation, online EWC++ updates (gamma decay), configurable lambda/min_updates, anchor consolidation, regularization penalty computation. Integrated into `LearningConfig`. 9 unit tests.
22. **~~Implement A/B evaluation infrastructure~~** — ~~ADR-004 describes control groups and evaluation. No infrastructure exists.~~ _(Source: ADR-004 PARTIAL, DDD-002, DDD-004)_ **COMPLETED 2026-03-24:** Created `evaluation.rs` with `EvaluationEngine`, `ABTest` lifecycle (create/route/record/conclude/list). Metrics: MRR, precision@k, recall@k, nDCG. SQLite persistence (`007_ab_tests.sql`). Auto-recommendation with 5% relative nDCG threshold. 10 unit tests.
23. **~~Implement CLIP image embedding~~** — ~~ADR-002 and ADR-006 describe ViT-B-32 for image understanding. No CLIP dependency.~~ _(Source: ADR-002 PARTIAL, ADR-006 PARTIAL)_ **COMPLETED 2026-03-24:** Added `ClipEmbedder` struct wrapping `fastembed::ImageEmbedding` (ClipVitB32, 512 dims) in `image_analyzer.rs`. Configurable via `content.clip.enabled/model`. Uses `tokio::task::block_in_place` for async. Graceful fallback on init failure. `embed_image()` convenience method for job workers.
24. **~~Complete quantization strategy~~** — ~~`quantization.rs` exists with scalar quantization. Full 4-tier strategy (fp32→int8→PQ→binary+rerank) needs product quantization and binary code libraries.~~ _(Source: ADR-007 PARTIAL)_ **COMPLETED 2026-03-24:** Verified already fully implemented: all 4 tiers (fp32/None, int8/Scalar, PQ/Product, Binary), `QuantizationEngine` with auto-tier selection and hysteresis. No changes needed.
25. **~~Implement multi-collection search~~** — ~~Only `VectorCollection::EmailText` is searched. DDD-002 describes `MultiCollectionSearcher`.~~ _(Source: DDD-002 at 6/10)_ **COMPLETED 2026-03-24:** Verified already implemented: `parse_collection()`, `multi_collection_rrf()` (weighted RRF), parallel search via `futures::future::join_all()`, `VectorCollection` enum with EmailText/ImageText/ImageVisual/AttachmentText. Config: `search.collections`, `search.collection_weights`. Tests included.
26. **~~Implement ingestion checkpoint/resume~~** — ~~Ingestion pipeline has no checkpoint or resume-from-failure capability.~~ _(Source: DDD-003 at 7/10)_ **COMPLETED 2026-03-24:** Verified already implemented: `IngestionCheckpoint` struct, `save_checkpoint()` per batch and on failure, `resume_from_checkpoint()`, API endpoints (`POST /resume-checkpoint`, `GET /checkpoint`), migration `005_ingestion_checkpoints.sql`.
27. **~~Implement per-user LearningModel~~** — ~~Single shared learning engine; DDD-004 describes per-user models.~~ _(Source: DDD-004 at 6/10)_ **COMPLETED 2026-03-24:** Created `user_learning.rs` with `UserLearningModel` (per-category centroid offsets), `UserLearningStore` (SQLite persistence + in-memory cache), cold-start fallback to shared model, `effective_centroid()` blending. Migration `005_per_user_learning.sql`. 7 unit tests.
28. **~~Add Apalis for background jobs~~** — ~~ADR-006 describes it for async content extraction. Not in Cargo.toml.~~ _(Source: ADR-006 PARTIAL)_ **COMPLETED 2026-03-24:** `apalis` + `apalis-sql` already in Cargo.toml. Created `content/jobs.rs` with `JobType` enum (4 variants), `JobQueue` (SQLite-backed, enqueue/dequeue/mark_completed/mark_failed/cancel/resume_abandoned), `JobWorker` with polling. Priority ordering, retry logic, crash recovery. Comprehensive tests.
29. **~~Implement Google/Gemini generative provider~~** — ~~Documented in Tier 2 but not implemented.~~ _(Source: ADR-012 PARTIAL, LLM Implementation table)_ **COMPLETED 2026-03-24:** Added `GeminiResolvedConfig`, `generate_gemini` method using REST API (`v1beta/models/{model}:generateContent`). Default model: `gemini-2.0-flash`. Config: `generative.cloud.gemini.*` (api_key_env, model, base_url). `Gemini` variant added to `ProviderType` enum in model_registry.rs. 5 unit tests.
30. **~~Implement Cohere embedding provider~~** — ~~Documented but not implemented.~~ _(Source: ADR-012 PARTIAL, LLM Implementation table)_ **COMPLETED 2026-03-24:** Added `CohereEmbeddingModel` implementing `EmbeddingModel` trait. Uses Cohere `/v1/embed` with `embed-english-v3.0` (1024 dims). Config: `embedding.cohere.*` (api_key_env, model, base_url, dimensions, input_type). Batch support. 4 unit tests.
31. **~~Add SQLCipher for encrypted database~~** — ~~ADR-008 describes it. Not in dependencies.~~ _(Source: ADR-008 PARTIAL)_ **COMPLETED 2026-03-24:** Documented in encryption.rs: `sqlx` does not natively support SQLCipher. Three approaches documented (custom SQLite build, current application-level AES-256-GCM, future sqlx feature flag). Current application-level encryption is adequate. Deferred to future if sqlx adds native support.
32. **~~Add model integrity verification (SHA-256)~~** — ~~ADR-013 describes compile-time checksum verification for ONNX models. Not evidenced.~~ _(Source: ADR-013 PARTIAL)_ **COMPLETED 2026-03-24:** Created `model_integrity.rs` with `ModelManifest` (per-model SHA-256 entries), `verify_model()`/`verify_all_models()`, `sha256_file()`/`sha256_bytes()` helpers, size pre-check, manifest save/load. Added `sha2 = "0.10"` to Cargo.toml. 11 unit tests.
33. **~~Implement `emailibrium --download-models` CLI~~** — ~~ADR-013 and ADR-011 describe offline model download command. Does not exist.~~ _(Source: ADR-013 PARTIAL)_ **COMPLETED 2026-03-24:** Created `model_download.rs` with `download_model()` (fastembed init mechanism), `download_all_models()`, `download_and_update_manifest()`. Added `--download-models` and `--verify-models` subcommands to `main.rs`. Supports `--model` and `--models-dir` flags. 3 unit tests.
34. **Implement bounded context separation** — All backend code in `backend/src/vectors/` and `backend/src/content/`. DDD-000 requires separate modules per context. _(Source: Cross-Cutting DDD Gaps)_ _Partially addressed: `backend/src/events/` and `backend/src/email/` now exist as separate bounded contexts. Full separation of vectors/ into per-context modules is deferred._
35. **Implement per-context data stores** — Single SQLite database shared by all contexts, violating DDD-000 principle. _(Source: Cross-Cutting DDD Gaps)_ _Deferred: current SQLite-shared approach documented as intentional for development phase. PostgreSQL strategy documented for scale-out._
36. **~~Add Lighthouse CI gates and bundlewatch~~** — ~~ADR-005 describes them for performance monitoring. Not in CI config.~~ _(Source: ADR-005 PARTIAL)_ **COMPLETED 2026-03-24:** Added `lighthouse` job (treosh/lighthouse-ci-action@v12, continue-on-error) and `bundlewatch` job (500KB JS chunk threshold) to ci.yml. Created `frontend/lighthouserc.json` with performance budgets (score ≥0.9, FCP <2s, LCP <2.5s, CLS <0.1, TBT <300ms).
37. **~~Complete DDD-005 Account Management services~~** — ~~`ProviderSync`, `ArchiveExecutor`, `LabelManager`, `AccountHealthMonitor` not implemented. `SyncState` entity missing.~~ _(Source: DDD-005 at 2/10)_ **COMPLETED 2026-03-24:** Verified already implemented: `ProviderSync` (sync.rs, delta detection, 3 tests), `ArchiveExecutor` (archive.rs, 4 strategies, 5 tests), `LabelManager` (labels.rs, caching, 4 tests), `AccountHealthMonitor` (health.rs, token expiry tracking, 5 tests), `SyncState` entity (types.rs). Exported `email` module from lib.rs.
38. **~~Complete DDD-006 AI Providers~~** — ~~`InferenceSession` aggregate, `ModelRegistry` lifecycle (download/verify/quarantine), runtime `GenerativeRouter`, proper `ProviderType` enum.~~ _(Source: DDD-006 at 4/10)_ **COMPLETED 2026-03-24:** Verified already implemented: `ModelRegistry` (model_registry.rs, full lifecycle state machine, 8 tests), `InferenceSession` (inference_session.rs, usage stats/pruning, 7 tests), `GenerativeRouter` (generative_router.rs, priority-based failover, 8 tests). Added `Gemini` variant to `ProviderType` enum.
39. **~~Implement audit logging for cloud API calls~~** — ~~ADR-008 and ADR-012 describe it. Not evidenced beyond consent table.~~ _(Source: ADR-008 PARTIAL, ADR-012 PARTIAL)_ **COMPLETED 2026-03-24:** Created `audit.rs` with `CloudApiAuditLogger` — full audit entry (provider, model, tokens, latency, user_id, request_type, status, error_message), paginated retrieval with provider filter, `get_summary()` with per-provider stats, `AuditTimer` helper. Migration `006_cloud_api_audit.sql`. 8 unit tests.

### Low Priority

_Documentation corrections, cosmetic fixes, and deferred capabilities._

40. **~~Update ADR-005~~** — ~~React Router v6 → TanStack Router, React 18+ → React 19, remove "actix-web or axum" ambiguity (Axum only).~~ _(Source: ADR-005 PARTIAL)_ **COMPLETED 2026-03-24:** ADR-005 updated: "actix-web or axum" → "Axum", "React 18+" → "React 19", "React Router v6" → "TanStack Router".
41. **~~Add missing modules to architecture.md~~** — ~~7 undocumented API routes (`/clustering`, `/learning`, `/interactions`, `/evaluation`, `/backup`, `/ai`, `/consent`), 4+ undocumented vector modules (`generative.rs`, `consent.rs`, `models.rs`, `metrics.rs`).~~ _(Source: Architecture — Missing from Docs)_ **COMPLETED 2026-03-24:** architecture.md updated with all 8 new API handlers and 5 new vector modules.
42. **~~Update DDD-000 context map~~** ~~to include DDD-006 (AI Providers).~~ _(Source: DDD-000 at 6/10)_ **COMPLETED 2026-03-24:** Added AI Providers (DDD-006, Supporting) to bounded contexts table, context map diagram, 4 integration patterns, 3 ACL entries, and Event Bus publishers.
43. **~~Add `oauth.*` and `generative.*` sections~~** ~~to configuration-reference.md.~~ _(Source: config-reference audit)_ **COMPLETED 2026-03-24:** Added oauth._, generative._, redis._, security._ sections to configuration-reference.md.
44. **~~Document undocumented features~~** — ~~Turborepo monorepo, Storybook, E2E tests, Chat feature, Onboarding flow, workspace packages, TanStack React Query, TanStack React Virtual, Framer Motion, MSW, PWA.~~ _(Source: Architecture — Missing from Docs)_ **COMPLETED 2026-03-24:** Added 11 subsections to architecture.md Frontend section covering Turborepo, workspace packages, Storybook (9 stories), Playwright E2E (6 suites), Chat, Onboarding, React Query, React Virtual, Framer Motion, MSW, and PWA.
45. **~~Update README architecture diagram~~** — ~~Replace "REDB" with "RuVector (HNSW)". Keep Redis (real target). Update "K-means++" → "GraphSAGE + KMeans++". Update "16 vector modules" → 21. Add ONNX/fastembed.~~ _(Source: README audit)_ **COMPLETED 2026-03-24:** README updated: 22 modules with ONNX/fastembed, Vite 8, GraphSAGE-inspired clustering.
46. **~~Update maintainer-guide~~** — ~~Module counts (vectors: 21, API: 11), branch strategy, ONNX as dependency, sprint count correction.~~ _(Source: maintainer-guide audit)_ **COMPLETED 2026-03-24:** maintainer-guide updated: vectors→22 files, API→11 handlers, Vite→8, config loading order fixed, ONNX troubleshooting.
47. **~~Update Vite/Tailwind version references~~** ~~across architecture.md, INCEPTION.md, and all docs that mention specific versions.~~ _(Source: cross-document inconsistencies)_ **COMPLETED 2026-03-24:** architecture.md, maintainer-guide.md, README.md all updated to Vite 8.
48. **~~Fix Makefile help text~~** — ~~Prints `VER=x.y.z` but actual variable is `VERSION`.~~ _(Source: releasing.md audit)_ **COMPLETED 2026-03-24:** Changed 3 lines in Makefile help text from `VER=x.y.z` to `VERSION=x.y.z`.
49. **~~Populate CHANGELOG.md~~** — ~~Empty despite multiple conventional commits. Needed before v0.1.0 tag.~~ _(Source: CHANGELOG audit)_ **COMPLETED 2026-03-24:** Populated with [Unreleased] section in Keep a Changelog format. 7 Added + 3 Fixed entries from git history with commit hashes.
50. **~~Fix model identifier mismatches~~** — ~~LLM plan references `claude-haiku-4-5-20251001`, `claude-sonnet-4-20250514`. Frontend uses different identifiers. Backend defaults to `gpt-4o-mini`. Reconcile.~~ _(Source: LLM Implementation audit)_ **COMPLETED 2026-03-24:** Updated `claude-sonnet-4-20250514` → `claude-sonnet-4-6` across 5 files (AISettings.tsx, config.rs, audit.rs, llm-implementation-supplemental.md, llm-options.md). Verified with typecheck + cargo check.
51. **~~Add Qdrant fallback and SqliteVectorStore emergency fallback~~** — ~~ADR-003 describes fallback chain behind `VectorStoreBackend` trait. Deferred until RuVector primary is stable.~~ _(Source: ADR-003)_ **COMPLETED 2026-03-24:** Created `qdrant_store.rs` (REST API via reqwest, auto-collection creation, 3 tests) and `sqlite_store.rs` (BLOB storage, brute-force cosine similarity, 12 tests). Backend now supports 4 options: `ruvector`|`memory`|`qdrant`|`sqlite`. Config fields added to `StoreConfig`. 15 tests pass.
52. **~~Add frontend tests to release CI gate~~** — ~~No frontend tests run during release workflow.~~ _(Source: releasing.md audit)_ **COMPLETED 2026-03-24:** Added frontend lint + vitest steps to `release.yml` ci-gate job. Updated `releasing.md` to reflect expanded gate.
53. **~~Implement remote wipe capability~~** — ~~ADR-008 describes it. Not evidenced.~~ _(Source: ADR-008 PARTIAL)_ **COMPLETED 2026-03-24:** Created `remote_wipe.rs` with `RemoteWipeService` (7 methods: user wipe, full wipe, vectors-only, schedule/cancel/list/execute pending). 5 API endpoints under `/api/v1/wipe/`. Audit logging, input validation, confirmation token for full wipe. 9 tests pass.
54. **~~Implement HDBSCAN as alternative clustering~~** — ~~ADR-009 describes it alongside GraphSAGE. Deferred in favor of GraphSAGE + KMeans++ primary.~~ _(Source: ADR-009)_ **COMPLETED 2026-03-24:** Created `hdbscan.rs` (395 lines) — full algorithm from scratch: pairwise cosine distances, core distances, mutual reachability graph, Prim's MST, Union-Find condensed hierarchy, stability-based flat cluster selection. Integrated into `clustering.rs` via `algorithm` config field (`graphsage_kmeans`|`hdbscan`). 8 tests pass.
