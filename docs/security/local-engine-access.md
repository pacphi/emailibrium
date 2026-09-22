# Local engine access

The HTTP engine is a private, single-user service. Both REST and the sibling MCP
transport require authentication. This is an access boundary for one local
principal, not per-user or per-account authorization.

## Start and unlock

Run the existing `scripts/setup-secrets.sh` setup to generate the environment's
`jwt_secret` and `oauth_encryption_key`. Docker Compose already delivers those
files to the backend; the entrypoint exposes their values as `JWT_SECRET` and
`OAUTH_ENCRYPTION_KEY`. The server now consumes both.

For native execution, supply `JWT_SECRET` from the generated `jwt_secret` using
your usual secure environment/secret manager. It must contain 32–512 printable,
non-space characters. Supply the OAuth encryption key as described below before
connecting email accounts. A missing or invalid HTTP credential stops startup
before database access, model initialization, or background mailbox work.

Open the frontend at `http://localhost:3000` or `http://127.0.0.1:3000`. Enter the
local access token (the value of `jwt_secret`) in **Connect to your engine**. The
frontend sends it once in an Authorization header and receives an HttpOnly,
SameSite=Strict session cookie. The token is never written to browser storage or
a URL; the old `auth_token` localStorage value is removed on startup.

Sessions have a fixed eight-hour lifetime, a maximum of 64 active sessions, and
are kept only in engine memory. **Lock engine** revokes the current session.
Restarting the engine revokes all sessions. Rotate `JWT_SECRET` and restart to
replace the root credential. Closing a tab alone does not revoke a session.
The browser checks its session on focus and periodically; late checks cannot
undo a completed lock. Failed revocation is reported rather than claimed successful.

The local HTTP cookie does not unconditionally set `Secure`, because native
loopback HTTP is supported. This configuration is intended for a local trusted
machine; publishing it remotely requires a separately designed TLS deployment.

## MCP clients

A local HTTP MCP client uses `http://localhost:8080/api/v1/mcp` and the header
`Authorization: Bearer <local-access-token>`. Load the token from a secret source,
not a query parameter or checked-in client configuration. A missing or wrong
credential receives 401. An untrusted Origin receives 403 even with a valid token.

`--mcp-stdio` does not start an HTTP listener and uses the operating system's
process/user trust boundary. It does not require the HTTP credential. It still
requires an encryption key to read stored email credentials. Do not expose its
stdin/stdout through an unauthenticated network relay.

## Encryption key precedence

OAuth/IMAP credential storage uses AES-256-GCM and the existing Argon2id key
derivation. Nonempty values are resolved in this order:

1. The explicitly configured environment variable (`EMAILIBRIUM_ENCRYPTION_MASTER_PASSWORD`
   by default, or `app.security.encryption_key_env`).
2. `config.encryption.master_password` from the loaded configuration.
3. The existing mounted `OAUTH_ENCRYPTION_KEY` secret.

Empty or whitespace-only values do not mask the next source. Keep the same key
across restarts to decrypt existing credentials. Without a key, credential writes
and reads fail closed. The former base64 fallback is removed. Existing base64 or
otherwise undecryptable credential rows are not migrated automatically: configure
a key and reconnect the affected account. No live credentials are changed by tests.

## Network and callback boundaries

Native and shipped environment files bind `127.0.0.1`. Compose explicitly binds
the backend to all interfaces **inside** its private network for the frontend
proxy, and publishes the frontend/backend ports only on host loopback.

Allowed Origins are exact configured values. Defaults cover localhost and
127.0.0.1 on ports 3000 and 5173. Origin checks apply to session setup, mutations,
MCP, and reads; `null` and unlisted origins are rejected. Valid preflight requests
return only CORS metadata. Native clients may omit Origin but must authenticate.

`GET /healthz` is the only credential-free health response and returns only
`{"status":"ok"}`. Detailed vector health remains authenticated.

The OAuth callback is a narrow exception: only a state issued by an authenticated
connection request, still pending within ten minutes, can pass. The handler
consumes it once before contacting a provider. Pending states are bounded to 64
and disappear on restart. Invalid, expired, and replayed states require restarting
the connection. Request trace spans log only method and path; query logging also
redacts OAuth state and authorization codes without changing the real request URI.

## Verification scope

HTTP tests use real ephemeral loopback listeners with synthetic REST/MCP handlers.
They cover missing/wrong/correct credentials, malicious origins, preflight,
bootstrap/session cookies, expiry/restart/logout, and bounded issuance. OAuth tests
use in-memory databases and synthetic credentials; they prove missing-key writes
persist nothing and one-time state consumption. Trace tests capture logging output.
Browser tests simulate the session endpoint and cover setup, bad credentials,
reload, lock, expiration, offline retry, and stale-response races. They do not prove
live OAuth-provider authentication or deliver messages to a real mailbox.
