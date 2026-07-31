<!-- Operating rules, ruflo CLI, swarm defaults (hierarchical-mesh / 15 / hybrid), and AQE guidance live in ~/.claude/CLAUDE.md — not repeated here. -->

# emailibrium

Vector-native, local-first email intelligence: semantic search, clustering, classification, and inbox cleanup over 10k+ emails with no cloud processing. Rust backend + React SPA.

## Layout

| Path                   | What                                                                                                                   | Stack                                                              |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `backend/`             | Axum REST + SSE API and the intelligence layer                                                                         | Rust 1.96 (edition 2021), SQLx/SQLite, Moka, Redis                 |
| `backend/src/vectors/` | 22+ vector intelligence modules: embedding, HNSW search, SONA learning, clustering, RAG, encryption                    | —                                                                  |
| `backend/src/mcp/`     | MCP server exposing email tools                                                                                        | —                                                                  |
| `backend/migrations/`  | Numbered SQLite migrations — **append the next number, never edit an applied one**                                     | —                                                                  |
| `frontend/`            | pnpm + Turborepo monorepo (app in `apps/web/`)                                                                         | React 19, TS 5.9, Vite 8, TanStack Router/Query, Zustand, Tailwind |
| `ruvector/`            | **Git submodule** (ruvnet/ruvector) — the vector engine. Treat as vendored: don't edit; backend depends on it via path | Rust workspace                                                     |
| `docs/`                | `architecture.md`, `ADRs/`, `DDDs/`, evaluation, setup/oauth guides                                                    | —                                                                  |
| `config/`, `secrets/`  | Runtime config + dev secrets (never commit secrets)                                                                    | —                                                                  |

## Build & Test — Makefile-driven, not npm

The root `package.json` only wires Husky; **do not run `npm build`/`npm test`**. Use `make`:

```bash
make ci          # format-check + lint + typecheck + test (run before committing)
make test        # backend (cargo) + frontend (Vitest)
make build       # build everything
make dev         # full stack: backend :8080, frontend :3000
make lint        # code + docs (markdownlint, yamllint)
make audit       # cargo-audit + npm audit
make help        # all targets
```

Backend-only: `cd backend && cargo test` / `cargo clippy`. Frontend-only: `cd frontend && pnpm test` / `pnpm lint` / `pnpm typecheck`.

Backend Cargo features: `vectors` (default), `builtin-llm` (llama-cpp, opt-in), `proptest`.

## Conventions

- **Decisions are ADR/DDD-gated.** Before changing architecture, vector storage, learning, or AI providers, check `docs/ADRs/` and `docs/DDDs/` — e.g. ADR-003 fixes RuVector as the primary vector store. Record new decisions as an ADR.
- **Privacy is a hard guarantee, not a setting.** Embeddings/models run and stay local; cloud AI is strictly opt-in. Don't add code paths that send email content off-machine by default.
- Encryption at rest is AES-256-GCM + Argon2id — keep crypto changes within `backend/src/vectors/encryption.rs` and consent-gated.
- For code navigation, impact analysis, and safe refactors, use the GitNexus MCP tools per `AGENTS.md`.

<!-- managed by ruflo-setup-aqe — aqe init skips regeneration when this sentinel is present -->
<!-- Agentic QE v3: see ~/.claude/CLAUDE.md for full AQE operating guidance -->
