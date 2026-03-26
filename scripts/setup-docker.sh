#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Emailibrium — Docker Environment Setup
# ============================================================================
# Prepares Docker environment: configs, secrets, build, and optional start.
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
BOLD=$(tput bold 2>/dev/null || echo '')
GREEN=$(tput setaf 2 2>/dev/null || echo '')
YELLOW=$(tput setaf 3 2>/dev/null || echo '')
BLUE=$(tput setaf 4 2>/dev/null || echo '')
RED=$(tput setaf 1 2>/dev/null || echo '')
RESET=$(tput sgr0 2>/dev/null || echo '')

echo "${BOLD}Setting up Docker environment...${RESET}"
echo ""

# ─── Check Docker ────────────────────────────────────────────────────────────

echo "${BOLD}${BLUE}Checking Docker...${RESET}"

if ! command -v docker &>/dev/null; then
  echo "  ${RED}[error]${RESET} Docker is not installed."
  echo "         Install: https://docs.docker.com/get-docker/"
  exit 1
fi

if ! docker info &>/dev/null 2>&1; then
  echo "  ${RED}[error]${RESET} Docker daemon is not running."
  echo "         Start Docker Desktop or run: sudo systemctl start docker"
  exit 1
fi

echo "  ${GREEN}[ok]${RESET} Docker is running"
echo ""

# ─── Check Docker Compose ───────────────────────────────────────────────────

if ! docker compose version &>/dev/null 2>&1; then
  echo "  ${RED}[error]${RESET} 'docker compose' is not available."
  echo "         Upgrade Docker Desktop or install the compose plugin."
  exit 1
fi

echo "  ${GREEN}[ok]${RESET} Docker Compose available"
echo ""

# ─── Config templates ────────────────────────────────────────────────────────

echo "${BOLD}${BLUE}Checking configuration files...${RESET}"

for env in development production; do
  local_config="$PROJECT_ROOT/configs/config.${env}.yaml"
  if [[ -f "$local_config" ]]; then
    echo "  ${GREEN}[exists]${RESET} configs/config.${env}.yaml"
  else
    echo "  ${YELLOW}[missing]${RESET} configs/config.${env}.yaml"
    echo "          This file should exist in the repository."
  fi
done

echo ""

# ─── Secrets ─────────────────────────────────────────────────────────────────

echo "${BOLD}${BLUE}Checking Docker secrets...${RESET}"

SECRETS_DIR="$PROJECT_ROOT/secrets/dev"
SECRETS_NEEDED=(jwt_secret oauth_encryption_key db_password database_url
                google_client_id google_client_secret
                microsoft_client_id microsoft_client_secret)

missing_secrets=false
for secret in "${SECRETS_NEEDED[@]}"; do
  if [[ ! -f "$SECRETS_DIR/$secret" ]]; then
    missing_secrets=true
    break
  fi
done

if [[ "$missing_secrets" == "true" ]]; then
  echo "  ${YELLOW}[missing]${RESET} Some secrets not configured"
  echo "  Running secrets setup..."
  echo ""
  bash "$SCRIPT_DIR/setup-secrets.sh"
  echo ""
else
  echo "  ${GREEN}[ok]${RESET} All secret files present in secrets/dev/"
fi

echo ""

# ─── Build ───────────────────────────────────────────────────────────────────

echo "${BOLD}${BLUE}Building Docker images...${RESET}"
echo "  This may take several minutes on first run."
echo ""

(cd "$PROJECT_ROOT" && docker compose build) || {
  echo ""
  echo "  ${RED}[error]${RESET} Docker build failed. Check the output above."
  exit 1
}

echo ""
echo "  ${GREEN}[ok]${RESET} Docker images built successfully"
echo ""

# ─── Start services ─────────────────────────────────────────────────────────

read -rp "${BOLD}Start services now? (docker compose up -d) [y/N]: ${RESET}" start_choice
if [[ "$(echo "$start_choice" | tr '[:upper:]' '[:lower:]')" == "y" ]]; then
  echo ""
  echo "  Starting services..."
  (cd "$PROJECT_ROOT" && docker compose up -d)
  echo ""

  # Wait briefly for containers to initialize
  sleep 3

  echo "${BOLD}Container status:${RESET}"
  (cd "$PROJECT_ROOT" && docker compose ps --format "table {{.Name}}\t{{.Status}}\t{{.Ports}}")
  echo ""

  echo "${GREEN}Services started.${RESET}"
  echo "  Backend:  http://localhost:8080"
  echo "  Frontend: http://localhost:3000"
  echo ""
  echo "  Logs:     make docker-logs"
  echo "  Stop:     make docker-down"
else
  echo ""
  echo "  ${YELLOW}Skipped.${RESET} Start later with: make docker-up"
fi
