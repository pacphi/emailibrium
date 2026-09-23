# Secrets Management

For local integration-test files and GitHub test-account secrets, see
[Integration environments](../docs/testing/integration-environments.md).
Integration tests use `secrets/integration/` independently of the development and
production application secrets. Only explicitly selected provider suites connect
to their dedicated test accounts.

## Quick Start

Generate development secrets:

```bash
mkdir -p secrets/dev
openssl rand -base64 32 > secrets/dev/jwt_secret
openssl rand -base64 32 > secrets/dev/oauth_encryption_key
echo "postgres://emailibrium:devpass@postgres:5432/emailibrium" > secrets/dev/database_url
echo "devpass" > secrets/dev/db_password

# OAuth credentials (see docs/deployment-guide.md for setup instructions)
echo "YOUR_GOOGLE_CLIENT_ID.apps.googleusercontent.com" > secrets/dev/google_client_id
echo "YOUR_GOOGLE_CLIENT_SECRET" > secrets/dev/google_client_secret
echo "YOUR_MICROSOFT_CLIENT_ID" > secrets/dev/microsoft_client_id
echo "YOUR_MICROSOFT_CLIENT_SECRET" > secrets/dev/microsoft_client_secret

chmod 600 secrets/dev/*
```

The `secrets/dev/` directory is gitignored. Use `secrets/dev.example/` as a template.

## Directory Structure

```text
secrets/
├── dev/                     # Development secrets (gitignored)
│   ├── jwt_secret           # openssl rand -base64 32
│   ├── oauth_encryption_key # openssl rand -base64 32
│   ├── database_url         # postgres://emailibrium:devpass@postgres:5432/emailibrium
│   └── db_password          # devpass
├── dev.example/                 # Template (committed to git)
│   ├── jwt_secret               # REPLACE_ME_jwt_secret_32_chars_minimum
│   ├── oauth_encryption_key     # REPLACE_ME_encryption_key_32_chars
│   ├── database_url             # postgres://emailibrium:REPLACE@postgres:5432/emailibrium
│   ├── db_password              # REPLACE_ME
│   ├── google_client_id         # Google OAuth Client ID
│   ├── google_client_secret     # Google OAuth Client Secret
│   ├── microsoft_client_id      # Microsoft Entra App Registration Client ID
│   └── microsoft_client_secret  # Microsoft Entra App Registration Client Secret
├── integration/                # Integration-test secrets (gitignored)
│   ├── database_url            # Disposable PostgreSQL test database
│   ├── google_client_id
│   ├── google_client_secret
│   ├── google_refresh_token
│   ├── google_expected_email
│   ├── microsoft_client_id
│   ├── microsoft_client_secret
│   ├── microsoft_refresh_token
│   ├── microsoft_expected_email
│   └── microsoft_tenant_id      # Optional; common by default
├── integration.example/        # Empty required files; tenant is common (committed)
└── .gitignore                  # Allows the example directories and README
```

## Integration Test Secrets

Create the test-secret directory without overwriting existing files:

```bash
umask 077
mkdir -p secrets/integration
chmod 700 secrets/integration
cp -n secrets/integration.example/* secrets/integration/
chmod 600 secrets/integration/*
```

Fill the selected mode's files in a local editor with literal values. Do not add
variable names, quote delimiters, comments, or shell commands. Terminal newlines
are trimmed. The [integration guide](../docs/testing/integration-environments.md)
lists the file-to-environment mapping and account setup steps.

Explicit `EMAILIBRIUM_TEST_*` environment variables override files, including an
explicit empty value, which fails validation. The runner does not load dotenv
files or fall back to development or production secrets. An existing
`.env.integration` remains ignored; manually copy any needed values in your
editor without its dotenv syntax. No automatic migration reads or deletes it.

Validate selected settings without connecting, then run a mode when ready:

```bash
node scripts/test-integration.mjs gmail --check-config
just test-integration local
```

## Production Secrets

For production, use your CI/CD pipeline or secret management tool (Vault, AWS Secrets Manager, etc.) to populate `secrets/production/` at deploy time. The entrypoint script validates that all required secrets are present when `APP_ENV=production`.

## Security Notes

- Never commit actual secrets to version control
- The `secrets/dev/`, `secrets/production/`, and `secrets/integration/` directories are gitignored
- Use `chmod 700` for local secret directories and `chmod 600` for their files
- Application secrets are mounted as files at `/run/secrets/` inside containers
- The backend entrypoint resolves application secret files into environment variables at startup; integration tests use only their selected test files or explicit test environment variables
