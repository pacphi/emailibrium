# Local-model assessment — 2026-09-22

Assessment target: Apple M4 Max, 128 GiB unified memory; 150,000 messages in under 10 hours. No model was downloaded and no inference throughput or email accuracy was measured. Listed model sizes are registry download sizes, not measured working sets. Recommendations below are evaluation priorities, not benchmark winners.

## Recommendation

Benchmark current Qwen3.8 27B as the primary quality challenger, alongside Gemma 4 12B and Qwen3.6 35B-A3B. Retain Qwen3.5 4B and 9B as compact speed controls for schema-constrained, non-thinking bulk classification; their role is a measured baseline, not an assertion that older models are best. Select the smallest candidate that meets held-out quality thresholds. Include Qwen3.8-Flash-Next as an experimental upper-quality challenger with explicit memory limits; do not silently make a 105 GB preview artifact the bulk-processing default.

| Candidate                         | Registry size | Intended evaluation role               | Official source                                                                  |
| --------------------------------- | ------------: | -------------------------------------- | -------------------------------------------------------------------------------- |
| qwen3.5:4b                        |        3.4 GB | Fast baseline for short classification | [Ollama Qwen3.5](https://ollama.com/library/qwen3.5)                             |
| qwen3.5:9b                        |        6.6 GB | Quality/throughput balance candidate   | [Ollama Qwen3.5](https://ollama.com/library/qwen3.5)                             |
| gemma4:12b                        |        7.6 GB | Independent-family comparator          | https://ollama.com/library/gemma4                                                |
| qwen3.6:35b (35B total/3B active) |         23 GB | Sparse model challenger                | https://ollama.com/library/qwen3.6 ; https://huggingface.co/Qwen/Qwen3.6-35B-A3B |
| qwen3.8:27b                       |         18 GB | Ambiguous cases and local chat         | https://ollama.com/library/qwen3.8                                               |
| qwen3.8-flash-next:125b-mlx       |        105 GB | Experimental upper-quality challenger  | https://ollama.com/library/qwen3.8-flash-next/tags                               |

Qwen3.8-Flash-Next is an experimental architecture preview: 125B parameters, 6B active per token, plus a 51B n-gram embedding table (the original card also lists 4B MTP). It uses Qwen Community License 1.0; do not describe it as Apache-2.0. Thinking is on by default. The official model card describes coding, agentic, vision, and general benchmarks, none establishing accuracy for this user's email categories. Sources: https://huggingface.co/Qwen/Qwen3.8-Flash-Next ; https://ollama.com/library/qwen3.8-flash-next .

The Flash registry also lists Q4_K_M at 120 GB, Q8_0 at 189 GB, and BF16 at 355 GB. 128 GiB is approximately 137.4 decimal GB: subtracting only the 105 GB file leaves approximately 30.2 GiB; subtracting 120 GB leaves approximately 16.2 GiB. These are arithmetic upper bounds before OS, database, app, activations, KV cache, runtime scratch buffers, other models, and concurrent requests—not proof that either variant runs comfortably. The Q8/BF16 artifacts exceed physical memory outright. Active parameter count describes computation, not the resident weight set. https://ollama.com/library/qwen3.8-flash-next/tags .

Qwen3.5 needs the runtime's explicit non-thinking control; its official card says the older /think and /nothink text switches are not supported. https://huggingface.co/Qwen/Qwen3.5-4B . Qwen3.6-35B-A3B is Apache-2.0 according to its original model card. https://huggingface.co/Qwen/Qwen3.6-35B-A3B . Gemma 4 12B is a current 11.95B dense model, while E4B is 4.5B effective / 8B with embeddings; the effective count must not be used as a weight-memory estimate. https://ai.google.dev/gemma/docs/core/model_card_4 .

## Throughput acceptance calculation

150,000 / 36,000 seconds = 4.167 completed messages/second end to end, including fetching, embedding, database writes, model work, retries and backfill. A fully serial whole-pipeline average budget would be 240 ms/message. If fraction f requires the LLM, the LLM subsystem must sustain at least 4.167\*f classifications/second while sharing resources with the other stages. A 10-item batch at 100% LLM usage allows 2.4 seconds/batch on a single effective lane; HTTP concurrency is not evidence of corresponding GPU throughput.

At illustrative 256/512/1024 input tokens per message, an all-LLM pipeline needs 1067/2133/4267 aggregate prompt tokens/sec before output and overhead. At 10/30/60 output tokens per message, it needs 42/125/250 output tokens/sec. These are workload requirements, not estimates of any model's performance. Long reasoning can erase the benefit of short label outputs, so the benchmark must count reasoning tokens too.

## Evaluation matrix to execute before selection

1. Freeze model digest, quantization, runtime version, prompt hash, schema version, context limit, think flag, sampling settings, and preprocessing. Never evaluate only a mutable latest tag. Use identical held-out messages across candidates.
2. Label a stratified pilot (e.g. 1,000–2,000 messages) across accounts, categories, languages, long/short messages, duplicates, attachments, marketing versus transactional messages, and adversarial email instructions. Split by sender/domain/thread and time to avoid duplicates or near-identical campaigns leaking between tuning and validation.
3. Measure macro-F1, per-category precision/recall, confusion matrix, abstention rate, schema validity, ID/count alignment, retries and truncations. Separately measure precision for any eventual archive/delete suggestions; don't infer destructive-action safety from average accuracy. Calibrate routing from observed errors; a model's self-reported confidence is not a calibrated probability.
4. For each model measure think=false (and a small think=true challenger arm only for difficult cases), context 2K/4K/8K as required by actual input, batch 1/4/10 and concurrency 1/2/4. Stop increasing concurrency if memory pressure, latency or error rates worsen. Benchmark MLX separately from GGUF; they are distinct runtime/artifact experiments.
5. Record cold load, warm prompt evaluation speed, output speed, p50/p95 latency, sustained completed emails/sec, first-pass valid output rate, peak memory, swap, retry cost, queue wait and energy/thermal stability. Record model-generated reasoning separately.
6. Perform a 10k-message sustained pilot including all ingestion stages, restart/recovery and failure injection. Extrapolate with uncertainty, then accept the 150k target only after an end-to-end run. A small-model cascade is preferred if it meets quality thresholds; the threshold itself requires the user's risk tolerance or an existing product requirement.

## Privacy and structured output setup

Enforce inference locality on the actual endpoint and artifact, not the provider display name. Ollama supports cloud models even behind a local server; use its documented local-only setting (OLLAMA_NO_CLOUD=1 or disable_ollama_cloud=true), and disallow remote endpoints/cloud tags in the app's private mode. https://docs.ollama.com/faq . The local Ollama API accepts a JSON schema in format; client-side validation is still necessary for exact IDs/counts and permitted categories. Its docs currently say Ollama Cloud does not support structured outputs. https://docs.ollama.com/capabilities/structured-outputs .

## Not tested

No model download, local LLM run, new runtime installation, Apple GPU benchmark, task-specific accuracy measurement, total ingestion timing, digest verification of installed models, runtime compatibility test for the experimental Flash architecture, or live outbound-network privacy test occurred in this research subtask. A focused loopback-only regression test for the app's Ollama routing is being handled separately and cannot establish any of those model claims.
