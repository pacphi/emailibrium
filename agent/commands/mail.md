---
name: mail
description: Read-only mail chief-of-staff commands — doctor, snapshot, brief, review, map
---

# /mail

Thin wrapper over the `tools/staff` MCP tools (see `agent/SKILL.md`). Read-only in this
milestone: nothing here files, archives, or applies anything.

## Usage

```text
/mail doctor [account]
/mail snapshot <account>
/mail brief [account]
/mail review [account]
/mail map
```

`account` is the account's UUID (from `list_accounts` / `doctor`'s own output), not an email
address. `snapshot` requires it — a snapshot describes one mailbox's shape, so it cannot run
across every account at once. The others default to every connected account, merged, when
omitted.

## Subcommands

### `/mail doctor [account]`

Call the `doctor` tool with `account_id` set to the given UUID, or omitted for every account.
Report, per account: provider, connection status, write capabilities, label strategy, and the
scan-time estimate. If an account's `status` isn't `connected`, lead with that.

### `/mail snapshot <account>`

Call `get_snapshot` with the given `account_id`. Report the Kind distribution, human/receipts/
GitHub share, inflow rate, history length, and the detected archetype. If `role_mismatch` is
present, surface its `message` as a direct question to the user — never resolve it yourself.
Name every entry under `approximated` so the user knows which numbers are estimates.

### `/mail brief [account]`

Call `brief`. Lead with `needs_you` (unread People-kind mail in the inbox) if non-empty,
ordered as returned; otherwise say the inbox has nothing needing a person right now. Follow
with `filed_counts` as a one-line summary. Name what's in `not_yet_available` once, briefly —
don't repeat the caveat for every field it covers.

### `/mail review [account]`

Call `review`. Report `new_senders` (first seen in the last 7 days) and
`unsubscribe_candidates` as two short lists, each item a yes/no the user can answer. This is a
report, not an approval flow — don't imply that answering here changes anything yet.

### `/mail map`

Call `identity_map` (no arguments). Report `cross_account_senders`, largest overlap first.
For each, name the accounts involved and the per-account message count. This is the raw
overlap only — no migration-nudge judgment yet (see the response's `not_yet_available`).
