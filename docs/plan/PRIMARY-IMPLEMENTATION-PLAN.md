# PRIMARY IMPLEMENTATION PLAN
## Emailibrium: RuVector-Powered Intelligent Email Platform
Version 2.0 | Date: 2026-03-23 | Status: Sprint-Ready

---

## 1. Plan Overview

This plan addresses gaps identified in the docs/research/initial.md academic evaluation of the original INCEPTION.md v1.4:
- **Gap 1**: SONA learning model underspecified → Addressed in ADR-004, Sprint 3
- **Gap 2**: RuVector maturity risk → Addressed in ADR-003 (VectorStore facade), Sprint 1
- **Gap 3**: Embedding model limitations (no domain adaptation, no multilingual) → Addressed in ADR-002, Sprint 6
- **Gap 4**: Multi-asset extraction reliability → Addressed in ADR-006 (quality scoring, graceful degradation), Sprint 2
- **Gap 5**: Embedding invertibility/privacy risk → Addressed in ADR-008, Sprint 1
- **Gap 6**: Scalability on older hardware → Addressed in Sprint 7 (hardware matrix benchmarks)
- **Gap 7**: No formal evaluation methodology → Addressed in Sprint 7 (evaluation framework)
- **Gap 8**: No error recovery/graceful degradation → Addressed across all sprints

Cross-references:
- Original Plan: docs/INCEPTION.md v1.4
- Research: docs/research/initial.md
- ADRs: docs/ADRs/ADR-001 through ADR-010
- DDDs: docs/DDDs/DDD-001 through DDD-005

---

## 2. Architecture Summary

### Bounded Contexts (from DDDs)
1. **Email Intelligence** (Core) — Embedding, classification, clustering
2. **Search** — Hybrid search, SONA re-ranking
3. **Ingestion** — Multi-asset extraction, progress streaming
4. **Learning** — SONA 3-tier adaptive model
5. **Account Management** — OAuth, multi-provider sync

### Key Architecture Decisions (from ADRs)
| ADR | Decision | Sprint |
|-----|----------|--------|
| ADR-001 | Hybrid Search (FTS5+HNSW+RRF) | Sprint 2 |
| ADR-002 | Pluggable Embedding Model | Sprint 1 |
| ADR-003 | RuVector with VectorStore Facade | Sprint 1 |
| ADR-004 | SONA Formal Specification | Sprint 3 |
| ADR-005 | Web SPA (no Tauri) | Sprint 4 |
| ADR-006 | Multi-Asset Extraction Pipeline | Sprint 2 |
| ADR-007 | Adaptive Quantization | Sprint 3 |
| ADR-008 | Privacy & Embedding Security | Sprint 1 |
| ADR-009 | GraphSAGE Clustering | Sprint 3 |
| ADR-010 | Ingest-Tag-Archive Pipeline | Sprint 2 |

---

## 3. Sprint Plan

### Sprint 0: Foundation & Validation (Week 0 — 1 week)
**Goal**: Validate RuVector viability and set up project infrastructure

**Tasks**:
- [ ] **S0-01**: RuVector vs Qdrant comparison benchmark (ADR-003)
  - Write throughput, search latency, recall@10, crash recovery
  - Test with 1K, 10K, 100K synthetic vectors
  - Decision gate: if RuVector fails crash recovery or recall@10 < 0.95, switch to Qdrant
- [ ] **S0-02**: Embedding model evaluation harness (ADR-002)
  - Evaluate all-MiniLM-L6-v2 on sample email data (Enron subset + synthetic)
  - Measure: embed latency, recall@10 on known-relevant pairs, memory footprint
  - Validate 5ms/embed claim on target hardware
- [ ] **S0-03**: Project scaffolding
  - Backend: create `backend/src/vectors/`, `backend/src/content/` module structure
  - Frontend: scaffold `apps/web/` with Vite 8 + React 19 + TanStack Router
  - Monorepo: verify pnpm + Turborepo setup
  - CI: GitHub Actions for Rust build + frontend build + lint
- [ ] **S0-04**: Docker Compose setup (from INCEPTION.md Appendix B)
  - Backend, frontend, PostgreSQL, Redis containers
  - Secrets directory structure
  - Dev override for hot-reloading
- [ ] **S0-05**: SQLite migration: add embedding status columns (from INCEPTION.md Section 6.1)

**Exit Criteria**: RuVector benchmark passes, embedding model validated, project compiles, Docker stack runs.

---

### Sprint 1: Vector Foundation (Weeks 1-2 — 2 weeks)
**Goal**: Core vector infrastructure — embedding, storage, basic search

**Bounded Context**: Email Intelligence (DDD-001)

**Tasks**:
- [ ] **S1-01**: VectorStore facade trait (ADR-003)
  - trait VectorStore { insert, batch_insert, search, search_filtered, delete, update, health }
  - RuVectorStore implementation
  - Unit tests including crash recovery simulation
- [ ] **S1-02**: EmbeddingPipeline (ADR-002)
  - trait EmbeddingModel { embed, embed_batch, dimensions }
  - RuvLLM → Ollama → Cloud fallback chain
  - Moka cache (10K entries) for repeated embeddings
  - Short query augmentation for queries < 5 tokens
- [ ] **S1-03**: Vector encryption at rest (ADR-008)
  - AES-256-GCM encryption for REDB storage
  - Argon2id key derivation from master password
  - Zeroize on shutdown
- [ ] **S1-04**: SQLite backup for vectors (ADR-003)
  - Background job: sync vector IDs + blobs to SQLite
  - Rebuild-from-scratch capability
- [ ] **S1-05**: Basic vector search endpoint
  - POST /api/v1/search/semantic — pure vector search
  - POST /api/v1/search/similar/:email_id — find similar emails
- [ ] **S1-06**: VectorCategorizer — centroid-based classification
  - Cosine similarity to category centroids
  - LLM fallback when confidence < 0.7
  - Initial centroid seeding from first 100 categorized emails
- [ ] **S1-07**: Health and stats endpoints
  - GET /api/v1/vectors/health
  - GET /api/v1/vectors/stats

**Exit Criteria**: Can embed emails, store vectors, search by similarity, classify by centroid. Encryption at rest verified.

---

### Sprint 2: Search & Ingestion (Weeks 3-4 — 2 weeks)
**Goal**: Hybrid search, multi-asset extraction, ingestion pipeline

**Bounded Contexts**: Search (DDD-002), Ingestion (DDD-003)

**Tasks**:
- [ ] **S2-01**: HybridSearch implementation (ADR-001)
  - FTS5 + HNSW parallel execution via tokio::join!
  - Reciprocal Rank Fusion (k=60)
  - Configurable weights (60/40 semantic/keyword)
  - POST /api/v1/search (hybrid mode)
- [ ] **S2-02**: Multi-asset content extraction pipeline (ADR-006)
  - HtmlExtractor: ammonia → scraper → html2text
  - LinkAnalyzer: URL extraction, redirect resolution (5s timeout, max 20 URLs)
  - ImageAnalyzer: OCR via ocrs + CLIP embedding via fastembed
  - AttachmentExtractor: infer → pdf-extract / calamine / dotext
  - Quality scoring per asset type
  - Graceful degradation (log error, continue pipeline)
- [ ] **S2-03**: IngestionPipeline (streaming)
  - 6-stage pipeline: parse → embed → extract assets → classify → detect patterns → apply rules
  - Fast mode (text-only, ~5ms) and deep mode (full extraction, background)
  - Checkpoint every 1000 emails for resume capability
  - Apalis background job integration
- [ ] **S2-04**: SSE progress streaming
  - GET /api/v1/ingestion/status (SSE endpoint)
  - IngestionProgress events: phase, processed, embedded, failed, ETA, emails/sec
  - Discovery events: subscription detected, cluster formed
- [ ] **S2-05**: Ingest-Tag-Archive pipeline (ADR-010)
  - Gmail: watch() → fetch → classify → label (EM/{category}) → archive
  - Configurable timing: instant / delayed (60s) / manual
  - Safety: confidence threshold 0.7, sender whitelist, 5-min undo buffer
  - Rate limiting: max 1000 archives per 10 minutes
  - First-run safety: tag only, no archive until user confirms
- [ ] **S2-06**: InsightEngine — subscription detection
  - Header scan (List-Unsubscribe, Precedence: bulk)
  - Sender frequency analysis (inter-arrival times)
  - Content similarity clustering within same-sender groups
  - Recurrence model fitting (daily/weekly/monthly/irregular)
  - GET /api/v1/insights/subscriptions, /recurring, /report

**Exit Criteria**: Hybrid search works end-to-end. Ingestion pipeline processes emails with progress streaming. Subscriptions detected.

---

### Sprint 3: Intelligence & Learning (Weeks 5-6 — 2 weeks)
**Goal**: GNN clustering, SONA learning, quantization

**Bounded Contexts**: Email Intelligence (DDD-001), Learning (DDD-004)

**Tasks**:
- [ ] **S3-01**: GraphSAGE clustering (ADR-009)
  - Graph construction: HNSW neighbors (k=20) + same-sender + same-thread edges
  - 2-layer GraphSAGE, mean aggregation, hidden dim 128
  - HDBSCAN on GNN embeddings
  - Fallback: mini-batch K-means if HDBSCAN noise > 30%
- [ ] **S3-02**: Cluster stability guardrails (ADR-009)
  - Hysteresis (delta=0.05), pinning, minimum age (3 runs), merge threshold (0.85)
  - Incremental assignment for new emails (O(1) centroid lookup)
  - Hourly mini-batch re-clustering, daily full re-clustering
- [ ] **S3-03**: SONA Tier 1 — Instant learning (ADR-004)
  - EMA centroid updates: alpha=0.05 positive, beta=0.02 negative
  - Bounded shift: ||delta-mu|| <= max_shift
  - Minimum 10 feedback events before activation
- [ ] **S3-04**: SONA Tier 2 — Session learning (ADR-004)
  - Session preference vector: mean(clicked) - mean(skipped)
  - Re-ranking boost: score'(d) = score(d) + gamma * cos(v(d), p_s), gamma=0.15
- [ ] **S3-05**: SONA Tier 3 — Long-term consolidation (ADR-004)
  - Hourly: mini-batch K-means, reclassify low-confidence (<0.6)
  - Daily: full HDBSCAN, centroid recomputation, EWC++ consolidation
  - Centroid snapshots: daily with rollback capability
- [ ] **S3-06**: SONA safeguards (ADR-004)
  - Position bias detection (>95% rank-1 clicks)
  - Centroid drift alarm (>20% shift)
  - A/B evaluation: 10% control group without SONA
- [ ] **S3-07**: Adaptive quantization (ADR-007)
  - Tier detection: count-based with 10% hysteresis
  - Background reconstruction with atomic swap
  - Post-reconstruction validation: 100 stored queries, recall@10 check
  - Config override for manual tier selection
- [ ] **S3-08**: Search interaction tracking (DDD-002)
  - Log queries, clicks, feedback to search_interactions table
  - Feed into SONA Tier 1 and Tier 2

**Exit Criteria**: Clusters discovered automatically. SONA learning operational with safeguards. Quantization auto-scales.

---

### Sprint 4: Frontend Foundation (Weeks 7-8 — 2 weeks)
**Goal**: React web app shell, command center, onboarding

**Bounded Context**: Cross-cutting (all DDDs surfaced in UI)

**Tasks**:
- [ ] **S4-01**: App shell (ADR-005)
  - Vite 8 + React 19 + TanStack Router + TanStack Query
  - shadcn/ui + Radix + Tailwind CSS 4
  - Zustand for client state
  - Route structure: /onboarding, /command-center, /inbox-cleaner, /insights, /rules, /settings, /chat, /email
- [ ] **S4-02**: REST API client (packages/api/)
  - ky-based HTTP client with auth interceptor
  - SSE EventSource wrapper for streaming endpoints
  - Type-safe API hooks via TanStack Query
- [ ] **S4-03**: Secure storage (ADR-005, ADR-008)
  - Web Crypto API (AES-GCM) + IndexedDB
  - Non-extractable CryptoKey
  - Content Security Policy headers
- [ ] **S4-04**: Onboarding flow (multi-account)
  - Gmail OAuth, Outlook OAuth, IMAP manual config
  - Provider presets (Yahoo, iCloud, Fastmail)
  - Connect 1+ accounts, unified inbox setup
  - Account settings: archive strategy, sync depth, label prefix
- [ ] **S4-05**: Command Center (primary screen)
  - cmdk command palette (Cmd+K)
  - Stats cards: total emails, inbox active, subscriptions, topics, rules
  - Quick actions, recent activity feed
  - Topic cluster visualization
- [ ] **S4-06**: Semantic search UI
  - Search mode toggle: hybrid/semantic/keyword
  - Filter sidebar: date, sender, category, labels, attachment type, match location
  - Relevance scoring visualization
  - Related clusters and suggested queries
- [ ] **S4-07**: Type packages (packages/types/)
  - search.ts, vectors.ts, insights.ts, ingestion.ts, email.ts, auth.ts

**Exit Criteria**: Web app loads, onboarding works, search functional, command center displays data.

---

### Sprint 5: Frontend Features (Weeks 9-10 — 2 weeks)
**Goal**: Inbox cleaner, insights, email client, rules studio

**Tasks**:
- [ ] **S5-01**: Ingestion progress screen
  - SSE-connected real-time progress bars (per phase)
  - Live discovery feed (subscriptions, clusters forming)
  - Pause/resume/cancel controls
- [ ] **S5-02**: Inbox Cleaner wizard (4-step flow)
  - Step 1: Connect (done in onboarding)
  - Step 2: Review subscriptions (never opened / rarely opened / regularly opened)
  - Step 3: Clean topics (archive/delete/keep per cluster)
  - Step 4: Set rules + archive strategy
  - Batch action execution with progress
- [ ] **S5-03**: Insights Explorer
  - Overview: inbox health score, category pie chart, volume line chart, top senders bar chart
  - Subscriptions tab: grouped by engagement level
  - Senders tab: communication frequency analysis
  - Topics tab: cluster visualization
  - Trends tab: email volume over time
- [ ] **S5-04**: Email Client — Thread View
  - Left sidebar: groups (inbox, categories, topics, subscriptions)
  - Email list with virtual scrolling (@tanstack/react-virtual)
  - Thread view: full conversation, attachments, links
  - Actions: archive, star, reclassify, move group, delete
  - Reclassify/move triggers learning feedback
- [ ] **S5-05**: Email Compose & Reply
  - Rich text editor (markdown-compatible)
  - Reply, reply-all, forward with proper headers (In-Reply-To, References)
  - Account-aware: reply from same account, compose from default account
  - Attachment upload
  - Draft save/send
- [ ] **S5-06**: Rules Studio
  - Active rules list with match count and accuracy
  - AI-suggested rules (from pattern detection)
  - Semantic rule conditions ("emails similar to X")
  - Rule builder UI + chat-based rule building
- [ ] **S5-07**: Settings
  - Account management (add/edit/disconnect)
  - Per-account: archive strategy, sync frequency, sync depth, label prefix
  - AI/LLM settings: embedding model, LLM provider
  - Privacy: encryption settings, audit log viewer
  - Appearance: theme, layout preferences

**Exit Criteria**: All major UI features functional. Inbox cleaner completes full workflow. Email viewing/composing works.

---

### Sprint 6: Polish & Hardening (Weeks 11-12 — 2 weeks)
**Goal**: Quality, accessibility, performance, domain adaptation

**Tasks**:
- [ ] **S6-01**: Responsive design pass (mobile-friendly)
- [ ] **S6-02**: Accessibility audit (WCAG 2.1 AA)
  - axe-core in CI
  - Keyboard navigation testing
  - Screen reader testing (NVDA, VoiceOver)
  - Focus management for modals and dialogs
- [ ] **S6-03**: PWA support (ADR-005)
  - vite-plugin-pwa, manifest, service worker
  - Offline capability: cache API responses, queue mutations
  - Install prompt, standalone window
- [ ] **S6-04**: Performance optimization
  - Bundle size budget: < 200KB gzipped initial load
  - Route-based code splitting
  - Virtual scrolling for email lists (10K+)
  - Lighthouse CI gate: LCP < 1.5s, FID < 100ms, CLS < 0.1
- [ ] **S6-05**: Error handling & graceful degradation
  - Network errors: retry with exponential backoff, offline indicator
  - Vector service down: fall back to FTS5-only search
  - Embedding service down: queue emails for later embedding
  - Provider API errors: surface to user with actionable guidance
  - Rate limit handling: backoff + notify user
- [ ] **S6-06**: Domain adaptation evaluation (ADR-002, research gap)
  - Test embedding quality on real email domains (tech, finance, legal)
  - Evaluate multilingual-e5-large for non-English inboxes
  - Document model switching procedure
- [ ] **S6-07**: Storybook component documentation
- [ ] **S6-08**: E2E tests with Playwright
  - Onboarding flow, search, inbox cleaner, compose, rules

**Exit Criteria**: Accessibility passes. Performance budgets met. PWA installable. E2E tests green.

---

### Sprint 7: Evaluation & Validation (Weeks 13-14 — 2 weeks)
**Goal**: Formal evaluation framework, benchmarks, documentation (addresses research gap 7)

**Tasks**:
- [ ] **S7-01**: Retrieval quality evaluation (docs/research/initial.md Section 5.1)
  - Build evaluation dataset: 500 queries with relevance labels (from Enron + synthetic)
  - Measure: Recall@5, Recall@10, Recall@20, NDCG@10, MRR
  - Ablation: FTS5 alone vs vector alone vs hybrid vs hybrid+SONA
  - Document results in docs/evaluation/search-quality.md
- [ ] **S7-02**: Classification accuracy evaluation (docs/research/initial.md Section 5.2)
  - Label 1000 emails across all categories
  - Measure: macro-F1, per-category precision/recall, LLM fallback rate
  - Target: >95% accuracy, LLM fallback rate <15%
- [ ] **S7-03**: Clustering quality evaluation (docs/research/initial.md Section 5.3)
  - Measure: silhouette coefficient, ARI against human-labeled topics
  - Compare: GraphSAGE+HDBSCAN vs K-means vs HDBSCAN-only
  - Subscription detection: precision/recall (target >98% recall)
- [ ] **S7-04**: Performance benchmarks (docs/research/initial.md Section 5.5)
  - Microbenchmarks: embed latency, batch throughput, HNSW search at 1K/10K/100K/500K
  - End-to-end: ingestion pipeline emails/sec (text-only and multi-asset)
  - Memory profiling: RSS at steady state for 10K/50K/100K emails
  - Hardware matrix: Apple Silicon (M1/M2/M3), Intel AVX2, AMD, ARM without AVX (research gap 6)
  - Document results in docs/evaluation/performance.md
- [ ] **S7-05**: "10-Minute Inbox Zero" validation (docs/research/initial.md Section 5.4)
  - Internal dogfooding: team members test with real inboxes
  - Measure: time to zero unread, actions required, satisfaction
  - Document protocol for future formal user study
- [ ] **S7-06**: Security audit
  - Verify encryption at rest (ADR-008)
  - Test embedding invertibility risk: attempt text recovery from stored vectors
  - CSP header validation
  - OAuth token storage security
- [ ] **S7-07**: Documentation
  - API documentation (OpenAPI spec)
  - Deployment guide
  - User guide
  - Architecture documentation (referencing ADRs and DDDs)

**Exit Criteria**: All evaluation metrics documented. Performance targets validated or gaps identified with remediation plan.

---

## 4. Feature-to-Sprint Mapping

Map of all features from INCEPTION.md Section 11 to sprints:

| Feature | Sprint | Status |
|---------|--------|--------|
| FEAT-050 Semantic Hybrid Search | Sprint 2 | New |
| FEAT-051 Vector Embedding Pipeline | Sprint 1 | New |
| FEAT-052 Inbox Cleaner Wizard | Sprint 5 | New |
| FEAT-053 Subscription Intelligence | Sprint 2 | New |
| FEAT-054 Topic Clustering | Sprint 3 | New |
| FEAT-055 Ingestion Pipeline (SSE) | Sprint 2 | New |
| FEAT-056 Command Center | Sprint 4 | New |
| FEAT-057 Insights Explorer | Sprint 5 | New |
| FEAT-058 SONA Self-Learning | Sprint 3 | New |
| FEAT-059 Semantic Rule Conditions | Sprint 5 | New |
| FEAT-060 AI Rule Suggestions | Sprint 5 | New |
| FEAT-061 Inbox Health Score | Sprint 5 | New |
| FEAT-062 Conversation Threading | Sprint 5 | New |
| FEAT-063 Smart Digest Creation | Future | Deferred |
| FEAT-064 Image & Visual Content Analysis | Sprint 2 | New |
| FEAT-065 Hyperlink Extraction & Intelligence | Sprint 2 | New |
| FEAT-066 Attachment Content Extraction | Sprint 2 | New |
| FEAT-067 Deep HTML Body Extraction | Sprint 2 | New |
| FEAT-068 Multi-Asset Semantic Search | Sprint 2 | New |
| FEAT-069 Full Email Client UI | Sprint 5 | New |
| FEAT-070 Ingest-Tag-Archive Pipeline | Sprint 2 | New |
| FEAT-071 Dynamic Auto-Grouping | Sprint 3 | New |
| FEAT-072 Continuous Learning System | Sprint 3 | New |
| FEAT-073 Periodic Re-Classification | Sprint 3 | New |
| FEAT-074 Gmail Push Notifications | Sprint 2 | New |
| FEAT-075 Email Compose & Reply | Sprint 5 | New |
| FEAT-076 Multi-Account Onboarding | Sprint 4 | New |
| FEAT-077 Account Management | Sprint 5 | New |
| FEAT-078 Unified Inbox | Sprint 4 | New |
| Existing FEAT-001 through FEAT-015 | Enhanced in respective sprints | Existing |

---

## 5. Risk Register (Updated from Research)

| # | Risk | Likelihood | Impact | Mitigation | Sprint |
|---|------|-----------|--------|------------|--------|
| R1 | RuVector fails crash recovery benchmark | Medium | Critical | VectorStore facade enables swap to Qdrant (ADR-003). Sprint 0 benchmark is go/no-go gate. | S0 |
| R2 | SONA feedback loops corrupt centroids | Medium | High | Formal safeguards (ADR-004): min 10 events, drift alarm, A/B eval, rollback | S3 |
| R3 | Embedding model poor on domain jargon | Medium | Medium | Pluggable model (ADR-002), domain evaluation (S6-06), multilingual fallback | S6 |
| R4 | OCR/PDF extraction unreliable | Medium | Medium | Quality scoring + graceful degradation (ADR-006). Low-quality assets weighted lower. | S2 |
| R5 | Embedding invertibility privacy leak | Low | High | Encryption at rest + Gaussian noise injection (ADR-008). Privacy documentation. | S1 |
| R6 | Performance degrades on older hardware | Medium | Medium | Hardware matrix benchmarks (S7-04). Configurable quality/speed tradeoffs. | S7 |
| R7 | Gmail API quota exhaustion | Low | High | Rate limiting (1000/10min), exponential backoff, quota monitoring (ADR-010) | S2 |
| R8 | Cluster churn confuses users | Medium | Medium | Stability guardrails (ADR-009): hysteresis, pinning, minimum age | S3 |
| R9 | Breaking changes in RuVector updates | Medium | High | Pin exact versions, integration test suite, facade abstraction | S0+ |
| R10 | Browser storage limitations (IndexedDB) | Low | Medium | Graceful degradation to server-side state, quota monitoring | S4 |

---

## 6. Definition of Done (per Sprint)

Each sprint is complete when:
1. All tasks marked complete with passing tests
2. No P0/P1 bugs open
3. Code reviewed and merged to main
4. API endpoints documented (OpenAPI)
5. Unit test coverage > 80% for new code
6. Integration tests pass
7. Performance targets met (where applicable)
8. Relevant ADR implementation verified

---

## 7. Dependencies & Prerequisites

| Dependency | Required By | Notes |
|-----------|------------|-------|
| Rust 1.94.0 | Sprint 0 | rustup default stable |
| Node.js 24+ | Sprint 4 | For Vite 8 |
| pnpm 10.32+ | Sprint 4 | corepack enable |
| Docker + Compose | Sprint 0 | For development stack |
| Gmail API credentials | Sprint 2 | OAuth2 client ID/secret |
| Outlook (Graph API) credentials | Sprint 2 | App registration |
| RuVector crates | Sprint 0 | ruvector-core 2.0, ruvector-gnn 2.0, ruvllm 2.0 |
| SONA (git dep) | Sprint 3 | From ruvector monorepo (ADR-004 note) |

---

## 8. Success Metrics

| Metric | Target | Measured In |
|--------|--------|-------------|
| Hybrid search latency (p95) | < 50ms | Sprint 7 |
| Vector search latency (100K) | < 10ms | Sprint 7 |
| Email ingestion rate | > 500 emails/sec (text-only) | Sprint 7 |
| Categorization accuracy | > 95% macro-F1 | Sprint 7 |
| Subscription detection recall | > 98% | Sprint 7 |
| Memory footprint (100K emails) | < 700MB | Sprint 7 |
| Time to inbox zero (10K emails) | < 10 minutes | Sprint 7 |
| LLM fallback rate | < 15% of classifications | Sprint 7 |
| Lighthouse performance score | > 90 | Sprint 6 |
| WCAG 2.1 AA compliance | 100% | Sprint 6 |

---

## 9. Cross-Reference Index

| Document | Location | Purpose |
|----------|----------|---------|
| Original Plan | INCEPTION.md | Full technical specification |
| Research Evaluation | docs/research/initial.md | Academic analysis of plan feasibility |
| ADR-001 Hybrid Search | docs/ADRs/ADR-001.md | Search architecture decision |
| ADR-002 Embedding Model | docs/ADRs/ADR-002.md | Model selection and pluggability |
| ADR-003 RuVector | docs/ADRs/ADR-003.md | Vector DB selection with facade |
| ADR-004 SONA Specification | docs/ADRs/ADR-004.md | Learning model formalization |
| ADR-005 Web SPA | docs/ADRs/ADR-005.md | Frontend architecture decision |
| ADR-006 Multi-Asset Pipeline | docs/ADRs/ADR-006.md | Content extraction strategy |
| ADR-007 Quantization | docs/ADRs/ADR-007.md | Memory optimization strategy |
| ADR-008 Privacy | docs/ADRs/ADR-008.md | Embedding security architecture |
| ADR-009 GNN Clustering | docs/ADRs/ADR-009.md | Topic discovery architecture |
| ADR-010 Ingest-Tag-Archive | docs/ADRs/ADR-010.md | Zero-inbox pipeline strategy |
| DDD-000 Context Map | docs/DDDs/DDD-000-context-map.md | Bounded context relationships |
| DDD-001 Email Intelligence | docs/DDDs/DDD-001-email-intelligence.md | Core domain model |
| DDD-002 Search | docs/DDDs/DDD-002-search.md | Search domain model |
| DDD-003 Ingestion | docs/DDDs/DDD-003-ingestion.md | Ingestion domain model |
| DDD-004 Learning | docs/DDDs/DDD-004-learning.md | SONA learning domain model |
| DDD-005 Account Management | docs/DDDs/DDD-005-account-management.md | Account domain model |
