# Documentation inventory

Ground-truth catalog of every git-tracked doc in the repo, bucketed by the persona that owns it
(per the docs-accuracy-audit pipeline goal — see `.autopilot/pipeline.yml`). This is phase 0's
only deliverable alongside `drift-report.md`: it records **what exists**, not whether it's
accurate — that's `drift-report.md` and phases 1–5.

**Method:** `git ls-files -- '*.md' '*.yaml' '*.yml' 'images/*'`, filtered to drop vendored
trees (`backend/`, `frontend/`, `ruvector/` — 118 files remain) and tooling artifacts that
aren't documentation prose: `.github/**` (8 — CI workflow definitions, not docs), `.autopilot/**`
(4), `.markdownlint.yaml`/`.yamllint.yaml` (2 lint configs), `pnpm-lock.yaml` (1), the
`config/*.yaml` runtime data files already covered as ground truth by
`scripts/audit/extract-config-keys.sh` (7 direct children of `config/`, plus 2 more nested under
`config/environments/` that the same rule intends but a literal `config/*.yaml` glob doesn't
reach), and this inventory's own 2 files (`docs/audit/inventory.md`,
`docs/audit/drift-report.md` — the catalog can't list itself mid-build). 118 − 8 − 4 − 2 − 1 − 7 −
2 − 2 = **92 tracked docs, zero unclassified** — every remaining file matched one of the plan's
six personas or a documented catch-all rule (below); verified by diffing the full 92-file list
against every path named in the six persona tables below (no file present in neither). "Last
meaningful update" is `git log -1 --format=%cs -- <file>` (commit date, not "content actually
changed" — a doc bumped only by a repo-wide reformat still shows that commit). Images use file
size in place of a line count.

## Persona definitions (from `.autopilot/pipeline.yml`)

| Persona             | Named doc set in the plan                                             | Catch-all rule applied here                                                                                                                                                 |
| ------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **End user**        | README, QUICKSTART, user-guide, UI overview, `images/*`               | — (fully named)                                                                                                                                                             |
| **Operator**        | setup/deployment/oauth guides, configuration-reference, compose files | + `secrets/README.md` (documents the secrets _mechanism_, which is this persona's job per phase 2's conventions)                                                            |
| **API**             | `docs/api/openapi.yaml`, MCP tool docs                                | No standalone MCP tool doc file exists yet — phase 3 creates one; that absence is itself a drift item below                                                                 |
| **Contributor**     | maintainer-guide, architecture, releasing                             | + `CHANGELOG.md`, `docs/evaluation/**`, `docs/test-plan-group-by-sender.md` — maintainer-facing, not end-user or decision-record                                            |
| **AI coding agent** | `CLAUDE.md`, `AGENTS.md`                                              | — (fully named)                                                                                                                                                             |
| **Decision reader** | `docs/ADRs/**`, `docs/DDDs/**`                                        | + `docs/plan/**`, `docs/research/**`, `OPTIMIZATION_SPEC.md` — all point-in-time historical records per the plan's phase-5 convention ("date them rather than update them") |

## End user (11 docs)

| Doc                                  | Last meaningful update | Lines         |
| ------------------------------------ | ---------------------- | ------------- |
| `QUICKSTART.md`                      | 2026-08-01             | 87            |
| `README.md`                          | 2026-08-01             | 265           |
| `docs/user-guide.md`                 | 2026-08-01             | 252           |
| `docs/user-interface-overview.md`    | 2026-04-23             | 88            |
| `images/02-command-center.png`       | 2026-04-23             | 792K (binary) |
| `images/03-email-reader.png`         | 2026-04-23             | 1.2M (binary) |
| `images/04-inbox-cleaner-wizard.png` | 2026-04-23             | 316K (binary) |
| `images/05-insights.png`             | 2026-04-23             | 476K (binary) |
| `images/06-rules-studio.png`         | 2026-04-23             | 352K (binary) |
| `images/07-chat.png`                 | 2026-04-23             | 368K (binary) |
| `images/08-settings.png`             | 2026-04-23             | 504K (binary) |

## Operator (7 docs)

| Doc                               | Last meaningful update | Lines |
| --------------------------------- | ---------------------- | ----- |
| `docker-compose.dev.yml`          | 2026-08-01             | 47    |
| `docker-compose.yml`              | 2026-08-01             | 258   |
| `docs/configuration-reference.md` | 2026-03-26             | 285   |
| `docs/deployment-guide.md`        | 2026-08-01             | 439   |
| `docs/oauth-setup-guide.md`       | 2026-08-01             | 284   |
| `docs/setup-guide.md`             | 2026-08-01             | 334   |
| `secrets/README.md`               | 2026-03-27             | 56    |

## API (1 doc)

| Doc                     | Last meaningful update | Lines |
| ----------------------- | ---------------------- | ----- |
| `docs/api/openapi.yaml` | 2026-03-24             | 817   |

No MCP tool documentation file exists — see `pl-no-mcp-tool-doc` in `drift-report.md`.

## Contributor (7 docs)

| Doc                                      | Last meaningful update | Lines |
| ---------------------------------------- | ---------------------- | ----- |
| `CHANGELOG.md`                           | 2026-05-18             | 2304  |
| `docs/architecture.md`                   | 2026-03-27             | 293   |
| `docs/evaluation/domain-adaptation.md`   | 2026-03-24             | 172   |
| `docs/evaluation/inbox-zero-protocol.md` | 2026-03-24             | 184   |
| `docs/maintainer-guide.md`               | 2026-08-01             | 694   |
| `docs/releasing.md`                      | 2026-08-01             | 158   |
| `docs/test-plan-group-by-sender.md`      | 2026-03-27             | 250   |

## AI coding agent (2 docs)

| Doc         | Last meaningful update | Lines |
| ----------- | ---------------------- | ----- |
| `AGENTS.md` | 2026-04-20             | 103   |
| `CLAUDE.md` | 2026-08-01             | 46    |

## Decision reader (64 docs)

| Doc                                                          | Last meaningful update | Lines |
| ------------------------------------------------------------ | ---------------------- | ----- |
| `OPTIMIZATION_SPEC.md`                                       | 2026-08-01             | 238   |
| `docs/ADRs/ADR-001-hybrid-search-architecture.md`            | 2026-05-12             | 101   |
| `docs/ADRs/ADR-002-embedding-model-selection.md`             | 2026-05-12             | 110   |
| `docs/ADRs/ADR-003-ruvector-vector-database.md`              | 2026-05-12             | 127   |
| `docs/ADRs/ADR-004-sona-adaptive-learning.md`                | 2026-05-12             | 165   |
| `docs/ADRs/ADR-005-tauri-to-web-spa-migration.md`            | 2026-05-12             | 143   |
| `docs/ADRs/ADR-006-multi-asset-content-extraction.md`        | 2026-05-12             | 79    |
| `docs/ADRs/ADR-007-adaptive-quantization-strategy.md`        | 2026-05-12             | 73    |
| `docs/ADRs/ADR-008-privacy-embedding-security.md`            | 2026-05-12             | 67    |
| `docs/ADRs/ADR-009-gnn-clustering-architecture.md`           | 2026-05-12             | 78    |
| `docs/ADRs/ADR-010-ingest-tag-archive-pipeline.md`           | 2026-05-12             | 87    |
| `docs/ADRs/ADR-011-onnx-runtime-embedding-provider.md`       | 2026-05-12             | 134   |
| `docs/ADRs/ADR-012-tiered-ai-provider-architecture.md`       | 2026-05-12             | 166   |
| `docs/ADRs/ADR-013-ai-model-lifecycle-management.md`         | 2026-05-12             | 173   |
| `docs/ADRs/ADR-014-rule-engine.md`                           | 2026-03-24             | 76    |
| `docs/ADRs/ADR-015-offline-sync.md`                          | 2026-03-24             | 102   |
| `docs/ADRs/ADR-016-security-middleware.md`                   | 2026-03-24             | 97    |
| `docs/ADRs/ADR-017-gdpr-compliance.md`                       | 2026-03-24             | 106   |
| `docs/ADRs/ADR-018-provider-folder-label-operations.md`      | 2026-03-27             | 144   |
| `docs/ADRs/ADR-019-email-body-rendering.md`                  | 2026-03-27             | 115   |
| `docs/ADRs/ADR-020-email-attachment-management.md`           | 2026-03-27             | 181   |
| `docs/ADRs/ADR-021-addendum-rust-backend-llm.md`             | 2026-03-27             | 127   |
| `docs/ADRs/ADR-021-built-in-local-llm.md`                    | 2026-03-27             | 191   |
| `docs/ADRs/ADR-021-clustering-performance.md`                | 2026-04-03             | 230   |
| `docs/ADRs/ADR-022-rag-pipeline.md`                          | 2026-03-27             | 113   |
| `docs/ADRs/ADR-028-mcp-tool-calling-chat.md`                 | 2026-07-31             | 886   |
| `docs/ADRs/ADR-029-enhanced-rag-pipeline.md`                 | 2026-05-12             | 734   |
| `docs/ADRs/ADR-030-cleanup-dry-run.md`                       | 2026-05-05             | 241   |
| `docs/ADRs/ADR-031-multi-tenancy-groundwork.md`              | 2026-06-15             | 95    |
| `docs/ADRs/ADR-032-make-to-just-task-runner.md`              | 2026-08-01             | 81    |
| `docs/DDDs/DDD-000-context-map.md`                           | 2026-03-27             | 217   |
| `docs/DDDs/DDD-001-email-intelligence.md`                    | 2026-03-27             | 253   |
| `docs/DDDs/DDD-002-search.md`                                | 2026-03-27             | 220   |
| `docs/DDDs/DDD-003-ingestion.md`                             | 2026-03-30             | 274   |
| `docs/DDDs/DDD-004-learning.md`                              | 2026-03-30             | 314   |
| `docs/DDDs/DDD-005-account-management.md`                    | 2026-03-30             | 316   |
| `docs/DDDs/DDD-006-ai-providers-addendum-built-in-llm.md`    | 2026-03-27             | 317   |
| `docs/DDDs/DDD-006-ai-providers.md`                          | 2026-03-27             | 679   |
| `docs/DDDs/DDD-007-rules-domain.md`                          | 2026-03-27             | 231   |
| `docs/DDDs/DDD-008-addendum-cleanup-planning.md`             | 2026-05-05             | 377   |
| `docs/DDDs/DDD-008-email-operations.md`                      | 2026-03-27             | 311   |
| `docs/DDDs/DDD-009-email-content-rendering.md`               | 2026-03-27             | 245   |
| `docs/DDDs/DDD-010-rag-domain.md`                            | 2026-03-27             | 121   |
| `docs/plan/built-in-llm-implementation.md`                   | 2026-03-27             | 315   |
| `docs/plan/ci-potential-improvements.md`                     | 2026-07-31             | 204   |
| `docs/plan/cleanup-dry-run-implementation.md`                | 2026-05-05             | 535   |
| `docs/plan/email-interaction-enhancements-implementation.md` | 2026-03-27             | 579   |
| `docs/plan/implementation.md`                                | 2026-05-12             | 519   |
| `docs/plan/inception.md`                                     | 2026-05-04             | 3417  |
| `docs/plan/ingestion-categorization-navigation.md`           | 2026-03-30             | 493   |
| `docs/plan/llm-implementation-supplemental.md`               | 2026-03-27             | 523   |
| `docs/plan/march-2026-audit.md`                              | 2026-03-24             | 772   |
| `docs/plan/march-2026-audit.v2.md`                           | 2026-04-01             | 302   |
| `docs/plan/mcp-maturation.md`                                | 2026-08-01             | 1479  |
| `docs/plan/model-catalog-externalization.md`                 | 2026-03-27             | 276   |
| `docs/plan/predecessor-recommendations.md`                   | 2026-03-27             | 341   |
| `docs/plan/rust-builtin-llm-implementation.md`               | 2026-07-31             | 189   |
| `docs/research/2026-model-leaderboard-research.md`           | 2026-03-27             | 61    |
| `docs/research/cleanup-dry-run-due-diligence.md`             | 2026-05-04             | 283   |
| `docs/research/email-interaction-enhancements.md`            | 2026-03-27             | 786   |
| `docs/research/hardcoded-config-audit.md`                    | 2026-03-27             | 27    |
| `docs/research/initial.md`                                   | 2026-03-27             | 398   |
| `docs/research/llm-options.md`                               | 2026-03-27             | 844   |
| `docs/research/onboarding-auth-alternatives.md`              | 2026-04-23             | 243   |

**Note:** `docs/ADRs/ADR-021-*` has three files sharing the number 021 (addendum-rust-backend-llm,
built-in-local-llm, clustering-performance) — an ADR-log numbering collision, logged as a
parking-lot item (`pl-adr-021-numbering-collision`) rather than fixed here per phase 0's
no-remediation rule.
