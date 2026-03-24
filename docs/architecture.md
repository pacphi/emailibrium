# Architecture Overview

> **Repository**: [github.com/pacphi/emailibrium](https://github.com/pacphi/emailibrium)

## System Architecture

Emailibrium is a **vector-native email intelligence platform** organized as a four-tier architecture (INCEPTION.md Section 3):

```
+------------------------------------------------------------------+
|                    PRESENTATION TIER                               |
|  React TypeScript SPA (Vite 7 + TanStack Router + shadcn/ui)     |
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
|  Embedding Pipeline (ADR-002): mock -> Ollama -> cloud fallback  |
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

| Context | Type | Document | Responsibility |
|---------|------|----------|----------------|
| **Email Intelligence** | Core | DDD-001 | Embedding generation, vector storage, classification, clustering |
| **Search** | Core | DDD-002 | Query execution, result fusion (FTS5 + HNSW), SONA re-ranking |
| **Ingestion** | Supporting | DDD-003 | Email sync from providers, multi-asset extraction, pipeline orchestration |
| **Learning** | Supporting | DDD-004 | SONA adaptive learning, centroid updates, feedback processing |
| **Account Management** | Supporting | DDD-005 | Provider connections (OAuth), sync state, archive strategy |

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

The email ingestion pipeline processes emails through six stages (INCEPTION.md Section 3.2):

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

| ADR | Title | Decision | Key Trade-off |
|-----|-------|----------|---------------|
| ADR-001 | Hybrid Search Architecture | FTS5 + HNSW + Reciprocal Rank Fusion | Complexity vs. search quality across exact and semantic queries |
| ADR-002 | Embedding Model Selection | Pluggable pipeline with fallback chain (local -> Ollama -> cloud) | Latency vs. quality; mock fallback ensures the system always works |
| ADR-003 | Vector Database | RuVector as primary store; SQLite backup for persistence | Rust-native performance vs. ecosystem maturity |
| ADR-004 | Adaptive Learning | SONA self-learning with centroid-based classification + LLM fallback | Continuous improvement vs. classification stability |
| ADR-005 | Frontend Architecture | Pure React TypeScript SPA replacing Tauri 2.0 desktop app | Web accessibility vs. native desktop integration |
| ADR-006 | Content Extraction | Multi-asset pipeline (HTML, images, attachments, links) | Extraction breadth vs. reliability across input types |
| ADR-007 | Quantization | Adaptive scalar quantization based on corpus size | 4x memory reduction vs. slight recall degradation |
| ADR-008 | Privacy & Security | AES-256-GCM encryption at rest, Argon2id key derivation, embedding noise | ~5-10% performance overhead vs. data protection |
| ADR-009 | Clustering | GraphSAGE on HNSW neighbor graph for topic discovery | Novel approach (needs empirical validation) vs. proven methods |
| ADR-010 | Inbox Strategy | Ingest-Tag-Archive pipeline ("Gmail is dumb store, Emailibrium is smart interface") | Aggressive automation vs. user safety and undo capability |

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
    encryption.rs      # AES-256-GCM encryption at rest (ADR-008)
    config.rs          # Layered configuration (figment)
    types.rs           # Core value objects (VectorDocument, etc.)
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
```

### Frontend (`frontend/`)

React TypeScript SPA built with Vite, TanStack Router, Zustand state management, and shadcn/ui components.

## Security Architecture

Refer to ADR-008 for the full security design. Key points:

- **Encryption at rest**: AES-256-GCM for vector store persistence
- **Key derivation**: Argon2id from master password; key held in memory only, zeroed on drop (`zeroize` crate)
- **Content Security Policy**: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'`
- **CORS**: Restricted to specific origins (not wildcard)
- **OAuth tokens**: Stored via Web Crypto API in browser (not localStorage)
- **Embedding privacy**: Calibrated Gaussian noise injection (differential privacy lite)
