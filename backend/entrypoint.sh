#!/bin/sh
# /app/entrypoint.sh — resolve /run/secrets/ into env vars
# Per OWASP: secrets delivered via file mounts, read at startup, not persisted in env
set -e

# Resolve /run/secrets/ into environment variables
for secret_file in /run/secrets/*; do
  if [ -f "$secret_file" ]; then
    var_name=$(basename "$secret_file" | tr '[:lower:]' '[:upper:]')
    export "$var_name"="$(cat "$secret_file")"
  fi
done

# Validate required secrets in production
if [ "$APP_ENV" = "production" ]; then
  for required in JWT_SECRET OAUTH_ENCRYPTION_KEY DATABASE_URL; do
    eval val=\$$required
    if [ -z "$val" ]; then
      echo "FATAL: Required secret $required not found in /run/secrets/" >&2
      exit 1
    fi
  done
fi

exec "$@"
