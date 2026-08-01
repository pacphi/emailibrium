# API Documentation Coverage

Phase 3 of the `docs-accuracy-audit` pipeline. Ground truth established with
`scripts/audit/extract-routes.sh` (resolves the real Axum router composition tree — nest/merge/
route — into full paths), diffed against `docs/api/openapi.yaml`'s `paths:` keys.

## Headline number

**136 / 136 real REST routes documented — 100% coverage.**

```console
$ bash scripts/audit/extract-routes.sh | wc -l
137
```

137 is the raw route count including the MCP mount; 136 are REST endpoints and 1 is not (see
Deliberate exclusions below). All 136 REST routes have a corresponding entry in
`docs/api/openapi.yaml`, verified by a direct set comparison between `extract-routes.sh`'s
output (with the `/api/v1` server prefix stripped) and `yaml.safe_load(...)['paths'].keys()`:
zero missing, zero extra.

Starting point for this phase (per `docs/audit/drift-report.md`, phase 0): 12 documented paths
against ~124 raw route fragments — most of the HTTP surface was undocumented. This phase closed
that gap in two passes:

- 8 pre-existing `openapi.yaml` entries were wrong (camelCase/snake_case mismatches, one
  hallucinated path) and were fixed rather than left in place.
- 124 previously-undocumented routes were added across 13 domains: AI (22), Emails +
  Attachments (25), Auth (10), Cleanup (10), Consent + Evaluation (16), Clustering (6), Rules
  (6), Interactions (4), Learning (4), Wipe (5), Backup (3), Unsubscribe (3), Vectors remainder
  (3), Ingestion remainder (8), Insights remainder (2).

## Deliberate exclusions

| Route         | Reason                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/api/v1/mcp` | Not a REST endpoint — it's the Model Context Protocol Streamable HTTP transport mount (JSON-RPC over HTTP, `rmcp` crate). Documented separately in `docs/api/mcp-tools.md`, which covers the 15 tools it exposes, request schemas, and transports (this HTTP mount plus stdio via `--mcp-stdio` / `EMAILIBRIUM_MCP_MODE=stdio`). OpenAPI describes REST resources, not a JSON-RPC tool-call surface, so modeling it as an OpenAPI path would misrepresent the protocol. |

No other route is excluded — every other route `extract-routes.sh` finds has an
`openapi.yaml` entry.

## MCP tool surface

Documented separately (`docs/api/mcp-tools.md`) since it isn't OpenAPI-shaped: 15 read-only
tools (7 pre-existing + 8 from the A3 batch), sourced directly from
`backend/src/tools/mod.rs::declarations()` — the single registry both the MCP transport and the
in-process chat orchestrator dispatch through. 3 further tools (`send_email`, `delete_email`,
`create_rule`) are recorded in `config/tools.yaml` as `deferred: true` — intended policy only,
not implemented, not callable.

## Request/response shapes verified against handler code

Per the phase's Definition of Done, at least 8 documented endpoints had their shape checked
directly against the handler, not inferred from the route name. This phase verified
substantially more than 8 while building out the new domains; a representative sample with
citations:

1. **`GET /clustering/clusters`** — `ClusterListResponse`/`ClusterSummary` fields (camelCase,
   nested `topTerms`/`representativeEmails`) verified against
   `backend/src/api/clustering.rs:99` (`list_clusters`) and its response structs at lines 33-68.
2. **`POST /clustering/clusters/{id}/unpin`** — verified the handler does NOT actually clear the
   pinned flag (the underlying `ClusterEngine::unpin_cluster` hasn't landed, per the handler's
   own comment) and documented that limitation explicitly rather than describing it as a working
   unpin — `backend/src/api/clustering.rs:292` (`unpin_cluster`).
3. **`POST /rules`** — `CreateRuleRequest`'s `RuleCondition`/`RuleAction` tagged-union shapes
   verified against `backend/src/rules/types.rs` (`#[serde(tag = "type", rename_all =
"camelCase")]`, real `MatchOperator` values: contains/equals/startsWith/endsWith/regex/
   greaterThan/lessThan — no `notContains`) and the handler at `backend/src/api/rules.rs:210`
   (`create_rule`).
4. **`POST /rules/{id}/run`** — `executed_count` semantics (can be less than `match_count` ×
   actions when a provider call fails; failures are logged, not fatal) verified against
   `backend/src/api/rules.rs:582` (`run_rule`).
5. **`POST /learning/feedback`** — `FeedbackActionRequest`'s internal tagging confirmed
   snake_case (`type`, `reclassify`/`move_to_group`/`star`/`reply`/`archive`/`delete`), distinct
   from the rules module's camelCase tagging on the same `type`-tag pattern; also verified a
   `reclassify` action persists the new category to the `emails` table immediately, ahead of and
   independent from the learning engine's own update — `backend/src/api/learning.rs:83`
   (`submit_feedback`).
6. **`POST /wipe/all`** — confirmed the confirmation-token gate is a literal string match
   (`"CONFIRM_FULL_WIPE"`), not a generated/rotating token — `backend/src/api/wipe.rs:131`
   (`wipe_all`).
7. **`GET /backup/stats`** — confirmed all three fields are computed live from the
   `vector_backups` table (`COUNT(*)`, `MAX(updated_at)`, `SUM(LENGTH(vector_data))`), not
   cached counters — `backend/src/api/backup.rs:68` (`backup_stats`).
8. **`POST /unsubscribe`** — confirmed `BatchUnsubscribeResponse` uses `#[serde(flatten)]` on its
   `result: BatchResult` field, so the response body is `BatchResult`'s fields directly at the
   top level (no wrapping `"result"` key) — documented as such rather than as a nested object —
   `backend/src/api/unsubscribe.rs:79` (`batch_unsubscribe`).
9. **`POST /vectors/search/hybrid`** — confirmed it reuses the exact same
   `SearchResponse`/`SearchResultItem` schema as `GET /vectors/search` rather than a distinct
   shape, and that `mode` falls back to `"hybrid"` for any value other than `"semantic"` or
   `"keyword"` (including omission) — `backend/src/api/vectors.rs:278` (`hybrid_search`).
10. **`GET /vectors/quantization`** — verified all six response fields
    (`current_tier`/`recommended_tier`/`vector_count`/`compression_ratio`/
    `estimated_memory_bytes`/`estimated_memory_uncompressed_bytes`) against
    `QuantizationStatusResponse` and confirmed no `rename_all` (snake_case wire format) —
    `backend/src/api/vectors.rs:492` (`quantization_status`).
11. **`GET /insights/temporal`** — verified `TemporalInsights` is camelCase while its four
    element types (`DailyCount`, `DayOfWeekCount`, `HourOfDayCount`) are snake_case-equivalent
    (no `rename_all`, single-word fields) except `CategoryDailyCount`, which is separately
    marked camelCase — a within-file rename_all inconsistency documented per-schema rather than
    assumed uniform — `backend/src/api/insights.rs:137` (`temporal_insights`).
12. **`POST /cleanup/plan`** (pre-existing entry, re-verified this phase) — confirmed the handler
    genuinely persists the built plan via `cleanup_plan_repo.save(&plan)` with a 30-minute
    `valid_until` TTL, correcting an earlier draft assumption that this endpoint was dry-run-only
    (that description applies to the _MCP tool_ `preview_cleanup_plan`, a separate code path) —
    `backend/src/cleanup/api/plan.rs:140` (`create_plan`).

## Known gap carried to a later phase

`docs/ADRs/ADR-028-mcp-tool-calling-chat.md` references a stale MCP-tool-to-REST-endpoint
mapping table (flagged in `docs/audit/drift-report.md`, phase 0, under the API persona). Fixing
that table is out of scope here — phase 3's `touches:` list covers `docs/api/**` only,
`docs/ADRs/**` belongs to phase 5 (Decision reader persona). This phase's `docs/api/mcp-tools.md`
is the accurate, current source; ADR-028's table should be reconciled against it when phase 5
runs.
