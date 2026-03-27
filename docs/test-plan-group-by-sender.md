# Test Plan: Group by Sender

**Feature**: Group emails by sender in the email list view
**Date**: 2026-03-25
**Status**: Draft

---

## 1. Test Infrastructure Summary

- **Runner**: Vitest 3.2.x (`vitest run --passWithNoTests`)
- **DOM**: jsdom
- **Component testing**: `@testing-library/react` + `@testing-library/jest-dom`
- **Mocking**: msw 2.x for API, Vitest built-in `vi` for functions
- **E2E**: Playwright
- **Email interface**: `@emailibrium/types` -- `Email` with `fromAddr: string`, `fromName?: string`, `receivedAt: string`, `isRead: boolean`, `isStarred: boolean`
- **Existing tests**: None in `frontend/apps/web/src/features/email/`. This feature introduces the first test files for this directory.

---

## 2. New Files and Functions Assumed

The test plan assumes the feature implementation will introduce:

| Artifact               | Path                                                                                                                     |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Pure grouping function | `frontend/apps/web/src/features/email/groupBySender.ts`                                                                  |
| Grouped list component | `frontend/apps/web/src/features/email/GroupedEmailList.tsx`                                                              |
| Updated filter pills   | Modified in `frontend/apps/web/src/features/email/EmailClient.tsx` (line 297 area, adding `'grouped'` to the pill array) |

Test files:

| Test file                                                                     | Category                            |
| ----------------------------------------------------------------------------- | ----------------------------------- |
| `frontend/apps/web/src/features/email/__tests__/groupBySender.test.ts`        | Unit -- pure grouping logic         |
| `frontend/apps/web/src/features/email/__tests__/GroupedEmailList.test.tsx`    | Component -- grouped list rendering |
| `frontend/apps/web/src/features/email/__tests__/FilterPills.test.tsx`         | Component -- filter pill behavior   |
| `frontend/apps/web/src/features/email/__tests__/EmailClient.grouped.test.tsx` | Integration -- full flow            |

---

## 3. Test Data Factory

All tests should share a factory helper (defined in a `__tests__/fixtures.ts` file or inline) that builds `Email` objects with sensible defaults. Key fields to vary per test:

```text
fromAddr, fromName, receivedAt, isRead, isStarred
```

---

## 4. Unit Tests -- Grouping Logic

**File**: `frontend/apps/web/src/features/email/__tests__/groupBySender.test.ts`

The function under test is expected to have a signature like:

```ts
groupBySender(emails: Email[]): SenderGroup[]
```

where `SenderGroup` is:

```ts
{ senderKey: string; displayName: string; emails: Email[]; mostRecent: string }
```

### 4.1 Core Grouping

| #    | Test name                                             | Input / Setup                                      | Expected outcome                                                                |
| ---- | ----------------------------------------------------- | -------------------------------------------------- | ------------------------------------------------------------------------------- |
| U-01 | returns empty array for empty input                   | `groupBySender([])`                                | `[]`                                                                            |
| U-02 | returns single group for single email                 | One email from `alice@acme.com`                    | Array of length 1; group contains that email; `senderKey` is `"alice@acme.com"` |
| U-03 | groups two emails from same address into one group    | Two emails, both `fromAddr: "alice@acme.com"`      | Single group with 2 emails                                                      |
| U-04 | separates emails from different addresses             | One from `alice@acme.com`, one from `bob@acme.com` | Two groups, each with 1 email                                                   |
| U-05 | does NOT merge different addresses at the same domain | `sales@acme.com` and `support@acme.com`            | Two separate groups                                                             |

### 4.2 Case Normalization

| #    | Test name                                          | Input / Setup                                                                           | Expected outcome                               |
| ---- | -------------------------------------------------- | --------------------------------------------------------------------------------------- | ---------------------------------------------- |
| U-06 | normalizes fromAddr to lowercase for grouping      | `"John@Acme.COM"` and `"john@acme.com"`                                                 | Single group; `senderKey` is `"john@acme.com"` |
| U-07 | normalizes mixed-case addresses across many emails | 5 emails with variants: `"A@B.com"`, `"a@b.com"`, `"a@B.COM"`, `"A@b.com"`, `"a@b.Com"` | Single group with 5 emails                     |

### 4.3 Display Name Resolution

| #    | Test name                                                     | Input / Setup                                                                               | Expected outcome                                   |
| ---- | ------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| U-08 | uses the most common fromName as displayName                  | 3 emails from same address: 2 with `fromName: "Alice Smith"`, 1 with `fromName: "A. Smith"` | `displayName` is `"Alice Smith"`                   |
| U-09 | falls back to fromAddr when all fromName values are undefined | 2 emails from `alice@acme.com` with no `fromName`                                           | `displayName` is `"alice@acme.com"`                |
| U-10 | falls back to fromAddr when fromName values are empty strings | 2 emails with `fromName: ""`                                                                | `displayName` is the normalized `fromAddr`         |
| U-11 | handles mix of present and missing fromName                   | 1 email with `fromName: "Alice"`, 1 email with `fromName: undefined`, same address          | `displayName` is `"Alice"` (most common non-empty) |

### 4.4 Sort Order

| #    | Test name                                                                    | Input / Setup                                                                                        | Expected outcome                                   |
| ---- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| U-12 | sorts emails within a group by receivedAt descending (most recent first)     | 3 emails from same sender at `2026-03-01`, `2026-03-03`, `2026-03-02`                                | Emails in group ordered: `03-03`, `03-02`, `03-01` |
| U-13 | sorts groups by most recent email descending                                 | Group A most recent `2026-03-01`, Group B most recent `2026-03-03`, Group C most recent `2026-03-02` | Group order: B, C, A                               |
| U-14 | breaks ties by senderKey alphabetically when most recent dates are identical | Two groups both with most recent `2026-03-03`; senders `alice@a.com` and `bob@b.com`                 | `alice@a.com` group first                          |

### 4.5 Edge Cases

| #    | Test name                                | Input / Setup                            | Expected outcome                                     |
| ---- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------------------- |
| U-15 | handles emails with identical timestamps | 3 emails, same sender, same `receivedAt` | Single group, all 3 emails present (order is stable) |
| U-16 | handles very large input (performance)   | 10,000 emails from 500 senders           | Returns 500 groups in under 100ms                    |
| U-17 | does not mutate the input array          | Pass a frozen array                      | No error thrown; original array unchanged            |

---

## 5. Unit Tests -- Filtering + Grouping Interaction

**File**: `frontend/apps/web/src/features/email/__tests__/groupBySender.test.ts` (second `describe` block)

These tests validate the combination of filtering emails first, then grouping them. The filtering itself may happen upstream, but the grouping function must produce correct results given a pre-filtered input.

| #    | Test name                                                         | Input / Setup                                                                                              | Expected outcome                                                                |
| ---- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| F-01 | grouped + unread filter shows only unread emails, still grouped   | 4 emails: 2 unread from Alice, 1 read from Alice, 1 unread from Bob. Filter to unread only, then group.    | 2 groups: Alice (2 emails), Bob (1 email)                                       |
| F-02 | grouped + starred filter shows only starred emails, still grouped | 3 emails: 1 starred from Alice, 1 unstarred from Alice, 1 starred from Bob. Filter to starred, then group. | 2 groups: Alice (1), Bob (1)                                                    |
| F-03 | group disappears entirely when all its emails are filtered out    | 3 emails: 2 read from Alice, 1 unread from Bob. Filter to unread.                                          | 1 group: Bob only. Alice group absent.                                          |
| F-04 | count in group reflects filtered count, not total                 | 5 emails from Alice: 3 unread, 2 read. Filter to unread, then group.                                       | Alice group has `emails.length === 3`                                           |
| F-05 | empty result when no emails pass filter                           | 3 read emails. Filter to starred.                                                                          | `[]`                                                                            |
| F-06 | read filter with grouping returns only read emails per group      | Mix of read/unread across 3 senders. Filter to read.                                                       | Each group contains only read emails; senders with no read emails are excluded. |

---

## 6. Component Tests -- GroupedEmailList

**File**: `frontend/apps/web/src/features/email/__tests__/GroupedEmailList.test.tsx`

Assumes a `<GroupedEmailList>` component that receives `SenderGroup[]` and renders collapsible sections.

| #    | Test name                                               | Input / Setup                             | Expected outcome                                                         |
| ---- | ------------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------ |
| C-01 | renders group headers with sender display name          | 2 groups: Alice (3 emails), Bob (1 email) | Two elements with role `heading` or similar containing "Alice" and "Bob" |
| C-02 | renders email count pill on each group header           | 2 groups: Alice (3), Bob (1)              | Text content "3" visible near Alice header; "1" near Bob header          |
| C-03 | all groups start expanded by default                    | 2 groups with emails                      | All email list items are visible in the DOM (not hidden)                 |
| C-04 | clicking group header collapses the group               | Render 2 groups, click Alice header       | Alice's email items are no longer visible; Bob's remain visible          |
| C-05 | clicking collapsed group header expands it              | Collapse Alice, then click again          | Alice's email items are visible again                                    |
| C-06 | renders email items within each group correctly         | 1 group with 2 emails                     | Each email's subject line is rendered inside the group                   |
| C-07 | renders empty state when groups array is empty          | `groups={[]}`                             | "No emails in this view" message or equivalent empty state               |
| C-08 | group header is keyboard accessible                     | Render groups, focus header, press Enter  | Group collapses/expands on Enter key                                     |
| C-09 | group header has correct aria-expanded attribute        | Render groups                             | `aria-expanded="true"` by default; `"false"` after collapse              |
| C-10 | renders groups in the order provided (does not re-sort) | Groups passed in order: Bob, Alice        | Bob group appears first in DOM                                           |

---

## 7. Component Tests -- Filter Pills

**File**: `frontend/apps/web/src/features/email/__tests__/FilterPills.test.tsx`

These tests validate the filter pill bar in the `EmailClient` header area (around line 296 of `EmailClient.tsx`).

| #    | Test name                                                    | Input / Setup                                   | Expected outcome                                                                         |
| ---- | ------------------------------------------------------------ | ----------------------------------------------- | ---------------------------------------------------------------------------------------- |
| P-01 | renders "Grouped" pill to the right of existing pills        | Render the filter pill bar                      | Buttons appear in order: All, Unread, Read, Starred, Grouped                             |
| P-02 | "Grouped" pill starts in inactive state                      | Initial render                                  | "Grouped" button has inactive styling class (`bg-gray-100`)                              |
| P-03 | clicking "Grouped" activates grouped mode                    | Click the "Grouped" button                      | Button gains active styling (`bg-indigo-600 text-white`)                                 |
| P-04 | clicking "Grouped" again deactivates it                      | Click "Grouped" twice                           | Button returns to inactive styling; email list is back to flat mode                      |
| P-05 | "Grouped" is independent of filter pills (toggle, not radio) | Click "Unread", then click "Grouped"            | Both "Unread" and "Grouped" show active styling                                          |
| P-06 | clicking a filter pill while grouped maintains grouped mode  | Activate "Grouped", then click "Starred"        | "Grouped" remains active; "Starred" is now the active filter; "Unread" etc. are inactive |
| P-07 | clicking "All" while grouped maintains grouped mode          | Activate "Grouped" + "Unread", then click "All" | "All" is active filter, "Grouped" remains active                                         |
| P-08 | "Grouped" pill is keyboard accessible                        | Tab to "Grouped", press Enter                   | Activates grouped mode                                                                   |

---

## 8. Integration Tests

**File**: `frontend/apps/web/src/features/email/__tests__/EmailClient.grouped.test.tsx`

These tests render the full `EmailClient` component (or a meaningful subtree) with mocked API responses (msw handlers).

| #    | Test name                                                       | Input / Setup                                                                              | Expected outcome                                                                                                     |
| ---- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------- |
| I-01 | full flow: load emails, click Grouped, verify grouped rendering | msw returns 6 emails from 3 senders. Render `EmailClient`. Click "Grouped".                | Flat list is replaced by grouped view with 3 collapsible sections, each showing correct sender name and email count. |
| I-02 | grouped view updates when new emails arrive                     | Start in grouped mode. Trigger a refetch that returns 1 new email from an existing sender. | Existing group gains +1 in count pill; email appears at top of that group.                                           |
| I-03 | selecting an email in grouped view opens thread panel           | In grouped mode, click an email item inside a group.                                       | Thread panel loads for that email; email is marked as read.                                                          |
| I-04 | archiving an email in grouped view removes it from the group    | In grouped mode, hover an email, click archive.                                            | Email disappears from group; count pill decrements; if group is now empty, group header disappears.                  |
| I-05 | starring an email in grouped view persists across filter toggle | Star an email in grouped mode, switch to "Starred" filter while still grouped.             | Starred email appears in its sender group; unstarred emails are excluded.                                            |
| I-06 | switching away from grouped mode restores flat list             | Activate grouped mode, verify groups, then click "Grouped" to deactivate.                  | Email list returns to flat, most-recent-first rendering.                                                             |
| I-07 | grouped mode persists across sidebar navigation                 | Activate grouped mode, switch sidebar group from Inbox to Work.                            | Grouped mode remains active; emails in Work category are grouped by sender.                                          |
| I-08 | infinite scroll works within grouped view                       | msw returns 50 emails (2 pages). Activate grouped, scroll to bottom.                       | Second page loads; new emails integrate into existing or new groups.                                                 |
| I-09 | bulk actions work in grouped view                               | Check 2 emails from different groups, click bulk archive.                                  | Both emails removed from their respective groups; counts update.                                                     |

---

## 9. Accessibility Tests

Validated within the component tests above but called out explicitly:

| #    | Requirement                                                                   | Validated in                          |
| ---- | ----------------------------------------------------------------------------- | ------------------------------------- |
| A-01 | Group header has `role="button"` and `aria-expanded`                          | C-09                                  |
| A-02 | Collapsed group content has `aria-hidden="true"` or is removed from tab order | C-04                                  |
| A-03 | Group header is focusable and operable via keyboard                           | C-08                                  |
| A-04 | Count pill is announced by screen readers (e.g., via `aria-label` on header)  | C-02 (extend with `aria-label` check) |
| A-05 | "Grouped" filter pill has `aria-pressed` attribute reflecting toggle state    | P-03, P-04                            |

---

## 10. Performance Acceptance Criteria

| #       | Criterion                                                                   | Validated in                          |
| ------- | --------------------------------------------------------------------------- | ------------------------------------- |
| Perf-01 | `groupBySender` processes 10,000 emails in < 100ms                          | U-16                                  |
| Perf-02 | Grouped view renders 50 groups (200 emails) without jank (< 16ms per frame) | Manual / Playwright performance trace |
| Perf-03 | Collapse/expand animation completes in < 300ms                              | Manual / Playwright                   |

---

## 11. Test Execution

```bash
# Run all new tests
cd frontend && pnpm vitest run src/features/email/__tests__/

# Run only unit tests (grouping logic)
cd frontend && pnpm vitest run src/features/email/__tests__/groupBySender.test.ts

# Run only component tests
cd frontend && pnpm vitest run src/features/email/__tests__/GroupedEmailList.test.tsx
cd frontend && pnpm vitest run src/features/email/__tests__/FilterPills.test.tsx

# Run integration tests
cd frontend && pnpm vitest run src/features/email/__tests__/EmailClient.grouped.test.tsx
```

---

## 12. Coverage Targets

| Metric     | Target                           |
| ---------- | -------------------------------- |
| Statements | > 90% for `groupBySender.ts`     |
| Branches   | > 85% for `groupBySender.ts`     |
| Statements | > 80% for `GroupedEmailList.tsx` |
| Branches   | > 75% for `GroupedEmailList.tsx` |

---

## 13. Dependencies and Risks

| Risk                                                                                                                                                                        | Mitigation                                                                                                                            |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `@tanstack/react-virtual` complicates grouped rendering (virtualizer expects flat list)                                                                                     | May need a custom virtualizer or render groups as nested lists within virtual rows. Test for correct scroll offset calculation.       |
| Filter state currently uses a union type `'all' \| 'read' \| 'unread' \| 'starred'` as a radio selector (line 52 of `EmailClient.tsx`). "Grouped" is a toggle, not a radio. | Implementation must separate the grouped toggle from the filter radio. Tests P-05 through P-07 explicitly validate this independence. |
| No existing vitest config file found in source (may be in workspace root or inherited from vite config).                                                                    | Verify vitest resolves correctly before writing tests. May need to add a `vitest.config.ts` at `frontend/apps/web/`.                  |
| Infinite scroll with grouped view needs careful handling of page boundaries.                                                                                                | I-08 covers this. Consider edge case where a sender's emails span two pages.                                                          |
