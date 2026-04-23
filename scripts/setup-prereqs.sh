#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Emailibrium — Prerequisite Checker
# ============================================================================
# Checks required tools and versions. Shows install instructions for missing.
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
BOLD=$(tput bold 2>/dev/null || echo '')
GREEN=$(tput setaf 2 2>/dev/null || echo '')
YELLOW=$(tput setaf 3 2>/dev/null || echo '')
RED=$(tput setaf 1 2>/dev/null || echo '')
RESET=$(tput sgr0 2>/dev/null || echo '')

MET=0
TOTAL=0

check_tool() {
  local name="$1"
  local cmd="$2"
  local min_version="$3"
  local install_hint="$4"

  TOTAL=$((TOTAL + 1))

  if ! command -v "$cmd" &>/dev/null; then
    echo "  ${RED}[missing]${RESET}  $name"
    echo "           Install: $install_hint"
    return
  fi

  local version=""
  case "$cmd" in
    rustc)    version=$(rustc --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -1) ;;
    node)     version=$(node --version 2>/dev/null | sed 's/^v//') ;;
    pnpm)     version=$(pnpm --version 2>/dev/null) ;;
    docker)   version=$(docker --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1) ;;
    make)     version=$(make --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?' | head -1) ;;
  esac

  if [[ -z "$version" ]]; then
    version="(unknown)"
  fi

  echo "  ${GREEN}[  ok  ]${RESET}  $name $version (need >= $min_version)"
  MET=$((MET + 1))
}

check_docker_compose() {
  TOTAL=$((TOTAL + 1))

  if docker compose version &>/dev/null 2>&1; then
    local version
    version=$(docker compose version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
    echo "  ${GREEN}[  ok  ]${RESET}  Docker Compose $version"
    MET=$((MET + 1))
  elif command -v docker-compose &>/dev/null; then
    local version
    version=$(docker-compose --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
    echo "  ${YELLOW}[ warn ]${RESET}  docker-compose (legacy) $version"
    echo "           Recommend: upgrade Docker Desktop for 'docker compose' (v2)"
    MET=$((MET + 1))
  else
    echo "  ${RED}[missing]${RESET}  Docker Compose"
    echo "           Install: included with Docker Desktop, or: https://docs.docker.com/compose/install/"
  fi
}

check_submodule() {
  TOTAL=$((TOTAL + 1))

  if [[ -f "$PROJECT_ROOT/.gitmodules" ]]; then
    local status
    status=$(cd "$PROJECT_ROOT" && git submodule status 2>/dev/null || echo "")
    if echo "$status" | grep -q '^-'; then
      echo "  ${YELLOW}[ warn ]${RESET}  ruvector/ submodule not initialized"
      echo "           Run: git submodule update --init --recursive"
    elif [[ -n "$status" ]]; then
      echo "  ${GREEN}[  ok  ]${RESET}  ruvector/ submodule initialized"
      MET=$((MET + 1))
    else
      echo "  ${YELLOW}[ warn ]${RESET}  No submodules found (expected ruvector/)"
    fi
  else
    echo "  ${YELLOW}[ skip ]${RESET}  No .gitmodules file (submodules not configured)"
    MET=$((MET + 1))
  fi
}

echo "${BOLD}Checking prerequisites...${RESET}"
echo ""

check_tool "Rust"           "rustc"  "1.95"   "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
check_tool "Node.js"        "node"   "22.12"  "brew install node@22  OR  https://nodejs.org/"
check_tool "pnpm"           "pnpm"   "10.32"  "corepack enable && corepack prepare pnpm@latest --activate"
check_tool "Docker"         "docker" "24.0"   "https://docs.docker.com/get-docker/"
check_docker_compose
check_tool "Make"           "make"   "3.81"   "Xcode CLI: xcode-select --install  OR  brew install make"

echo ""
echo "${BOLD}Submodules:${RESET}"
check_submodule

echo ""
echo "────────────────────────────────────────"
if [[ $MET -eq $TOTAL ]]; then
  echo "${GREEN}${BOLD}All $TOTAL of $TOTAL prerequisites met.${RESET}"
else
  echo "${YELLOW}${BOLD}$MET of $TOTAL prerequisites met.${RESET}"
  echo "Install the missing tools above, then re-run this check."
fi
