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

# Treat an empty EMAILIBRIUM_DATABASE_URL as unset.
#
# docker-compose.yml passes `${EMAILIBRIUM_DATABASE_URL:-}` through so the host env var
# is a one-flag override, but Compose materializes an unset host variable as an EMPTY
# string rather than omitting it. Left alone, that empty value would beat both the
# mounted secret below and the config default, and the app would fail to connect
# because nobody set the variable.
if [ -z "${EMAILIBRIUM_DATABASE_URL:-}" ]; then
  unset EMAILIBRIUM_DATABASE_URL
fi

# Bridge the database URL secret into the name the app actually reads.
#
# The loop above derives each variable from its secret's FILENAME, so
# /run/secrets/database_url becomes DATABASE_URL — but the app loads config through
# figment's EMAILIBRIUM_ prefix (backend/src/vectors/config.rs), so it never saw that
# variable and silently fell back to the SQLite default. That is precisely how
# .github/workflows/smoke.yml exercised SQLite for its whole life while mounting a
# postgres:// URL and believing it tested PostgreSQL (ADR-033 Context).
#
# An explicitly-provided EMAILIBRIUM_DATABASE_URL wins over the secret, so an operator
# can override the mounted value without rewriting the secret file.
if [ -n "${DATABASE_URL:-}" ] && [ -z "${EMAILIBRIUM_DATABASE_URL:-}" ]; then
  export EMAILIBRIUM_DATABASE_URL="$DATABASE_URL"
fi

# Validate required secrets in production.
# EMAILIBRIUM_DATABASE_URL, not DATABASE_URL: the check has to assert the value the
# app will actually read, which is either the bridged secret or a direct override.
if [ "$APP_ENV" = "production" ]; then
  for required in JWT_SECRET OAUTH_ENCRYPTION_KEY EMAILIBRIUM_DATABASE_URL; do
    eval val=\$$required
    if [ -z "$val" ]; then
      echo "FATAL: Required value $required is unset (expected a /run/secrets/ mount or an explicit env override)" >&2
      exit 1
    fi
  done
fi

exec "$@"
