# Emailibrium Audit v2 — March 2026

**Date:** 2026-03-31
**Auditor:** Agentic QE MCP (SAST) + cargo-audit + pnpm audit + manual review
**Scope:** Implementation, testing, documentation, security posture, commercial licensing exposure

---

## Executive Summary

| Domain             | Grade | Verdict                                                                                                                                                                                   |
| ------------------ | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Implementation** | B-    | Solid Rust backend, competent React frontend; 19 files exceed 500-line limit; pre-release deps in production                                                                              |
| **Testing**        | C+    | 70% estimated coverage; backend integration tests exist but are thin (3,566 LOC for 57K LOC codebase); frontend has 637 test files but no coverage gating in CI                           |
| **Documentation**  | A-    | 23 ADRs, 12 DDDs, OpenAPI spec, guides — best-in-class for a project this size; some ADRs reference unbuilt features                                                                      |
| **Security**       | B     | Good crypto primitives (AES-256-GCM, Argon2id), Docker hardening, CSP headers; one known CVE (RSA timing side-channel via sqlx-mysql) with no fix available; SAST noise from WASM codegen |
| **Licensing**      | A     | All-permissive stack (MIT/Apache-2.0/BSD); 9 MPL-2.0 crates (file-level copyleft, acceptable); zero GPL/AGPL/SSPL exposure                                                                |

**Overall quality gate: FAILED** (score 72/100, threshold 80)

---

## 1. Implementation — Brutally Honest

### What's good

- **Rust backend (57K LOC)** is well-structured around DDD bounded contexts: email, vectors, content, rules, cache, config, API. Type safety and SQLx compile-time query checking are real strengths.
- **Frontend monorepo (25K LOC)** uses a modern, well-chosen stack: React 19, TanStack Router/Query, Zustand, Radix UI, Tailwind, Vite 8. Workspace separation (`types`, `api`, `core`, `ui`, `web`) is clean.
- **Vector intelligence** via ruvector (HNSW, GNN clustering, quantization, SONA learning) is ambitious and genuinely differentiating.
- **Build system** is excellent: 100+ Makefile targets, Turborepo orchestration, Docker multi-stage builds, Dependabot with grouped updates.

### What's not

- **19 backend files exceed the 500-line limit** set in CLAUDE.md. The worst offenders:
  - `vectors/clustering.rs` — 1,867 lines
  - `vectors/search.rs` — 1,770 lines
  - `vectors/ingestion.rs` — 1,555 lines
  - `vectors/embedding.rs` — 1,497 lines
  - `email/gmail.rs` — 1,462 lines
  - `vectors/learning.rs` — 1,460 lines
  - `vectors/generative.rs` — 1,404 lines
  - `api/ingestion.rs` — 1,287 lines
  - `vectors/config.rs` — 1,272 lines
  - These files are 2-4x over the stated architectural ceiling. The vectors/ directory alone accounts for ~12K LOC in oversized files.

- **3 pre-release dependencies in production:**
  - `apalis 1.0.0-rc.6` — background job queue (release candidate)
  - `apalis-sql 1.0.0-rc.6` — SQL backend for apalis (release candidate)
  - `async_zip 0.0.18` — ZIP extraction (pre-1.0, version 0.0.x)
  - These carry API instability risk and may have undiscovered bugs.

- **Frontend god components:**
  - `EmailClient.tsx` — 853 lines (orchestrates too much)
  - `AISettings.tsx` — 775 lines (settings sprawl)

- **4 TODO/FIXME markers** remain in backend source.

- **Cyclomatic complexity: 10.69 average** (warning threshold). Likely concentrated in the oversized vector files.

- **Maintainability score: 68/100** — dragged down by file sizes and complexity.

---

## 2. Testing — Brutally Honest

### Coverage: 70% (estimated)

| Layer                     | Files     | Lines      | Ratio to Source | Verdict         |
| ------------------------- | --------- | ---------- | --------------- | --------------- |
| Backend integration tests | 11        | 3,566      | 6.2% of 57K     | **Thin**        |
| Frontend unit tests       | 637 files | ~15K (est) | ~60% of 25K     | Adequate        |
| E2E (Playwright)          | 6 specs   | —          | Low             | Needs expansion |

### What's good

- Property-based testing (`proptest`) for search, content extraction, quantization — smart.
- Criterion benchmarks for vector operations — performance is tracked.
- Frontend uses MSW for API mocking, `@testing-library/react` for user-centric tests.
- CI runs `cargo test`, `vitest`, format/lint/typecheck gates.

### What's not

- **Backend test-to-source ratio is 6.2%.** For a 57K LOC Rust backend handling email provider OAuth, encryption, vector search, and content extraction, 11 integration test files is dangerously thin. Key gaps:
  - No dedicated tests for OAuth token refresh flows
  - No tests for encryption key rotation
  - No tests for content extraction edge cases (malformed HTML, oversized PDFs, corrupt ZIP)
  - No tests for cache invalidation logic (Moka + Redis)
  - No tests for migration rollbacks
  - `security_audit.rs` exists but scope is unknown

- **No coverage gating in CI.** Tests run but there's no minimum coverage threshold enforced. The 70% estimate is heuristic — actual coverage is unmeasured.

- **E2E coverage is skeletal.** 6 Playwright specs for a full email client SPA with onboarding, settings, command palette, inbox cleaner, rules, and AI features.

- **No load/stress testing.** For a system that handles email sync, embedding generation, and vector search, there are no concurrent-user or throughput benchmarks.

- **No contract tests** between frontend API client and backend REST endpoints.

---

## 3. Documentation — Brutally Honest

### What's good

- **23 ADRs** covering hybrid search, embeddings, privacy, GDPR, security middleware, content extraction, quantization, RAG. This is exceptional.
- **12 DDD context maps** — domain modeling is rigorous and well-documented.
- **OpenAPI spec** for the REST API (12 endpoints).
- **Dedicated guides**: setup, deployment, OAuth, user guide, maintainer guide, releasing, configuration reference.
- **Total documentation: ~130KB** across 17+ markdown files — comprehensive.

### What's not

- **ADR gaps**: ADR-011, ADR-012, ADR-013 are listed as "(future)" — placeholder slots break the numbering contract.
- **ADR drift**: Some ADRs describe features at a design level that may not be fully implemented (SONA 3-tier learning, GNN clustering). No ADR status tracking (Proposed/Accepted/Deprecated).
- **No runbook or incident response** documentation. For a system handling OAuth tokens and encrypted email data, this is a gap.
- **No API versioning strategy** documented despite OpenAPI spec existing.
- **Configuration reference (27.9KB)** — large but may drift from actual code. No automated validation that config docs match `figment` schema.
- **Research docs reference academic papers** but don't clearly distinguish "implemented" from "aspirational."

---

## 4. Security Posture — Brutally Honest

### AQE SAST Results

| Severity | Raw Count | After Triage        |
| -------- | --------- | ------------------- |
| Critical | 229       | ~2 real (see below) |
| High     | 224       | ~10-20 (estimated)  |
| Medium   | 386       | ~50-100 (estimated) |

**Triage notes:** The SAST scanner flagged 839 findings across 5,727 files (1,914 JS/TS). The vast majority are **false positives from WASM-generated JavaScript** in the `ruvector/` submodule:

- **~220 "Code Injection via Function Constructor" criticals** — these are `wasm-bindgen` codegen artifacts (`new Function(...)` in WASM glue code). Not exploitable in the emailibrium context. **False positives.**
- **2 "SQL Injection" criticals** in `.claude/helpers/statusline.cjs` — a Claude Code tooling file, not production code. **False positives.**
- The remaining high/medium findings need manual review but are likely pattern-matching noise from the ruvector submodule's generated code.

### Real Security Findings

| #   | Finding                                                                           | Severity     | Status                                                                                                     |
| --- | --------------------------------------------------------------------------------- | ------------ | ---------------------------------------------------------------------------------------------------------- |
| 1   | **RUSTSEC-2023-0071**: RSA timing side-channel in `rsa 0.9.10` (via `sqlx-mysql`) | Medium (5.9) | **No fix available.** Emailibrium uses SQLite, not MySQL — risk is low but the crate is still compiled in. |
| 2   | **Unmaintained crate**: `bincode 1.3.3` and `2.0.1` (via `hnsw_rs`)               | Advisory     | Upstream dependency; monitor for replacement.                                                              |
| 3   | **Unmaintained crate**: `number_prefix 0.4.0` (via `hf-hub → fastembed`)          | Advisory     | Transitive; low risk.                                                                                      |
| 4   | **Unmaintained crate**: `paste 1.0.15`                                            | Advisory     | Widely used, forked alternatives exist.                                                                    |

### What's good

- **Crypto choices are correct**: AES-256-GCM for encryption at rest, Argon2id for password hashing, `zeroize` for key material cleanup.
- **Docker hardening** is solid: `read_only: true`, `cap_drop: ALL`, `no-new-privileges`, non-root user.
- **CSP headers** and security middleware (ADR-016).
- **GDPR compliance design** (ADR-017).
- **Secret management** via templates with `.gitignore` enforcement.
- **`cargo-audit` runs in CI** — known vulnerabilities are checked on every build.

### What's not

- **No DAST testing.** SAST alone misses runtime injection, auth bypass, and CORS misconfiguration.
- **No dependency pinning for ruvector submodule.** It's a local path dependency — whoever controls the submodule controls the entire backend's trust boundary.
- **No SBOM generation.** For a project handling sensitive email data, a Software Bill of Materials is expected for compliance.
- **No rate limiting documented** on API endpoints (OAuth callbacks, login, email sync triggers).
- **The RSA advisory has no fix.** SQLx pulls in `sqlx-mysql` even when only SQLite features are used. Consider disabling `sqlx-mysql` feature if possible.
- **npm audit: clean** (0 known vulnerabilities in production deps — good).

---

## 5. Commercial Licensing Exposure — Brutally Honest

### Rust Dependency Licenses (719 crates)

| License             | Crate Count | Risk                    |
| ------------------- | ----------- | ----------------------- |
| MIT                 | 143         | None                    |
| MIT OR Apache-2.0   | 334         | None                    |
| Apache-2.0          | 16          | None (permissive)       |
| Apache-2.0 OR MIT   | 43          | None                    |
| BSD-2-Clause        | 3           | None                    |
| BSD-3-Clause        | 7           | None                    |
| **MPL-2.0**         | **7**       | **File-level copyleft** |
| ISC                 | 6           | None                    |
| Unicode-3.0         | 18          | None                    |
| Unlicense OR MIT    | 7           | None                    |
| Zlib                | 2           | None                    |
| CC0-1.0             | 1           | None                    |
| BSL-1.0             | 1           | None                    |
| CDLA-Permissive-2.0 | 3           | None                    |

**MPL-2.0 crates (7):**

- `cssparser 0.35.0`, `cssparser 0.36.0`, `cssparser-macros 0.6.1` — CSS parsing (via `scraper`)
- `dtoa-short 0.3.5` — float-to-string conversion
- `option-ext 0.2.0` — Option extensions
- `selectors 0.36.1` — CSS selectors (via `scraper`)
- `webpki-roots 0.25.4` — TLS root certificates

**MPL-2.0 verdict:** File-level copyleft only. If you modify these specific crate source files, you must release those modifications under MPL-2.0. Using them as dependencies with no modifications imposes **zero obligations**. This is standard and commercially safe.

### npm Dependency Licenses (673 packages)

| License       | Package Count | Risk                    |
| ------------- | ------------- | ----------------------- |
| MIT           | 588           | None                    |
| ISC           | 36            | None                    |
| Apache-2.0    | 18            | None                    |
| BSD-2-Clause  | 8             | None                    |
| BSD-3-Clause  | 4             | None                    |
| BlueOak-1.0.0 | 7             | None (permissive)       |
| CC-BY-4.0     | 1             | None (attribution)      |
| CC0-1.0       | 1             | None                    |
| **MPL-2.0**   | **2**         | **File-level copyleft** |
| MIT-0         | 2             | None                    |
| MIT AND ISC   | 1             | None                    |
| 0BSD          | 1             | None                    |
| Unlicense     | 1             | None                    |

### Overall licensing verdict

**Zero GPL, AGPL, SSPL, EUPL, OSL, or CPAL exposure.** The entire dependency tree is commercially safe. The 9 total MPL-2.0 crates/packages impose file-level copyleft only on modifications to those specific files — standard, low-risk, and universally accepted in commercial software.

**The project license (MIT) is compatible with all dependencies.**

---

## 6. Actionable Recommendations (Priority Order)

### P0 — Before any production deployment

| #   | Action                                                                       | Effort  |
| --- | ---------------------------------------------------------------------------- | ------- |
| 1   | **Enforce test coverage threshold** in CI (target 80%, gate at 75%)          | 1 day   |
| 2   | **Add backend unit tests** for OAuth refresh, encryption, cache invalidation | 1 week  |
| 3   | **Disable `sqlx-mysql` feature** to eliminate RUSTSEC-2023-0071 RSA advisory | 1 hour  |
| 4   | **Add rate limiting** to auth and sync API endpoints                         | 2 days  |
| 5   | **Generate SBOM** (use `cargo-sbom` + `cyclonedx-npm`)                       | 2 hours |

### P1 — Next sprint

| #   | Action                                                                                                                        | Effort |
| --- | ----------------------------------------------------------------------------------------------------------------------------- | ------ |
| 6   | **Refactor oversized files** — split the 19 files >500 lines, starting with `clustering.rs` (1,867L) and `search.rs` (1,770L) | 1 week |
| 7   | **Replace pre-release deps**: find stable alternatives for `apalis` RC and `async_zip 0.0.x`, or pin and document the risk    | 2 days |
| 8   | **Add E2E test coverage** — expand from 6 Playwright specs to cover onboarding, settings, rules, AI features                  | 1 week |
| 9   | **Add API contract tests** between frontend `ky` client and backend OpenAPI spec                                              | 3 days |
| 10  | **Add ADR status tracking** (Proposed/Accepted/Superseded/Deprecated) and close placeholder gaps                              | 1 day  |

### P2 — Backlog

| #   | Action                                                                                                                         | Effort |
| --- | ------------------------------------------------------------------------------------------------------------------------------ | ------ |
| 11  | **Run DAST** scans against deployed backend (OWASP ZAP or similar)                                                             | 2 days |
| 12  | **Add incident response runbook** for OAuth token leak, data breach, key rotation                                              | 2 days |
| 13  | **Add load testing** (k6 or similar) for email sync + vector search concurrent users                                           | 3 days |
| 14  | **Exclude ruvector WASM artifacts** from SAST scanning (`.sast-ignore` or equivalent) to reduce noise from 839 to ~50 findings | 1 hour |
| 15  | **Document API versioning strategy** before adding more endpoints                                                              | 1 day  |
| 16  | **Split `EmailClient.tsx` (853L)** and `AISettings.tsx (775L)` into sub-components                                             | 1 day  |

---

## Appendix A — AQE Quality Gate

```text
Quality Score:     72 / 100  (FAILED — threshold 80)
Coverage:          70%
Complexity:        10.69 (warning)
Maintainability:   68.0
Security:          85.0
```

## Appendix B — AQE Security Scan Summary

```text
Files scanned:     5,727 (1,914 JS/TS + 3,813 other)
Lines scanned:     557,550
Rules applied:     83
Total findings:    839 (229 critical, 224 high, 386 medium)
After triage:      ~2 real criticals (RSA advisory + bincode unmaintained)
False positive rate: ~95% (WASM codegen + Claude tooling artifacts)
```

## Appendix C — Dependency Audit Summary

```text
Cargo audit:       1 vulnerability (RUSTSEC-2023-0071, medium, no fix)
                   3 unmaintained crate warnings
npm audit:         0 known vulnerabilities
License risk:      None (0 GPL/AGPL/SSPL)
MPL-2.0 exposure:  9 crates/packages (file-level copyleft, safe for commercial use)
Pre-release deps:  3 (apalis RC x2, async_zip 0.0.x)
```

## Appendix D — Codebase Metrics

```text
Backend (Rust):    57,055 LOC across 105 files
Frontend (TS/TSX): 24,764 LOC across ~100 files
Backend tests:     3,566 LOC across 11 files (6.2% ratio)
Frontend tests:    637 files
E2E tests:         6 Playwright specs
Documentation:     ~130KB across 17+ files, 23 ADRs, 12 DDDs
Makefile targets:  100+
CI jobs:           11 (format, lint, clippy, test, audit, lighthouse, bundlewatch, markdown, yaml, shell)
```
