#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Emailibrium — Setup Validator
# ============================================================================
# Validates the entire setup: secrets, builds, services, and connectivity.
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
BOLD=$(tput bold 2>/dev/null || echo '')
GREEN=$(tput setaf 2 2>/dev/null || echo '')
YELLOW=$(tput setaf 3 2>/dev/null || echo '')
RED=$(tput setaf 1 2>/dev/null || echo '')
RESET=$(tput sgr0 2>/dev/null || echo '')

PASS=0
WARN=0
FAIL=0
TOTAL=0

check() {
  local label="$1"
  local status="$2"  # pass, warn, fail
  local msg="$3"

  TOTAL=$((TOTAL + 1))
  case "$status" in
    pass)
      PASS=$((PASS + 1))
      echo "  ${GREEN}[pass]${RESET} $label"
      ;;
    warn)
      WARN=$((WARN + 1))
      echo "  ${YELLOW}[warn]${RESET} $label — $msg"
      ;;
    fail)
      FAIL=$((FAIL + 1))
      echo "  ${RED}[fail]${RESET} $label — $msg"
      ;;
  esac
}

echo "${BOLD}Validating Emailibrium setup...${RESET}"
echo ""

# ─── Secrets ─────────────────────────────────────────────────────────────────

echo "${BOLD}Secrets:${RESET}"

SECRETS_DIR="$PROJECT_ROOT/secrets/dev"
for secret in jwt_secret oauth_encryption_key db_password database_url; do
  if [[ -f "$SECRETS_DIR/$secret" ]]; then
    val=$(cat "$SECRETS_DIR/$secret" 2>/dev/null || echo "")
    if [[ -n "$val" ]] && [[ "$val" != *"REPLACE"* ]] && [[ "$val" != *"placeholder"* ]]; then
      check "$secret" "pass" ""
    else
      check "$secret" "fail" "contains placeholder value"
    fi
  else
    check "$secret" "fail" "file missing"
  fi
done

for secret in google_client_id google_client_secret microsoft_client_id microsoft_client_secret; do
  if [[ -f "$SECRETS_DIR/$secret" ]]; then
    val=$(cat "$SECRETS_DIR/$secret" 2>/dev/null || echo "")
    if [[ -n "$val" ]] && [[ "$val" != *"REPLACE"* ]] && [[ "$val" != *"placeholder"* ]]; then
      check "$secret" "pass" ""
    else
      check "$secret" "warn" "placeholder (OAuth not configured)"
    fi
  else
    check "$secret" "warn" "file missing (OAuth not configured)"
  fi
done

echo ""

# ─── Backend build ───────────────────────────────────────────────────────────

echo "${BOLD}Backend (Rust):${RESET}"

if command -v cargo &>/dev/null; then
  echo "  Checking backend compilation (cargo check)..."
  if (cd "$PROJECT_ROOT/backend" && cargo check 2>/dev/null); then
    check "cargo check" "pass" ""
  else
    check "cargo check" "fail" "compilation errors"
  fi
else
  check "cargo check" "fail" "cargo not installed"
fi

echo ""

# ─── Frontend build ─────────────────────────────────────────────────────────

echo "${BOLD}Frontend (React/TypeScript):${RESET}"

if command -v pnpm &>/dev/null; then
  if [[ -d "$PROJECT_ROOT/frontend/node_modules" ]]; then
    check "node_modules" "pass" ""
  else
    check "node_modules" "warn" "run 'make -C frontend install' first"
  fi

  echo "  Checking frontend build (pnpm build)..."
  if (cd "$PROJECT_ROOT/frontend" && pnpm build 2>/dev/null); then
    check "pnpm build" "pass" ""
  else
    check "pnpm build" "fail" "build errors"
  fi
else
  check "pnpm" "fail" "pnpm not installed"
fi

echo ""

# ─── Docker services ────────────────────────────────────────────────────────

echo "${BOLD}Docker services:${RESET}"

if command -v docker &>/dev/null && docker info &>/dev/null 2>&1; then
  check "Docker daemon" "pass" ""

  # Check if any emailibrium containers are running
  running=$(cd "$PROJECT_ROOT" && docker compose ps --status running -q 2>/dev/null | wc -l | tr -d ' ')
  if [[ "$running" -gt 0 ]]; then
    check "Docker containers" "pass" ""

    # Show container health
    echo ""
    echo "  Container status:"
    (cd "$PROJECT_ROOT" && docker compose ps --format "table {{.Name}}\t{{.Status}}" 2>/dev/null) | sed 's/^/    /'
  else
    check "Docker containers" "warn" "no containers running (start with: make docker-up)"
  fi
else
  check "Docker" "warn" "Docker not available"
fi

echo ""

# ─── Backend API health ─────────────────────────────────────────────────────

echo "${BOLD}Backend API:${RESET}"

if curl -sf http://localhost:8080/health &>/dev/null; then
  check "GET /health" "pass" ""
else
  check "GET /health" "warn" "backend not reachable on localhost:8080"
fi

echo ""

# ─── AI / ONNX ──────────────────────────────────────────────────────────────

echo "${BOLD}AI providers:${RESET}"

if [[ -d "$PROJECT_ROOT/backend/.fastembed_cache" ]]; then
  check "ONNX models" "pass" ""
else
  check "ONNX models" "warn" "not cached yet (will download on first run)"
fi

if [[ -f "$PROJECT_ROOT/.env.local" ]]; then
  check ".env.local" "pass" ""
else
  check ".env.local" "warn" "no cloud AI keys configured"
fi

echo ""

# ─── Summary ─────────────────────────────────────────────────────────────────

echo "════════════════════════════════════════"
echo "${BOLD}Summary: $PASS passed, $WARN warnings, $FAIL failed (of $TOTAL checks)${RESET}"
echo ""

if [[ $FAIL -eq 0 && $WARN -eq 0 ]]; then
  echo "${GREEN}${BOLD}Everything looks good! Run 'make dev' or 'make docker-up' to start.${RESET}"
elif [[ $FAIL -eq 0 ]]; then
  echo "${GREEN}${BOLD}Core setup is complete.${RESET} Warnings above are optional features."
  echo "Run 'make dev' or 'make docker-up' to start."
else
  echo "${RED}${BOLD}Some checks failed.${RESET} Address the failures above before running."
  echo "Re-run: make setup-validate"
fi
