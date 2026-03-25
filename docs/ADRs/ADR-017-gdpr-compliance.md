# ADR-017: GDPR Compliance — Consent Tracking, Audit Logging, and Data Portability

- **Status**: Accepted
- **Date**: 2026-03-24
- **Implements**: R-09 (Predecessor Recommendations)
- **Related**: ADR-008 (Privacy Architecture), DDD-006 (AI Providers — ConsentManager)

## Context

Emailibrium processes personal email data and, if cloud AI providers are used, transmits data to third parties. GDPR (and similar privacy regulations) require explicit consent tracking, audit logging of data access, data portability (Article 20), and right to erasure (Article 17). The current codebase has a consent API endpoint and a ConsentManager service (DDD-006), but consent decisions are not persisted and there is no audit log, data export, or erasure workflow.

## Decision

Implement four GDPR compliance capabilities: persistent consent tracking, append-only privacy audit logging, data export, and right to erasure.

### Consent Tracking

```sql
CREATE TABLE consent_decisions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    consent_type TEXT NOT NULL,    -- 'cloud_ai', 'analytics', 'data_sharing'
    provider TEXT,                 -- 'openai', 'anthropic', etc. (nullable for non-provider consent)
    granted BOOLEAN NOT NULL,
    granted_at DATETIME,
    revoked_at DATETIME,
    ip_address TEXT,
    user_agent TEXT,
    version INTEGER DEFAULT 1     -- consent policy version
);
```

Consent decisions are immutable records. Revoking consent inserts a new row with `granted = false` and `revoked_at` set. The most recent row per `(user_id, consent_type, provider)` determines current consent state. This preserves the full consent history for audit purposes.

### Privacy Audit Log

```sql
CREATE TABLE privacy_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    user_id TEXT NOT NULL,
    action TEXT NOT NULL,          -- 'data_access', 'data_export', 'data_erasure', 'consent_change', 'cloud_api_call'
    resource_type TEXT NOT NULL,   -- 'email', 'embedding', 'profile', 'consent'
    resource_id TEXT,
    details TEXT,                  -- JSON with action-specific metadata (no PII)
    ip_address TEXT
);
```

The audit log is append-only (no UPDATE or DELETE permitted by the application). It records all data access, export, and erasure events. The `details` field contains metadata (e.g., number of records exported, provider called) but never PII content.

### Data Export (Article 20)

The `GET /api/v1/privacy/export` endpoint generates a `UserDataExport` JSON archive containing:

- User profile and account metadata
- Email metadata (subject, sender, date, labels) — not full email bodies (those live on the provider)
- Category assignments and rule configurations
- Consent history
- Audit log entries for this user

The export is returned as a JSON file. For large exports, the endpoint returns a job ID and the export is built asynchronously.

### Right to Erasure (Article 17)

The `POST /api/v1/privacy/erase` endpoint triggers a complete data erasure:

1. Delete all email metadata from SQLite
2. Delete all vector embeddings from the vector store
3. Delete all category assignments and learning data (SONA centroids, feedback)
4. Delete all rule configurations
5. Retain the audit log entry recording that erasure occurred (legal requirement)
6. Retain consent records (legal requirement to prove consent was obtained)
7. Return an `ErasureReport` with counts of deleted records per resource type

Erasure is irreversible. The endpoint requires explicit confirmation (a `confirm: true` field in the request body).

## Consequences

### Positive

- Full GDPR Article 7 (consent), Article 17 (erasure), and Article 20 (portability) compliance
- Append-only audit log provides a tamper-evident record for regulatory inquiries
- Consent history preserves the complete timeline of grants and revocations
- Data export gives users full visibility into what Emailibrium stores about them
- Erasure workflow is comprehensive — no orphaned data in vector store or caches

### Negative

- Append-only audit log grows indefinitely; requires periodic archival (configurable retention, default 2 years)
- Erasure deletes SONA learning data, meaning the system must retrain personalization from scratch
- Data export for large mailboxes may take minutes; async job pattern adds complexity

## Alternatives Considered

### Soft Delete Instead of Hard Erasure

- **Pros**: Recoverable, simpler implementation
- **Cons**: Does not satisfy GDPR Article 17 which requires actual deletion, not just hiding
- **Verdict**: Rejected. Hard delete is legally required. The audit log entry serves as the record.

### File-Based Audit Log

- **Pros**: Simpler than database, easy to ship to external SIEM
- **Cons**: Not queryable, harder to correlate with user data, no transactional consistency with consent changes
- **Verdict**: Rejected. SQLite table is queryable and transactional. A future enhancement can export to file/SIEM.
