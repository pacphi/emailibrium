# Deployment Guide

## Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust | 1.75+ | `rustup default stable` |
| Node.js | 20+ | [nodejs.org](https://nodejs.org/) or `nvm install 20` |
| pnpm | 9+ | `corepack enable` (ships with Node.js 20+) |
| Docker + Compose | Latest | [docker.com](https://www.docker.com/) (optional) |
| Make | Any | Pre-installed on macOS/Linux |

## Quick Start

```bash
# 1. Clone the repository
git clone https://github.com/pacphi/emailibrium.git
cd emailibrium

# 2. Install all dependencies (backend build + frontend install)
make install

# 3. Start both dev servers
make dev

# 4. Open the application
open http://localhost:3000
```

The backend runs on `http://localhost:8080` and the frontend on `http://localhost:3000`.

## Configuration

### Configuration Hierarchy

Emailibrium uses [figment](https://docs.rs/figment) for layered configuration. Settings are loaded in order (later sources override earlier ones):

1. **Compiled defaults** -- hardcoded in `config.rs`
2. **`config.yaml`** -- project-level defaults (checked into the repo)
3. **`config.local.yaml`** -- local overrides (gitignored)
4. **Environment variables** -- prefixed with `EMAILIBRIUM_`, underscores map to nesting

### Key Configuration Options

```yaml
# config.yaml
host: "127.0.0.1"
port: 8080
database_url: "sqlite:emailibrium.db?mode=rwc"

store:
  path: "data/vectors"
  enabled: true

embedding:
  provider: "mock"       # "mock" | "ollama"
  model: "all-MiniLM-L6-v2"
  dimensions: 384
  batch_size: 64
  cache_size: 10000
  ollama_url: "http://localhost:11434"

encryption:
  enabled: false
  # master_password: set via env var, never in config file

search:
  default_limit: 20
  max_limit: 100
  similarity_threshold: 0.5
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
```

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

| Service | Port | Description |
|---------|------|-------------|
| `backend` | 8080 | Axum API server |
| `frontend` | 3000 | Vite dev server (or Nginx for production) |

### Docker Build Notes

- The backend uses a multi-stage Rust build to minimize image size
- SQLite data is persisted via a named volume
- The frontend is built as static assets and served via Nginx in production

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

| Secret | Storage | Notes |
|--------|---------|-------|
| Encryption master password | Environment variable | Never in config files or version control |
| OAuth client secrets | Environment variable | Per-provider (Gmail, Outlook) |
| Database path | Environment variable | Use absolute path in production |

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

| Command | Description |
|---------|-------------|
| `make install` | Install all dependencies |
| `make build` | Build backend and frontend |
| `make test` | Run all tests |
| `make lint` | Lint all code |
| `make format` | Format all code |
| `make ci` | Full CI pipeline (format-check, lint, typecheck, test) |
| `make dev` | Start dev servers |
| `make clean` | Clean build artifacts |
| `make audit` | Security audit dependencies |

## Troubleshooting

### SQLite "database is locked"

Increase the SQLite pool timeout or reduce `max_connections` to 1 for single-writer workloads:

```bash
export EMAILIBRIUM_DATABASE_URL="sqlite:emailibrium.db?mode=rwc"
```

### Embedding provider unavailable

If Ollama is not running, the system falls back to the mock embedding model automatically. To use real embeddings:

```bash
# Install and start Ollama
ollama serve &
ollama pull all-minilm:l6-v2
```

Then set the provider in config:

```yaml
embedding:
  provider: "ollama"
  ollama_url: "http://localhost:11434"
```

### Port already in use

```bash
# Check what is using port 8080
lsof -i :8080

# Use a different port
export EMAILIBRIUM_PORT=8081
```
