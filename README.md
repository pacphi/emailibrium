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

**Prerequisites:** Rust 1.95+, Node.js 26 (LTS)+, pnpm 10.32+ — or just Docker. See [Setup Guide](docs/setup-guide.md) for details.

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
| [UI Overview](docs/user-interface-overview.md)             | Visual tour — screenshots of every screen     |
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

See all ADRs in [docs/ADRs](https://github.com/pacphi/emailibrium/tree/main/docs/ADRs).

### Domain Model

See all DDDs in [docs/DDDs](https://github.com/pacphi/emailibrium/tree/main/docs/DDDs).

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
