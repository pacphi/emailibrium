# Configuration Reference

## Overview

Emailibrium uses a layered configuration system via [figment](https://docs.rs/figment).

**Loading order** (later overrides earlier):

1. `config.yaml` -- base defaults
2. `config.{APP_ENV}.yaml` -- environment-specific (development/staging/production)
3. `config.local.yaml` -- local overrides (gitignored)
4. `EMAILIBRIUM_*` environment variables -- runtime overrides
5. `/run/secrets/*` -- Docker secrets (production)

> **Note:** The current `VectorConfig::load()` implementation in `config.rs` loads
> `config.yaml`, then `config.local.yaml`, then `EMAILIBRIUM_*` env vars.
> Environment-specific files (`config.{APP_ENV}.yaml`) and Docker secrets require
> a small addition to the figment chain in `config.rs`.

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

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `host` | String | `127.0.0.1` | `EMAILIBRIUM_HOST` | Server bind address |
| `port` | u16 | `8080` | `EMAILIBRIUM_PORT` | Server listen port |
| `database_url` | String | `sqlite:emailibrium.db?mode=rwc` | `EMAILIBRIUM_DATABASE_URL` | SQLite/PostgreSQL connection URL |

### Store (`store.*`)

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `store.path` | String | `data/vectors` | `EMAILIBRIUM_STORE_PATH` | Path for vector data persistence |
| `store.enabled` | bool | `true` | `EMAILIBRIUM_STORE_ENABLED` | Whether the vector store is enabled |

### Embedding (`embedding.*`)

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `embedding.provider` | String | `mock` | `EMAILIBRIUM_EMBEDDING_PROVIDER` | Embedding provider: `mock`, `ollama`, or `cloud` |
| `embedding.model` | String | `all-MiniLM-L6-v2` | `EMAILIBRIUM_EMBEDDING_MODEL` | Model name for text embeddings |
| `embedding.dimensions` | usize | `384` | `EMAILIBRIUM_EMBEDDING_DIMENSIONS` | Embedding vector dimensions |
| `embedding.batch_size` | usize | `64` | `EMAILIBRIUM_EMBEDDING_BATCH_SIZE` | Batch size for bulk embedding operations |
| `embedding.cache_size` | u64 | `10000` | `EMAILIBRIUM_EMBEDDING_CACHE_SIZE` | Number of entries in the embedding cache |
| `embedding.ollama_url` | String | `http://localhost:11434` | `EMAILIBRIUM_EMBEDDING_OLLAMA_URL` | Ollama base URL (fallback provider) |
| `embedding.min_query_tokens` | usize | `5` | `EMAILIBRIUM_EMBEDDING_MIN_QUERY_TOKENS` | Minimum token count before query augmentation kicks in |

### Index (`index.*`) -- HNSW Parameters

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `index.m` | usize | `16` | `EMAILIBRIUM_INDEX_M` | HNSW M parameter (connections per node). Higher = better recall, more memory |
| `index.ef_construction` | usize | `200` | `EMAILIBRIUM_INDEX_EF_CONSTRUCTION` | HNSW build quality. Higher = slower build, better index quality |
| `index.ef_search` | usize | `100` | `EMAILIBRIUM_INDEX_EF_SEARCH` | HNSW search quality. Higher = slower search, better recall |

### Search (`search.*`)

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `search.default_limit` | usize | `20` | `EMAILIBRIUM_SEARCH_DEFAULT_LIMIT` | Default number of results returned |
| `search.max_limit` | usize | `100` | `EMAILIBRIUM_SEARCH_MAX_LIMIT` | Maximum number of results a client can request |
| `search.similarity_threshold` | f32 | `0.5` | `EMAILIBRIUM_SEARCH_SIMILARITY_THRESHOLD` | Minimum cosine similarity to include in results |

### Encryption (`encryption.*`) -- ADR-008

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `encryption.enabled` | bool | `false` | `EMAILIBRIUM_ENCRYPTION_ENABLED` | Whether encryption at rest is enabled |
| `encryption.master_password` | Option\<String\> | `None` | `EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD` | Master password for key derivation. **Never set in config files; use env var or Docker secret.** |

### Categorizer (`categorizer.*`) -- ADR-004

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `categorizer.confidence_threshold` | f32 | `0.7` | `EMAILIBRIUM_CATEGORIZER_CONFIDENCE_THRESHOLD` | Minimum confidence for vector centroid classification. Below this, falls back to LLM |
| `categorizer.max_centroid_shift` | f32 | `0.1` | `EMAILIBRIUM_CATEGORIZER_MAX_CENTROID_SHIFT` | Maximum centroid shift per feedback event |
| `categorizer.min_feedback_events` | u32 | `10` | `EMAILIBRIUM_CATEGORIZER_MIN_FEEDBACK_EVENTS` | Minimum feedback events before centroid updates activate |

### Backup (`backup.*`) -- ADR-003

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `backup.enabled` | bool | `false` | `EMAILIBRIUM_BACKUP_ENABLED` | Whether automatic SQLite backup is enabled |
| `backup.interval_secs` | u64 | `3600` | `EMAILIBRIUM_BACKUP_INTERVAL_SECS` | Backup interval in seconds (default: 1 hour) |

### Clustering (`clustering.*`) -- ADR-009

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `clustering.min_cluster_size` | usize | `5` | `EMAILIBRIUM_CLUSTERING_MIN_CLUSTER_SIZE` | Minimum number of emails to form a cluster |
| `clustering.merge_threshold` | f32 | `0.85` | `EMAILIBRIUM_CLUSTERING_MERGE_THRESHOLD` | Centroid similarity above which two clusters are merged |
| `clustering.hysteresis_delta` | f32 | `0.05` | `EMAILIBRIUM_CLUSTERING_HYSTERESIS_DELTA` | Minimum improvement to reassign an email to a new cluster |
| `clustering.min_stability_runs` | u32 | `3` | `EMAILIBRIUM_CLUSTERING_MIN_STABILITY_RUNS` | Consecutive stable runs before a cluster is visible |
| `clustering.max_clusters` | usize | `50` | `EMAILIBRIUM_CLUSTERING_MAX_CLUSTERS` | Maximum number of clusters to discover |
| `clustering.neighbor_count` | usize | `20` | `EMAILIBRIUM_CLUSTERING_NEIGHBOR_COUNT` | Number of nearest neighbors for the similarity graph |

### Learning / SONA (`learning.*`) -- ADR-004

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `learning.sona_enabled` | bool | `true` | `EMAILIBRIUM_LEARNING_SONA_ENABLED` | Master switch for the SONA learning engine |
| `learning.positive_learning_rate` | f32 | `0.05` | `EMAILIBRIUM_LEARNING_POSITIVE_LEARNING_RATE` | Positive learning rate (alpha multiplier for positive feedback) |
| `learning.negative_learning_rate` | f32 | `0.02` | `EMAILIBRIUM_LEARNING_NEGATIVE_LEARNING_RATE` | Negative learning rate (beta multiplier for negative feedback) |
| `learning.session_rerank_gamma` | f32 | `0.15` | `EMAILIBRIUM_LEARNING_SESSION_RERANK_GAMMA` | Session re-ranking weight for Tier 2 learning |
| `learning.max_centroid_shift` | f32 | `0.1` | `EMAILIBRIUM_LEARNING_MAX_CENTROID_SHIFT` | Maximum centroid shift per feedback event |
| `learning.min_feedback_events` | u32 | `10` | `EMAILIBRIUM_LEARNING_MIN_FEEDBACK_EVENTS` | Minimum feedback events before centroid updates activate (cold start) |
| `learning.low_confidence_threshold` | f32 | `0.6` | `EMAILIBRIUM_LEARNING_LOW_CONFIDENCE_THRESHOLD` | Emails below this confidence are reclassified during hourly consolidation |
| `learning.ab_control_percentage` | f32 | `0.10` | `EMAILIBRIUM_LEARNING_AB_CONTROL_PERCENTAGE` | Fraction of queries routed to the control group (no SONA). Set to 0 to disable A/B testing |
| `learning.drift_alarm_threshold` | f32 | `0.20` | `EMAILIBRIUM_LEARNING_DRIFT_ALARM_THRESHOLD` | Drift alarm fires when any centroid drifts beyond this fraction |
| `learning.position_bias_threshold` | f32 | `0.95` | `EMAILIBRIUM_LEARNING_POSITION_BIAS_THRESHOLD` | Position-bias alarm threshold (rank-1 click ratio) |
| `learning.max_snapshots` | usize | `30` | `EMAILIBRIUM_LEARNING_MAX_SNAPSHOTS` | Maximum number of daily snapshots to retain for rollback |

### Quantization (`quantization.*`) -- ADR-007

| Key | Type | Default | Env Override | Description |
|-----|------|---------|-------------|-------------|
| `quantization.mode` | String | `auto` | `EMAILIBRIUM_QUANTIZATION_MODE` | Quantization mode: `auto`, `none`, `scalar`, `product`, or `binary` |
| `quantization.scalar_threshold` | u64 | `50000` | `EMAILIBRIUM_QUANTIZATION_SCALAR_THRESHOLD` | Vector count threshold to activate scalar (int8) quantization (~4x compression) |
| `quantization.product_threshold` | u64 | `200000` | `EMAILIBRIUM_QUANTIZATION_PRODUCT_THRESHOLD` | Vector count threshold to activate product quantization (~16x compression) |
| `quantization.binary_threshold` | u64 | `1000000` | `EMAILIBRIUM_QUANTIZATION_BINARY_THRESHOLD` | Vector count threshold to activate binary quantization (~32x compression) |
| `quantization.hysteresis_percent` | f32 | `0.10` | `EMAILIBRIUM_QUANTIZATION_HYSTERESIS_PERCENT` | Hysteresis percentage to prevent thrashing near tier boundaries (0.10 = 10%) |

## Configuration Files

| File | Purpose | Committed to Git? |
|------|---------|-------------------|
| `config.yaml` | Base defaults for all environments | Yes |
| `config.development.yaml` | Development overrides | Yes |
| `config.production.yaml` | Production hardening | Yes |
| `config.local.yaml` | Personal local overrides | No (gitignored) |
| `config.local.yaml.example` | Template for local overrides | Yes |

## Sensitive Values

The following keys should **never** be set in committed config files:

- `encryption.master_password` -- use `EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD` env var
- `database_url` (in production) -- use `EMAILIBRIUM_DATABASE_URL` env var or `/run/secrets/database_url`

## Quantization Tiers (Auto Mode)

When `quantization.mode` is `auto`, the tier is selected based on vector count:

| Vector Count | Tier | Compression | Description |
|-------------|------|-------------|-------------|
| < 50,000 | None | 1x | Full fp32 precision |
| 50,000 -- 200,000 | Scalar | ~4x | int8 per-dimension min-max scaling |
| 200,000 -- 1,000,000 | Product | ~16x | Product quantization |
| > 1,000,000 | Binary | ~32x | 1-bit binary quantization |

Hysteresis (default 10%) prevents thrashing near boundaries.
