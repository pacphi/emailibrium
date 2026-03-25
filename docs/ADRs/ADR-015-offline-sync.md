# ADR-015: Offline-First Sync with Conflict Resolution

- **Status**: Accepted
- **Date**: 2026-03-24
- **Implements**: R-02 (Predecessor Recommendations), R-06 (Processing Checkpoints)
- **Related**: DDD-003 (Ingestion), DDD-005 (Account Management)

## Context

Emailibrium runs locally on user machines that frequently lose network connectivity (laptop sleep, travel, flaky Wi-Fi). The ingestion pipeline currently assumes continuous connectivity: if the network drops mid-sync, progress is lost and the sync restarts from scratch. The predecessor repository solved this with a queue-based offline system and processing checkpoints. The current PWA service worker already caches static assets for offline use, but backend operations have no offline support.

## Decision

Implement queue-based offline operations with conflict resolution and crash-recovery checkpoints. Offline actions are buffered locally and replayed when connectivity returns. Batch ingestion jobs save checkpoints so they can resume after crashes.

### Sync Queue

```sql
CREATE TABLE sync_queue (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    operation TEXT NOT NULL,       -- 'label', 'archive', 'delete', 'move', 'mark_read'
    payload TEXT NOT NULL,         -- JSON: email_id, target_label, etc.
    status TEXT DEFAULT 'pending', -- 'pending', 'syncing', 'completed', 'failed', 'conflict'
    retry_count INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    synced_at DATETIME
);
```

Operations performed while offline are inserted into `sync_queue` with `status = 'pending'`. The UI reflects the operation optimistically. When connectivity returns, the `SyncScheduler` drains the queue in FIFO order.

### Conflict Resolution

When a queued operation conflicts with a remote state change (e.g., user archived an email locally, but it was deleted remotely), the system applies a configurable `ConflictStrategy`:

| Strategy       | Behavior                                         | Default For   |
| -------------- | ------------------------------------------------ | ------------- |
| LastWriterWins | Most recent timestamp wins (local or remote)     | Labels, read  |
| LocalWins      | Local operation always takes precedence          | Archive, move |
| RemoteWins     | Remote state always takes precedence             | Delete        |
| Manual         | Mark as conflict, surface to user for resolution | None (opt-in) |

The default strategy per operation type is configurable in `config.yaml`. Conflicts that cannot be auto-resolved are surfaced in the UI with both states shown.

### SyncScheduler

The `SyncScheduler` runs as a background task and:

1. Monitors network connectivity via periodic health checks to the email provider
2. When online, drains `sync_queue` entries in order, applying conflict resolution
3. On transient failure (HTTP 429, 503), applies exponential backoff: 1s, 2s, 4s, 8s, max 60s
4. After 5 consecutive failures for the same entry, marks it `failed` and moves to the next
5. Emits `SyncQueueDrained` event when all pending operations are processed

### Processing Checkpoints

```sql
CREATE TABLE processing_checkpoints (
    job_id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    account_id TEXT NOT NULL,
    last_processed_id TEXT,
    total_count INTEGER,
    processed_count INTEGER,
    state TEXT DEFAULT 'running', -- 'running', 'completed', 'failed', 'paused'
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

During batch ingestion, a checkpoint is saved every N emails (default: 100, configurable). On crash or restart, the ingestion pipeline queries the checkpoint table and resumes from `last_processed_id` rather than restarting the entire batch.

## Consequences

### Positive

- Users can label, archive, and organize email while offline; changes sync when connectivity returns
- Crash during ingestion of large mailboxes (50,000+ emails) resumes from the last checkpoint, not from scratch
- Exponential backoff prevents hammering rate-limited provider APIs
- Conflict resolution is deterministic and configurable per operation type
- Pairs with existing PWA service worker for full offline-first experience

### Negative

- Optimistic UI may show state that diverges from remote (until sync completes)
- Conflict resolution adds complexity; edge cases (e.g., label renamed remotely while offline) require careful handling
- Checkpoint granularity (every 100 emails) means up to 100 emails may be reprocessed after a crash
- `sync_queue` table grows during extended offline periods; periodic cleanup of completed entries is needed

## Alternatives Considered

### No Offline Support (Current State)

- **Pros**: Simpler implementation, no conflict resolution needed
- **Cons**: Operations fail silently or with errors when offline, large syncs restart from scratch on crash
- **Verdict**: Rejected. Local-first applications must handle offline gracefully.

### CRDT-Based Sync

- **Pros**: Mathematically guaranteed convergence, no conflict resolution logic needed
- **Cons**: Significant implementation complexity, email operations do not map cleanly to CRDT data types, overkill for single-user local-first app
- **Verdict**: Rejected. Queue-based sync with configurable conflict strategies is simpler and sufficient for single-user scenarios.
