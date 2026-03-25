# OAuth Setup Guide

Configure Gmail (Google) and Outlook (Microsoft) OAuth for Emailibrium.

> **Official references**:
> [Google OAuth 2.0 for Web Apps](https://developers.google.com/identity/protocols/oauth2/web-server) |
> [Microsoft Entra App Registration](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app)

## Prerequisites

- A Google Cloud project and/or Microsoft Entra (Azure AD) app registration
- `make setup` completed (writes credentials to `secrets/dev/`)
- Backend running on `http://localhost:8080` (default)

---

## Google (Gmail)

### 1. Create OAuth Credentials

> See: [Setting up OAuth 2.0](https://support.google.com/googleapi/answer/6158849)

1. Go to [Google Cloud Console > Credentials](https://console.cloud.google.com/apis/credentials)
2. Click **Create Credentials > OAuth client ID**
3. Application type: **Web application**
4. Name: `emailibrium-dev` (or any name)
5. **Authorized redirect URIs**: add `http://localhost:8080/api/v1/auth/callback`
6. Click **Create** and note the **Client ID** and **Client Secret**

### 2. Configure OAuth Consent Screen

> See: [Configure OAuth Consent](https://developers.google.com/workspace/guides/configure-oauth-consent)

1. Go to **APIs & Services > OAuth consent screen**
2. Choose **External** user type (required for non-Workspace Gmail accounts)
3. Fill in app name, support email, developer email
4. **Scopes** ([full reference](https://developers.google.com/workspace/gmail/api/auth/scopes)): add these:
   - `https://www.googleapis.com/auth/gmail.modify` (read/write email — **restricted**)
   - `https://www.googleapis.com/auth/gmail.labels` (manage labels — **sensitive**)
   - `https://www.googleapis.com/auth/userinfo.email` (identify user — non-sensitive)
5. **Test users**: add each Gmail address you want to test with
6. Save

> **Important**: If your GCP project is in a Google Workspace organization,
> the consent screen defaults to "Internal" (org-only). You must select
> "External" to allow personal Gmail accounts like `@gmail.com`.

### 3. Enable Gmail API

1. Go to **APIs & Services > Library**
2. Search for **Gmail API**
3. Click **Enable**

### 4. Store Credentials

Run `make setup-secrets` or manually create the files:

```bash
echo "YOUR_CLIENT_ID" > secrets/dev/google_client_id
echo "YOUR_CLIENT_SECRET" > secrets/dev/google_client_secret
chmod 600 secrets/dev/google_client_id secrets/dev/google_client_secret
```

The `make dev` target automatically exports these as `EMAILIBRIUM_GOOGLE_CLIENT_ID` and `EMAILIBRIUM_GOOGLE_CLIENT_SECRET` environment variables.

### 5. Test

1. Start the app: `make dev`
2. Navigate to `http://localhost:3000/onboarding`
3. Click **Gmail** > authenticate with a test user account
4. You should be redirected back to the app after consent

### Google OAuth: Testing vs Production

| Aspect              | Testing Mode                     | Production Mode                              |
| ------------------- | -------------------------------- | -------------------------------------------- |
| **Who can sign in** | Only explicitly added test users | Any Google account                           |
| **Token lifetime**  | 7 days (must re-consent)         | Standard (1 hour access, long-lived refresh) |
| **Verification**    | None required                    | Google verification required                 |
| **How to publish**  | N/A                              | OAuth consent screen > Publish App           |

#### Publishing to Production

When ready to allow any Gmail user (not just test users):

1. **OAuth consent screen > Publish App**
2. Google requires verification based on your scopes:

| Scope                      | Classification | Verification                         |
| -------------------------- | -------------- | ------------------------------------ |
| `userinfo.email`, `openid` | Non-sensitive  | Auto-approved                        |
| `gmail.readonly`           | Sensitive      | 3-5 business days                    |
| `gmail.modify`             | Restricted     | 4-6 weeks + CASA security assessment |

3. **Requirements for `gmail.modify` verification** ([full details](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification)):
   - Privacy policy at a public URL
   - App homepage at a public URL
   - Authorized domain verified in Google Search Console
   - YouTube demo video (2-5 min) showing how the app uses Gmail data
   - Written justification for each scope
   - [CASA](https://appdefensealliance.dev/casa) security assessment via an [authorized assessor](https://appdefensealliance.dev/casa/casa-assessors) (cost negotiated directly with assessor; Google does not charge fees)
   - **Annual recertification** required (12-month cycle from Letter of Assessment approval)

4. **Stepping stone**: Start with `gmail.readonly` to get verified faster, then upgrade to `gmail.modify` later. To use read-only mode, update `config.yaml`:

```yaml
oauth:
  gmail:
    scopes:
      - 'https://www.googleapis.com/auth/gmail.readonly'
      - 'https://www.googleapis.com/auth/gmail.labels'
      - 'https://www.googleapis.com/auth/userinfo.email'
```

---

## Microsoft (Outlook / Microsoft 365)

### 1. Register an Application

> See: [Quickstart: Register an app](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app)

1. Go to [Microsoft Entra > App registrations](https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade)
2. Click **New registration**
3. Name: `emailibrium-dev`
4. Supported account types: **Accounts in any organizational directory and personal Microsoft accounts**
5. Redirect URI: **Web** > `http://localhost:8080/api/v1/auth/callback`
6. Click **Register**
7. Note the **Application (client) ID** from the overview page

### 2. Create a Client Secret

1. In your app registration, go to **Certificates & secrets**
2. Click **New client secret**
3. Description: `emailibrium-dev-secret`
4. Expiry: choose duration (recommend 24 months for dev)
5. Click **Add** and copy the **Value** immediately (it's only shown once)

### 3. Configure API Permissions

> See: [Graph permissions reference](https://learn.microsoft.com/en-us/graph/permissions-reference) |
> [Mail API overview](https://learn.microsoft.com/en-us/graph/api/resources/mail-api-overview)

1. Go to **API permissions**
2. Click **Add a permission > Microsoft Graph > Delegated permissions**
3. Add these permissions:
   - `Mail.ReadWrite` (read and write mail)
   - `Mail.Send` (send mail)
   - `User.Read` (get user profile)
   - `offline_access` (refresh tokens)
4. Click **Add permissions**

> **Note**: `Mail.ReadWrite` and `Mail.Send` do not require admin consent
> for personal Microsoft accounts. For organizational accounts, an admin
> may need to grant consent.

### 4. Configure Authentication

> See: [Redirect URI best practices](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url) |
> [Auth code flow](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow)

1. Go to **Authentication**
2. Under **Web > Redirect URIs**, verify `http://localhost:8080/api/v1/auth/callback` is listed
3. Under **Implicit grant and hybrid flows**, leave both checkboxes unchecked (we use authorization code flow)
4. **Supported account types**: ensure "Accounts in any organizational directory and personal Microsoft accounts" is selected

### 5. Tenant Configuration

The default tenant is `common`, which allows both personal Microsoft accounts and organizational accounts. This is set in `config.yaml`:

```yaml
oauth:
  outlook:
    tenant: 'common' # "common" = any account, or a specific tenant UUID
```

| Tenant value    | Who can sign in                                   |
| --------------- | ------------------------------------------------- |
| `common`        | Any Microsoft account (personal + organizational) |
| `organizations` | Only organizational (work/school) accounts        |
| `consumers`     | Only personal Microsoft accounts                  |
| `{tenant-id}`   | Only accounts in a specific organization          |

### 6. Store Credentials

```bash
echo "YOUR_APPLICATION_CLIENT_ID" > secrets/dev/microsoft_client_id
echo "YOUR_CLIENT_SECRET_VALUE" > secrets/dev/microsoft_client_secret
chmod 600 secrets/dev/microsoft_client_id secrets/dev/microsoft_client_secret
```

### 7. Test

1. Start the app: `make dev`
2. Navigate to `http://localhost:3000/onboarding`
3. Click **Outlook** > authenticate with a Microsoft account
4. Grant the requested permissions when prompted

---

## Configuration Reference

All OAuth settings live in `backend/config.yaml` under the `oauth` key:

```yaml
oauth:
  redirect_base_url: 'http://localhost:8080' # Base URL for callback

  gmail:
    client_id_env: 'EMAILIBRIUM_GOOGLE_CLIENT_ID' # env var name
    client_secret_env: 'EMAILIBRIUM_GOOGLE_CLIENT_SECRET'
    scopes:
      - 'https://www.googleapis.com/auth/gmail.modify'
      - 'https://www.googleapis.com/auth/gmail.labels'
      - 'https://www.googleapis.com/auth/userinfo.email'
    auth_url: 'https://accounts.google.com/o/oauth2/v2/auth'
    token_url: 'https://oauth2.googleapis.com/token'

  outlook:
    client_id_env: 'EMAILIBRIUM_MICROSOFT_CLIENT_ID'
    client_secret_env: 'EMAILIBRIUM_MICROSOFT_CLIENT_SECRET'
    tenant: 'common'
    scopes:
      - 'Mail.ReadWrite'
      - 'Mail.Send'
      - 'offline_access'
      - 'User.Read'
```

### Environment Variables

| Variable                                 | Source                                | Purpose                                      |
| ---------------------------------------- | ------------------------------------- | -------------------------------------------- |
| `EMAILIBRIUM_GOOGLE_CLIENT_ID`           | `secrets/dev/google_client_id`        | Google OAuth Client ID                       |
| `EMAILIBRIUM_GOOGLE_CLIENT_SECRET`       | `secrets/dev/google_client_secret`    | Google OAuth Client Secret                   |
| `EMAILIBRIUM_MICROSOFT_CLIENT_ID`        | `secrets/dev/microsoft_client_id`     | Microsoft Application (Client) ID            |
| `EMAILIBRIUM_MICROSOFT_CLIENT_SECRET`    | `secrets/dev/microsoft_client_secret` | Microsoft Client Secret Value                |
| `EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD` | `secrets/dev/oauth_encryption_key`    | AES-256-GCM key for token encryption at rest |

These are loaded automatically by `make dev` from the `secrets/dev/` directory.

### API Endpoints

| Endpoint                       | Method | Description                             |
| ------------------------------ | ------ | --------------------------------------- |
| `/api/v1/auth/gmail/connect`   | GET    | Redirects to Google OAuth consent       |
| `/api/v1/auth/outlook/connect` | GET    | Redirects to Microsoft OAuth consent    |
| `/api/v1/auth/callback`        | GET    | OAuth callback (handles both providers) |
| `/api/v1/auth/accounts`        | GET    | List connected accounts                 |
| `/api/v1/auth/accounts/:id`    | DELETE | Disconnect an account                   |

---

## Troubleshooting

### `redirect_uri_mismatch` (Google Error 400)

The redirect URI sent by the app doesn't match what's configured in Google Cloud Console.

**Fix**: In your OAuth client settings, add `http://localhost:8080/api/v1/auth/callback` as an authorized redirect URI. Must match exactly (protocol, host, port, path).

### `org_internal` (Google Error 403)

Your GCP project's OAuth consent screen is set to "Internal" (Workspace org only).

**Fix**: Go to OAuth consent screen, change to "External" user type. Add test users. See [Google section](#2-configure-oauth-consent-screen) above.

### `missing env var EMAILIBRIUM_GOOGLE_CLIENT_ID`

The backend can't find the OAuth credentials in environment variables.

**Fix**: Either run `make setup-secrets` to store credentials, or ensure `make dev` is used to start the server (it exports secrets from `secrets/dev/`). Running `cargo run` directly won't load secrets.

### Token expired / refresh failed

Google tokens in Testing mode expire every 7 days. Users must re-authenticate.

**Fix**: In development, this is expected. In production (after publishing the app), refresh tokens are long-lived.

### Microsoft: `AADSTS50011` (reply URL mismatch)

Same as Google's redirect_uri_mismatch but for Microsoft.

**Fix**: In Entra > App registrations > Authentication, add `http://localhost:8080/api/v1/auth/callback` as a redirect URI.
