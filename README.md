# Emailibrium

**Your inbox found its balance.**

> _Email + Equilibrium = Emailibrium._ Because your inbox shouldn't feel like a second job.

Emailibrium is a vector-native email intelligence platform that replaces keyword search and manual filters with semantic understanding. Connect your accounts, and in under 10 minutes it clusters, classifies, and cleans 10,000+ emails — then keeps learning from every interaction.

No cloud processing. No data leaving your machine. Just fast, private, intelligent email.

---

## What It Does

| Capability                      | How                                                                                      |
| ------------------------------- | ---------------------------------------------------------------------------------------- |
| **Semantic search**             | Find "that budget spreadsheet from Sarah" — not just emails containing the word "budget" |
| **10-minute inbox zero**        | Guided cleanup wizard with batch actions across thousands of emails                      |
| **Subscription intelligence**   | Auto-detects 47 newsletters you forgot you signed up for                                 |
| **Topic clustering**            | Emails self-organize into projects, threads, and themes                                  |
| **Continuous learning**         | Every click, star, and archive makes search and classification smarter                   |
| **Multi-account unified inbox** | Gmail, Outlook, IMAP — one interface, one search, one brain                              |

## How It Works

```text
Email arrives → Embed as vector → Classify via centroid similarity → Cluster by topic → Archive
                    ↓                        ↓                           ↓
              Searchable in <50ms    Learns from corrections    Groups evolve over time
```

Under the hood: HNSW vector indexing, Reciprocal Rank Fusion hybrid search, GraphSAGE-inspired clustering, 3-tier adaptive learning (SONA), and AES-256-GCM encryption at rest. All running locally in Rust.

## Quick Start

```bash
# Clone
git clone https://github.com/pacphi/emailibrium.git
cd emailibrium

# Guided setup (recommended for first time)
make setup            # interactive wizard: prerequisites, secrets, AI, Docker

# Option A: Native
make install
make dev
# → Backend: http://localhost:8080  Frontend: http://localhost:3000

# Option B: Docker
make setup-secrets    # generate dev secrets (first time only)
make docker-up-dev    # start with hot-reload
```

**Prerequisites:** Rust 1.94+, Node.js 24 (LTS)+, pnpm 10.32+ — or just Docker. See [Setup Guide](docs/setup-guide.md) for details.

## Architecture

```text
React TypeScript SPA ──REST + SSE──→ Axum API Gateway
         │                                │
    TanStack Router                  Intelligence Layer
    TanStack Query              ┌─────────┼─────────┐
    Zustand + PWA               │    RuVector Engine  │
                                │  HNSW · SONA · GNN  │
                                └─────────┼─────────┘
                                     Data Layer
                                SQLite · Redis · REDB
```

- **Backend:** Rust (Axum 0.8), SQLite, 22 vector intelligence modules (ONNX/fastembed default embeddings)
- **Frontend:** React 19, TypeScript, Tailwind CSS, 8 features, PWA-ready
- **Privacy:** All embeddings generated and stored locally. Cloud is opt-in, never required.

## Features at a Glance

- **Command Center** — search hub with Cmd+K palette
- **Inbox Cleaner** — 4-step guided cleanup wizard
- **Insights Explorer** — charts, subscription analytics, health score
- **Email Client** — view, reply, compose with thread view
- **Rules Studio** — AI-suggested rules with semantic conditions
- **Settings** — per-account config, encryption, appearance

## Documentation

### For Everyone

| Document                                                   | Description                                   |
| ---------------------------------------------------------- | --------------------------------------------- |
| [User Guide](docs/user-guide.md)                           | Getting started, features, keyboard shortcuts |
| [Deployment Guide](docs/deployment-guide.md)               | Install, Docker, production setup             |
| [Configuration Reference](docs/configuration-reference.md) | Every config key, default, and env override   |

### For the Team

| Document                                     | Description                                                  |
| -------------------------------------------- | ------------------------------------------------------------ |
| [Maintainer Guide](docs/maintainer-guide.md) | Developer, designer, operator, security, and PM perspectives |
| [Architecture](docs/architecture.md)         | 4-tier system design, bounded contexts, data flow            |
| [Releasing](docs/releasing.md)               | Version, tag, changelog, Docker image publishing             |
| [API Spec](docs/api/openapi.yaml)            | OpenAPI 3.0 — all 12 endpoints with schemas                  |

### Architecture Decisions

| ADR                             | Decision                             |
| ------------------------------- | ------------------------------------ |
| [ADR-001](docs/ADRs/ADR-001.md) | Hybrid Search (FTS5 + HNSW + RRF)    |
| [ADR-002](docs/ADRs/ADR-002.md) | Pluggable Embedding Model            |
| [ADR-003](docs/ADRs/ADR-003.md) | RuVector with VectorStore Facade     |
| [ADR-004](docs/ADRs/ADR-004.md) | SONA Adaptive Learning Specification |
| [ADR-005](docs/ADRs/ADR-005.md) | Web SPA (no Tauri)                   |
| [ADR-006](docs/ADRs/ADR-006.md) | Multi-Asset Content Extraction       |
| [ADR-007](docs/ADRs/ADR-007.md) | Adaptive Quantization                |
| [ADR-008](docs/ADRs/ADR-008.md) | Privacy & Embedding Security         |
| [ADR-009](docs/ADRs/ADR-009.md) | GNN Clustering (GraphSAGE)           |
| [ADR-010](docs/ADRs/ADR-010.md) | Ingest-Tag-Archive Pipeline          |

### Domain Model

| Context                                                       | Scope                                 |
| ------------------------------------------------------------- | ------------------------------------- |
| [Context Map](docs/DDDs/DDD-000-context-map.md)               | How the 5 domains connect             |
| [Email Intelligence](docs/DDDs/DDD-001-email-intelligence.md) | Embedding, classification, clustering |
| [Search](docs/DDDs/DDD-002-search.md)                         | Hybrid search, SONA re-ranking        |
| [Ingestion](docs/DDDs/DDD-003-ingestion.md)                   | Multi-asset extraction, SSE progress  |
| [Learning](docs/DDDs/DDD-004-learning.md)                     | 3-tier SONA adaptive model            |
| [Account Management](docs/DDDs/DDD-005-account-management.md) | OAuth, multi-provider sync            |

### Research & Evaluation

| Document                                                             | Description                                  |
| -------------------------------------------------------------------- | -------------------------------------------- |
| [Research: Initial Evaluation](docs/research/initial.md)             | Academic evaluation with 30 citations        |
| [Research: LLM Options](docs/research/llm-options.md)                | ONNX, Ollama, cloud — tiered AI architecture |
| [Search Quality](backend/docs/evaluation/search-quality.md)          | Recall, NDCG, MRR methodology                |
| [Classification](backend/docs/evaluation/classification-accuracy.md) | Macro-F1, per-category P/R                   |
| [Clustering](backend/docs/evaluation/clustering-quality.md)          | Silhouette, ARI, detection metrics           |
| [Performance](backend/docs/evaluation/performance.md)                | Benchmarks and memory profiling              |
| [Domain Adaptation](docs/evaluation/domain-adaptation.md)            | Model switching, multilingual                |
| [Inbox Zero Protocol](docs/evaluation/inbox-zero-protocol.md)        | User study design                            |

## Development

```bash
make help              # see all available targets
make ci                # format-check + lint + typecheck + test
make test              # backend (Rust) + frontend (Vitest)
make docker-up-dev     # full stack with hot-reload
make upgrade           # upgrade all dependencies
make outdated          # check what's stale
```

See the [Maintainer Guide](docs/maintainer-guide.md) for the full developer experience.

## Tech Stack

| Layer               | Technology                                                                                            |
| ------------------- | ----------------------------------------------------------------------------------------------------- |
| Backend             | Rust, Axum 0.8, SQLite (SQLx), Moka cache                                                             |
| Vector Intelligence | HNSW indexing, SONA learning, GraphSAGE-inspired clustering, adaptive quantization (scalar/PQ/binary) |
| Frontend            | React 19, TypeScript 5.9, Vite 8, TanStack Router + Query, Zustand, Tailwind CSS                      |
| UI Components       | shadcn/ui pattern, Radix primitives, cmdk, Recharts, Framer Motion                                    |
| Infrastructure      | Docker Compose, GitHub Actions CI, Dependabot, Husky + lint-staged                                    |
| Security            | AES-256-GCM encryption at rest, Argon2id KDF, Web Crypto API, CSP headers                             |

## License

MIT

---

_Emailibrium: where email finds its equilibrium._
