# Configuration Reference

## Overview

Emailibrium uses a layered configuration system via [figment](https://docs.rs/figment).

**Loading order** (later overrides earlier):

1. `config.yaml` -- base defaults (in the backend working directory)
2. `config.local.yaml` -- local overrides (gitignored)
3. `EMAILIBRIUM_*` environment variables -- runtime overrides

> **Note:** Environment-specific files (`config.{APP_ENV}.yaml`) and Docker
> secrets (`/run/secrets/*`) are not yet wired into the figment chain.
> They are documented in `docs/configuration/CONFIG_HIERARCHY.md` as a
> planned extension. For now, use `config.local.yaml` or environment
> variables for environment-specific overrides.

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

## Complete Key Reference

### Top-Level

| Key            | Type   | Default                          | Env Override               | Description                      |
| -------------- | ------ | -------------------------------- | -------------------------- | -------------------------------- |
| `host`         | String | `127.0.0.1`                      | `EMAILIBRIUM_HOST`         | Server bind address              |
| `port`         | u16    | `8080`                           | `EMAILIBRIUM_PORT`         | Server listen port               |
| `database_url` | String | `sqlite:emailibrium.db?mode=rwc` | `EMAILIBRIUM_DATABASE_URL` | SQLite/PostgreSQL connection URL |

### Store (`store.*`)

| Key             | Type   | Default        | Env Override                | Description                         |
| --------------- | ------ | -------------- | --------------------------- | ----------------------------------- |
| `store.path`    | String | `data/vectors` | `EMAILIBRIUM_STORE_PATH`    | Path for vector data persistence    |
| `store.enabled` | bool   | `true`         | `EMAILIBRIUM_STORE_ENABLED` | Whether the vector store is enabled |

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

### Generative AI (`generative.*`) -- ADR-012

Controls the generative LLM used for classification fallback and chat features.

| Key                                      | Type   | Default                                     | Env Override                                         | Description                                        |
| ---------------------------------------- | ------ | ------------------------------------------- | ---------------------------------------------------- | -------------------------------------------------- |
| `generative.provider`                    | String | `none`                                      | `EMAILIBRIUM_GENERATIVE_PROVIDER`                    | Provider selection: `none`, `ollama`, or `cloud`   |
| `generative.ollama.base_url`             | String | `http://localhost:11434`                    | `EMAILIBRIUM_GENERATIVE_OLLAMA_BASE_URL`             | Ollama API base URL                                |
| `generative.ollama.classification_model` | String | `llama3.2:1b`                               | `EMAILIBRIUM_GENERATIVE_OLLAMA_CLASSIFICATION_MODEL` | Model for classification prompts                   |
| `generative.ollama.chat_model`           | String | `llama3.2:3b`                               | `EMAILIBRIUM_GENERATIVE_OLLAMA_CHAT_MODEL`           | Model for chat / free-form generation              |
| `generative.cloud.provider`              | String | `openai`                                    | `EMAILIBRIUM_GENERATIVE_CLOUD_PROVIDER`              | Cloud provider: `openai`, `anthropic`, or `gemini` |
| `generative.cloud.api_key_env`           | String | `EMAILIBRIUM_CLOUD_API_KEY`                 | `EMAILIBRIUM_GENERATIVE_CLOUD_API_KEY_ENV`           | Env var name holding the cloud API key             |
| `generative.cloud.model`                 | String | `gpt-4o-mini`                               | `EMAILIBRIUM_GENERATIVE_CLOUD_MODEL`                 | Cloud model identifier                             |
| `generative.cloud.base_url`              | String | `https://api.openai.com`                    | `EMAILIBRIUM_GENERATIVE_CLOUD_BASE_URL`              | Cloud provider API base URL                        |
| `generative.cloud.gemini.api_key_env`    | String | `EMAILIBRIUM_GEMINI_API_KEY`                | `EMAILIBRIUM_GENERATIVE_CLOUD_GEMINI_API_KEY_ENV`    | Gemini API key env var                             |
| `generative.cloud.gemini.model`          | String | `gemini-2.0-flash`                          | `EMAILIBRIUM_GENERATIVE_CLOUD_GEMINI_MODEL`          | Gemini model identifier                            |
| `generative.cloud.gemini.base_url`       | String | `https://generativelanguage.googleapis.com` | `EMAILIBRIUM_GENERATIVE_CLOUD_GEMINI_BASE_URL`       | Gemini API base URL                                |

### OAuth (`oauth.*`) -- DDD-005

OAuth client credentials are loaded from environment variables (never from config files) to prevent accidental secret exposure. The config controls which env vars to read and endpoint URLs.

| Key                               | Type          | Default                                                  | Env Override                          | Description                                     |
| --------------------------------- | ------------- | -------------------------------------------------------- | ------------------------------------- | ----------------------------------------------- |
| `oauth.redirect_base_url`         | String        | `http://localhost:8080`                                  | `EMAILIBRIUM_OAUTH_REDIRECT_BASE_URL` | Base URL for constructing OAuth redirect URIs   |
| `oauth.gmail.client_id_env`       | String        | `EMAILIBRIUM_GOOGLE_CLIENT_ID`                           | --                                    | Env var holding the Google OAuth Client ID      |
| `oauth.gmail.client_secret_env`   | String        | `EMAILIBRIUM_GOOGLE_CLIENT_SECRET`                       | --                                    | Env var holding the Google OAuth Client Secret  |
| `oauth.gmail.scopes`              | Vec\<String\> | `[gmail.modify, gmail.labels, userinfo.email]`           | --                                    | OAuth scopes requested from Google              |
| `oauth.gmail.auth_url`            | String        | `https://accounts.google.com/o/oauth2/v2/auth`           | --                                    | Google authorization endpoint                   |
| `oauth.gmail.token_url`           | String        | `https://oauth2.googleapis.com/token`                    | --                                    | Google token endpoint                           |
| `oauth.outlook.client_id_env`     | String        | `EMAILIBRIUM_MICROSOFT_CLIENT_ID`                        | --                                    | Env var holding the Microsoft Client ID         |
| `oauth.outlook.client_secret_env` | String        | `EMAILIBRIUM_MICROSOFT_CLIENT_SECRET`                    | --                                    | Env var holding the Microsoft Client Secret     |
| `oauth.outlook.tenant`            | String        | `common`                                                 | `EMAILIBRIUM_OAUTH_OUTLOOK_TENANT`    | Microsoft tenant ID (`common` for multi-tenant) |
| `oauth.outlook.scopes`            | Vec\<String\> | `[Mail.ReadWrite, Mail.Send, offline_access, User.Read]` | --                                    | OAuth scopes requested from Microsoft           |

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
