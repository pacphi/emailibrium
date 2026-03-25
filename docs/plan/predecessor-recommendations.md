# Predecessor Comparison: pacphi/emailibrium vs Current Repo

**Date:** 2026-03-24
**Purpose:** Identify capabilities from the predecessor Rust/Tauri implementation worth adopting into the current vector-native platform.

---

## Executive Summary

The predecessor (`pacphi/emailibrium`) is a Rust + Tauri 2.0 desktop application targeting inbox zero with multi-LLM support, offline-first sync, and a rule engine. The current repo is a Rust + React web SPA emphasizing vector-native intelligence (HNSW, SONA learning, semantic search, GraphSAGE clustering). Both share Rust/Axum backends and React frontends but diverge significantly in AI strategy and deployment model.

**Key finding:** The predecessor has 12 production-ready capabilities that the current repo either lacks entirely or has only scaffolded. Adopting them would close functional gaps without conflicting with the current vector-native architecture.

---

## Capability Comparison Matrix

| Capability                               | Predecessor                                       | Current Repo                                    | Gap                  |
| ---------------------------------------- | ------------------------------------------------- | ----------------------------------------------- | -------------------- |
| **Email Providers (Gmail/Outlook/IMAP)** | Full API clients wired                            | OAuth UI scaffolded, no API clients             | Critical             |
| **POP3 Support**                         | Implemented                                       | Not present                                     | Minor                |
| **Offline-First Sync**                   | Queue-based with CRDT conflict resolution         | Not implemented                                 | Significant          |
| **Rule Engine**                          | Full parser + validator + processor               | AI-suggested rules (UI only)                    | Significant          |
| **Adaptive Categorizer (feedback loop)** | Per-user training from corrections                | SONA learning (designed, partially wired)       | Moderate             |
| **AI Chat Interface**                    | Streaming responses, session mgmt                 | Chat UI exists, no LLM wired                    | Moderate             |
| **Bulk Unsubscribe**                     | Intelligent detection + false-positive prevention | Subscription detection (insights only)          | Moderate             |
| **Native Desktop (Tauri 2.0)**           | Cross-platform DMG/MSI/DEB                        | Web SPA + PWA                                   | Architectural choice |
| **Hot-Reload Config**                    | File-watch with live updates                      | Layered figment config (no hot-reload)          | Minor                |
| **Redis Distributed Cache**              | Moka + Redis fallback                             | Moka only (Redis in docker-compose but unwired) | Minor                |
| **Processing Checkpoints**               | Resume batch jobs after crash                     | Not implemented                                 | Moderate             |
| **Rate Limiting Middleware**             | Governor-based per-route                          | Not implemented                                 | Moderate             |
| **Security Headers Middleware**          | X-Frame-Options, CSP, HSTS                        | CSP planned but not wired                       | Moderate             |
| **Log Scrubbing**                        | Automatic secret removal from logs                | Not implemented                                 | Minor                |
| **SQLCipher DB Encryption**              | Optional full-DB encryption                       | SQLite encryption extension (planned)           | Minor                |
| **GDPR Consent Tracking**                | Consent table + audit trail                       | Consent API endpoint exists, no persistence     | Moderate             |
| **CLI Setup Wizard**                     | `config init` + `config validate`                 | `make setup` Makefile wizard                    | Parity               |
| **Property Testing**                     | proptest for fuzzing                              | Not present                                     | Minor                |
| **Semantic Search**                      | Not present                                       | HNSW + FTS5 + RRF hybrid                        | Current leads        |
| **SONA Adaptive Learning**               | Not present                                       | 3-tier with EWC++                               | Current leads        |
| **GraphSAGE Clustering**                 | Not present                                       | GNN on HNSW graph                               | Current leads        |
| **Multi-Asset Extraction**               | Not present                                       | HTML, images, PDFs, OCR                         | Current leads        |
| **Quantization**                         | Not present                                       | Scalar/PQ/binary adaptive                       | Current leads        |
| **Differential Privacy**                 | Not present                                       | Gaussian noise injection                        | Current leads        |

---

## Recommendations: Capabilities to Adopt

### Priority 1 — Critical (blocks core user flows)

#### R-01: Wire Email Provider API Clients — COMPLETED

**What predecessor has:** Fully implemented Gmail (Google API), Outlook (Microsoft Graph), and IMAP providers with OAuth2 PKCE, token encryption (AES-256), automatic refresh, and incremental sync.

**Current gap:** OAuth UI scaffolded (`GmailConnect.tsx`, `OutlookConnect.tsx`) but no backend provider client code. The `EmailProvider` trait exists but has no concrete implementations calling actual APIs.

**Recommendation:** Port the provider client architecture from the predecessor. The predecessor's `gmail.rs`, `outlook.rs`, and `imap.rs` provide battle-tested patterns for:

- OAuth2 PKCE flow with encrypted token storage
- Incremental sync via `historyId` (Gmail) and `deltaLink` (Outlook)
- Token refresh service with retry logic
- Provider-specific label/folder mapping

**Effort:** Large (2-3 sprints)
**Impact:** Unlocks the entire application for real use

---

#### R-02: Offline-First Sync with Conflict Resolution — COMPLETED

**What predecessor has:** Queue-based offline operations with CRDT-like conflict resolution, checkpoint/resume for batch jobs, and eventually-consistent sync.

**Current gap:** No offline support. Ingestion pipeline assumes continuous connectivity.

**Recommendation:** Adopt the predecessor's sync architecture:

- `sync_queue` table for offline operation buffering
- Conflict resolution strategy (last-writer-wins with vector clock tiebreaker)
- `processing_checkpoints` table for crash recovery during batch ingestion
- Background sync scheduler with exponential backoff

This pairs well with the current PWA service worker (already caching static assets) to deliver true offline-first behavior.

**Effort:** Medium (1-2 sprints)
**Impact:** Essential for desktop/mobile reliability

---

### Priority 2 — Significant (major feature gaps)

#### R-03: Rule Engine with Parser and Validator — COMPLETED

**What predecessor has:** A complete rule engine with:

- Rule parser (natural language and structured conditions)
- Rule validator (prevents contradictions and infinite loops)
- Rule processor (applies rules to incoming emails)
- AI-powered rule generation from natural language

**Current gap:** Rules Studio UI exists with AI-suggested semantic conditions, but no backend rule execution engine.

**Recommendation:** Build a hybrid rule engine combining:

- Predecessor's structured parser/validator for deterministic rules
- Current repo's semantic conditions (vector similarity) for fuzzy matching
- Rule priority ordering and conflict detection

The semantic dimension is a unique advantage — "emails about budgets from finance team" as a rule condition is far more powerful than regex patterns.

**Effort:** Medium (1-2 sprints)
**Impact:** Core inbox management feature

---

#### R-04: Bulk Unsubscribe with Safety Guards — COMPLETED

**What predecessor has:** Intelligent `List-Unsubscribe` header detection, false-positive prevention (warns before unsubscribing from important senders), batch unsubscribe execution, and rollback capability.

**Current gap:** Subscription detection exists in Insights Explorer (47+ newsletter types detected), but no unsubscribe action execution.

**Recommendation:** Extend the existing insights module with:

- `List-Unsubscribe` header parsing and one-click execution
- `List-Unsubscribe-Post` (RFC 8058) for one-click mailto-less unsubscribe
- False-positive guard: require confirmation for senders with >50% open rate
- Batch unsubscribe with undo buffer (reuse predecessor's 5-minute window pattern)

**Effort:** Small-Medium (1 sprint)
**Impact:** High user value — direct inbox reduction

---

#### R-05: Rate Limiting and Security Middleware — COMPLETED

**What predecessor has:** Governor-based rate limiting per route, security headers (X-Frame-Options, CSP, HSTS, X-Content-Type-Options), and CORS with specific origin allowlists.

**Current gap:** CSP headers planned in ADR-008 but not wired. No rate limiting. CORS exists but is permissive.

**Recommendation:**

- Add `tower-governor` (or `tower`-native rate limiting) with per-route configuration
- Wire CSP, HSTS, X-Frame-Options, X-Content-Type-Options headers via `tower-http`
- Tighten CORS to specific allowed origins
- Add log scrubbing middleware (predecessor strips API keys and tokens from log output)

**Effort:** Small (1 sprint)
**Impact:** Security hardening required before any production use

---

#### R-06: Processing Checkpoints for Crash Recovery — COMPLETED

**What predecessor has:** A `processing_checkpoints` table that tracks batch job progress. If the server crashes mid-ingestion of 50,000 emails, it resumes from the last checkpoint rather than restarting.

**Current gap:** Ingestion pipeline has no checkpoint/resume. A crash during a large sync loses all progress.

**Recommendation:** Add a checkpoint table to the existing SQLite schema:

```sql
CREATE TABLE processing_checkpoints (
    job_id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    account_id TEXT NOT NULL,
    last_processed_id TEXT,
    total_count INTEGER,
    processed_count INTEGER,
    state TEXT DEFAULT 'running',
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

Wire into the existing `IngestionJobAggregate` (DDD-003) as a domain event (`CheckpointSaved`).

**Effort:** Small (days)
**Impact:** Reliability for large mailboxes

---

### Priority 3 — Moderate (quality of life)

#### R-07: AI Chat with Streaming Responses — COMPLETED

**What predecessor has:** Full chat interface with streaming LLM responses, session management, context-aware email conversation, and multi-turn dialogue.

**Current gap:** Chat UI exists (`features/chat/`), generative AI routing defined in ADR-012, but no LLM actually wired for chat.

**Recommendation:** Wire the existing Tier 1 (Ollama) and Tier 2 (Cloud) generative providers to the chat endpoint. The predecessor's session management pattern (session ID, message history, context window) is a good reference. Use SSE (already used for ingestion progress) for streaming responses.

**Effort:** Small-Medium (1 sprint)
**Impact:** Differentiating feature for user engagement

---

#### R-08: Hot-Reload Configuration — COMPLETED

**What predecessor has:** File-watch system that detects changes to `config.yaml` and applies them without restart. Falls back to defaults on parse error.

**Current gap:** Figment loads config at startup only. Changes require restart.

**Recommendation:** Add `notify` crate file watcher on `config.yaml` and `config.local.yaml`. On change:

1. Parse new config
2. Validate against schema
3. If valid, swap `Arc<Config>` via `arc_swap` or similar
4. If invalid, log warning, keep current config

**Effort:** Small (days)
**Impact:** Developer experience improvement

---

#### R-09: GDPR Consent Persistence — COMPLETED

**What predecessor has:** Consent tracking table, privacy audit logs, data export (GDPR Article 20), and right-to-erasure workflows.

**Current gap:** Consent API endpoint exists but consent decisions are not persisted. Privacy audit logging planned but not implemented.

**Recommendation:**

- Persist consent decisions to SQLite (add migration)
- Log data access events to append-only audit table
- Add data export endpoint (user's emails + metadata as JSON/ZIP)
- Wire remote wipe to actually delete vector store + SQLite data

**Effort:** Small-Medium (1 sprint)
**Impact:** Required for any EU user base

---

#### R-10: Property-Based Testing with proptest — COMPLETED

**What predecessor has:** `proptest` for fuzzing rule parsing, email parsing, and configuration loading. Catches edge cases that unit tests miss.

**Current gap:** Only criterion benchmarks and integration tests. No fuzzing.

**Recommendation:** Add `proptest` to backend `dev-dependencies` and write property tests for:

- Email content extraction (arbitrary HTML → clean text)
- Vector quantization (round-trip correctness)
- Search query parsing (arbitrary input → valid query or error)
- Configuration loading (arbitrary YAML → valid config or error)

**Effort:** Small (days)
**Impact:** Catches rare bugs early

---

### Priority 4 — Consider Later

#### R-11: Native Desktop via Tauri 2.0

**What predecessor has:** Full Tauri 2.0 desktop app with native OS integration, direct file system access, and platform-specific builds (DMG, MSI, DEB).

**Current decision:** ADR-005 explicitly chose React SPA over Tauri. The current PWA support provides installability.

**Recommendation:** Revisit if users demand native features (system tray, global hotkeys, deeper OS integration). The current architecture (Rust backend + React frontend) is Tauri-compatible — wrapping the web app in Tauri would be straightforward.

**Effort:** Medium (if pursued)
**Impact:** Better desktop UX, but not blocking

---

#### R-12: Redis for Distributed Caching

**What predecessor has:** Moka (in-memory) + Redis (distributed) cache with fallback.

**Current gap:** Redis service defined in `docker-compose.yml` but not wired in backend code.

**Recommendation:** Wire Redis as optional cache backend behind the existing `cache/` module. Use for:

- Embedding cache (avoid recomputing for same content)
- Session storage (if multi-instance deployment needed)
- Pub/sub for real-time updates (alternative to SSE polling)

**Effort:** Small (if Redis client crate already in deps)
**Impact:** Enables horizontal scaling

---

## Capabilities NOT Recommended for Adoption

| Predecessor Capability                   | Reason to Skip                                           |
| ---------------------------------------- | -------------------------------------------------------- |
| **XState state machines**                | Current Zustand stores are simpler and sufficient        |
| **Figment config (predecessor version)** | Already using figment in current repo                    |
| **SQLx PostgreSQL dual-mode**            | Current SQLite-first strategy is correct for local-first |
| **Tauri IPC bridge**                     | Only needed if adopting Tauri (R-11)                     |
| **OAuth `state` parameter CSRF**         | Current PKCE flow provides equivalent protection         |

---

## Capabilities Where Current Repo Leads

These are areas where the predecessor should NOT influence direction — the current repo's approach is architecturally superior:

| Capability                    | Current Advantage                                         |
| ----------------------------- | --------------------------------------------------------- |
| **Semantic Search**           | HNSW + FTS5 + RRF hybrid vs no search in predecessor      |
| **SONA Adaptive Learning**    | 3-tier learning with EWC++ vs simple feedback categorizer |
| **GraphSAGE Clustering**      | GNN on HNSW graph vs no clustering                        |
| **Multi-Asset Extraction**    | HTML + images + PDFs + OCR vs text-only                   |
| **Vector Quantization**       | Scalar/PQ/binary adaptive vs none                         |
| **Differential Privacy**      | Noise injection on embeddings vs none                     |
| **Subscription Intelligence** | 47+ pattern detection vs basic categorization             |
| **DDD Architecture**          | 6 bounded contexts with event sourcing vs module-based    |
| **Evaluation Framework**      | NDCG@10, Silhouette, Macro-F1 suites vs basic tests       |

---

## Implementation Roadmap

```
Sprint N:   R-01 (Email Providers) — start, longest lead time
            R-05 (Security Middleware) — parallel, small scope

Sprint N+1: R-01 (continued)
            R-03 (Rule Engine) — start
            R-06 (Checkpoints) — small, parallel

Sprint N+2: R-01 (complete)
            R-02 (Offline Sync) — start
            R-04 (Bulk Unsubscribe) — parallel

Sprint N+3: R-02 (complete)
            R-07 (AI Chat Wiring)
            R-09 (GDPR Consent)

Sprint N+4: R-08 (Hot-Reload Config)
            R-10 (Property Testing)
            R-11/R-12 (evaluate need)
```

**Total estimated effort:** 4-5 sprints for Priority 1-3 items.

---

## Source Material

- Predecessor repo: `https://github.com/pacphi/emailibrium` (cloned and analyzed 2026-03-24)
- Current repo: `/Users/cphillipson/Documents/development/ai/emailibrium`
- Analysis method: Exhaustive source code and documentation scan of both repositories
