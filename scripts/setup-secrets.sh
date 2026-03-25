#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Emailibrium — Secrets Generator
# ============================================================================
# Generates and configures secrets for development.
# Auto-generates cryptographic secrets; prompts for OAuth credentials.
# Idempotent: skips secrets that are already configured.
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SECRETS_DIR="$PROJECT_ROOT/secrets/dev"

# Colors
BOLD=$(tput bold 2>/dev/null || echo '')
GREEN=$(tput setaf 2 2>/dev/null || echo '')
YELLOW=$(tput setaf 3 2>/dev/null || echo '')
BLUE=$(tput setaf 4 2>/dev/null || echo '')
RED=$(tput setaf 1 2>/dev/null || echo '')
RESET=$(tput sgr0 2>/dev/null || echo '')

mkdir -p "$SECRETS_DIR"

# Returns true if a secret file exists and has a non-placeholder value
is_configured() {
  local file="$SECRETS_DIR/$1"
  if [[ -f "$file" ]]; then
    local val
    val=$(cat "$file" 2>/dev/null || echo "")
    if [[ -n "$val" ]] && [[ "$val" != *"REPLACE"* ]]; then
      return 0
    fi
  fi
  return 1
}

write_secret() {
  local name="$1"
  local value="$2"
  echo "$value" > "$SECRETS_DIR/$name"
  chmod 600 "$SECRETS_DIR/$name"
}

echo "${BOLD}Configuring secrets in secrets/dev/...${RESET}"
echo ""

# ─── Auto-generated secrets ─────────────────────────────────────────────────

echo "${BOLD}Auto-generated secrets:${RESET}"

if is_configured "jwt_secret"; then
  echo "  ${GREEN}[exists]${RESET}  jwt_secret"
else
  write_secret "jwt_secret" "$(openssl rand -base64 32)"
  echo "  ${GREEN}[created]${RESET} jwt_secret"
fi

if is_configured "oauth_encryption_key"; then
  echo "  ${GREEN}[exists]${RESET}  oauth_encryption_key"
else
  write_secret "oauth_encryption_key" "$(openssl rand -base64 32)"
  echo "  ${GREEN}[created]${RESET} oauth_encryption_key"
fi

if is_configured "db_password"; then
  echo "  ${GREEN}[exists]${RESET}  db_password"
else
  DB_PASS=$(openssl rand -base64 32)
  write_secret "db_password" "$DB_PASS"
  echo "  ${GREEN}[created]${RESET} db_password"
fi

if is_configured "database_url"; then
  echo "  ${GREEN}[exists]${RESET}  database_url"
else
  # For Docker dev, use Postgres URL; read the generated db_password
  DB_PASS=$(cat "$SECRETS_DIR/db_password")
  write_secret "database_url" "postgres://emailibrium:${DB_PASS}@postgres:5432/emailibrium"
  echo "  ${GREEN}[created]${RESET} database_url (Postgres for Docker dev)"
  echo "           ${YELLOW}Note: For native dev without Docker, SQLite is used via config.yaml.${RESET}"
fi

echo ""

# ─── OAuth credentials (interactive) ────────────────────────────────────────

echo "${BOLD}OAuth credentials:${RESET}"
echo ""

# Google OAuth
if is_configured "google_client_id" && is_configured "google_client_secret"; then
  echo "  ${GREEN}[exists]${RESET}  Google OAuth (client ID + secret)"
else
  echo "  ${BLUE}Google OAuth Setup:${RESET}"
  echo "    1. Go to https://console.cloud.google.com/apis/credentials"
  echo "    2. Create an OAuth 2.0 Client ID (Web application)"
  echo "    3. Add authorized redirect URI: ${BOLD}http://localhost:8080/api/v1/auth/callback${RESET}"
  echo "    4. Copy the Client ID and Client Secret"
  echo ""
  echo "  ${YELLOW}Important — also do these steps or Gmail auth will fail:${RESET}"
  echo "    5. Enable the ${BOLD}Gmail API${RESET}: search 'Gmail API' in the Cloud Console and click Enable"
  echo "    6. Go to ${BOLD}Google Auth Platform > Data Access${RESET} and click 'Add or remove scopes'"
  echo "    7. In 'Manually add scopes', paste these (one per line) and click 'Add to table':"
  echo "         https://www.googleapis.com/auth/gmail.modify"
  echo "         https://www.googleapis.com/auth/gmail.labels"
  echo "         https://www.googleapis.com/auth/userinfo.email"
  echo "    8. Click 'Update', then 'Save'"
  echo ""
  read -rp "  Enter Google Client ID (or press Enter to skip): " google_id
  if [[ -n "$google_id" ]]; then
    write_secret "google_client_id" "$google_id"
    read -rp "  Enter Google Client Secret: " google_secret
    write_secret "google_client_secret" "$google_secret"
    echo "  ${GREEN}[saved]${RESET}   Google OAuth credentials"
  else
    echo "  ${YELLOW}[skipped]${RESET} Google OAuth (you can configure later with: make setup-secrets)"
    # Write placeholder so Docker Compose doesn't fail on missing files
    if [[ ! -f "$SECRETS_DIR/google_client_id" ]]; then
      write_secret "google_client_id" "placeholder-configure-later"
    fi
    if [[ ! -f "$SECRETS_DIR/google_client_secret" ]]; then
      write_secret "google_client_secret" "placeholder-configure-later"
    fi
  fi
fi

echo ""

# Microsoft OAuth
if is_configured "microsoft_client_id" && is_configured "microsoft_client_secret"; then
  echo "  ${GREEN}[exists]${RESET}  Microsoft OAuth (client ID + secret)"
else
  echo "  ${BLUE}Microsoft (Azure AD) OAuth Setup:${RESET}"
  echo "    1. Go to https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps"
  echo "    2. Register a new application"
  echo "    3. Add redirect URI: http://localhost:8080/api/v1/auth/callback"
  echo "    4. Under Certificates & secrets, create a new client secret"
  echo "    5. Copy the Application (client) ID and client secret value"
  echo ""
  read -rp "  Enter Microsoft Client ID (or press Enter to skip): " ms_id
  if [[ -n "$ms_id" ]]; then
    write_secret "microsoft_client_id" "$ms_id"
    read -rp "  Enter Microsoft Client Secret: " ms_secret
    write_secret "microsoft_client_secret" "$ms_secret"
    echo "  ${GREEN}[saved]${RESET}   Microsoft OAuth credentials"
  else
    echo "  ${YELLOW}[skipped]${RESET} Microsoft OAuth (you can configure later with: make setup-secrets)"
    if [[ ! -f "$SECRETS_DIR/microsoft_client_id" ]]; then
      write_secret "microsoft_client_id" "placeholder-configure-later"
    fi
    if [[ ! -f "$SECRETS_DIR/microsoft_client_secret" ]]; then
      write_secret "microsoft_client_secret" "placeholder-configure-later"
    fi
  fi
fi

echo ""
echo "────────────────────────────────────────"
echo "${BOLD}Secrets summary (secrets/dev/):${RESET}"
for f in jwt_secret oauth_encryption_key db_password database_url \
         google_client_id google_client_secret \
         microsoft_client_id microsoft_client_secret; do
  if is_configured "$f"; then
    echo "  ${GREEN}[ok]${RESET}       $f"
  elif [[ -f "$SECRETS_DIR/$f" ]]; then
    echo "  ${YELLOW}[placeholder]${RESET} $f"
  else
    echo "  ${RED}[missing]${RESET}    $f"
  fi
done
echo ""
echo "${GREEN}All secret files have permissions 600 (owner read/write only).${RESET}"
