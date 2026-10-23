# Model integration findings at develop 881868f7c57ef98552612a6cf15edde15cdd43aa

Read-only audit snapshot: /private/tmp/emailibrium-audit-20260922. Line references below belong to that immutable base, not the later routing fix. Base links can be formed as https://github.com/pacphi/emailibrium/blob/881868f7c57ef98552612a6cf15edde15cdd43aa/<path>#L<line> . Findings were established by source tracing unless explicitly identified as a runtime test.

## P1 — Consent revocation does not reliably prevent cloud inference

Critical deferred architectural work. `has_consent` has no production call sites outside its definition (only consent unit tests call it). Startup creates the configured cloud generator and registers it without a persisted-consent check (`backend/src/vectors/mod.rs:405`, `:553`); router registration always enables it (`generative_router.rs:84`). Thus restart can re-enable a previously revoked provider. More directly, ingestion retains the raw generator (`mod.rs:516`; `ingestion.rs:351`, `:400`, `:1145`) and passes it to categorizer (`categorizer.rs:277`, `:384`), bypassing router enable/disable. API revocation only disables the router (`backend/src/api/consent.rs:188`). Existing jobs retain clones. Generic cloud is registered as OpenAi regardless of underlying Anthropic/Gemini configuration (`mod.rs:557`), and cloud_ai revocation omits OpenRouter (`api/consent.rs:248`). Router failover itself only filters enabled/available providers (`generative_router.rs:145`, `:188`), with no per-request local-only policy. No claim is made that user mail was actually transmitted during this audit.

Required follow-up: single request-time policy gate covering generative chat, classification, batch, background jobs, embeddings, tool-capable providers and failover; restore persisted consent on startup; map actual provider identities; local-only endpoint/model enforcement; policy changes must affect already-created jobs. Verify with recording local servers and negative outbound assertions across restart/revoke/failover cases. Do not represent current private defaults as an enforced privacy boundary.

## P1 — Ollama classification silently uses chat_model

`backend/src/vectors/generative.rs:260` always selects `chat_model` inside the shared generator, called by single classification at `:320` and batched classification at `:347`. `model_name()` reports `classification_model` at `:369`, so classification provenance is misleading. Changing classification_model alone changes the reported name but not requests. This can load a much larger model during bulk classification and invalidate throughput/quality comparisons. Targeted fix authorized in separate worktree, only this file and focused loopback tests. Also note chat logs still lack operation-specific model provenance; that broader trait design is outside the bounded fix.

## P1/P2 — Built-in Gemma 4 uses Gemma 3 template tokens

Catalog entries use chat_template: gemma for Gemma 3 and Gemma 4. `backend/src/vectors/generative_builtin.rs:217` maps both to the same formatter; `:402-438` formats `<start_of_turn>` / `<end_of_turn>`, uses role `model`, and moves system text into the first user turn. Google's Gemma 4 specification instead uses `<|turn>` / `<turn|>` and supports an explicit system turn (the raw model-turn role is still `model`; `assistant` is the external chat API role): https://ai.google.dev/gemma/docs/core/prompt-formatting-gemma4 . This is a concrete compatibility mismatch, not proof of a specific accuracy decline without inference. Prefer runtime-supplied authoritative chat templates; add per-family golden formatting tests and a real cached-artifact smoke test before advertising Gemma 4 built-in support. The Ollama path delegates templating to Ollama and is a separate integration.

## P2 — Classification output is unconstrained, batch alignment is fragile

Ollama request fields (`generative.rs:230-243`) have no schema format, think, num_ctx, timeout or response duration/count metadata. The shared client is constructed without an app-level inference timeout (`:219`). Individual classification strictly validates text (`:1054`) but batches split nonempty lines and accept the first category name found as a substring (`:994-1050`), ignoring extra trailing lines and using position rather than message IDs. For example a negated multi-category line can be accepted as the first category; an extra leading line containing a category can shift later results. Failed batch positions trigger serial individual calls (`:350-364`), increasing unpredictable work. Built-in single classification also permits substring matches (`generative_builtin.rs:725-729`). `config/tuning.yaml:18` allows 200 output tokens/email; batching 10 allows 2,000 tokens although the intended result is a short list. Thinking defaults in current Qwen models can consume that budget before an answer.

Required follow-up: explicit non-thinking setting for supported models, schema-constrained category enum with immutable email IDs, strict expected-ID/count/duplicate/extraneous-field validation, bounded retry and timeout, explicit context/token budget and metrics. Test invalid JSON, unexpected IDs, duplicates, reordering, extra/missing results, reasoning-only output, token truncation, malicious mail instructions and retry exhaustion.

## P2 — Model integrity is not publisher-pinned

Built-in ModelSpec contains only repo_id/filename (`generative_builtin.rs:34-36`); any matching app-cache filename is accepted (`:81-84`); Hub downloads select the default repository revision without an explicit commit/checksum (`:97-103`). Embedding manifest default SHA256 values are empty (`model_integrity.rs:48-73`), and the downloader records a computed hash after download (`model_download.rs:155-160`) rather than comparing to an independently trusted publisher pin. This can detect a later change against that local baseline but does not prove initial artifact provenance. Frontend model manifest declares SHA256 while model-downloader.ts builds a mutable hf URI (`:91`) and downloads (`:126`) without checking that field. Verify downloaded artifacts against approved full digests/revisions before loading; store license, tokenizer/template and runtime compatibility with the pin.

## P2 — Built-in feature is outside ordinary backend CI coverage

`backend/Cargo.toml:16-19` sets default features to vectors; builtin-llm is opt-in. `.github/workflows/ci.yml:98` archives tests without enabling it; benches/doctests likewise use default features (`:141`, `:147`); release test (`release.yml:86`) also omits it. The module is feature-gated and `generative_builtin.rs` has no local test module. Current default-feature test success cannot validate llama.cpp compilation, model resolution, prompt templates, load/unload, Metal inference or new catalog architecture compatibility. Parent owns any actual optional-feature build attempt; none executed by research subtask.

## P2 — Model memory recommendations are static estimates

`model_catalog.rs:150-158` subtracts a fixed OS overhead from total RAM; it does not measure current available memory, model working-set, concurrent context/KV allocations or memory pressure. `config/tuning.yaml:57-58` requests batch size 10 / concurrency 8. New 105–120 GB Flash artifacts must not be enabled simply because a static catalog RAM number is below 128 GiB. Even Gemma E4B effective parameter naming differs from total weights. Use workload-aware capacity checks and measured safe concurrency.

## Existing evidence limits

The file named `backend/tests/classification_evaluation.rs` begins with metrics arithmetic tests on synthetic label pairs (`:12-92`); its existence does not establish real-model accuracy. This audit identified risk and corrective tests; it did not run a private-mail benchmark or measure any model's M4 Max performance.

## P2 — Ollama availability does not establish model readiness

`generative.rs:372-375` treats any successfully transported response from /api/tags as available; it does not require an HTTP success status, deserialize the list or check the selected classification/chat model. A reachable Ollama instance missing the configured artifact (or returning an HTTP error) therefore appears available. Readiness should separately identify runtime reachability, supported architecture and exact installed/loaded model digests, with bounded timeouts and an explicit model-not-installed state. This finding is outside the routing-only patch.
