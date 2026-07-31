# Emailibrium Setup Guide

This guide walks through setting up Emailibrium for local development.
Run `just setup` for an interactive wizard that automates these steps.

## Prerequisites

| Tool           | Minimum Version | Install Command                                                   |
| -------------- | --------------- | ----------------------------------------------------------------- |
| Rust           | 1.97            | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Node.js        | 26 (LTS)        | `brew install node@26` or [nodejs.org](https://nodejs.org/)       |
| pnpm           | 11.5            | `npm install -g pnpm@11`                                          |
| Docker         | 24.0+           | [docs.docker.com/get-docker](https://docs.docker.com/get-docker/) |
| Docker Compose | v2              | Included with Docker Desktop                                      |
| Make           | 3.81+           | `xcode-select --install` (macOS) or `apt install build-essential` |

Check all prerequisites at once:

```bash
just setup-prereqs
```

### Git Submodules

The `ruvector/` submodule must be initialized:

```bash
git submodule update --init --recursive
```

## Step 1: Secrets

Secrets live in `secrets/dev/` (gitignored). The setup script auto-generates
cryptographic secrets and prompts for OAuth credentials.

```bash
just setup-secrets
```

### Auto-generated secrets

These are created automatically using `openssl rand -base64 32`:

- `jwt_secret` -- Signs JWT authentication tokens
- `oauth_encryption_key` -- Encrypts OAuth tokens at rest
- `db_password` -- PostgreSQL password (Docker dev environment)
- `database_url` -- PostgreSQL connection string for Docker

### OAuth credentials (manual)

OAuth requires registering apps with Google and Microsoft.

#### Google OAuth

1. Go to [Google Cloud Console > Credentials](https://console.cloud.google.com/apis/credentials)
2. Create a project (or select an existing one)
3. Click **Create Credentials > OAuth client ID**
4. Application type: **Web application**
5. Add authorized redirect URI: `http://localhost:8080/api/auth/google/callback`
6. Copy the **Client ID** and **Client Secret**

#### Microsoft (Azure AD) OAuth

1. Go to [Azure App Registrations](https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps)
2. Click **New registration**
3. Name: `Emailibrium Dev`
4. Supported account types: **Accounts in any organizational directory and personal Microsoft accounts**
5. Redirect URI: **Web** > `http://localhost:8080/api/auth/microsoft/callback`
6. Under **Certificates & secrets**, create a new **Client secret**
7. Copy the **Application (client) ID** from the Overview page and the **secret value**

OAuth is optional for initial development. You can skip it and configure later.

## AI Configuration

Emailibrium uses AI for email classification and smart features. It works out of the box with zero configuration.

### Default Setup (Recommended)

The default configuration uses:

- **Embedding**: ONNX Runtime (`all-MiniLM-L6-v2`) — runs locally, downloads ~23 MB on first use
- **Classification**: Built-in LLM (`qwen2.5-0.5b-q4km`) — runs locally, downloads ~350 MB on first use

No API keys, no external services, no data leaves your machine.

### Pre-download Models (Optional)

To avoid the first-use download delay:

```bash
just download-models
```

Or download individually:

```bash
# ONNX embedding model (23 MB)
cd backend && cargo run -- --download-models

# GGUF LLM model (350 MB)
npx tsx scripts/models.ts download --default
```

### Check Your Configuration

```bash
just diagnose
```

Shows embedding status, LLM model status, Ollama availability, and cloud API keys.

### Alternative Providers

| Want                   | Set                                      | Notes                   |
| ---------------------- | ---------------------------------------- | ----------------------- |
| No AI (fastest)        | `EMAILIBRIUM_GENERATIVE_PROVIDER=none`   | Rule-based only         |
| Ollama (larger models) | `EMAILIBRIUM_GENERATIVE_PROVIDER=ollama` | Requires `ollama serve` |
| Cloud (GPT-4o, Claude) | `EMAILIBRIUM_GENERATIVE_PROVIDER=cloud`  | Requires API key        |

See [Configuration Reference](configuration-reference.md) for all options.

## Step 2: AI Providers

Emailibrium supports a tiered AI architecture. Configure providers with:

```bash
just setup-ai
```

### ONNX (default, local)

- Runs fully offline, no API key needed
- Models download automatically on first backend start
- Pre-download with: `emailibrium --download-models`
- Default model: `all-MiniLM-L6-v2` (384-dimension embeddings)

### Ollama (local LLM)

- Install from [ollama.com](https://ollama.com/download)
- Start the server: `ollama serve`
- Pull a model: `ollama pull llama3.2`

### Cloud Providers

API keys are stored in `.env.local` (gitignored). Set any combination:

| Provider  | Environment Variable            | Get a Key                                                            |
| --------- | ------------------------------- | -------------------------------------------------------------------- |
| OpenAI    | `EMAILIBRIUM_OPENAI_API_KEY`    | [platform.openai.com](https://platform.openai.com/api-keys)          |
| Anthropic | `EMAILIBRIUM_ANTHROPIC_API_KEY` | [console.anthropic.com](https://console.anthropic.com/settings/keys) |
| Gemini    | `EMAILIBRIUM_GEMINI_API_KEY`    | [aistudio.google.com](https://aistudio.google.com/apikey)            |
| Cohere    | `EMAILIBRIUM_COHERE_API_KEY`    | [dashboard.cohere.com](https://dashboard.cohere.com/api-keys)        |

## Step 3: Development Environment

Choose between Docker and native development.

### Docker Development (recommended for first run)

```bash
just setup-docker    # Build images, optionally start services
just docker-up-dev   # Start with hot-reload
just docker-logs     # Tail logs
just docker-down     # Stop
```

Docker Compose starts: PostgreSQL, Redis, backend (Rust), frontend (React).

### Native Development

```bash
just install         # Install all dependencies
just dev             # Start backend + frontend dev servers
```

Native dev uses SQLite by default (configured in `configs/config.development.yaml`).

## Step 4: Validate

Run all validation checks:

```bash
just setup-validate
```

This checks: secrets, backend compilation, frontend build, Docker health,
API reachability, and AI model availability.

## Connecting an MCP client

The backend embeds an MCP server exposing 15 read-only tools over your mailbox. It runs in
two modes.

### HTTP mode (default)

The backend serves MCP at `http://localhost:8080/api/v1/mcp` whenever it is running. Nothing
extra to start. For Claude Code, add an entry to `.mcp.json` at the repo root:

```json
{
  "mcpServers": {
    "emailibrium": {
      "type": "http",
      "url": "http://localhost:8080/api/v1/mcp"
    }
  }
}
```

This repo already ships that entry, so Claude Code picks it up once the backend is running.
Because the backend is a long-running server, an HTTP entry references it by URL — the client
does not spawn it.

### stdio mode

For clients that launch a subprocess and speak JSON-RPC over stdin/stdout — Claude Desktop
being the common one — start the backend in stdio mode instead:

```json
{
  "mcpServers": {
    "emailibrium": {
      "command": "/absolute/path/to/emailibrium",
      "args": ["--mcp-stdio"]
    }
  }
}
```

In stdio mode the backend serves the same tools over stdin/stdout and **does not start the
HTTP server**. Logs go to **stderr**, not stdout — stdout carries only JSON-RPC frames, so
anything written there would corrupt the protocol stream. Your logs have not vanished; check
stderr and `data/logs/emailibrium.log`.

### Selecting the mode

The mode is an enum, `http` (default) or `stdio`, and can be set two ways:

| Source  | Form                         |
| ------- | ---------------------------- |
| CLI     | `--mcp-stdio`                |
| Env var | `EMAILIBRIUM_MCP_MODE=stdio` |

The CLI flag wins if both are set. Note the two spellings are not symmetric: the flag is a
boolean shorthand, the environment variable takes the mode name. There is no `--mcp-mode`
flag and no `EMAILIBRIUM_MCP_STDIO` variable.

The env form suits client configs that set environment rather than arguments:

```json
{
  "mcpServers": {
    "emailibrium": {
      "command": "/absolute/path/to/emailibrium",
      "env": { "EMAILIBRIUM_MCP_MODE": "stdio" }
    }
  }
}
```

### Smoke test

With the backend running in HTTP mode, confirm the endpoint is mounted:

```bash
curl -sS http://localhost:8080/api/v1/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

A JSON-RPC response means the server is reachable. A connection refused means the backend
is not running; a 404 means it started without the MCP routes mounted.

> **Localhost only.** `/api/v1` is unauthenticated by design — emailibrium is a local-first
> single-user app, and MCP inherits that stance. Anyone who can reach port 8080 can read your
> mail. Keep the port bound to localhost, and treat bearer auth as a prerequisite for any
> non-localhost deployment rather than an optional hardening step.

## Troubleshooting

### "Docker build failed"

- Ensure Docker Desktop is running and has enough disk space
- Try `docker system prune -f` to clean old images
- Rebuild without cache: `just docker-build-no-cache`

### "cargo check failed"

- Update Rust: `rustup update`
- Check the ruvector submodule: `git submodule update --init --recursive`

### "pnpm build failed"

- Install dependencies: `cd frontend && pnpm install`
- Clear cache: `cd frontend && pnpm store prune`

### "Backend not reachable on localhost:8080"

- Check if port 8080 is already in use: `lsof -i :8080`
- For Docker: check container logs with `just docker-logs-backend`
- For native: check `cd backend && just dev` output

### "ONNX model download slow"

- Models download from Hugging Face (~30 MB for all-MiniLM-L6-v2)
- If behind a proxy, set `HTTPS_PROXY` environment variable
- Models are cached in `backend/.fastembed_cache/`

### "OAuth callback error"

- Verify redirect URIs match exactly (including trailing slash)
- Google: `http://localhost:8080/api/auth/google/callback`
- Microsoft: `http://localhost:8080/api/auth/microsoft/callback`
- Check that client ID and secret are correct in `secrets/dev/`

## Environment Variables Reference

| Variable                         | Default                  | Description                                |
| -------------------------------- | ------------------------ | ------------------------------------------ |
| `APP_ENV`                        | `development`            | Environment name (development, production) |
| `BACKEND_PORT`                   | `8080`                   | Backend API port                           |
| `FRONTEND_PORT`                  | `3000`                   | Frontend web port                          |
| `RUST_LOG`                       | `emailibrium=info`       | Rust log filter                            |
| `VITE_API_URL`                   | `http://localhost:8080`  | Frontend API URL                           |
| `EMAILIBRIUM_OPENAI_API_KEY`     | --                       | OpenAI API key                             |
| `EMAILIBRIUM_ANTHROPIC_API_KEY`  | --                       | Anthropic API key                          |
| `EMAILIBRIUM_GEMINI_API_KEY`     | --                       | Gemini API key                             |
| `EMAILIBRIUM_COHERE_API_KEY`     | --                       | Cohere embedding API key                   |
| `EMAILIBRIUM_EMBEDDING_PROVIDER` | `onnx`                   | Override embedding provider                |
| `EMAILIBRIUM_OLLAMA_URL`         | `http://localhost:11434` | Ollama server URL                          |
| `EMAILIBRIUM_OLLAMA_MODEL`       | `llama3.2`               | Ollama model name                          |
| `REDIS_URL`                      | `redis://redis:6379`     | Redis connection (Docker)                  |
