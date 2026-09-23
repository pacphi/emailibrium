# Local and GitHub integration environments

Integration tests have three separate boundaries. Local/database tests use real
Emailibrium code and disposable data. Provider credential tests make real OAuth
and provider API calls only when explicitly selected. Mailbox mutation and model
quality tests remain separate work; a successful credential test does not prove
that filing 150,000 messages is correct or fast enough.

## Local configuration

The development app already loads `secrets/dev/` through `just dev`, while Compose
mounts secret files. Plain `cargo test` does not load a dotenv file. The integration
entry point uses Node's built-in dotenv support explicitly:

```bash
cp -n .env.integration.example .env.integration
chmod 600 .env.integration
just test-integration local
```

Requirements: the repository's pinned Rust toolchain, Node 24 or newer, Python
3.11 or newer for the native HTTP probe, and the initialized RuVector submodule.
Fill `.env.integration` in a local editor. It is ignored by Git; only the empty
example is committed. Existing shell environment values override file values.
Values are dotenv data, not shell commands; there is no `source` or shell
substitution. See [Node's environment-file documentation](https://nodejs.org/api/cli.html#--env-filefile).

Validate a provider's selected settings without building or connecting:

```bash
node --env-file-if-exists=.env.integration scripts/test-integration.mjs gmail --check-config
```

Equivalent direct invocation:

```bash
node --env-file-if-exists=.env.integration scripts/test-integration.mjs local
```

| Mode | What executes | Required configuration |
| --- | --- | --- |
| `local` | Default Rust tests, then the assembled HTTP engine with temporary SQLite and mock embeddings | No account credentials; HTTP/encryption keys are generated per probe |
| `postgres` | Plan, job, audit, and timestamp-boundary contracts against PostgreSQL | `EMAILIBRIUM_TEST_PG_URL`, pointing only to a disposable database |
| `gmail` | Real token refresh, expected identity, Gmail label listing | Four Google test-account values below |
| `outlook` | Real token refresh, expected identity, Outlook category listing | Four Microsoft test-account values below; optional tenant |
| `providers` | Both provider credential suites | Both sets |

The runner validates selected credentials before execution, excludes ordinary app
secrets from child processes, fails on missing values, and rejects a selected test
that executes zero tests. It never treats an unconfigured live suite as passing.
Provider tests do not start the application, read message bodies, send mail, change
labels/categories or read state, archive messages, or delete anything. Identity
and category contents are not printed. Provider calls have a bounded timeout.

For a disposable PostgreSQL instance:

```bash
docker run --rm -d --name emailibrium-integration-postgres \
  -p 127.0.0.1:55499:5432 \
  -e POSTGRES_USER=emailibrium -e POSTGRES_PASSWORD=integration-only \
  -e POSTGRES_DB=emailibrium_test postgres:16-alpine
```

Set this test-only value in `.env.integration`:

```dotenv
EMAILIBRIUM_TEST_PG_URL=postgres://emailibrium:integration-only@127.0.0.1:55499/emailibrium_test
```

Then run `just test-integration postgres`; stop the disposable server with
`docker stop emailibrium-integration-postgres`. These tests migrate the database
and write test rows. Never point them at your application database.

## Dedicated provider accounts and credentials

Use dedicated Gmail and Outlook test accounts. Authorize each account interactively
once using its provider's OAuth application, then store the resulting refresh
token locally. A client ID and client secret identify the application; the refresh
token represents the account's delegated authorization. An access token alone is
short lived and unsuitable as the stored CI credential.

| Local environment variable / GitHub secret | Purpose |
| --- | --- |
| `EMAILIBRIUM_TEST_GOOGLE_CLIENT_ID` | Google OAuth application's client ID |
| `EMAILIBRIUM_TEST_GOOGLE_CLIENT_SECRET` | Its client secret |
| `EMAILIBRIUM_TEST_GOOGLE_REFRESH_TOKEN` | Refresh token authorized by the dedicated Gmail account |
| `EMAILIBRIUM_TEST_GOOGLE_EXPECTED_EMAIL` | Account identity the test must match |
| `EMAILIBRIUM_TEST_MICROSOFT_CLIENT_ID` | Microsoft Entra application's client ID |
| `EMAILIBRIUM_TEST_MICROSOFT_CLIENT_SECRET` | Its client secret value, not the secret's identifier |
| `EMAILIBRIUM_TEST_MICROSOFT_REFRESH_TOKEN` | Refresh token authorized by the dedicated Outlook account |
| `EMAILIBRIUM_TEST_MICROSOFT_EXPECTED_EMAIL` | Account identity the test must match |

`EMAILIBRIUM_TEST_MICROSOFT_TENANT_ID` is optional and defaults to `common`.
It accepts a tenant GUID, `common`, `organizations`, or `consumers`; on GitHub,
store it as an environment **variable**, rather than a secret.

For Gmail, enable the Gmail API and authorize `gmail.readonly`, requesting offline
access to obtain a refresh token. Use a server/desktop OAuth flow whose registered
redirect URI matches the initial authorization. GitHub subsequently uses the refresh
grant and does not need a public callback server. See [Google's OAuth flow](https://developers.google.com/identity/protocols/oauth2/web-server).
External apps in Google's Testing publishing status can issue refresh tokens that
expire after seven days for Gmail scopes; plan renewal or the appropriate production
OAuth configuration. See [Google's refresh-token expiration rules](https://developers.google.com/identity/protocols/oauth2#expiration).
The requested scope covers the [Gmail label-list API](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.labels/list).

For Outlook, use delegated `User.Read`, `MailboxSettings.Read`, and `offline_access`.
Category enumeration specifically requires `MailboxSettings.Read`, not `Mail.Read`.
See [Graph's category permissions](https://learn.microsoft.com/en-us/graph/api/outlookuser-list-mastercategories?view=graph-rest-1.0).
These scopes intentionally cover this credential/label contract, not later mailbox
mutation tests. Prefer a confidential application flow suitable for unattended
refresh, rather than an SPA token. Tokens can expire or be revoked; reconnect the
test account and replace its saved token when necessary. The tests do not print or
automatically write rotated credentials into GitHub. See [Microsoft's refresh-token lifecycle](https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens).

## GitHub setup

In `pacphi/emailibrium`, open **Settings → Environments**, create
**live-integration**, and add the selected provider's four secrets using the exact
names above. Restrict the environment to `develop` and `main`; use required reviewers
where available. The committed workflow is manual and also restricts its jobs to
those branches. No PR or fork-triggered workflow receives these credentials.

Alternatively, GitHub CLI prompts for a secret without placing its value in command
history. Repeat for the selected provider's entries:

```bash
gh secret set EMAILIBRIUM_TEST_GOOGLE_CLIENT_ID \
  --repo pacphi/emailibrium --env live-integration
gh secret list --repo pacphi/emailibrium --env live-integration
```

GitHub injects each `${{ secrets.NAME }}` as the corresponding environment variable;
CI does not need to write a `.env` file. See [GitHub's secrets documentation](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets).

Once `live-integration.yml` is available on the repository's default branch, select
**Actions → Live Provider Integration → Run workflow**, choose a permitted branch
and `gmail`, `outlook`, or `providers`. The CLI equivalent is:

```bash
gh workflow run live-integration.yml --repo pacphi/emailibrium \
  --ref develop -f provider=gmail
```

The existing PostgreSQL CI job creates its own service database. Native HTTP and
Compose smoke tests generate temporary access/encryption keys. Those checks need
no permanent `JWT_SECRET`, database password, or encryption key in GitHub. Never
upload your production `secrets/dev/` directory or application database as test
fixtures. Private model benchmarks will need a separate runner with the intended
hardware and pinned local model artifacts; OAuth secrets do not supply that capability.
