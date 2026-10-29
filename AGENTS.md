# emailibrium

Vector-native, local-first email intelligence: semantic search, clustering, classification, and inbox cleanup over 10k+ emails with no cloud processing. Rust backend + React SPA.

## Layout

| Path                   | What                                                                                                                   | Stack                                                                 |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `backend/`             | Axum REST + SSE API and the intelligence layer                                                                         | Rust 1.97 (edition 2021), SQLx/SQLite, Moka, Redis (optional)         |
| `backend/src/vectors/` | 48 vector intelligence modules: embedding, HNSW search, SONA learning, clustering, RAG, encryption                     | —                                                                     |
| `backend/src/mcp/`     | MCP server exposing email tools                                                                                        | —                                                                     |
| `backend/migrations/`  | Numbered SQLite migrations — **append the next number, never edit an applied one**                                     | —                                                                     |
| `frontend/`            | pnpm + Turborepo monorepo (app in `apps/web/`)                                                                         | React 19, TS ^6.0.3, Vite 8, TanStack Router/Query, Zustand, Tailwind |
| `ruvector/`            | **Git submodule** (ruvnet/ruvector) — the vector engine. Treat as vendored: don't edit; backend depends on it via path | Rust workspace                                                        |
| `docs/`                | `architecture.md`, `ADRs/`, `DDDs/`, evaluation, setup/oauth guides                                                    | —                                                                     |
| `config/`, `secrets/`  | Runtime config + dev secrets (never commit secrets)                                                                    | —                                                                     |

## Build & Test — justfile-driven, not npm

The root `package.json` only wires Husky; **do not run `npm build`/`npm test`**. Use `just`:

```bash
just ci          # format-check + lint + typecheck + test (run before committing)
just test        # backend (cargo) + frontend (Vitest)
just build       # build everything
just dev         # full stack: backend :8080, frontend :3000
just lint        # cargo clippy + eslint + docs (markdownlint, yamllint)
just audit       # cargo-audit + pnpm audit
just --list        # all targets
```

Backend-only: `cd backend && cargo test` / `cargo clippy`. Frontend-only: `cd frontend && pnpm test` / `pnpm lint` / `pnpm typecheck`.

Backend Cargo features: `vectors` (default), `builtin-llm` (llama-cpp, opt-in), `proptest`.

## Conventions

- **Decisions are ADR/DDD-gated.** Before changing architecture, vector storage, learning, or AI providers, check `docs/ADRs/` and `docs/DDDs/` — e.g. ADR-003 fixes RuVector as the primary vector store. Record new decisions as an ADR.
- **Privacy is a hard guarantee, not a setting.** Embeddings/models run and stay local; cloud AI is strictly opt-in. Don't add code paths that send email content off-machine by default.
- Encryption at rest is AES-256-GCM + Argon2id — keep crypto changes within `backend/src/vectors/encryption.rs` and consent-gated.
- For code navigation, impact analysis, and safe refactors, use the GitNexus guidance below.

<!-- managed by ruflo-setup-aqe — aqe init skips regeneration when this sentinel is present -->

<!-- gitnexus:start -->

# GitNexus — Code Intelligence

This project is indexed by GitNexus as **emailibrium** (228117 symbols, 447347 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## When Debugging

1. `gitnexus_query({query: "<error or symptom>"})` — find execution flows related to the issue
2. `gitnexus_context({name: "<suspect function>"})` — see all callers, callees, and process participation
3. `READ gitnexus://repo/emailibrium/process/{processName}` — trace the full execution flow step by step
4. For regressions: `gitnexus_detect_changes({scope: "compare", base_ref: "main"})` — see what your branch changed

## When Refactoring

- **Renaming**: MUST use `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` first. Review the preview — graph edits are safe, text_search edits need manual review. Then run with `dry_run: false`.
- **Extracting/Splitting**: MUST run `gitnexus_context({name: "target"})` to see all incoming/outgoing refs, then `gitnexus_impact({target: "target", direction: "upstream"})` to find all external callers before moving code.
- After any refactor: run `gitnexus_detect_changes({scope: "all"})` to verify only expected files changed.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Tools Quick Reference

| Tool             | When to use                   | Command                                                                 |
| ---------------- | ----------------------------- | ----------------------------------------------------------------------- |
| `query`          | Find code by concept          | `gitnexus_query({query: "auth validation"})`                            |
| `context`        | 360-degree view of one symbol | `gitnexus_context({name: "validateUser"})`                              |
| `impact`         | Blast radius before editing   | `gitnexus_impact({target: "X", direction: "upstream"})`                 |
| `detect_changes` | Pre-commit scope check        | `gitnexus_detect_changes({scope: "staged"})`                            |
| `rename`         | Safe multi-file rename        | `gitnexus_rename({symbol_name: "old", new_name: "new", dry_run: true})` |
| `cypher`         | Custom graph queries          | `gitnexus_cypher({query: "MATCH ..."})`                                 |

## Impact Risk Levels

| Depth | Meaning                               | Action                |
| ----- | ------------------------------------- | --------------------- |
| d=1   | WILL BREAK — direct callers/importers | MUST update these     |
| d=2   | LIKELY AFFECTED — indirect deps       | Should test           |
| d=3   | MAY NEED TESTING — transitive         | Test if critical path |

## Resources

| Resource                                     | Use for                                  |
| -------------------------------------------- | ---------------------------------------- |
| `gitnexus://repo/emailibrium/context`        | Codebase overview, check index freshness |
| `gitnexus://repo/emailibrium/clusters`       | All functional areas                     |
| `gitnexus://repo/emailibrium/processes`      | All execution flows                      |
| `gitnexus://repo/emailibrium/process/{name}` | Step-by-step execution trace             |

## Self-Check Before Finishing

Before completing any code modification task, verify:

1. `gitnexus_impact` was run for all modified symbols
2. No HIGH/CRITICAL risk warnings were ignored
3. `gitnexus_detect_changes()` confirms changes match expected scope
4. All d=1 (WILL BREAK) dependents were updated

## Keeping the Index Fresh

After committing code changes, the GitNexus index becomes stale. Re-run analyze to update it:

```bash
npx gitnexus analyze
```

If the index previously included embeddings, preserve them by adding `--embeddings`:

```bash
npx gitnexus analyze --embeddings
```

To check whether embeddings exist, inspect `.gitnexus/meta.json` — the `stats.embeddings` field shows the count (0 means no embeddings). **Running analyze without `--embeddings` will delete any previously generated embeddings.**

> No PostToolUse hook wires this automatically in this repo (`.claude/settings.json` and
> `.claude/settings.local.json` have no `gitnexus` hook entry) -- re-run `npx gitnexus
analyze` by hand after `git commit` or `git merge`.

## CLI

No `.claude/skills/gitnexus/` skill files exist in this repo (`find . -iname "*gitnexus*"`
returns nothing) -- the six-row skill-file table this section used to point at was
aspirational, not present. `gitnexus` itself is a real CLI (available on `PATH` / via `npx
gitnexus`); run `npx gitnexus --help` for its command reference directly.

<!-- gitnexus:end -->

## Project-local tool usage

- Use the active host's supported tools and project configuration; do not infer
  runtime health, provider identity, or authorization from an instruction file.
- Discover AQE/Ruflo tools when needed and use bounded commands. Preserve project
  state, databases, patterns, and evidence; deletion needs explicit approval and backup.
- Report actual checks and their limitations; do not invent tests, coverage or results.
- Keep credentials out of source. Respect configured providers and spending limits.
- Commits, pushes, publishing and destructive cleanup require explicit user approval.
- Keep shared instructions in this file and CLAUDE.md as exactly `@AGENTS.md`.
  Do not regenerate machine-wide guidance or depend on user-level instruction files.
