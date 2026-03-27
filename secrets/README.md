# Secrets Management

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
└── .gitignore               # Ignores everything except dev.example/ and README
```

## Production Secrets

For production, use your CI/CD pipeline or secret management tool (Vault, AWS Secrets Manager, etc.) to populate `secrets/production/` at deploy time. The entrypoint script validates that all required secrets are present when `APP_ENV=production`.

## Security Notes

- Never commit actual secrets to version control
- The `secrets/dev/` directory is gitignored by default
- All secret files should have `chmod 600` permissions
- Secrets are mounted as files at `/run/secrets/` inside containers (per OWASP recommendation)
- The backend entrypoint resolves file-based secrets into environment variables at startup
