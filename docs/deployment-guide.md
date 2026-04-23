# Deployment Guide

## Prerequisites

| Tool             | Version | Install                                               |
| ---------------- | ------- | ----------------------------------------------------- |
| Rust             | 1.95+   | `rustup default stable`                               |
| Node.js          | 24+     | [nodejs.org](https://nodejs.org/) or `nvm install 24` |
| pnpm             | 10.32+  | `corepack enable` (ships with Node.js 24+)            |
| Docker + Compose | Latest  | [docker.com](https://www.docker.com/) (optional)      |
| Make             | Any     | Pre-installed on macOS/Linux                          |

## Quick Start

```bash
# 1. Clone the repository
git clone https://github.com/pacphi/emailibrium.git
cd emailibrium

# 2. Guided first-time setup (recommended)
make setup              # interactive wizard: prereqs, secrets, AI providers, Docker

# 3. Or skip the wizard and go directly:
make install            # install all dependencies
make dev                # start backend + frontend dev servers

# 4. Open the application
open http://localhost:3000
```

The backend runs on `http://localhost:8080` and the frontend on `http://localhost:3000`.

> **First time?** Run `make setup` for a guided walkthrough that checks prerequisites, generates secrets, configures AI providers, and validates your environment. See [Setup Guide](setup-guide.md) for the full reference.

## Configuration

### Configuration Hierarchy

Emailibrium uses [figment](https://docs.rs/figment) for layered configuration. Settings are loaded in order (later sources override earlier ones):

1. **Compiled defaults** -- hardcoded in `config.rs`
2. **`config.yaml`** -- project-level defaults (checked into the repo)
3. **`config.local.yaml`** -- local overrides (gitignored)
4. **Environment variables** -- prefixed with `EMAILIBRIUM_`, underscores map to nesting

### AI Model Pre-download

For production deployments, pre-download models during the build phase to avoid runtime downloads:

```bash
# During Docker build or deployment setup:
make download-models

# Or in Dockerfile:
RUN cargo run --release -- --download-models
```

The built-in LLM model (~350 MB) is cached in `~/.emailibrium/models/llm/`. Include this directory in your persistent volume.

### Key Configuration Options

```yaml
# config.yaml
host: '127.0.0.1'
port: 8080
database_url: 'sqlite:emailibrium.db?mode=rwc'

store:
  path: 'data/vectors'
  enabled: true

embedding:
  provider: 'onnx' # "onnx" | "mock" | "ollama" | "cloud"
  model: 'all-MiniLM-L6-v2'
  dimensions: 384
  batch_size: 64
  cache_size: 10000
  ollama_url: 'http://localhost:11434'
  onnx:
    model: 'all-MiniLM-L6-v2'
    show_download_progress: true
    dimensions: 384

encryption:
  enabled: false
  # master_password: set via env var, never in config file

search:
  default_limit: 20
  max_limit: 100
  similarity_threshold: 0.5

# Generative AI (ADR-012) -- classification fallback and chat
generative:
  provider: 'none' # "none" | "ollama" | "cloud"
  ollama:
    base_url: 'http://localhost:11434'
    classification_model: 'llama3.2'
    chat_model: 'llama3.2'
  cloud:
    provider: 'openai' # "openai" | "anthropic"
    api_key_env: 'EMAILIBRIUM_CLOUD_API_KEY'
    model: 'gpt-4o-mini'
    base_url: 'https://api.openai.com'
```

### Environment Variables

Sensitive values should always be set via environment variables:

```bash
# Encryption master password (ADR-008)
export EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD="your-secure-password"
export EMAILIBRIUM_ENCRYPTION_ENABLED=true

# Database URL override
export EMAILIBRIUM_DATABASE_URL="sqlite:/path/to/production.db?mode=rwc"

# Server binding
export EMAILIBRIUM_HOST="0.0.0.0"
export EMAILIBRIUM_PORT=8080

# OAuth credentials (see "Email Provider OAuth Setup" below)
export EMAILIBRIUM_GOOGLE_CLIENT_ID="your-google-client-id.apps.googleusercontent.com"
export EMAILIBRIUM_GOOGLE_CLIENT_SECRET="your-google-client-secret"
export EMAILIBRIUM_MICROSOFT_CLIENT_ID="your-microsoft-app-client-id"
export EMAILIBRIUM_MICROSOFT_CLIENT_SECRET="your-microsoft-app-client-secret"
```

### Email Provider OAuth Setup

Emailibrium connects to Gmail and Outlook via OAuth2. You must register your application with each provider to get client credentials. IMAP accounts use direct username/password and do not need OAuth setup.

#### Gmail (Google Cloud Console)

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project (or select an existing one)
3. Enable the **Gmail API**: APIs & Services > Library > search "Gmail API" > Enable
4. Create OAuth credentials: APIs & Services > Credentials > Create Credentials > OAuth client ID
   - Application type: **Web application**
   - Authorized redirect URI: `http://localhost:8080/api/v1/auth/gmail/callback` (development) or `https://your-domain.com/api/v1/auth/gmail/callback` (production)
5. Copy the **Client ID** and **Client Secret**
6. Set them as environment variables:

   ```bash
   export EMAILIBRIUM_GOOGLE_CLIENT_ID="123456789-abc.apps.googleusercontent.com"
   export EMAILIBRIUM_GOOGLE_CLIENT_SECRET="GOCSPX-your-secret"
   ```

   Or place them in `secrets/dev/google_client_id` and `secrets/dev/google_client_secret` for Docker.

#### Outlook (Microsoft Entra / Azure AD)

1. Go to [Microsoft Entra Admin Center](https://entra.microsoft.com/) > App registrations > New registration
2. Set the **Redirect URI**: `http://localhost:8080/api/v1/auth/outlook/callback` (Web platform)
3. Under **API permissions**, add:
   - `Mail.ReadWrite` (Delegated)
   - `Mail.Send` (Delegated)
   - `offline_access` (Delegated)
   - `User.Read` (Delegated)
4. Under **Certificates & secrets**, create a new client secret
5. Copy the **Application (client) ID** and **Client Secret value**
6. Set them as environment variables:

   ```bash
   export EMAILIBRIUM_MICROSOFT_CLIENT_ID="your-app-client-id"
   export EMAILIBRIUM_MICROSOFT_CLIENT_SECRET="your-client-secret-value"
   ```

   Or place them in `secrets/dev/microsoft_client_id` and `secrets/dev/microsoft_client_secret` for Docker.

#### IMAP (No OAuth Required)

IMAP accounts (Yahoo, iCloud, Fastmail, etc.) use direct credentials entered in the onboarding UI. No external setup is needed. Some providers require an "app password" instead of your main password.

## Docker Compose

A Docker Compose configuration is available for containerized deployment.

### Building and Running

```bash
# Build and start all services
docker compose up --build -d

# View logs
docker compose logs -f

# Stop
docker compose down
```

### Compose Services

| Service    | Port     | Description                               |
| ---------- | -------- | ----------------------------------------- |
| `backend`  | 8080     | Axum API server (Rust 1.95+)              |
| `frontend` | 3000     | Vite dev server (or Nginx for production) |
| `postgres` | internal | PostgreSQL 16 (no external port)          |
| `redis`    | internal | Redis 7 cache (no external port)          |

### Docker Configuration Files

Docker Compose mounts environment-specific config from the `configs/` directory:

```text
configs/
  config.development.yaml   # SQLite, ONNX embeddings, debug logging
  config.production.yaml    # PostgreSQL, Ollama embeddings, info logging
```

The active config is selected via the `APP_ENV` variable (defaults to `development`):

```bash
# Development (default)
docker compose up -d

# Production
APP_ENV=production docker compose up -d
```

### Docker Build Notes

- The backend uses a multi-stage Rust 1.95 build to minimize image size
- PostgreSQL and Redis data are persisted via named volumes
- The frontend is built as static assets and served via Nginx in production
- Secrets are mounted via Docker secrets from `secrets/{env}/` (see `secrets/dev.example/`)

## Database Strategy: SQLite vs PostgreSQL

SQLite is the primary database for development and single-node deployment. It requires no external process, supports the full feature set, and is the default (`database_url: "sqlite:emailibrium.db?mode=rwc"`).

PostgreSQL 16 is available via Docker Compose for scale-out scenarios and production deployments requiring concurrent write access. The backend uses SQLx with compile-time-checked queries that work against both SQLite and PostgreSQL -- switching is a matter of changing the `database_url` connection string.

| Scenario                     | Recommended Database | Rationale                                         |
| ---------------------------- | -------------------- | ------------------------------------------------- |
| Local development            | SQLite               | Zero setup, fast iteration                        |
| Single-user deployment       | SQLite               | No external dependencies, simpler operations      |
| Multi-user / team deployment | PostgreSQL 16        | Concurrent write safety, connection pooling       |
| High-availability production | PostgreSQL 16        | Replication, backup tooling, monitoring ecosystem |

To switch to PostgreSQL, set the database URL:

```bash
export EMAILIBRIUM_DATABASE_URL="postgres://user:password@localhost:5432/emailibrium"
```

## Vector Store Backend

The vector store backend is configured via `store.backend` in `config.yaml` or the `EMAILIBRIUM_STORE_BACKEND` environment variable.

| Backend    | Use Case                         | Requirements                                       |
| ---------- | -------------------------------- | -------------------------------------------------- |
| `ruvector` | **Default.** HNSW-indexed search | RuVector submodule (`ruvector/`)                   |
| `memory`   | Development / testing            | None (in-process, brute-force)                     |
| `qdrant`   | Managed vector DB at scale       | Qdrant server (REST API)                           |
| `sqlite`   | Emergency fallback               | Existing SQLite DB (brute-force cosine similarity) |

```bash
# Switch to Qdrant
export EMAILIBRIUM_STORE_BACKEND=qdrant
export EMAILIBRIUM_STORE_QDRANT_URL=http://localhost:6334

# Switch to SQLite emergency fallback
export EMAILIBRIUM_STORE_BACKEND=sqlite
```

The fallback chain per ADR-003 is: RuVector (primary) → Qdrant (managed) → SQLite (emergency).

The Docker Compose configuration provisions a PostgreSQL 16 instance automatically. Data is persisted via a named Docker volume.

## Production Deployment

### Reverse Proxy

Place Emailibrium behind a reverse proxy (Nginx, Caddy, or Traefik) for TLS termination and static asset serving.

#### Nginx Example

```nginx
server {
    listen 443 ssl http2;
    server_name emailibrium.example.com;

    ssl_certificate     /etc/letsencrypt/live/emailibrium.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/emailibrium.example.com/privkey.pem;

    # Frontend (static assets)
    location / {
        root /var/www/emailibrium/frontend/dist;
        try_files $uri $uri/ /index.html;
    }

    # Backend API
    location /api/ {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE support
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 86400s;
    }
}
```

#### Caddy Example

```caddyfile
emailibrium.example.com {
    root * /var/www/emailibrium/frontend/dist
    try_files {path} /index.html
    file_server

    handle /api/* {
        reverse_proxy localhost:8080
    }
}
```

### HTTPS

- Use Let's Encrypt via `certbot` or Caddy's automatic TLS
- Set `Strict-Transport-Security` header in the reverse proxy
- Redirect all HTTP traffic to HTTPS

### Secrets Management

| Secret                     | Storage              | Notes                                    |
| -------------------------- | -------------------- | ---------------------------------------- |
| Encryption master password | Environment variable | Never in config files or version control |
| OAuth client secrets       | Environment variable | Per-provider (Gmail, Outlook)            |
| Database path              | Environment variable | Use absolute path in production          |

Recommended approaches:

- **Systemd**: Use `EnvironmentFile=/etc/emailibrium/env`
- **Docker**: Use Docker secrets or `.env` file (not committed)
- **Cloud**: Use the platform's secret manager (AWS Secrets Manager, GCP Secret Manager, etc.)

### Systemd Service

```ini
[Unit]
Description=Emailibrium Backend
After=network.target

[Service]
Type=simple
User=emailibrium
WorkingDirectory=/opt/emailibrium/backend
EnvironmentFile=/etc/emailibrium/env
ExecStart=/opt/emailibrium/backend/target/release/emailibrium
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

### Health Checks

The backend exposes a health endpoint for monitoring:

```bash
# Check vector service health
curl -s http://localhost:8080/api/v1/vectors/health | jq .

# Expected response:
# { "status": "healthy", "store_healthy": true, "embedding_available": true, ... }
```

Use this endpoint for load balancer health checks and uptime monitoring.

## Build Commands Reference

| Command        | Description                                            |
| -------------- | ------------------------------------------------------ |
| `make install` | Install all dependencies                               |
| `make build`   | Build backend and frontend                             |
| `make test`    | Run all tests                                          |
| `make lint`    | Lint all code                                          |
| `make format`  | Format all code                                        |
| `make ci`      | Full CI pipeline (format-check, lint, typecheck, test) |
| `make dev`     | Start dev servers                                      |
| `make clean`   | Clean build artifacts                                  |
| `make audit`   | Security audit dependencies                            |

## Troubleshooting

### SQLite "database is locked"

Increase the SQLite pool timeout or reduce `max_connections` to 1 for single-writer workloads:

```bash
export EMAILIBRIUM_DATABASE_URL="sqlite:emailibrium.db?mode=rwc"
```

### Embedding provider unavailable

The default embedding provider is ONNX, which runs locally without any external service. If you need Ollama instead:

```bash
# Install and start Ollama
ollama serve &
ollama pull all-minilm:l6-v2
```

Then set the provider in config:

```yaml
embedding:
  provider: 'ollama' # onnx | mock | ollama | cloud
  ollama_url: 'http://localhost:11434'
```

The ONNX provider downloads the model on first use. To configure ONNX options:

```yaml
embedding:
  provider: 'onnx'
  onnx:
    model: 'all-MiniLM-L6-v2'
    show_download_progress: true
    dimensions: 384
```

### Port already in use

```bash
# Check what is using port 8080
lsof -i :8080

# Use a different port
export EMAILIBRIUM_PORT=8081
```
