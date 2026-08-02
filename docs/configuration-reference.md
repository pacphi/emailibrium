# Configuration Reference

## Overview

Emailibrium uses a layered configuration system via [figment](https://docs.rs/figment).

**Loading order** (later overrides earlier):

1. `config.yaml` -- base defaults (in the backend working directory)
2. `config.local.yaml` -- local overrides (gitignored)
3. `EMAILIBRIUM_*` environment variables -- runtime overrides

> **Note:** Figment itself only merges `config.yaml` -> `config.local.yaml` -> env vars — it
> doesn't natively load `config.{APP_ENV}.yaml` files. Docker Compose achieves environment-specific
> config a different way: it bind-mounts `config/environments/config.${APP_ENV}.yaml` (see
> [Deployment Guide](deployment-guide.md#docker-configuration-files)) directly over `/app/config.yaml`
> inside the container, so the "base defaults" file itself changes per environment rather than
> being layered by Figment. Docker secrets (`/run/secrets/*`) are resolved into env vars by
> `backend/entrypoint.sh` before the backend starts (see the entrypoint script for exactly which
> names it exports). Outside Docker, use `config.local.yaml` or environment variables directly.

## Environment Variables

All config keys can be overridden via env vars prefixed with `EMAILIBRIUM_` using `_` as the nested-key separator:

```bash
EMAILIBRIUM_PORT=9090
EMAILIBRIUM_HOST=0.0.0.0
EMAILIBRIUM_DATABASE_URL="sqlite:custom.db?mode=rwc"
EMAILIBRIUM_EMBEDDING_PROVIDER=ollama
EMAILIBRIUM_EMBEDDING_CACHE_SIZE=50000
EMAILIBRIUM_ENCRYPTION_ENABLED=true
EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD=mysecretpassword
EMAILIBRIUM_BACKUP_ENABLED=true
EMAILIBRIUM_BACKUP_INTERVAL_SECS=1800
EMAILIBRIUM_LEARNING_SONA_ENABLED=true
EMAILIBRIUM_QUANTIZATION_MODE=scalar
```

One incidental exception: the backend also reads the OS-standard `HOME` variable directly
(`backend/src/vectors/model_integrity.rs`) as a fallback base path for the model cache directory
when `generative.builtin.cache_dir` isn't set. It isn't an Emailibrium-specific config knob and
has no `EMAILIBRIUM_` equivalent.

## Complete Key Reference

### Top-Level

| Key            | Type   | Default                          | Env Override               | Description                                                                                                                                     |
| -------------- | ------ | -------------------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `host`         | String | `127.0.0.1`                      | `EMAILIBRIUM_HOST`         | Server bind address                                                                                                                             |
| `port`         | u16    | `8080`                           | `EMAILIBRIUM_PORT`         | Server listen port                                                                                                                              |
| `database_url` | String | `sqlite:emailibrium.db?mode=rwc` | `EMAILIBRIUM_DATABASE_URL` | SQLite connection URL -- `postgres://` is not yet supported, see [Deployment Guide](deployment-guide.md#database-strategy-sqlite-vs-postgresql) |

### Store (`store.*`)

| Key                              | Type   | Default                 | Env Override                                 | Description                                                          |
| -------------------------------- | ------ | ----------------------- | -------------------------------------------- | -------------------------------------------------------------------- |
| `store.backend`                  | String | `ruvector`              | `EMAILIBRIUM_STORE_BACKEND`                  | Vector store backend: `ruvector` \| `memory` \| `qdrant` \| `sqlite` |
| `store.path`                     | String | `data/vectors`          | `EMAILIBRIUM_STORE_PATH`                     | Path for vector data persistence                                     |
| `store.enabled`                  | bool   | `true`                  | `EMAILIBRIUM_STORE_ENABLED`                  | Whether the vector store is enabled                                  |
| `store.qdrant_url`               | String | `http://localhost:6334` | `EMAILIBRIUM_STORE_QDRANT_URL`               | Qdrant REST API endpoint (only when backend=qdrant)                  |
| `store.qdrant_collection_prefix` | String | `emailibrium`           | `EMAILIBRIUM_STORE_QDRANT_COLLECTION_PREFIX` | Qdrant collection name prefix                                        |
| `store.qdrant_api_key`           | String | _(none)_                | `EMAILIBRIUM_STORE_QDRANT_API_KEY`           | Qdrant API key (optional)                                            |

### Embedding (`embedding.*`)

| Key                          | Type   | Default                  | Env Override                             | Description                                                        |
| ---------------------------- | ------ | ------------------------ | ---------------------------------------- | ------------------------------------------------------------------ |
| `embedding.provider`         | String | `onnx`                   | `EMAILIBRIUM_EMBEDDING_PROVIDER`         | Embedding provider: `onnx`, `mock`, `ollama`, `cloud`, or `cohere` |
| `embedding.model`            | String | `all-MiniLM-L6-v2`       | `EMAILIBRIUM_EMBEDDING_MODEL`            | Model name for text embeddings                                     |
| `embedding.dimensions`       | usize  | `384`                    | `EMAILIBRIUM_EMBEDDING_DIMENSIONS`       | Embedding vector dimensions                                        |
| `embedding.batch_size`       | usize  | `64`                     | `EMAILIBRIUM_EMBEDDING_BATCH_SIZE`       | Batch size for bulk embedding operations                           |
| `embedding.cache_size`       | u64    | `10000`                  | `EMAILIBRIUM_EMBEDDING_CACHE_SIZE`       | Number of entries in the embedding cache                           |
| `embedding.ollama_url`       | String | `http://localhost:11434` | `EMAILIBRIUM_EMBEDDING_OLLAMA_URL`       | Ollama base URL (fallback provider)                                |
| `embedding.min_query_tokens` | usize  | `5`                      | `EMAILIBRIUM_EMBEDDING_MIN_QUERY_TOKENS` | Minimum token count before query augmentation kicks in             |

### Embedding / ONNX (`embedding.onnx.*`) -- ADR-011

The ONNX provider uses [fastembed](https://github.com/Anush008/fastembed-rs) to run sentence-transformer models entirely in-process via ONNX Runtime. The model is downloaded from Hugging Face Hub on first use and cached locally.

| Key                                     | Type             | Default            | Env Override                                        | Description                                                                        |
| --------------------------------------- | ---------------- | ------------------ | --------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `embedding.onnx.model`                  | String           | `all-MiniLM-L6-v2` | `EMAILIBRIUM_EMBEDDING_ONNX_MODEL`                  | Model name. Supported: `all-MiniLM-L6-v2`, `bge-small-en-v1.5`, `bge-base-en-v1.5` |
| `embedding.onnx.show_download_progress` | bool             | `true`             | `EMAILIBRIUM_EMBEDDING_ONNX_SHOW_DOWNLOAD_PROGRESS` | Show progress bar on first model download                                          |
| `embedding.onnx.dimensions`             | usize            | `384`              | `EMAILIBRIUM_EMBEDDING_ONNX_DIMENSIONS`             | Output embedding dimensions (must match the chosen model)                          |
| `embedding.onnx.cache_dir`              | Option\<String\> | `None`             | `EMAILIBRIUM_EMBEDDING_ONNX_CACHE_DIR`              | Local cache directory for downloaded model files. `None` uses fastembed default    |

### Embedding / Cloud (`embedding.cloud.*`)

Uses the OpenAI embeddings API (`text-embedding-3-small` by default).

| Key                           | Type   | Default                      | Env Override                              | Description                                    |
| ----------------------------- | ------ | ---------------------------- | ----------------------------------------- | ---------------------------------------------- |
| `embedding.cloud.api_key_env` | String | `EMAILIBRIUM_OPENAI_API_KEY` | `EMAILIBRIUM_EMBEDDING_CLOUD_API_KEY_ENV` | Name of the env var holding the OpenAI API key |
| `embedding.cloud.model`       | String | `text-embedding-3-small`     | `EMAILIBRIUM_EMBEDDING_CLOUD_MODEL`       | OpenAI embedding model                         |
| `embedding.cloud.base_url`    | String | `https://api.openai.com`     | `EMAILIBRIUM_EMBEDDING_CLOUD_BASE_URL`    | OpenAI API base URL                            |
| `embedding.cloud.dimensions`  | usize  | `1536`                       | `EMAILIBRIUM_EMBEDDING_CLOUD_DIMENSIONS`  | Output embedding dimensions                    |

### Embedding / Cohere (`embedding.cohere.*`)

Uses the Cohere embed API v2 (`embed-english-v3.0` by default).

| Key                            | Type   | Default                      | Env Override                               | Description                                    |
| ------------------------------ | ------ | ---------------------------- | ------------------------------------------ | ---------------------------------------------- |
| `embedding.cohere.api_key_env` | String | `EMAILIBRIUM_COHERE_API_KEY` | `EMAILIBRIUM_EMBEDDING_COHERE_API_KEY_ENV` | Name of the env var holding the Cohere API key |
| `embedding.cohere.model`       | String | `embed-english-v3.0`         | `EMAILIBRIUM_EMBEDDING_COHERE_MODEL`       | Cohere embedding model                         |
| `embedding.cohere.base_url`    | String | `https://api.cohere.com`     | `EMAILIBRIUM_EMBEDDING_COHERE_BASE_URL`    | Cohere API base URL                            |
| `embedding.cohere.dimensions`  | usize  | `1024`                       | `EMAILIBRIUM_EMBEDDING_COHERE_DIMENSIONS`  | Output embedding dimensions                    |
| `embedding.cohere.input_type`  | String | `search_document`            | `EMAILIBRIUM_EMBEDDING_COHERE_INPUT_TYPE`  | Cohere input type hint                         |

### Index (`index.*`) -- HNSW Parameters

| Key                     | Type  | Default | Env Override                        | Description                                                                  |
| ----------------------- | ----- | ------- | ----------------------------------- | ---------------------------------------------------------------------------- |
| `index.m`               | usize | `16`    | `EMAILIBRIUM_INDEX_M`               | HNSW M parameter (connections per node). Higher = better recall, more memory |
| `index.ef_construction` | usize | `200`   | `EMAILIBRIUM_INDEX_EF_CONSTRUCTION` | HNSW build quality. Higher = slower build, better index quality              |
| `index.ef_search`       | usize | `100`   | `EMAILIBRIUM_INDEX_EF_SEARCH`       | HNSW search quality. Higher = slower search, better recall                   |

### Search (`search.*`)

| Key                           | Type  | Default | Env Override                              | Description                                     |
| ----------------------------- | ----- | ------- | ----------------------------------------- | ----------------------------------------------- |
| `search.default_limit`        | usize | `20`    | `EMAILIBRIUM_SEARCH_DEFAULT_LIMIT`        | Default number of results returned              |
| `search.max_limit`            | usize | `100`   | `EMAILIBRIUM_SEARCH_MAX_LIMIT`            | Maximum number of results a client can request  |
| `search.similarity_threshold` | f32   | `0.5`   | `EMAILIBRIUM_SEARCH_SIMILARITY_THRESHOLD` | Minimum cosine similarity to include in results |

### Encryption (`encryption.*`) -- ADR-008

| Key                          | Type             | Default | Env Override                             | Description                                                                                      |
| ---------------------------- | ---------------- | ------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `encryption.enabled`         | bool             | `false` | `EMAILIBRIUM_ENCRYPTION_ENABLED`         | Whether encryption at rest is enabled                                                            |
| `encryption.master_password` | Option\<String\> | `None`  | `EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD` | Master password for key derivation. **Never set in config files; use env var or Docker secret.** |

### Categorizer (`categorizer.*`) -- ADR-004

| Key                                | Type | Default | Env Override                                   | Description                                                                          |
| ---------------------------------- | ---- | ------- | ---------------------------------------------- | ------------------------------------------------------------------------------------ |
| `categorizer.confidence_threshold` | f32  | `0.7`   | `EMAILIBRIUM_CATEGORIZER_CONFIDENCE_THRESHOLD` | Minimum confidence for vector centroid classification. Below this, falls back to LLM |
| `categorizer.max_centroid_shift`   | f32  | `0.1`   | `EMAILIBRIUM_CATEGORIZER_MAX_CENTROID_SHIFT`   | Maximum centroid shift per feedback event                                            |
| `categorizer.min_feedback_events`  | u32  | `10`    | `EMAILIBRIUM_CATEGORIZER_MIN_FEEDBACK_EVENTS`  | Minimum feedback events before centroid updates activate                             |

### Backup (`backup.*`) -- ADR-003

| Key                    | Type | Default | Env Override                       | Description                                  |
| ---------------------- | ---- | ------- | ---------------------------------- | -------------------------------------------- |
| `backup.enabled`       | bool | `false` | `EMAILIBRIUM_BACKUP_ENABLED`       | Whether automatic SQLite backup is enabled   |
| `backup.interval_secs` | u64  | `3600`  | `EMAILIBRIUM_BACKUP_INTERVAL_SECS` | Backup interval in seconds (default: 1 hour) |

### Clustering (`clustering.*`) -- ADR-009

| Key                             | Type  | Default | Env Override                                | Description                                               |
| ------------------------------- | ----- | ------- | ------------------------------------------- | --------------------------------------------------------- |
| `clustering.min_cluster_size`   | usize | `5`     | `EMAILIBRIUM_CLUSTERING_MIN_CLUSTER_SIZE`   | Minimum number of emails to form a cluster                |
| `clustering.merge_threshold`    | f32   | `0.85`  | `EMAILIBRIUM_CLUSTERING_MERGE_THRESHOLD`    | Centroid similarity above which two clusters are merged   |
| `clustering.hysteresis_delta`   | f32   | `0.05`  | `EMAILIBRIUM_CLUSTERING_HYSTERESIS_DELTA`   | Minimum improvement to reassign an email to a new cluster |
| `clustering.min_stability_runs` | u32   | `3`     | `EMAILIBRIUM_CLUSTERING_MIN_STABILITY_RUNS` | Consecutive stable runs before a cluster is visible       |
| `clustering.max_clusters`       | usize | `50`    | `EMAILIBRIUM_CLUSTERING_MAX_CLUSTERS`       | Maximum number of clusters to discover                    |
| `clustering.neighbor_count`     | usize | `20`    | `EMAILIBRIUM_CLUSTERING_NEIGHBOR_COUNT`     | Number of nearest neighbors for the similarity graph      |

### Learning / SONA (`learning.*`) -- ADR-004

| Key                                 | Type  | Default | Env Override                                    | Description                                                                                |
| ----------------------------------- | ----- | ------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `learning.sona_enabled`             | bool  | `true`  | `EMAILIBRIUM_LEARNING_SONA_ENABLED`             | Master switch for the SONA learning engine                                                 |
| `learning.positive_learning_rate`   | f32   | `0.05`  | `EMAILIBRIUM_LEARNING_POSITIVE_LEARNING_RATE`   | Positive learning rate (alpha multiplier for positive feedback)                            |
| `learning.negative_learning_rate`   | f32   | `0.02`  | `EMAILIBRIUM_LEARNING_NEGATIVE_LEARNING_RATE`   | Negative learning rate (beta multiplier for negative feedback)                             |
| `learning.session_rerank_gamma`     | f32   | `0.15`  | `EMAILIBRIUM_LEARNING_SESSION_RERANK_GAMMA`     | Session re-ranking weight for Tier 2 learning                                              |
| `learning.max_centroid_shift`       | f32   | `0.1`   | `EMAILIBRIUM_LEARNING_MAX_CENTROID_SHIFT`       | Maximum centroid shift per feedback event                                                  |
| `learning.min_feedback_events`      | u32   | `10`    | `EMAILIBRIUM_LEARNING_MIN_FEEDBACK_EVENTS`      | Minimum feedback events before centroid updates activate (cold start)                      |
| `learning.low_confidence_threshold` | f32   | `0.6`   | `EMAILIBRIUM_LEARNING_LOW_CONFIDENCE_THRESHOLD` | Emails below this confidence are reclassified during hourly consolidation                  |
| `learning.ab_control_percentage`    | f32   | `0.10`  | `EMAILIBRIUM_LEARNING_AB_CONTROL_PERCENTAGE`    | Fraction of queries routed to the control group (no SONA). Set to 0 to disable A/B testing |
| `learning.drift_alarm_threshold`    | f32   | `0.20`  | `EMAILIBRIUM_LEARNING_DRIFT_ALARM_THRESHOLD`    | Drift alarm fires when any centroid drifts beyond this fraction                            |
| `learning.position_bias_threshold`  | f32   | `0.95`  | `EMAILIBRIUM_LEARNING_POSITION_BIAS_THRESHOLD`  | Position-bias alarm threshold (rank-1 click ratio)                                         |
| `learning.max_snapshots`            | usize | `30`    | `EMAILIBRIUM_LEARNING_MAX_SNAPSHOTS`            | Maximum number of daily snapshots to retain for rollback                                   |

### Quantization (`quantization.*`) -- ADR-007

| Key                               | Type   | Default   | Env Override                                  | Description                                                                     |
| --------------------------------- | ------ | --------- | --------------------------------------------- | ------------------------------------------------------------------------------- |
| `quantization.mode`               | String | `auto`    | `EMAILIBRIUM_QUANTIZATION_MODE`               | Quantization mode: `auto`, `none`, `scalar`, `product`, or `binary`             |
| `quantization.scalar_threshold`   | u64    | `50000`   | `EMAILIBRIUM_QUANTIZATION_SCALAR_THRESHOLD`   | Vector count threshold to activate scalar (int8) quantization (~4x compression) |
| `quantization.product_threshold`  | u64    | `200000`  | `EMAILIBRIUM_QUANTIZATION_PRODUCT_THRESHOLD`  | Vector count threshold to activate product quantization (~16x compression)      |
| `quantization.binary_threshold`   | u64    | `1000000` | `EMAILIBRIUM_QUANTIZATION_BINARY_THRESHOLD`   | Vector count threshold to activate binary quantization (~32x compression)       |
| `quantization.hysteresis_percent` | f32    | `0.10`    | `EMAILIBRIUM_QUANTIZATION_HYSTERESIS_PERCENT` | Hysteresis percentage to prevent thrashing near tier boundaries (0.10 = 10%)    |

## Generative AI (ADR-012, ADR-021)

Controls email classification and chat. Default provider is `builtin` — a small language model that runs locally with no external service.

| Key                                      | Type    | Default                       | Description                                                  |
| ---------------------------------------- | ------- | ----------------------------- | ------------------------------------------------------------ |
| `generative.provider`                    | string  | `"builtin"`                   | Provider: `builtin`, `none`, `ollama`, `cloud`, `openrouter` |
| `generative.builtin.model_id`            | string  | `"qwen3-1.7b-q4km"`           | GGUF model identifier                                        |
| `generative.builtin.context_size`        | integer | `2048`                        | Context window in tokens                                     |
| `generative.builtin.gpu_layers`          | integer | `99`                          | GPU layer offload (0=CPU, 99=all)                            |
| `generative.builtin.idle_timeout_secs`   | integer | `300`                         | Seconds before unloading idle model                          |
| `generative.builtin.cache_dir`           | string  | `~/.emailibrium/models/llm`   | Model cache directory                                        |
| `generative.ollama.base_url`             | string  | `"http://localhost:11434"`    | Ollama API URL                                               |
| `generative.ollama.classification_model` | string  | `"llama3.2:1b"`               | Model for classification                                     |
| `generative.ollama.chat_model`           | string  | `"llama3.2:3b"`               | Model for chat                                               |
| `generative.cloud.provider`              | string  | `"openai"`                    | Cloud provider: `openai`, `anthropic`, `gemini`              |
| `generative.cloud.model`                 | string  | `"gpt-4o-mini"`               | Cloud model identifier                                       |
| `generative.cloud.api_key_env`           | string  | `"EMAILIBRIUM_CLOUD_API_KEY"` | Environment variable for API key                             |

### Provider Tiers

| Tier    | Provider            | What It Does                                                             | Requirements                                          |
| ------- | ------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------- |
| **0**   | `none`              | Rule-based keyword heuristics only                                       | Nothing                                               |
| **0.5** | `builtin` (default) | Local LLM via llama.cpp + ONNX embeddings                                | ~1.1 GB model download (default model)                |
| **1**   | `ollama`            | Local Ollama server + ONNX embeddings                                    | Ollama installed and running                          |
| **2**   | `cloud`             | Cloud LLM API (OpenAI, Anthropic, or Gemini) + optional cloud embeddings | API key + consent                                     |
| **2**   | `openrouter`        | 300+ models via OpenRouter's OpenAI-compatible proxy                     | API key + consent (separate from `cloud` — see below) |

### Environment Variable Overrides

```bash
EMAILIBRIUM_GENERATIVE_PROVIDER=builtin   # or: none, ollama, cloud, openrouter
EMAILIBRIUM_GENERATIVE_BUILTIN_MODEL_ID=qwen3-1.7b-q4km
EMAILIBRIUM_GENERATIVE_BUILTIN_GPU_LAYERS=0  # CPU only
```

### Available Built-in Models

The full, current catalog lives in `config/models-llm.yaml` (edit that file to add/remove
models -- this table is a snapshot). Each model's `default_for_ram_mb` (if set) makes it the
auto-selected default once the machine's available RAM crosses that threshold;
`qwen3-1.7b-q4km` is the overall default (`generative.builtin.model_id`) below that.

| Model ID                            | Disk    | Min RAM | Quality   | RAM tier            |
| ----------------------------------- | ------- | ------- | --------- | ------------------- |
| `qwen3-1.7b-q4km`                   | 1.1 GB  | 1.5 GB  | Fair      | 8 GB (default)      |
| `qwen2.5-3b-q4km`                   | 2.0 GB  | 2.5 GB  | Good      | 8 GB                |
| `qwen3-4b-q4km`                     | 2.5 GB  | 4.0 GB  | Good      | 16 GB               |
| `gemma3-4b-q4km`                    | 2.5 GB  | 4.5 GB  | Good      | 16 GB               |
| `phi4-mini-q4km`                    | 2.4 GB  | 4.0 GB  | Good      | 16 GB               |
| `gemma4-e4b-q4km`                   | 2.8 GB  | 4.5 GB  | Excellent | 16 GB               |
| `nemotron3-nano-4b-q4km`            | 2.7 GB  | 5.0 GB  | Good      | 16 GB               |
| `qwen3-8b-q4km`                     | 5.0 GB  | 7.0 GB  | Excellent | 16 GB (default)     |
| `gemma3-12b-q4km`                   | 8.1 GB  | 11 GB   | Excellent | 32 GB               |
| `qwen3-14b-q4km`                    | 9.0 GB  | 12 GB   | Excellent | 32 GB (default)     |
| `mistral-small-24b-q4km`            | 14 GB   | 18 GB   | Excellent | 64-128 GB           |
| `gemma4-26b-a4b-q4km` (MoE)         | 16 GB   | 20 GB   | Excellent | 64-128 GB           |
| `nemotron3-nano-30b-a3b-q4km` (MoE) | 18 GB   | 20 GB   | Excellent | 64-128 GB           |
| `qwen3-30b-a3b-q4km` (MoE)          | 18.6 GB | 22 GB   | Excellent | 64-128 GB (default) |
| `qwen3-32b-q4km`                    | 19.8 GB | 24 GB   | Excellent | 64-128 GB           |
| `gemma4-31b-q4km`                   | 20 GB   | 24 GB   | Excellent | 64-128 GB           |

Also available via `generative.provider: ollama` (pull with `ollama pull <tag>`),
`generative.provider: cloud` (OpenAI, Anthropic, or Gemini -- set `generative.cloud.provider`
accordingly, API key required), or `generative.provider: openrouter` (300+ models via
OpenRouter's OpenAI-compatible proxy -- a **separate** top-level provider value, not a `cloud`
option; reuses `generative.cloud.model` as the model id and reads its API key/base
URL/headers from `config/models-llm.yaml`'s `providers.openrouter` catalog entry, falling back
to `config/app.yaml`'s `providers.openrouter.*`). See `config/models-llm.yaml` for the full
`ollama`/`openai`/`anthropic`/`openrouter` model catalogs.

### OAuth (`oauth.*`) -- DDD-005

OAuth client credentials are loaded from environment variables (never from config files) to prevent accidental secret exposure. The config controls which env vars to read and endpoint URLs.

| Key                               | Type          | Default                                                  | Env Override                          | Description                                                                                                                                           |
| --------------------------------- | ------------- | -------------------------------------------------------- | ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `oauth.redirect_base_url`         | String        | `http://localhost:8080`                                  | `EMAILIBRIUM_OAUTH_REDIRECT_BASE_URL` | Base URL for constructing OAuth redirect URIs                                                                                                         |
| `oauth.gmail.client_id_env`       | String        | `EMAILIBRIUM_GOOGLE_CLIENT_ID`                           | --                                    | Env var holding the Google OAuth Client ID                                                                                                            |
| `oauth.gmail.client_secret_env`   | String        | `EMAILIBRIUM_GOOGLE_CLIENT_SECRET`                       | --                                    | Env var holding the Google OAuth Client Secret                                                                                                        |
| `oauth.gmail.scopes`              | Vec\<String\> | `[gmail.modify, gmail.labels, userinfo.email]`           | --                                    | OAuth scopes requested from Google                                                                                                                    |
| `oauth.gmail.auth_url`            | String        | `https://accounts.google.com/o/oauth2/v2/auth`           | --                                    | Google authorization endpoint                                                                                                                         |
| `oauth.gmail.token_url`           | String        | `https://oauth2.googleapis.com/token`                    | --                                    | Google token endpoint                                                                                                                                 |
| `oauth.outlook.client_id_env`     | String        | `EMAILIBRIUM_MICROSOFT_CLIENT_ID`                        | --                                    | Env var holding the Microsoft Client ID                                                                                                               |
| `oauth.outlook.client_secret_env` | String        | `EMAILIBRIUM_MICROSOFT_CLIENT_SECRET`                    | --                                    | Env var holding the Microsoft Client Secret                                                                                                           |
| `oauth.outlook.tenant`            | String        | `common`                                                 | `EMAILIBRIUM_OAUTH_OUTLOOK_TENANT`    | Microsoft tenant ID (`common` for multi-tenant)                                                                                                       |
| `oauth.outlook.scopes`            | Vec\<String\> | `[Mail.ReadWrite, Mail.Send, offline_access, User.Read]` | --                                    | OAuth scopes requested from Microsoft                                                                                                                 |
| `oauth.frontend_url`              | String        | `http://localhost:3000`                                  | `EMAILIBRIUM_OAUTH_FRONTEND_URL`      | Frontend origin the backend redirects to after an OAuth connect/deny/success -- override this in production if the frontend isn't on `localhost:3000` |

### Redis (`redis.*`)

The backend operates without Redis (graceful degradation). When enabled, hot-path data is cached in Redis.

| Key                    | Type   | Default                  | Env Override                       | Description                              |
| ---------------------- | ------ | ------------------------ | ---------------------------------- | ---------------------------------------- |
| `redis.enabled`        | bool   | `false`                  | `EMAILIBRIUM_REDIS_ENABLED`        | Whether Redis caching is enabled         |
| `redis.url`            | String | `redis://127.0.0.1:6379` | `EMAILIBRIUM_REDIS_URL`            | Redis connection URL                     |
| `redis.cache_ttl_secs` | u64    | `3600`                   | `EMAILIBRIUM_REDIS_CACHE_TTL_SECS` | Default TTL for cached entries (seconds) |

### Security (`security.*`)

| Key                        | Type          | Default                                          | Env Override                           | Description                     |
| -------------------------- | ------------- | ------------------------------------------------ | -------------------------------------- | ------------------------------- |
| `security.allowed_origins` | Vec\<String\> | `[http://localhost:3000, http://localhost:5173]` | `EMAILIBRIUM_SECURITY_ALLOWED_ORIGINS` | CORS allowed origins            |
| `security.csp_enabled`     | bool          | `true`                                           | `EMAILIBRIUM_SECURITY_CSP_ENABLED`     | Whether CSP headers are emitted |

#### Rate limiting (`security.rate_limit.*`) -- R-05

| Key                                       | Type | Default | Env Override                                          | Description                              |
| ----------------------------------------- | ---- | ------- | ----------------------------------------------------- | ---------------------------------------- |
| `security.rate_limit.enabled`             | bool | `true`  | `EMAILIBRIUM_SECURITY_RATE_LIMIT_ENABLED`             | Whether rate limiting is enabled         |
| `security.rate_limit.requests_per_second` | u32  | `10`    | `EMAILIBRIUM_SECURITY_RATE_LIMIT_REQUESTS_PER_SECOND` | Sustained requests per second per IP     |
| `security.rate_limit.burst_size`          | u32  | `50`    | `EMAILIBRIUM_SECURITY_RATE_LIMIT_BURST_SIZE`          | Maximum burst size (initial token count) |

Separately, `backend/src/middleware/rate_limit.rs` also reads a set of **raw, non-`EMAILIBRIUM_`-prefixed**
env vars for its route-specific presets and Redis-backed distributed limiting -- these are not part
of the Figment chain above:

| Variable                        | Description                                                        |
| ------------------------------- | ------------------------------------------------------------------ |
| `RATE_LIMIT_PRESET`             | Named preset selecting a full set of route limits at once          |
| `RATE_LIMIT_AUTH_START`         | Override for the OAuth-start route's limit                         |
| `RATE_LIMIT_AUTH_CALLBACK`      | Override for the OAuth-callback route's limit                      |
| `RATE_LIMIT_SESSION_STATUS`     | Override for the session-status route's limit                      |
| `RATE_LIMIT_TOKEN_REFRESH`      | Override for the token-refresh route's limit                       |
| `RATE_LIMIT_REDIS_URL`          | Redis URL for distributed (multi-instance) rate limiting           |
| `RATE_LIMIT_ENABLE_REDIS`       | Whether to use Redis instead of in-memory limiting                 |
| `RATE_LIMIT_REDIS_FALLBACK`     | Whether to fall back to in-memory limiting if Redis is unreachable |
| `RATE_LIMIT_ENABLE_USER_LIMITS` | Whether to apply additional per-authenticated-user limits          |
| `RATE_LIMIT_USER_MULTIPLIER`    | Multiplier applied to the base limit for authenticated users       |

#### HSTS (`security.hsts.*`) -- R-05

| Key                                | Type | Default    | Env Override                                   | Description                                               |
| ---------------------------------- | ---- | ---------- | ---------------------------------------------- | --------------------------------------------------------- |
| `security.hsts.enabled`            | bool | `false`    | `EMAILIBRIUM_SECURITY_HSTS_ENABLED`            | Whether the `Strict-Transport-Security` header is emitted |
| `security.hsts.max_age_secs`       | u64  | `63072000` | `EMAILIBRIUM_SECURITY_HSTS_MAX_AGE_SECS`       | `max-age` directive in seconds (default: 2 years)         |
| `security.hsts.include_subdomains` | bool | `true`     | `EMAILIBRIUM_SECURITY_HSTS_INCLUDE_SUBDOMAINS` | Whether the `includeSubDomains` directive is added        |

`backend/src/middleware/security_headers.rs` also reads its own raw, non-`EMAILIBRIUM_`-prefixed
env vars (these take priority over the `security.hsts.*`/`security.csp_enabled` keys above when set):

| Variable                  | Description                                                         |
| ------------------------- | ------------------------------------------------------------------- |
| `HSTS_MAX_AGE`            | Overrides the HSTS `max-age` (seconds)                              |
| `HSTS_PRELOAD`            | Whether to add the `preload` directive                              |
| `CSP_REPORT_URI`          | CSP `report-uri` directive target                                   |
| `CSP_ALLOW_INLINE_STYLES` | Whether to allow `'unsafe-inline'` in the CSP `style-src` directive |
| `CSP_CONNECT_SRC_ORIGINS` | Additional origins allowed in the CSP `connect-src` directive       |

## Application Settings (`config/app.yaml`)

A **separate** config file from everything above -- general application settings, polling
intervals, frontend cache tuning, UI defaults, and provider metadata. Loaded directly via
`vectors::yaml_config::load_yaml_config`, **not** through Figment: no `config.local.yaml` layer,
no `EMAILIBRIUM_*` env-var overrides. To change a value, edit `config/app.yaml` itself (or the
per-environment copies in `config/environments/`) and restart the backend.

### Sync & Polling (`sync.*`)

| Key                                      | Default | Description                                            |
| ---------------------------------------- | ------- | ------------------------------------------------------ |
| `sync.poll_interval_secs`                | `15`    | Background email poll scheduler tick                   |
| `sync.default_sync_frequency_minutes`    | `5`     | User-configurable sync interval default                |
| `sync.sync_completion_stable_checks`     | `2`     | Number of stable count checks before marking sync done |
| `sync.sync_completion_check_interval_ms` | `3000`  | Interval between stability checks                      |
| `sync.max_sync_wait_polls`               | `120`   | Max polls before giving up (120 \* 3s = 6 min)         |

### Email (`email.*`)

| Key                                 | Default | Description                                    |
| ----------------------------------- | ------- | ---------------------------------------------- |
| `email.trash_retention_days`        | `30`    | Auto-purge trashed emails after N days         |
| `email.spam_retention_days`         | `30`    | Auto-purge spam emails after N days            |
| `email.label_repair_interval_hours` | `6`     | Re-resolve unresolved label IDs (0 = disabled) |

### Frontend Cache -- React Query (`cache.*`)

Tuning knobs for the frontend's TanStack Query cache. Rarely need changing; listed here for
completeness since they're real, operator-editable settings.

| Key                                             | Default | Description                                                                  |
| ----------------------------------------------- | ------- | ---------------------------------------------------------------------------- |
| `cache.default_stale_time_ms`                   | `30000` | Global default for React Query `staleTime`                                   |
| `cache.default_retry_count`                     | `1`     | Global default retry count                                                   |
| `cache.email_counts_stale_time_ms`              | `10000` | Stale time for email-count queries                                           |
| `cache.email_counts_refetch_interval_ms`        | `30000` | Refetch interval for email-count queries                                     |
| `cache.categories_stale_time_ms`                | `30000` | Stale time for category-list queries                                         |
| `cache.labels_stale_time_ms`                    | `30000` | Stale time for label-list queries                                            |
| `cache.chat_sessions_stale_time_ms`             | `30000` | Stale time for chat-session queries                                          |
| `cache.subscriptions_stale_time_ms`             | `60000` | Stale time for subscription-list queries                                     |
| `cache.ollama_models_stale_time_ms`             | `30000` | Stale time for the Ollama model-list query                                   |
| `cache.model_catalog_stale_time_ms`             | `60000` | Stale time for the model-catalog query                                       |
| `cache.clusters_stale_time_ms`                  | `10000` | Stale time for cluster-list queries (idle)                                   |
| `cache.clusters_refetch_interval_ms`            | `30000` | Refetch interval for cluster-list queries (idle)                             |
| `cache.clusters_active_stale_time_ms`           | `3000`  | Stale time for cluster-list queries during active ingestion                  |
| `cache.clusters_active_refetch_interval_ms`     | `5000`  | Refetch interval for cluster-list queries during active ingestion            |
| `cache.clustering_status_stale_time_ms`         | `5000`  | Stale time for the clustering-status query                                   |
| `cache.clustering_status_refetch_interval_ms`   | `10000` | Refetch interval for the clustering-status query                             |
| `cache.dashboard_accounts_refetch_interval_ms`  | `10000` | Refetch interval for the dashboard accounts widget                           |
| `cache.dashboard_embedding_refetch_interval_ms` | `10000` | Refetch interval for the dashboard embedding-status widget                   |
| `cache.embedding_active_refetch_interval_ms`    | `5000`  | Refetch interval during active re-embedding (must stay under the rate limit) |
| `cache.ingestion_active_refetch_interval_ms`    | `3000`  | Poll interval for email counts + stats during active ingestion               |
| `cache.ingestion_active_stale_time_ms`          | `2000`  | Stale time for queries during active ingestion                               |
| `cache.stats_refetch_interval_ms`               | `30000` | Vector-stats poll interval (idle)                                            |
| `cache.stats_active_refetch_interval_ms`        | `5000`  | Vector-stats poll interval (active ingestion)                                |

### Network Timeouts (`network.*`)

| Key                                      | Default  | Description                                        |
| ---------------------------------------- | -------- | -------------------------------------------------- |
| `network.ollama_fetch_timeout_ms`        | `3000`   | Timeout for the Ollama model-list fetch            |
| `network.model_catalog_fetch_timeout_ms` | `3000`   | Timeout for the model-catalog fetch                |
| `network.ingestion_start_timeout_ms`     | `300000` | 5 min -- sync fetches all emails from the provider |
| `network.recluster_timeout_ms`           | `300000` | 5 min -- GraphSAGE + KMeans is compute-heavy       |
| `network.reembed_timeout_ms`             | `60000`  | 1 min -- resets DB rows then triggers ingestion    |
| `network.model_switch_poll_interval_ms`  | `2000`   | Poll interval during a model download              |
| `network.model_switch_max_polls`         | `150`    | 150 \* 2s = 5 min max download wait                |

### UI Defaults (`defaults.*`)

Initial values for user-configurable settings; users can override them via the Settings page
(persisted to `localStorage`).

| Key                                  | Default       | Description                               |
| ------------------------------------ | ------------- | ----------------------------------------- |
| `defaults.theme`                     | `system`      | `light` \| `dark` \| `system`             |
| `defaults.sidebar_position`          | `left`        | `left` \| `right`                         |
| `defaults.font_size_px`              | `14`          | Base UI font size                         |
| `defaults.email_density`             | `comfortable` | `compact` \| `comfortable` \| `spacious`  |
| `defaults.data_retention_days`       | `90`          | GDPR: auto-delete local data after N days |
| `defaults.sona_learning_enabled`     | `true`        | SONA adaptive learning on/off             |
| `defaults.learning_rate_sensitivity` | `0.5`         | 0.0-1.0, how aggressively to adapt        |

### Providers (`providers.*`)

API key **environment variable names** and base URLs only -- the actual keys are never stored in
this file. These are a separate registry from `generative.cloud.*`/`embedding.cloud.*` in
`backend/config.yaml` above; both exist in the codebase today.

| Key                                                  | Default                               | Description                                                   |
| ---------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------- |
| `providers.ollama.base_url`                          | `http://localhost:11434`              | Ollama server URL                                             |
| `providers.openai.api_key_env`                       | `OPENAI_API_KEY`                      | Env var name for the OpenAI key (no `EMAILIBRIUM_` prefix)    |
| `providers.anthropic.api_key_env`                    | `ANTHROPIC_API_KEY`                   | Env var name for the Anthropic key (no `EMAILIBRIUM_` prefix) |
| `providers.openrouter.api_key_env`                   | `OPENROUTER_API_KEY`                  | Env var name for the OpenRouter key                           |
| `providers.openrouter.base_url`                      | `https://openrouter.ai/api/v1`        | OpenRouter API base URL                                       |
| `providers.openrouter.required_headers.HTTP-Referer` | `https://emailibrium.app`             | Required OpenRouter attribution header                        |
| `providers.openrouter.required_headers.X-Title`      | `Emailibrium`                         | Required OpenRouter attribution header                        |
| `providers.google_oauth.client_id_env`               | `EMAILIBRIUM_GOOGLE_CLIENT_ID`        | Env var name for the Google OAuth client ID                   |
| `providers.google_oauth.client_secret_env`           | `EMAILIBRIUM_GOOGLE_CLIENT_SECRET`    | Env var name for the Google OAuth client secret               |
| `providers.microsoft_oauth.client_id_env`            | `EMAILIBRIUM_MICROSOFT_CLIENT_ID`     | Env var name for the Microsoft OAuth client ID                |
| `providers.microsoft_oauth.client_secret_env`        | `EMAILIBRIUM_MICROSOFT_CLIENT_SECRET` | Env var name for the Microsoft OAuth client secret            |

### Hardware Detection (`hardware.*`)

| Key                         | Default                      | Description                                        |
| --------------------------- | ---------------------------- | -------------------------------------------------- |
| `hardware.backend_priority` | `[metal, cuda, vulkan, cpu]` | GPU backend selection priority order               |
| `hardware.os_overhead_mb`   | `4096`                       | RAM reserved for OS + app when recommending models |

### Security (`security.*`, app.yaml) -- distinct from `security.*` above

Env var **names**, not the secrets themselves, plus two rate-limit/HSTS defaults that mirror (and
today can drift from) the `security.rate_limit.*`/`security.hsts.*` keys in `backend/config.yaml`.

| Key                                  | Default                                  | Description                                                          |
| ------------------------------------ | ---------------------------------------- | -------------------------------------------------------------------- |
| `security.jwt_secret_env`            | `JWT_SECRET`                             | Env var name for the JWT signing secret                              |
| `security.encryption_key_env`        | `EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD` | Env var name for the encryption master password                      |
| `security.rate_limit_capacity`       | `500`                                    | Max requests in the burst window (high for dev; lower in production) |
| `security.rate_limit_refill_per_sec` | `20.0`                                   | Tokens refilled per second                                           |
| `security.hsts_max_age_secs`         | `63072000`                               | HSTS `max-age` (2 years)                                             |

### Rules Studio (`rules.*`)

| Key                                 | Default | Description                                       |
| ----------------------------------- | ------- | ------------------------------------------------- |
| `rules.suggestions_page_size`       | `5`     | Suggestions loaded per "Build with AI" click      |
| `rules.suggestions_min_email_count` | `5`     | Min emails from a sender to appear in suggestions |

### Paths (`paths.*`)

| Key                         | Default                     | Description                          |
| --------------------------- | --------------------------- | ------------------------------------ |
| `paths.llm_cache_dir`       | `~/.emailibrium/models/llm` | GGUF model cache directory           |
| `paths.embedding_cache_dir` | `.fastembed_cache`          | ONNX/fastembed model cache directory |
| `paths.vector_data_dir`     | `data/vectors`              | Vector store data directory          |
| `paths.database_file`       | `emailibrium.db`            | SQLite database filename             |

### Other

| Key       | Default | Description                            |
| --------- | ------- | -------------------------------------- |
| `version` | `"1.0"` | `config/app.yaml`'s own schema version |

## Configuration Files

| File                        | Purpose                            | Committed to Git? |
| --------------------------- | ---------------------------------- | ----------------- |
| `config.yaml`               | Base defaults for all environments | Yes               |
| `config.local.yaml`         | Personal local overrides           | No (gitignored)   |
| `config.local.yaml.example` | Template for local overrides       | Yes               |

## Sensitive Values

The following keys should **never** be set in committed config files:

- `encryption.master_password` -- use `EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD` env var
- `database_url` (in production) -- use `EMAILIBRIUM_DATABASE_URL` env var or `/run/secrets/database_url`

## Quantization Tiers (Auto Mode)

When `quantization.mode` is `auto`, the tier is selected based on vector count:

| Vector Count         | Tier    | Compression | Description                        |
| -------------------- | ------- | ----------- | ---------------------------------- |
| < 50,000             | None    | 1x          | Full fp32 precision                |
| 50,000 -- 200,000    | Scalar  | ~4x         | int8 per-dimension min-max scaling |
| 200,000 -- 1,000,000 | Product | ~16x        | Product quantization               |
| > 1,000,000          | Binary  | ~32x        | 1-bit binary quantization          |

Hysteresis (default 10%) prevents thrashing near boundaries.
