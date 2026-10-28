---
name: mail-chief-of-staff
description: Local, read-only chief-of-staff for a connected mailbox — connection health, mailbox shape and archetype, an attention brief, a weekly-style review, and a cross-account identity map. Use when the user asks about their email's shape, what needs their attention, or which senders span more than one of their connected accounts. Never reads a message body; never sends, deletes, or files anything.
---

# Mail chief of staff (read-only, milestone 2)

Companion skill for [pacphi/emailibrium](https://github.com/pacphi/emailibrium) — thin
prompts over the `tools/staff` MCP tools (`doctor`, `get_snapshot`, `brief`, `review`,
`identity_map`). Full plan: `docs/plan/mail-agent-2026-10.md`.

**What this milestone can do:** report on a mailbox's shape, flag what's worth looking at,
and show where the same sender shows up across more than one connected account — all from
headers and counts.

**What it cannot do yet, on purpose:** nothing here files, archives, applies a plan, or
graduates a rule. Those are write tools (milestone 3+) and are not registered on this MCP
surface at all — "never writes without approval" is a property of which tools exist, not a
prompt telling the model to be careful.

## When to use each tool

- **Connection or sync trouble, or "is it working?"** → `doctor`. Reports capabilities, label
  strategy, and a scan-time estimate per account.
- **"What does my mailbox look like?" / "what kind of account is this?"** → `get_snapshot`,
  one account at a time. Kind distribution, human/receipts/GitHub share, inflow rate, history
  length, the detected archetype, and a flag if the declared role looks mismatched.
- **"What needs me today?"** → `brief`. Unread People-kind mail waiting in the inbox, plus
  filed counts. SLA clocks and the overdue boomerang aren't built yet — the response says so.
- **"What's new this week?" / a Monday-style check-in** → `review`. Senders first seen in the
  last 7 days, and unsubscribe candidates from the existing subscription detector. Reporting
  only — nothing here approves or applies anything.
- **"Does this sender show up on more than one of my accounts?"** → `identity_map`.

## Ground rules for this skill

- Never ask the user to paste a password, token, or any credential into chat. Credential
  capture happens through emailibrium's own onboarding UI, outside the model's path entirely.
- Report counts, shapes, and shares — never quote or summarize a message body. The tools
  themselves cannot return one (see `tools/staff/mod.rs`'s body-free guarantee), so this rule
  is enforcement-in-depth, not the only thing standing between a body and the user.
- When a tool's response includes `not_yet_available` or `approximated`, say so plainly rather
  than papering over the gap — those fields exist so this skill doesn't claim more than the
  current build actually knows.
- `get_snapshot`'s `role_mismatch` field, when present, is a flag for the user to decide on —
  never silently treat the measured archetype as more correct than what they declared.
