# ADR-016: Security Middleware — Rate Limiting, HSTS, and Log Scrubbing

- **Status**: Accepted
- **Date**: 2026-03-24
- **Implements**: R-05 (Predecessor Recommendations)
- **Related**: ADR-008 (Privacy Architecture)

## Context

ADR-008 defines encryption at rest and CSP headers but does not cover rate limiting, HSTS, or log scrubbing. The predecessor repository ships production-ready security middleware (Governor-based rate limiting, full security header suite, automatic secret removal from logs). Before any production deployment, the API gateway needs these protections against abuse, clickjacking, protocol downgrade attacks, and accidental secret leakage in logs.

## Decision

Add three security middleware layers to the Axum API gateway: token bucket rate limiting, security response headers (including HSTS), and log scrubbing.

### Token Bucket Rate Limiting

Rate limiting uses a per-IP token bucket algorithm implemented as tower middleware:

- Each IP address gets a bucket with `burst_size` tokens (default: 50)
- Tokens replenish at `requests_per_second` rate (default: 10)
- When the bucket is empty, requests receive HTTP 429 with `Retry-After` header
- Stale buckets (no requests for 10 minutes) are automatically cleaned up to prevent memory growth
- Rate limit configuration is per-server, not per-route (simplicity over granularity)

The token bucket is chosen over fixed-window or sliding-window because it handles bursty traffic (common in email sync operations) without penalizing legitimate usage patterns.

### Security Response Headers

All responses include the following headers via a tower-http layer:

| Header                    | Value                                              | Purpose                          |
| ------------------------- | -------------------------------------------------- | -------------------------------- |
| Strict-Transport-Security | max-age=63072000; includeSubDomains; preload       | Enforce HTTPS (HSTS)             |
| X-Frame-Options           | DENY                                               | Prevent clickjacking             |
| X-Content-Type-Options    | nosniff                                             | Prevent MIME sniffing            |
| Content-Security-Policy   | default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' | XSS mitigation (from ADR-008) |
| Referrer-Policy           | strict-origin-when-cross-origin                    | Limit referrer leakage           |
| Permissions-Policy        | camera=(), microphone=(), geolocation=()           | Disable unnecessary browser APIs |

HSTS `max-age` and `includeSubDomains` are configurable in `config.yaml` for environments where HTTPS is not yet configured (development).

### Log Scrubbing

A tracing layer subscriber filter automatically redacts sensitive patterns from log output:

- OAuth tokens (`ya29.*`, `Bearer .*`)
- API keys (any string matching common key formats)
- Email content beyond the first 50 characters of subjects
- Password fields in configuration dumps

Scrubbing runs at the tracing subscriber level so it applies to all log output regardless of the logging call site.

### Configuration

```yaml
security:
  rate_limit:
    enabled: true
    requests_per_second: 10
    burst_size: 50
  hsts:
    enabled: true
    max_age_secs: 63072000
    include_subdomains: true
```

## Consequences

### Positive

- Rate limiting prevents API abuse and protects against simple DoS
- HSTS enforces HTTPS, preventing protocol downgrade attacks
- Security headers provide defense-in-depth against clickjacking, MIME sniffing, and XSS
- Log scrubbing prevents accidental secret leakage in log files, CI output, and error reports
- All middleware is configurable and can be disabled for development

### Negative

- Per-IP rate limiting does not account for shared IPs (NAT, corporate proxies); legitimate users behind the same IP share a bucket
- Token bucket state is in-memory; rate limits reset on server restart
- Log scrubbing uses pattern matching which may have false positives (redacting non-sensitive strings that match patterns) or false negatives (missing novel secret formats)
- HSTS with `preload` is irreversible once submitted to browser preload lists

## Alternatives Considered

### Governor Crate (Predecessor Approach)

- **Pros**: Battle-tested, per-route configuration, distributed rate limiting support
- **Cons**: Additional dependency, per-route configuration is overkill for a local-first single-user app
- **Verdict**: Rejected for now. A simple in-memory token bucket is sufficient. Governor can be adopted later if multi-user deployment is needed.

### No Rate Limiting

- **Pros**: Zero overhead, simpler middleware stack
- **Cons**: Any exposed endpoint is vulnerable to abuse; a runaway frontend bug could overwhelm the backend
- **Verdict**: Rejected. Even local-first apps benefit from rate limiting as a safety net.
