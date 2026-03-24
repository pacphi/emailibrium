#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Emailibrium — Setup Wizard (main orchestrator)
# ============================================================================
# Interactive first-time setup. Run all steps or pick individual ones.
# Usage: ./scripts/setup.sh
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors (match Makefile)
BOLD=$(tput bold 2>/dev/null || echo '')
GREEN=$(tput setaf 2 2>/dev/null || echo '')
YELLOW=$(tput setaf 3 2>/dev/null || echo '')
BLUE=$(tput setaf 4 2>/dev/null || echo '')
RED=$(tput setaf 1 2>/dev/null || echo '')
RESET=$(tput sgr0 2>/dev/null || echo '')

banner() {
  echo ""
  echo "${BOLD}${BLUE}╔════════════════════════════════════════════════════════════════════╗${RESET}"
  echo "${BOLD}${BLUE}║              Emailibrium — First-Time Setup Wizard                ║${RESET}"
  echo "${BOLD}${BLUE}╚════════════════════════════════════════════════════════════════════╝${RESET}"
  echo ""
}

# Detect what is already configured
detect_status() {
  local prereqs_ok=false secrets_ok=false ai_ok=false docker_ok=false

  # Prerequisites: check for rust and node at minimum
  if command -v rustc &>/dev/null && command -v node &>/dev/null && command -v pnpm &>/dev/null; then
    prereqs_ok=true
  fi

  # Secrets: check if secrets/dev/ exists with non-placeholder files
  if [[ -d "$PROJECT_ROOT/secrets/dev" ]] && [[ -f "$PROJECT_ROOT/secrets/dev/jwt_secret" ]]; then
    local jwt_val
    jwt_val=$(cat "$PROJECT_ROOT/secrets/dev/jwt_secret" 2>/dev/null || echo "")
    if [[ -n "$jwt_val" ]] && [[ "$jwt_val" != *"REPLACE"* ]]; then
      secrets_ok=true
    fi
  fi

  # AI: check if .env.local exists or ONNX model cache exists
  if [[ -f "$PROJECT_ROOT/.env.local" ]] || [[ -d "$PROJECT_ROOT/backend/.fastembed_cache" ]]; then
    ai_ok=true
  fi

  # Docker: check if docker is available
  if command -v docker &>/dev/null && docker info &>/dev/null 2>&1; then
    docker_ok=true
  fi

  echo "${BOLD}Current Status:${RESET}"
  echo "  1) Prerequisites     $(status_icon "$prereqs_ok")"
  echo "  2) Secrets            $(status_icon "$secrets_ok")"
  echo "  3) AI Providers       $(status_icon "$ai_ok")"
  echo "  4) Docker             $(status_icon "$docker_ok")"
  echo ""
}

status_icon() {
  if [[ "$1" == "true" ]]; then
    echo "${GREEN}[configured]${RESET}"
  else
    echo "${YELLOW}[not configured]${RESET}"
  fi
}

run_step() {
  local script="$1"
  local label="$2"
  echo ""
  echo "${BOLD}${BLUE}─── $label ───────────────────────────────────────────${RESET}"
  echo ""
  bash "$SCRIPT_DIR/$script"
  echo ""
  echo "${GREEN}$label complete.${RESET}"
  echo ""
}

menu() {
  echo "${BOLD}What would you like to do?${RESET}"
  echo ""
  echo "  ${BOLD}a)${RESET}  Run all steps (recommended for first-time setup)"
  echo "  ${BOLD}1)${RESET}  Check prerequisites"
  echo "  ${BOLD}2)${RESET}  Generate/configure secrets"
  echo "  ${BOLD}3)${RESET}  Configure AI providers"
  echo "  ${BOLD}4)${RESET}  Set up Docker environment"
  echo "  ${BOLD}5)${RESET}  Validate entire setup"
  echo "  ${BOLD}q)${RESET}  Quit"
  echo ""
  read -rp "${BOLD}Choice [a/1-5/q]: ${RESET}" choice

  case "$choice" in
    a|A)
      run_step "setup-prereqs.sh" "Step 1: Prerequisites"
      run_step "setup-secrets.sh" "Step 2: Secrets"
      run_step "setup-ai.sh" "Step 3: AI Providers"
      run_step "setup-docker.sh" "Step 4: Docker Environment"
      run_step "setup-validate.sh" "Step 5: Validation"
      echo ""
      echo "${BOLD}${GREEN}Setup complete! Run 'make dev' to start developing.${RESET}"
      ;;
    1) run_step "setup-prereqs.sh" "Prerequisites" ;;
    2) run_step "setup-secrets.sh" "Secrets" ;;
    3) run_step "setup-ai.sh" "AI Providers" ;;
    4) run_step "setup-docker.sh" "Docker Environment" ;;
    5) run_step "setup-validate.sh" "Validation" ;;
    q|Q)
      echo "Exiting."
      exit 0
      ;;
    *)
      echo "${RED}Invalid choice.${RESET}"
      menu
      ;;
  esac
}

banner
detect_status
menu
