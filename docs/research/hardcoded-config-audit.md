# Hardcoded Configuration Audit

- **Date**: 2026-03-27
- **Scope**: Full codebase scan for config that should be externalized to YAML
- **Items Found**: 120+

## Summary by Category

| Category                   | Count         | Target YAML File                                         |
| -------------------------- | ------------- | -------------------------------------------------------- |
| System Prompts             | 3 (Rust + TS) | `config/prompts.yaml`                                    |
| Model Names & Catalogs     | 30+           | `config/models-llm.yaml`, `config/models-embedding.yaml` |
| Tuning Parameters          | 25+           | `config/tuning.yaml`                                     |
| Provider Config            | 10+           | `config/providers.yaml`                                  |
| Behavioral / Feature Flags | 15+           | `config/app.yaml`                                        |
| Classification Rules       | 2 instances   | `config/classification.yaml`                             |
| UI Descriptions            | 25+           | Driven from model/provider YAML `description` fields     |

## Key Findings

1. **System prompts** are hardcoded in `chat.rs` (Rust) and `built-in-llm-adapter.ts` (Node)
2. **Model catalogs** are duplicated across `model_catalog.rs`, `model-manifest.ts`, `AISettings.tsx`, and `generative_builtin.rs`
3. **Tuning parameters** (temperature, max_tokens, timeouts) are scattered across 6+ files with some duplicated
4. **Classification keywords** are duplicated between `generative-router.ts` and `error-recovery.ts`
5. **UI descriptions** for models/providers are hardcoded in `AISettings.tsx` — should come from the YAML `description` field

See full audit data in the task output for file:line details.
