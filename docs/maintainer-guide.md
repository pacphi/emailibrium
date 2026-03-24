# Emailibrium Maintainer Guide

> **Repository**: [github.com/pacphi/emailibrium](https://github.com/pacphi/emailibrium)

## 1. Welcome and Project Philosophy

Emailibrium is a **vector-native email intelligence platform** that transforms how people interact with email. Instead of folders and filters, it uses vector embeddings to understand email semantically -- enabling instant search, automatic categorization, and intelligent clustering without shipping your data to the cloud.

### Core Design Principles

- **Vector-first, keyword-fallback.** Every email is embedded into a high-dimensional vector space. Full-text search (FTS5) exists as a complement, not the primary mechanism. Hybrid results are fused via Reciprocal Rank Fusion (ADR-001).
- **Stream, don't batch.** Ingestion progress is delivered over Server-Sent Events. The UI never blocks waiting for a bulk operation to finish.
- **Learn from every interaction.** SONA (Self-Optimizing Neural Architecture) adjusts search rankings and category centroids based on implicit and explicit user feedback (ADR-004).
- **Privacy by architecture.** All processing happens locally. Vectors are encrypted at rest with AES-256-GCM. There is no telemetry server, no cloud dependency (ADR-008).
- **The 10-minute promise.** A new user should reach inbox zero within 10 minutes of first launch, driven by the Ingest-Tag-Archive pipeline (ADR-010).

For the full product vision, see [INCEPTION.md](./INCEPTION.md). For the academic grounding and gap analysis, see [RESEARCH.md](./RESEARCH.md).

---

## 2. Repository Layout

```
emailibrium/
  backend/                   Rust backend (Axum, SQLite, vector engine)
    src/
      main.rs                Server setup, middleware, startup
      lib.rs                 Public module re-exports for integration tests
      api/                   HTTP route handlers (vectors, ingestion, insights)
      db/                    SQLite connection pool (sqlx)
      content/               Multi-asset extraction pipeline (ADR-006)
      vectors/               Core vector engine (embedding, store, search, SONA, encryption)
    tests/                   Integration tests (search, classification, clustering, security)
    benches/                 Criterion benchmarks (vector_benchmarks)
    Cargo.toml               Rust dependencies (edition 2021, MSRV 1.94)
    Makefile                 Backend-specific make targets
    Dockerfile               Multi-stage Rust build

  frontend/                  React TypeScript monorepo (pnpm workspaces + Turborepo)
    apps/web/                Main SPA (Vite 6, React 19, TanStack Router)
      src/features/          Feature-sliced modules (command-center, inbox-cleaner, etc.)
      e2e/                   Playwright end-to-end tests
    packages/
      types/                 Shared TypeScript type definitions (@emailibrium/types)
      api/                   API client and hooks (@emailibrium/api)
      ui/                    Shared component library (@emailibrium/ui)
      core/                  Business logic utilities (@emailibrium/core)
    Makefile                 Frontend-specific make targets
    nginx.conf               Production reverse-proxy config
    Dockerfile               Multi-stage Node build

  docs/
    ADRs/                    Architecture Decision Records (ADR-001 through ADR-010)
    DDDs/                    Domain-Driven Design documents (DDD-000 through DDD-005)
    plan/                    PRIMARY-IMPLEMENTATION-PLAN.md (78 features, 7 sprints)
    evaluation/              Evaluation framework and metrics
    api/                     API documentation
    architecture.md          System architecture overview
    configuration-reference.md  Complete config key reference
    deployment-guide.md      Production deployment instructions
    user-guide.md            End-user documentation
    INCEPTION.md             Original product vision
    RESEARCH.md              Academic evaluation and gap analysis

  docker-compose.yml         Production container orchestration
  docker-compose.dev.yml     Dev overlay (hot-reload, debug ports)
  Makefile                   Root Makefile (delegates to backend/ and frontend/)
  config.yaml                Base configuration defaults
  secrets/                   Generated development secrets (gitignored)
  CLAUDE.md                  AI assistant configuration
```

---

## 3. For Developers

### Getting Started

**Prerequisites:**

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.94+ | Backend compilation |
| Node.js | 24+ | Frontend toolchain |
| pnpm | 10.32+ | Frontend package management |
| Docker | 24+ | Containerized deployment |
| SQLite | 3.35+ | Database (usually pre-installed on macOS/Linux) |

**First-time setup:**

```bash
# Clone and install all dependencies
make install

# Generate development secrets
make docker-secrets

# Start both servers in dev mode
make dev
# Backend: http://localhost:8080
# Frontend: http://localhost:3000
```

### Daily Workflow

**Native development** (recommended for fast iteration):

```bash
make dev                  # Start backend + frontend with hot-reload
make test                 # Run all tests (backend + frontend)
make lint                 # Lint everything (Rust + TypeScript + Markdown + YAML)
make ci                   # Full CI pipeline: format-check, lint, typecheck, test
```

**Docker development** (matches production environment):

```bash
make docker-up-dev        # Start with hot-reload via docker-compose.dev.yml
make docker-logs          # Tail all container logs
make docker-health        # Check container status
make docker-down          # Stop everything
```

### Backend Development

The backend is organized into four module groups under `backend/src/`:

| Module | Files | Responsibility |
|--------|-------|----------------|
| `api/` | `vectors.rs`, `ingestion.rs`, `insights.rs` | HTTP handlers, request validation, response serialization |
| `db/` | `mod.rs` | SQLite connection pool via sqlx |
| `content/` | `html_extractor.rs`, `image_analyzer.rs`, `link_analyzer.rs`, `attachment_extractor.rs`, `tracking_detector.rs` | Multi-asset extraction pipeline (ADR-006) |
| `vectors/` | 17 files | Core engine: embeddings, store, search, categorizer, clustering, encryption, quantization, SONA learning, backup, insights |

**Adding a new API endpoint:**

1. Add the handler function in the appropriate `api/*.rs` file.
2. Register the route in `api/mod.rs`.
3. If it needs new vector operations, add them to `vectors/mod.rs` (the `VectorService` facade).
4. Write an integration test in `backend/tests/`.
5. Run `make -C backend test` to verify.

**Adding a new vector module:**

1. Create the file in `backend/src/vectors/`.
2. Add `pub mod your_module;` to `vectors/mod.rs`.
3. Expose needed functionality through `VectorService`.
4. Add unit tests in-module with `#[cfg(test)]`.
5. Add integration tests in `backend/tests/`.

**Running benchmarks:**

```bash
make -C backend bench     # Run Criterion benchmarks
```

### Frontend Development

The frontend follows a **feature-sliced design** pattern. Features are self-contained modules under `frontend/apps/web/src/features/`:

| Feature | Description |
|---------|-------------|
| `command-center/` | Main dashboard and command palette |
| `inbox-cleaner/` | Bulk email triage and cleanup |
| `email/` | Email reading and detail view |
| `chat/` | Conversational email interface |
| `insights/` | Analytics and subscription detection |
| `rules/` | Automation rule builder |
| `settings/` | User preferences and account management |
| `onboarding/` | First-run setup flow |

**Adding a new feature:**

1. Create a directory under `src/features/your-feature/`.
2. Add the route in the TanStack Router configuration.
3. Create API hooks using `@tanstack/react-query` and the `@emailibrium/api` client.
4. Use shared components from `@emailibrium/ui` (Button, Card, Badge, Input, Select, Toggle, Spinner, Avatar, EmptyState, Skeleton).
5. Add a Playwright E2E test in `e2e/your-feature.spec.ts`.

**Shared packages:**

- `@emailibrium/ui` -- Reusable components (see Section 4 for component list)
- `@emailibrium/api` -- API client with typed request/response
- `@emailibrium/types` -- Shared TypeScript interfaces
- `@emailibrium/core` -- Business logic utilities

### Testing Strategy

| Layer | Tool | Location | Run Command |
|-------|------|----------|-------------|
| Backend unit | `#[cfg(test)]` modules | In each `.rs` file | `make -C backend test` |
| Backend integration | `#[test]` functions | `backend/tests/*.rs` | `make -C backend test` |
| Backend benchmarks | Criterion | `backend/benches/` | `make -C backend bench` |
| Frontend unit | Vitest + Testing Library | Co-located with source | `make -C frontend test` |
| Frontend E2E | Playwright | `frontend/apps/web/e2e/` | `cd frontend/apps/web && pnpm test:e2e` |
| Security audit | Custom test suite | `backend/tests/security_audit.rs` | `make -C backend test` |

### Code Style

- **Rust:** `cargo fmt` for formatting, `cargo clippy` for lints. Both run in CI.
- **TypeScript:** Prettier for formatting, ESLint for lints. Config lives in the frontend workspace root.
- **Markdown and YAML:** Prettier + markdownlint-cli2 + yamllint. Run `make lint-docs`.
- **Commits:** Use conventional commit messages (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).

### Dependency Management

```bash
make outdated             # Show outdated deps across both stacks (no changes)
make upgrade              # Upgrade within semver ranges
make audit                # Run cargo audit + pnpm audit
```

Pin exact versions for security-critical crates (`aes-gcm`, `argon2`, `zeroize`). Use semver ranges for everything else.

---

## 4. For Designers

### Component Library

The shared component library lives at `frontend/packages/ui/src/components/`:

| Component | Purpose |
|-----------|---------|
| `Button` | Primary, secondary, ghost, and danger variants |
| `Card` | Content container with header, body, footer slots |
| `Badge` | Status indicators and labels |
| `Input` | Text input with validation states |
| `Select` | Dropdown selection |
| `Toggle` | Boolean switch control |
| `Spinner` | Loading indicator |
| `Avatar` | User/sender avatar with fallback initials |
| `EmptyState` | Placeholder for empty lists and search results |
| `Skeleton` | Loading placeholder with shimmer animation |

### Design System

- **Framework:** Tailwind CSS 3.4 with custom configuration
- **Primary palette:** Indigo (`indigo-50` through `indigo-950`)
- **Dark mode:** Class-based strategy (`dark:` prefix), toggled at the `<html>` element
- **Breakpoints:** `sm` (640px), `md` (768px), `lg` (1024px), `xl` (1280px), `2xl` (1536px)
- **Typography:** System font stack with monospace for code elements
- **Radix UI primitives:** Dialog, Tooltip, Progress, Tabs, DropdownMenu, Popover, Select, Switch, Label, Separator

### Storybook

Browse and develop components in isolation:

```bash
cd frontend/apps/web && npx storybook dev
```

### Accessibility

Target: **WCAG 2.1 AA** compliance.

- `axe-core` accessibility checks run in CI
- Focus trapping via `useFocusTrap` hook for modals and dialogs
- Screen reader announcements via `useAnnounce` hook
- Skip-to-content link on every page
- Keyboard shortcut system via `useKeyboard` hook
- All interactive elements have visible focus indicators

### Adding a New Component

1. Create `YourComponent.tsx` in `frontend/packages/ui/src/components/`.
2. Add a Storybook story alongside it.
3. Export from the package's `index.ts`.
4. Use in features via `import { YourComponent } from '@emailibrium/ui'`.

---

## 5. For Operators and DevOps

### Docker Deployment

```bash
make docker-build         # Build all images
make docker-up            # Start production stack
make docker-health        # Check container health
make docker-logs          # Tail all logs
make docker-down          # Stop everything
make docker-down-volumes  # Stop and destroy data (CAUTION)
```

### Configuration

Emailibrium uses a **layered configuration** system via figment. Later layers override earlier ones:

1. `config.yaml` -- base defaults (committed)
2. `config.{APP_ENV}.yaml` -- environment-specific (committed)
3. `config.local.yaml` -- local overrides (gitignored)
4. `EMAILIBRIUM_*` environment variables -- runtime overrides
5. `/run/secrets/*` -- Docker secrets (production)

For the complete key reference, see [configuration-reference.md](./configuration-reference.md).

### Secrets Management

- Use **file-based secrets** via Docker secrets in production. Never use environment variables for sensitive values in production.
- Generate development secrets: `make docker-secrets` (creates `secrets/dev/`).
- Sensitive keys that must never appear in config files: `encryption.master_password`, `database_url` (production).

### Health Checks

| Endpoint | Purpose |
|----------|---------|
| `GET /api/v1/vectors/health` | Backend readiness (vector store, database, embedding pipeline) |
| Docker `HEALTHCHECK` | Container-level liveness, configured in `docker-compose.yml` |

### Backup

- SQLite vector backup is built into the backend (`vectors/backup.rs`).
- Enable with `backup.enabled: true` in config.
- Default interval: 3600 seconds (1 hour). Adjust with `backup.interval_secs`.
- Backups are written to the configured store path.

### Scaling Considerations

Emailibrium is a **single-node, local-first** application by design. Scaling is primarily about memory management.

| Mailbox Size | Quantization Tier | Estimated Memory |
|--------------|-------------------|-----------------|
| 10,000 emails | None (fp32) | ~15 MB vectors |
| 100,000 emails | Scalar (int8) | ~100 MB vectors |
| 1,000,000 emails | Product/Binary | ~200-400 MB vectors |

Quantization auto-scales when `quantization.mode: auto` (default). See ADR-007 for the tier thresholds and hysteresis logic.

If you need multi-user or multi-node deployment, consider replacing SQLite with PostgreSQL (`database_url` already supports connection URLs).

### Troubleshooting

| Issue | Cause | Fix |
|-------|-------|-----|
| Port 8080 or 3000 already in use | Another process on the port | `lsof -i :8080` to find it, or change ports in config |
| `database is locked` | Concurrent SQLite writes | Ensure only one backend instance runs; check for orphan processes |
| High memory usage | Large vector store without quantization | Enable `quantization.mode: auto` or reduce `embedding.cache_size` |
| Embedding pipeline returns mock vectors | Provider set to `mock` | Set `embedding.provider: ollama` and ensure Ollama is running |
| Docker build fails on Rust compilation | Insufficient memory for release build | Allocate at least 4 GB RAM to Docker |

**Log levels:** Set `RUST_LOG` to control backend verbosity:

```bash
RUST_LOG=info              # Production default
RUST_LOG=debug             # Detailed request/response logging
RUST_LOG=emailibrium=trace # Maximum detail for the application crate
```

---

## 6. For Security and Compliance

### Threat Model

Emailibrium's local-first architecture shifts the threat model from cloud breaches to device compromise. There is no central server holding user data. The primary risks are:

- Physical device access (mitigated by encryption at rest)
- Malicious browser extensions (mitigated by CSP headers and Web Crypto token storage)
- Embedding invertibility (mitigated by optional Gaussian noise injection)

### Encryption at Rest

- **Algorithm:** AES-256-GCM for vector store persistence (ADR-008)
- **Key derivation:** Argon2id from a master password
- **Key lifecycle:** Held in memory only during runtime; zeroed on shutdown via the `zeroize` crate
- **Enable:** Set `encryption.enabled: true` and provide the master password via env var or Docker secret

### Browser Security

- **Token storage:** OAuth tokens encrypted via Web Crypto API (not localStorage)
- **CSP headers:** `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'`
- **CORS:** Restricted to specific origins (never wildcard in production)

### OAuth Security

- **Flow:** PKCE (Proof Key for Code Exchange) for Gmail and Outlook
- **Token storage:** Encrypted in the browser via Web Crypto API
- **Refresh rotation:** Tokens are rotated on each refresh cycle

### Embedding Privacy

Vector embeddings are derived data, but partial text recovery is theoretically possible (Morris et al. 2023). Mitigations:

- Optional calibrated Gaussian noise injection (differential privacy lite)
- Encryption at rest for persisted vectors
- Document the risk in your privacy policy

### Audit and Testing

```bash
make audit                # cargo audit + pnpm audit
```

The security test suite at `backend/tests/security_audit.rs` validates:

- Encryption roundtrip correctness
- Nonce randomness (no reuse)
- Key zeroing on drop
- Embedding invertibility resistance
- CSP and CORS header correctness

**Dependency auditing:** Run `make audit` regularly. Enable GitHub Dependabot for automated vulnerability alerts.

### ADR References

- **ADR-008:** Privacy Architecture (encryption, key management, embedding privacy, browser security)
- **ADR-003:** VectorStore facade (swap-ability enables future encrypted backend alternatives)

---

## 7. For Product Managers

### Feature Map

The implementation plan defines **78 features across 7 sprints**. See [PRIMARY-IMPLEMENTATION-PLAN.md](./plan/PRIMARY-IMPLEMENTATION-PLAN.md) Section 4 for the complete feature list with sprint assignments and dependencies.

### Architecture Decisions

10 Architecture Decision Records document every major technical trade-off:

| ADR | Decision | Key Trade-off |
|-----|----------|---------------|
| ADR-001 | Hybrid Search (FTS5 + HNSW + RRF) | Complexity vs. search quality |
| ADR-002 | Pluggable Embedding Pipeline | Latency vs. quality; mock fallback ensures the system always works |
| ADR-003 | RuVector with VectorStore Facade | Rust-native performance vs. ecosystem maturity |
| ADR-004 | SONA Adaptive Learning | Continuous improvement vs. classification stability |
| ADR-005 | Web SPA (replacing Tauri) | Web accessibility vs. native desktop integration |
| ADR-006 | Multi-Asset Content Extraction | Extraction breadth vs. reliability |
| ADR-007 | Adaptive Scalar Quantization | 4x memory reduction vs. slight recall degradation |
| ADR-008 | Privacy and Encryption | ~5-10% performance overhead vs. data protection |
| ADR-009 | GraphSAGE Clustering | Novel approach vs. proven methods |
| ADR-010 | Ingest-Tag-Archive Pipeline | Aggressive automation vs. user safety |

All ADRs live in `docs/ADRs/` and follow a consistent format (Context, Decision, Consequences).

### Domain Model

Five bounded contexts defined in `docs/DDDs/`:

| Context | Type | Document |
|---------|------|----------|
| Email Intelligence | Core | DDD-001 |
| Search | Core | DDD-002 |
| Ingestion | Supporting | DDD-003 |
| Learning | Supporting | DDD-004 |
| Account Management | Supporting | DDD-005 |

See [DDD-000-context-map.md](./DDDs/DDD-000-context-map.md) for integration patterns between contexts.

### Evaluation Framework

Located in `docs/evaluation/`. Metrics include:

- **Search quality:** Recall@k, NDCG@10, MRR
- **Classification accuracy:** Macro-F1 across all categories
- **Clustering quality:** Silhouette coefficient, Adjusted Rand Index
- **User study:** "10-minute inbox zero" protocol with task completion time measurement

### Deferred Features (Roadmap)

- FEAT-063: Smart Digest (daily email summary)
- Mobile applications (iOS, Android)
- Calendar integration
- End-to-end encryption (device-to-device)
- Compliance features (GDPR export, retention policies)

### Success Metrics

| Metric | Target |
|--------|--------|
| Search latency (p95) | < 50 ms |
| Categorization accuracy | > 95% macro-F1 |
| Subscription detection | > 98% precision |
| Memory (100K emails) | < 700 MB |
| Lighthouse score | > 90 |

---

## 8. Contributing

### Branch Strategy

- Feature branches from `develop` (e.g., `feat/sona-tier2`, `fix/search-ranking`)
- Pull requests target `develop`
- Releases are cut from `main`

### PR Checklist

Before requesting review, verify:

- [ ] `make ci` passes (format-check, lint, typecheck, test)
- [ ] No new `cargo clippy` warnings
- [ ] TypeScript compiles cleanly (`make typecheck`)
- [ ] New features include tests
- [ ] Security-sensitive changes include `make audit`

### ADR Process

For architectural decisions that affect multiple modules or introduce new dependencies:

1. Create a new ADR in `docs/ADRs/` following the existing format (Context, Decision, Consequences).
2. Number it sequentially (next: ADR-011).
3. Reference it in your PR description.

### Issue Labels

| Label | Usage |
|-------|-------|
| `bug` | Something is broken |
| `feature` | New functionality |
| `documentation` | Docs improvements |
| `security` | Security-related changes |
| `performance` | Performance improvements or regressions |

---

## 9. Useful Commands Reference

### Build and Install

| Command | Description |
|---------|-------------|
| `make install` | Install all dependencies (backend build + frontend install) |
| `make build` | Build everything (backend release + frontend production) |
| `make clean` | Remove all build artifacts |

### Development

| Command | Description |
|---------|-------------|
| `make dev` | Start backend and frontend dev servers with hot-reload |
| `make docker-up-dev` | Start dev stack in Docker with hot-reload |

### Testing

| Command | Description |
|---------|-------------|
| `make test` | Run all tests (backend + frontend) |
| `make ci` | Full CI pipeline: format-check, lint, typecheck, test |
| `make ci-full` | CI + link checking |
| `make -C backend bench` | Run Criterion benchmarks |

### Code Quality

| Command | Description |
|---------|-------------|
| `make lint` | Lint all code and docs |
| `make format` | Auto-format all code and docs |
| `make format-check` | Check formatting without changes |
| `make typecheck` | TypeScript type checking |
| `make deadcode` | Detect unused code |
| `make audit` | Security audit all dependencies |

### Docker

| Command | Description |
|---------|-------------|
| `make docker-build` | Build all Docker images |
| `make docker-build-no-cache` | Build images without cache |
| `make docker-up` | Start production stack |
| `make docker-up-dev` | Start dev stack with hot-reload |
| `make docker-down` | Stop all containers |
| `make docker-down-volumes` | Stop and remove volumes (destroys data) |
| `make docker-restart` | Restart all containers |
| `make docker-health` | Show container health status |
| `make docker-logs` | Tail logs from all containers |
| `make docker-logs-backend` | Tail backend logs only |
| `make docker-logs-frontend` | Tail frontend logs only |
| `make docker-ps` | Show running containers |
| `make docker-exec-backend` | Shell into backend container |
| `make docker-exec-frontend` | Shell into frontend container |
| `make docker-secrets` | Generate development secrets |
| `make docker-clean` | Remove stopped containers and dangling images |

### Dependencies

| Command | Description |
|---------|-------------|
| `make outdated` | Show outdated dependencies (no changes) |
| `make upgrade` | Upgrade within semver ranges |

### Documentation

| Command | Description |
|---------|-------------|
| `make lint-md` | Lint Markdown files |
| `make lint-yaml` | Lint YAML files |
| `make format-md` | Format Markdown with Prettier |
| `make format-yaml` | Format YAML with Prettier |
| `make links-check` | Check internal file links |
| `make links-check-external` | Check external HTTP links |
| `make links-check-all` | Check all links |
